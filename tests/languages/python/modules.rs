use super::*;
use super::PythonWorkspace as Workspace;
use rusqlite::Connection;
use std::path::PathBuf;

fn rows(conn: &Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn status(conn: &Connection, expression: &str) -> String {
    conn.query_row(
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.py' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1 AND line=1",
        [expression],
        |row| row.get(0),
    ).unwrap()
}

fn call_targets(conn: &Connection) -> Vec<String> {
    rows(conn, "SELECT dst.qualname FROM edges e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE src.qualname='consumer.consume' AND e.kind='calls' ORDER BY dst.qualname")
}

fn call_target_paths(conn: &Connection) -> Vec<String> {
    rows(conn, "SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edges e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE src.qualname='consumer.consume' AND e.kind='calls' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)")
}

fn import_targets(conn: &Connection) -> Vec<String> {
    rows(conn, "SELECT dst.qualname FROM edges e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE src.qualname='consumer' AND e.kind='imports' ORDER BY dst.qualname")
}

fn import_target_paths(conn: &Connection) -> Vec<String> {
    rows(conn, "SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edges e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE src.qualname='consumer' AND e.kind='imports' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)")
}

#[test]
fn oversized_python_flow_keeps_type_only_imports_fail_closed_after_persistence() {
    let workspace = PythonWorkspace::new();
    workspace.write("src/pkg/model.py", "class Client:\n    pass\n");
    let mut source = String::from(
        "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from pkg.model import Client\n",
    );
    for _ in 0..65_536 {
        source.push_str("x = 0\n");
    }
    let call_line = source.lines().count() as i64 + 1;
    source.push_str("Client()\n");
    workspace.write("src/consumer.py", &source);
    let db = workspace.0.join(".forge/code-map.sqlite");
    let check = |conn: &rusqlite::Connection| {
        assert_eq!(
            persisted_status(conn, "src/consumer.py", call_line, "Client"),
            "unresolved"
        );
        let blob: Vec<u8> = conn
            .query_row(
                "SELECT facts_blob FROM local_facts WHERE path='src/consumer.py'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let facts: serde_json::Value =
            serde_json::from_slice(&zstd::stream::decode_all(blob.as_slice()).unwrap()).unwrap();
        let module = facts["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["kind"] == "module")
            .unwrap();
        let flow = &module["details"]["value_flow"];
        assert_eq!(flow["fact_limit_exceeded"], true);
        assert!(flow.get("type_only_imports").is_none());
        assert!(flow.get("annotation_references").is_none());
    };
    contextunity_forge_mcp::db::writer::build(&workspace.0, &db, None).unwrap();
    let cold = contextunity_forge_mcp::db::reader::open(&db, &workspace.0).unwrap();
    check(&cold);
    drop(cold);
    source.push_str("# changed for incremental indexing\n");
    workspace.write("src/consumer.py", &source);
    contextunity_forge_mcp::db::writer::delta(
        &workspace.0,
        &db,
        &[PathBuf::from("src/consumer.py")],
    )
    .unwrap();
    let delta = contextunity_forge_mcp::db::reader::open(&db, &workspace.0).unwrap();
    check(&delta);
}

#[test]
fn type_checking_imports_resolve_annotations_without_runtime_calls() {
    let (primary, second) = linked_python_roots();
    for workspace in [&primary, &second] {
        workspace.write(
            "src/pkg/model.py",
            "class Client:\n    @classmethod\n    def make(cls):\n        return cls()\n",
        );
        workspace.write(
            "src/consumer.py",
            "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from pkg.model import Client\ndef use(value: Client = Client.make()) -> None:\n    Client()\n    Client.make()\n    value.make()\ndef alias_use() -> None:\n    alias = Client\n    alias.make()\n",
        );
        workspace.write("src/unproven.py", "if TYPE_CHECKING:\n    from pkg.model import Client\ndef use(value: Client) -> None:\n    Client()\n");
        workspace.write("src/nested.py", "from typing import TYPE_CHECKING\ndef use() -> None:\n    if TYPE_CHECKING:\n        from pkg.model import Client\n    Client()\n");
        workspace.write("src/rebound_guard.py", "from typing import TYPE_CHECKING\ndel TYPE_CHECKING\nif TYPE_CHECKING:\n    from pkg.model import Client\ndef use(value: Client) -> None:\n    Client()\n");
        workspace.write(
            "src/runtime.py",
            "from pkg.model import Client\nClient()\nClient.make()\n",
        );
        workspace.write(
            "src/aliased_import.py",
            "import typing as aliases\nif aliases.TYPE_CHECKING:\n    from pkg.model import Client\ndef use(value: Client) -> None:\n    Client()\n",
        );
        workspace.write(
            "src/spoofed_typing.py",
            "import os as typing\nif typing.TYPE_CHECKING:\n    from pkg.model import Client\ndef use(value: Client) -> None:\n    Client()\n",
        );
        workspace.write(
            "src/renamed_guard.py",
            "from typing import TYPE_CHECKING as TC\nif TC:\n    from pkg.model import Client\ndef use(value: Client) -> None:\n    Client()\n",
        );
        let rebound_guard_source = "from typing import TYPE_CHECKING as TC\nTC = False\nif TC:\n    from pkg.model import Client\ndef use(value: Client) -> None:\n    Client()\n";
        workspace.write("src/renamed_rebound_guard.py", rebound_guard_source);
        let extracted = ast::extract(
            "src/renamed_rebound_guard.py",
            "python",
            rebound_guard_source,
        )
        .unwrap();
        let rebound_import = extracted
            .references
            .iter()
            .find(|reference| reference.module.as_deref() == Some("pkg.model"))
            .expect("guarded import remains indexed after alias rebinding");
        assert!(!matches!(
            rebound_import.receiver_hint.as_ref(),
            Some(ReceiverHint::TypeOnlyImport)
        ));
        assert_eq!(rebound_import.alias.as_deref(), Some("Client"));
    }
    let conn = primary.build();
    for (path, provider) in [
        ("src/consumer.py", "src/pkg/model.py:Client"),
        (
            "[second]/src/consumer.py",
            "[second]/src/pkg/model.py:Client",
        ),
    ] {
        assert_eq!(persisted_status(&conn, path, 4, "Client"), "resolved");
        assert_eq!(
            persisted_status(&conn, path, 4, "Client.make"),
            "unresolved"
        );
        assert_eq!(persisted_status(&conn, path, 5, "Client"), "unresolved");
        assert_eq!(
            persisted_status(&conn, path, 6, "Client.make"),
            "unresolved"
        );
        assert_eq!(persisted_status(&conn, path, 7, "value.make"), "resolved");
        assert_eq!(
            persisted_status(&conn, path, 10, "alias.make"),
            "unresolved"
        );
        let targets: Vec<String> = conn
            .prepare("SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) || ':' || dst.name FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE p.path=?1 AND e.line=4 AND e.kind='references' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id),dst.name")
            .unwrap()
            .query_map([path], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(targets, vec![provider]);
    }
    for prefix in ["", "[second]/"] {
        let unproven = format!("{prefix}src/unproven.py");
        assert_eq!(
            persisted_status(&conn, &unproven, 3, "Client"),
            "unresolved"
        );
        assert_eq!(
            persisted_status(&conn, &unproven, 4, "Client"),
            "unresolved"
        );
        let nested = format!("{prefix}src/nested.py");
        assert_eq!(
            persisted_status_rows(&conn, &nested, 5, "Client"),
            ["unresolved"]
        );
        let rebound = format!("{prefix}src/rebound_guard.py");
        assert_eq!(
            persisted_status_rows(&conn, &rebound, 5, "Client"),
            ["unresolved"]
        );
        assert_eq!(
            persisted_status_rows(&conn, &rebound, 6, "Client"),
            ["unresolved"]
        );
        let runtime = format!("{prefix}src/runtime.py");
        assert_eq!(persisted_status(&conn, &runtime, 2, "Client"), "resolved");
        assert_eq!(
            persisted_status(&conn, &runtime, 3, "Client.make"),
            "resolved"
        );
        let aliased = format!("{prefix}src/aliased_import.py");
        assert_eq!(persisted_status(&conn, &aliased, 4, "Client"), "resolved");
        assert_eq!(persisted_status(&conn, &aliased, 5, "Client"), "unresolved");
        let spoofed = format!("{prefix}src/spoofed_typing.py");
        assert_eq!(persisted_status(&conn, &spoofed, 4, "Client"), "unresolved");
        assert_eq!(persisted_status(&conn, &spoofed, 5, "Client"), "unresolved");
        let renamed = format!("{prefix}src/renamed_guard.py");
        assert_eq!(persisted_status(&conn, &renamed, 4, "Client"), "resolved");
        assert_eq!(persisted_status(&conn, &renamed, 5, "Client"), "unresolved");
    }
    let (shadow_primary, shadow_second) = linked_python_roots();
    for workspace in [&shadow_primary, &shadow_second] {
        workspace.write("src/pkg/model.py", "class Client:\n    pass\n");
        workspace.write("src/typing.py", "TYPE_CHECKING = bool(input())\n");
        workspace.write("src/shadow_consumer.py", "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from pkg.model import Client\ndef use(value: Client) -> None:\n    Client()\n");
    }
    let shadow_conn = shadow_primary.build();
    for prefix in ["", "[second]/"] {
        let path = format!("{prefix}src/shadow_consumer.py");
        assert_eq!(
            persisted_status_rows(&shadow_conn, &path, 4, "Client"),
            ["unresolved"]
        );
        assert_eq!(
            persisted_status_rows(&shadow_conn, &path, 5, "Client"),
            ["unresolved"]
        );
    }
    for (declared, annotation_status) in [(false, "unresolved"), (true, "resolved")] {
        let (extension_primary, extension_second) = linked_python_roots();
        for workspace in [&extension_primary, &extension_second] {
            workspace.write("src/pkg/model.py", "class Client:\n    pass\n");
            if declared {
                workspace.write(
                    "pyproject.toml",
                    "[project]\ndependencies = [\"typing-extensions>=4\"]\n",
                );
            }
            workspace.write("src/extension_consumer.py", "from typing_extensions import TYPE_CHECKING\nif TYPE_CHECKING:\n    from pkg.model import Client\ndef use(value: Client) -> None:\n    Client()\n");
        }
        let extension_conn = extension_primary.build();
        for prefix in ["", "[second]/"] {
            let path = format!("{prefix}src/extension_consumer.py");
            assert_eq!(
                persisted_status_rows(&extension_conn, &path, 4, "Client"),
                [annotation_status]
            );
            assert_eq!(
                persisted_status_rows(&extension_conn, &path, 5, "Client"),
                ["unresolved"]
            );
        }
    }
}

#[test]
fn package_reexports_follow_callable_alias_targets() {
    let (primary, second) = linked_python_roots();
    for workspace in [&primary, &second] {
        workspace.write(
            "src/pkg/__init__.py",
            "from .implementation import dispatch\n__all__ = ['dispatch']\n",
        );
        workspace.write(
            "src/pkg/implementation.py",
            "def implementation(): return 'ready'\ndispatch = implementation\n",
        );
        workspace.write("src/ambiguous/__init__.py", "flag = bool(input())\nif flag:\n    from .first import dispatch\nelse:\n    from .second import dispatch\n__all__ = ['dispatch']\n");
        workspace.write("src/ambiguous/first.py", "def dispatch(): return 'first'\n");
        workspace.write(
            "src/ambiguous/second.py",
            "def dispatch(): return 'second'\n",
        );
        workspace.write(
            "src/rebound_alias.py",
            "def implementation(): return 'ready'\ndispatch = implementation\ndispatch = None\n",
        );
        workspace.write("src/conditional_alias.py", "def implementation(): return 'ready'\nif bool(input()):\n    dispatch = implementation\n");
        workspace.write("src/consumer.py", "def use():\n    dispatch()\n    from pkg import dispatch\n    dispatch()\ndef uncertain():\n    from ambiguous import dispatch\n    dispatch()\ndef changed():\n    from rebound_alias import dispatch\n    dispatch()\ndef maybe():\n    from conditional_alias import dispatch\n    dispatch()\n");
    }
    let conn = primary.build();
    for (path, prefix) in [
        ("src/consumer.py", "src/"),
        ("[second]/src/consumer.py", "[second]/src/"),
    ] {
        assert_eq!(
            persisted_status(&conn, path, 2, "dispatch"),
            "unresolved",
            "{path}: before import"
        );
        let (status, targets) = persisted_import(&conn, path, 3, "dispatch");
        assert_eq!(status, "resolved", "{path}: {targets:?}");
        assert_eq!(
            targets,
            vec![
                format!("{prefix}pkg/__init__.py:__init__.py"),
                format!("{prefix}pkg/implementation.py:implementation")
            ],
            "{path}"
        );
        assert_eq!(
            persisted_status(&conn, path, 4, "dispatch"),
            "resolved",
            "{path}: after import"
        );
        let called: Vec<String> = conn.prepare(
            "SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) || ':' || dst.name FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes dst ON dst.node_hash=e.dst_hash JOIN coverage_evidence v ON v.evidence_id=e.evidence_id WHERE p.path=?1 AND e.line=4 AND e.kind='calls' AND v.evidence='dispatch' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id),dst.name",
        ).unwrap().query_map([path], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(
            called,
            vec![format!("{prefix}pkg/implementation.py:implementation")],
            "{path}"
        );
        let (status, targets) = persisted_import(&conn, path, 6, "dispatch");
        assert!(
            matches!(status.as_str(), "ambiguous" | "unresolved"),
            "{path}: {status} {targets:?}"
        );
        assert_eq!(
            targets,
            vec![format!("{prefix}ambiguous/__init__.py:__init__.py")],
            "{path}: no conditional provider may be selected"
        );
        assert!(matches!(
            persisted_status(&conn, path, 7, "dispatch").as_str(),
            "ambiguous" | "unresolved"
        ));
        for line in [10, 13] {
            assert!(
                matches!(
                    persisted_status(&conn, path, line, "dispatch").as_str(),
                    "ambiguous" | "unresolved"
                ),
                "{path}:{line}"
            );
            let direct_targets: i64 = conn.query_row(
                "SELECT count(*) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE p.path=?1 AND e.line=?2 AND e.kind='calls' AND dst.name='implementation'",
                rusqlite::params![path, line], |row| row.get(0),
            ).unwrap();
            assert_eq!(
                direct_targets, 0,
                "{path}:{line}: rebound or conditional alias selected a callable"
            );
        }
    }
}

#[test]
fn module_assignment_exports_and_reexports_persist_exact_targets() {
    let (primary, second) = linked_python_roots();
    for w in [&primary, &second] {
        w.write("src/pkg/__init__.py", "# package\n");
        w.write("src/pkg/catalog.py", "CONSTANT = 7\nfactory_value = make_value()\ndef implementation(): return 1\ncallable_alias = implementation\ndef named(): return 2\n");
        w.write("src/pkg/facade.py", "from .catalog import CONSTANT\n");
        w.write("src/pkg/duplicate.py", "VALUE = 1\nVALUE = 2\n");
        w.write(
            "src/pkg/rebound.py",
            "from .catalog import CONSTANT\nCONSTANT = 99\n",
        );
        w.write("src/pkg/rebound_declarations.py", "class Foo: pass\nFoo = 1\ndef make(): return 1\nmake = lambda: 2\nPRE = 0\nclass PRE: pass\n");
        w.write("src/rebound_pkg/__init__.py", "class Foo: pass\nFoo = 1\n");
        w.write("src/rebound_pkg/Foo.py", "def unrelated(): return 1\n");
        w.write("src/consumer.py", "from pkg.catalog import CONSTANT\nfrom pkg.catalog import factory_value\nfrom pkg.catalog import callable_alias\nfrom pkg.facade import CONSTANT as reexported\nfrom pkg.catalog import named\nfrom pkg.catalog import absent\nfrom pkg.duplicate import VALUE\nfrom pkg.rebound import CONSTANT as rebound\nfrom pkg.rebound_declarations import Foo\nfrom pkg.rebound_declarations import make\nfrom pkg.rebound_declarations import PRE\nfrom rebound_pkg import Foo as rebound_foo\n");
    }
    let conn = primary.build();
    for (consumer, prefix) in [
        ("src/consumer.py", "src/"),
        ("[second]/src/consumer.py", "[second]/src/"),
    ] {
        for (line, name) in [
            (1, "CONSTANT"),
            (2, "factory_value"),
            (4, "CONSTANT"),
            (5, "named"),
        ] {
            let (status, targets) = persisted_import(&conn, consumer, line, name);
            assert_eq!(status, "resolved", "{consumer}:{line} {name}: {targets:?}");
            assert!(
                targets.contains(&format!("{prefix}pkg/catalog.py:{name}")),
                "{consumer}:{line} {name}: {targets:?}"
            );
            assert!(
                !targets
                    .iter()
                    .any(|target| target.starts_with(if prefix == "src/" {
                        "[second]/"
                    } else {
                        "src/"
                    })),
                "cross-project target: {targets:?}"
            );
        }
        let (status, targets) = persisted_import(&conn, consumer, 3, "callable_alias");
        assert_eq!(status, "resolved", "{consumer}: {targets:?}");
        assert!(
            targets.contains(&format!("{prefix}pkg/catalog.py:implementation")),
            "{consumer}: {targets:?}"
        );
        assert!(
            !targets
                .iter()
                .any(|target| target.starts_with(if prefix == "src/" {
                    "[second]/"
                } else {
                    "src/"
                })),
            "cross-project target: {targets:?}"
        );
        assert_eq!(
            persisted_import(&conn, consumer, 6, "absent").0,
            "unresolved"
        );
        let (status, targets) = persisted_import(&conn, consumer, 7, "VALUE");
        assert!(
            matches!(status.as_str(), "unresolved" | "ambiguous"),
            "duplicate: {status} {targets:?}"
        );
        assert!(
            !targets
                .iter()
                .any(|target| target.ends_with("duplicate.py:VALUE")),
            "duplicate selected a target: {targets:?}"
        );
        let (status, targets) = persisted_import(&conn, consumer, 8, "CONSTANT");
        assert!(
            matches!(status.as_str(), "unresolved" | "ambiguous"),
            "rebound: {status} {targets:?}"
        );
        assert!(
            !targets
                .iter()
                .any(|target| target.ends_with("catalog.py:CONSTANT")),
            "rebound import retained prior target: {targets:?}"
        );
        assert!(
            !targets
                .iter()
                .any(|target| target.ends_with("rebound.py:CONSTANT")),
            "rebound import selected replacement arbitrarily: {targets:?}"
        );
        for (line, name) in [(9, "Foo"), (10, "make")] {
            let (status, targets) = persisted_import(&conn, consumer, line, name);
            assert!(
                matches!(status.as_str(), "unresolved" | "ambiguous"),
                "rebound declaration {name}: {status} {targets:?}"
            );
            assert!(
                !targets
                    .iter()
                    .any(|target| target.ends_with(&format!("rebound_declarations.py:{name}"))),
                "stale declaration selected: {targets:?}"
            );
        }
        let (status, targets) = persisted_import(&conn, consumer, 11, "PRE");
        assert_eq!(status, "resolved", "later class must own PRE: {targets:?}");
        assert!(
            targets.contains(&format!("{prefix}pkg/rebound_declarations.py:PRE")),
            "{targets:?}"
        );
        let class_targets: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE p.path=?1 AND e.line=11 AND e.kind='imports' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)=?2 AND dst.name='PRE' AND dst.kind='class'",
            rusqlite::params![consumer, format!("{prefix}pkg/rebound_declarations.py")], |row| row.get(0),
        ).unwrap();
        assert_eq!(
            class_targets, 1,
            "later class must be selected exactly once"
        );
        let (status, targets) = persisted_import(&conn, consumer, 12, "Foo");
        assert!(
            matches!(status.as_str(), "unresolved" | "ambiguous"),
            "rebound package Foo: {status} {targets:?}"
        );
        assert!(
            !targets
                .iter()
                .any(|target| target.starts_with(&format!("{prefix}rebound_pkg/Foo.py:"))),
            "rebound Foo selected child module: {targets:?}"
        );
    }
}

#[test]
fn module_assignment_aliases_to_imported_classes_keep_exact_origin() {
    let (primary, second) = linked_python_roots();
    for w in [&primary, &second] {
        w.write("src/pkg/__init__.py", "# package\n");
        w.write(
            "src/pkg/selection.py",
            "class Selection: pass\nclass PushRangeUnavailable: pass\n",
        );
        w.write("src/pkg/alternate.py", "class Selection: pass\n");
        w.write("src/pkg/facade.py", "from . import selection as _selection\nSelection = _selection.Selection\nPushRangeUnavailable = _selection.PushRangeUnavailable\n");
        w.write("src/pkg/rebound.py", "from . import selection as _selection\n_selection = None\nSelection = _selection.Selection\n");
        w.write("src/pkg/conditional.py", "try:\n    from . import selection as _selection\nexcept ImportError:\n    from . import alternate as _selection\nSelection = _selection.Selection\n");
        w.write(
            "src/pkg/late.py",
            "Selection = _selection.Selection\nfrom . import selection as _selection\n",
        );
        w.write(
            "src/pkg/same_line.py",
            "from . import selection as _selection; Selection = _selection.Selection\n",
        );
        w.write("src/consumer.py", "from pkg.facade import Selection\nfrom pkg.facade import PushRangeUnavailable\nfrom pkg.rebound import Selection as rebound\nfrom pkg.conditional import Selection as conditional\ndef build(): return Selection()\nfrom pkg.late import Selection as late\nfrom pkg.same_line import Selection as same_line\n");
    }
    let conn = primary.build();
    for (consumer, prefix) in [
        ("src/consumer.py", "src/"),
        ("[second]/src/consumer.py", "[second]/src/"),
    ] {
        for (line, name) in [(1, "Selection"), (2, "PushRangeUnavailable")] {
            let (status, targets) = persisted_import(&conn, consumer, line, name);
            assert_eq!(status, "resolved", "{consumer}:{line} {name}: {targets:?}");
            assert!(
                targets.contains(&format!("{prefix}pkg/selection.py:{name}")),
                "{consumer}:{line}: {targets:?}"
            );
            assert!(
                !targets
                    .iter()
                    .any(|target| target.starts_with(if prefix == "src/" {
                        "[second]/"
                    } else {
                        "src/"
                    })),
                "cross-project alias target: {targets:?}"
            );
        }
        let calls: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE p.path=?1 AND e.line=5 AND e.kind='calls' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)=?2 AND dst.name='Selection' AND dst.kind='class'",
            rusqlite::params![consumer, format!("{prefix}pkg/selection.py")], |row| row.get(0),
        ).unwrap();
        assert_eq!(calls, 1, "Selection() must call the indexed source class");
        for (line, name) in [(3, "Selection"), (4, "Selection"), (6, "Selection")] {
            let (status, targets) = persisted_import(&conn, consumer, line, name);
            assert!(
                matches!(status.as_str(), "unresolved" | "ambiguous"),
                "{consumer}:{line}: {status} {targets:?}"
            );
            assert!(
                !targets
                    .iter()
                    .any(|target| target.ends_with("selection.py:Selection")
                        || target.ends_with("alternate.py:Selection")),
                "unsupported alias selected source class: {targets:?}"
            );
        }
        let (status, targets) = persisted_import(&conn, consumer, 7, "Selection");
        assert_eq!(
            status, "resolved",
            "same-line import precedes alias assignment: {targets:?}"
        );
        assert!(
            targets.contains(&format!("{prefix}pkg/selection.py:Selection")),
            "{targets:?}"
        );
    }
}

#[test]
fn lazy_relative_package_exports_resolve_only_same_project_providers() {
    let (primary, second) = linked_python_roots();
    for w in [&primary, &second] {
        w.write("src/pkg/__init__.py", "from . import sibling\n__all__ = ['Item']\ndef __getattr__(name):\n    if name in __all__:\n        return getattr(sibling, name)\n    raise AttributeError(name)\n");
        w.write("src/pkg/sibling.py", "class Item: pass\n");
        w.write("src/pkg/facade.py", "from . import sibling\n__all__ = ['Item']\ndef __getattr__(name):\n    if name in __all__:\n        return getattr(sibling, name)\n    raise AttributeError(name)\n");
        w.write("src/pkg/direct.py", "from .sibling import Item\n__all__ = ['Item']\ndef __getattr__(name):\n    if name in __all__:\n        return Item\n    raise AttributeError(name)\n");
        w.write("src/pkg/rebound.py", "from . import sibling\nsibling = None\n__all__ = ['Item']\ndef __getattr__(name):\n    if name in __all__:\n        return getattr(sibling, name)\n    raise AttributeError(name)\n");
        w.write("src/pkg/missing.py", "from . import unavailable\n__all__ = ['Item']\ndef __getattr__(name):\n    if name in __all__:\n        return getattr(unavailable, name)\n    raise AttributeError(name)\n");
        w.write("src/pkg/dynamic.py", "from . import sibling\n__all__ = ['Item']\ndef __getattr__(name):\n    if name in __all__:\n        return getattr(sibling, name.lower())\n    raise AttributeError(name)\n");
        w.write("src/pkg/namespace/child.py", "def operation(): return 1\n");
        w.write(
            "src/pkg/namespace/child.pyi",
            "def operation() -> int: ...\n",
        );
        w.write(
            "src/pkg/importlib_facade/lazy_module.py",
            "class Child: pass\n",
        );
        w.write("src/pkg/importlib_facade/name.py", "class Nested: pass\n");
        w.write("src/pkg/name.py", "class WrongBase: pass\n");
        w.write("src/pkg/importlib_facade/__init__.py", "import importlib\n__all__ = ['lazy_module']\ndef __getattr__(name):\n    if name in __all__:\n        return importlib.import_module(f'.{name}', __name__)\n    raise AttributeError(name)\n");
        w.write("src/pkg/importlib_literal/__init__.py", "import importlib\n__all__ = ['name']\ndef __getattr__(export_name):\n    if export_name in __all__:\n        return importlib.import_module('.name', __name__)\n    raise AttributeError(export_name)\n");
        w.write(
            "src/pkg/importlib_literal/name.py",
            "class LiteralTarget: pass\n",
        );
        w.write("src/consumer.py", "from pkg import Item\nfrom pkg.facade import Item as facade_item\nfrom pkg.direct import Item as direct_item\nfrom pkg import Unknown\nfrom pkg.rebound import Item as rebound_item\nfrom pkg.missing import Item as missing_item\nfrom pkg.namespace import child\nfrom pkg.dynamic import Item as dynamic_item\nfrom pkg.importlib_facade import lazy_module\nfrom pkg.importlib_literal import name\n");
    }
    let conn = primary.build();
    for (consumer, prefix) in [
        ("src/consumer.py", "src/"),
        ("[second]/src/consumer.py", "[second]/src/"),
    ] {
        for (line, name) in [(1, "Item"), (2, "Item"), (3, "Item")] {
            let (status, targets) = persisted_import(&conn, consumer, line, name);
            assert_eq!(status, "resolved", "{consumer}:{line}: {targets:?}");
            assert!(
                targets.contains(&format!("{prefix}pkg/sibling.py:Item")),
                "{consumer}:{line}: {targets:?}"
            );
            assert!(
                !targets
                    .iter()
                    .any(|target| target.starts_with(if prefix == "src/" {
                        "[second]/"
                    } else {
                        "src/"
                    })),
                "cross-project target: {targets:?}"
            );
        }
        let (status, targets) = persisted_import(&conn, consumer, 7, "child");
        assert_eq!(
            status, "resolved",
            "{consumer} namespace child: {targets:?}"
        );
        assert!(
            targets
                .iter()
                .any(|target| target.starts_with(&format!("{prefix}pkg/namespace/child.py:"))),
            "{targets:?}"
        );
        assert!(
            !targets
                .iter()
                .any(|target| target.starts_with(&format!("{prefix}pkg/namespace/child.pyi:"))),
            "stub was selected over runtime: {targets:?}"
        );
        let (status, targets) = persisted_import(&conn, consumer, 9, "lazy_module");
        assert_eq!(
            status, "resolved",
            "{consumer} importlib module: {targets:?}"
        );
        assert!(
            targets.iter().any(|target| target
                .starts_with(&format!("{prefix}pkg/importlib_facade/lazy_module.py:"))),
            "{targets:?}"
        );
        let (status, targets) = persisted_import(&conn, consumer, 10, "name");
        assert_eq!(
            status, "resolved",
            "{consumer} literal importlib module: {targets:?}"
        );
        assert!(
            targets.iter().any(
                |target| target.starts_with(&format!("{prefix}pkg/importlib_literal/name.py:"))
            ),
            "{targets:?}"
        );
        assert!(
            !targets
                .iter()
                .any(|target| target.starts_with(&format!("{prefix}pkg/name.py:"))),
            "wrong importlib base: {targets:?}"
        );
        for (line, name) in [(4, "Unknown"), (5, "Item"), (6, "Item"), (8, "Item")] {
            let (status, targets) = persisted_import(&conn, consumer, line, name);
            assert_eq!(status, "unresolved", "{consumer}:{line}: {targets:?}");
            assert!(
                !targets
                    .iter()
                    .any(|target| target.ends_with("sibling.py:Item")),
                "{consumer}:{line}: {targets:?}"
            );
        }
    }
}

#[test]
fn lazy_exports_recognize_manifest_lists_and_attribute_access() {
    let source = r#"
__all__ = ['ItemA', 'ItemB']
def __getattr__(name):
    if name in __all__:
        import importlib
        return importlib.import_module(f".{name}", __name__)
    raise AttributeError(name)
"#;
    let facts = ast::extract("package/__init__.py", "python", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let init = facts.nodes.iter().find(|n| n.kind == "module").unwrap();
    if let Some(exports) = init.details.get("lazy_exports").and_then(|v| v.as_array()) {
        let names: Vec<_> = exports.iter().filter_map(|v| v.as_str()).collect();
        assert!(names.contains(&"ItemA"));
        assert!(names.contains(&"ItemB"));
    }
}

#[test]
fn package_reexports_and_monorepo_hubs_resolve_overloads_type_aliases_and_runtime_implementations()
{
    let types_src = r#"
from typing import TypeAlias
JsonPrimitive: TypeAlias = str | int
JsonDict: TypeAlias = dict[str, JsonPrimitive]
StructData = JsonDict
"#;
    let decorators_src = r#"
from typing import overload

@overload
def grpc_error_handler(method: None = None): ...

@overload
def grpc_error_handler(method: object): ...

def grpc_error_handler(method: object = None):
    return method
"#;
    let grpc_errors_src = r#"
from .grpc_error_decorators import grpc_error_handler
"#;
    let core_init_src = r#"
from .types import StructData
from .grpc_errors import grpc_error_handler
"#;
    let commerce_typing_src = r#"
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    class TypedTabularInline:
        """Type stub only."""
        pass
else:
    class TypedTabularInline:
        """Runtime implementation."""
        def helper(self):
            return 42
"#;
    let consumer_src = r#"
from contextunity.core import StructData, grpc_error_handler
from contextunity.commerce.typing import TypedTabularInline

def run(data: StructData):
    return grpc_error_handler(data)

def admin():
    return TypedTabularInline().helper()
"#;

    let mut facts_map = BTreeMap::new();
    facts_map.insert(
        "packages/core/src/contextunity/core/types.py".to_owned(),
        ast::extract(
            "packages/core/src/contextunity/core/types.py",
            "python",
            types_src,
        )
        .unwrap(),
    );
    facts_map.insert(
        "packages/core/src/contextunity/core/grpc_error_decorators.py".to_owned(),
        ast::extract(
            "packages/core/src/contextunity/core/grpc_error_decorators.py",
            "python",
            decorators_src,
        )
        .unwrap(),
    );
    facts_map.insert(
        "packages/core/src/contextunity/core/grpc_errors.py".to_owned(),
        ast::extract(
            "packages/core/src/contextunity/core/grpc_errors.py",
            "python",
            grpc_errors_src,
        )
        .unwrap(),
    );
    facts_map.insert(
        "packages/core/src/contextunity/core/__init__.py".to_owned(),
        ast::extract(
            "packages/core/src/contextunity/core/__init__.py",
            "python",
            core_init_src,
        )
        .unwrap(),
    );
    facts_map.insert(
        "extensions/commerce/src/contextunity/commerce/typing.py".to_owned(),
        ast::extract(
            "extensions/commerce/src/contextunity/commerce/typing.py",
            "python",
            commerce_typing_src,
        )
        .unwrap(),
    );
    facts_map.insert(
        "consumer.py".to_owned(),
        ast::extract("consumer.py", "python", consumer_src).unwrap(),
    );

    let graph = linker::link(&facts_map);
    for (name, line, provider) in [
        (
            "StructData",
            2,
            "packages/core/src/contextunity/core/types.py",
        ),
        (
            "grpc_error_handler",
            2,
            "packages/core/src/contextunity/core/grpc_error_decorators.py",
        ),
        (
            "TypedTabularInline",
            3,
            "extensions/commerce/src/contextunity/commerce/typing.py",
        ),
    ] {
        let definition = facts_map[provider]
            .nodes
            .iter()
            .filter(|node| node.name == name)
            .max_by_key(|node| node.line)
            .unwrap();
        assert!(
            graph.edges.iter().any(|edge| {
                edge.path == "consumer.py"
                    && edge.line == line
                    && edge.kind == "imports"
                    && edge.evidence == name
                    && edge.dst == definition.id
            }),
            "{name} must import its runtime definition from {provider}"
        );
    }

    let coverage_for = |expr: &str, line: usize| {
        graph
            .coverage
            .iter()
            .find(|c| c.path == "consumer.py" && c.expression == expr && c.line == line)
            .unwrap_or_else(|| panic!("missing coverage for {expr} at line {line} in consumer.py"))
    };

    // Import coverage
    let struct_data_import = coverage_for("StructData", 2);
    assert_eq!(
        struct_data_import.status, "resolved",
        "StructData import should resolve: {}",
        struct_data_import.evidence
    );

    let handler_import = coverage_for("grpc_error_handler", 2);
    assert_eq!(
        handler_import.status, "resolved",
        "grpc_error_handler import should resolve: {}",
        handler_import.evidence
    );

    let tabular_import = coverage_for("TypedTabularInline", 3);
    assert_eq!(
        tabular_import.status, "resolved",
        "TypedTabularInline import should resolve (not ambiguous): {}",
        tabular_import.evidence
    );

    // Call coverage
    let handler_call = coverage_for("grpc_error_handler", 6);
    assert_eq!(
        handler_call.status, "resolved",
        "grpc_error_handler call should resolve: {}",
        handler_call.evidence
    );

    let helper_call = coverage_for("TypedTabularInline().helper", 9);
    assert_eq!(
        helper_call.status, "resolved",
        "TypedTabularInline().helper call should resolve: {}",
        helper_call.evidence
    );
}

#[test]
fn unique_child_module_is_imported_and_its_calls_resolve() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.py", "def value(): return 1\n");
    w.write(
        "consumer.py",
        "from pkg import child\ndef consume(): return child.value()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "resolved");
    assert_eq!(import_targets(&conn), ["pkg", "pkg.child"]);
    assert_eq!(call_targets(&conn), ["pkg.child.value"]);
}

#[test]
fn ordinary_module_and_transitive_reexports_preserve_delta_resolution() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "from .api import Config\n");
    w.write("pkg/api.py", "from .models import Config\n");
    w.write("pkg/models.py", "class Config: pass\n");
    w.write("pkg/alternate.py", "class Config: pass\n");
    w.write(
        "consumer.py",
        "from pkg import Config\ndef consume(): return Config()\n",
    );
    w.build();
    assert_eq!(status(&w.open(), "Config"), "resolved");
    assert_eq!(call_target_paths(&w.open()), ["pkg/models.py"]);
    w.write("pkg/api.py", "from .alternate import Config\n");
    w.delta("pkg/api.py");
    assert_eq!(call_target_paths(&w.open()), ["pkg/alternate.py"]);
    w.assert_cold_equivalent();
    w.write(
        "consumer.py",
        "import pkg.api as api\ndef consume(): return api.Config()\n",
    );
    w.delta("consumer.py");
    assert_eq!(call_target_paths(&w.open()), ["pkg/alternate.py"]);
    w.assert_cold_equivalent();
}

#[test]
fn cyclic_reexports_and_missing_providers_remain_unresolved() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "from .a import Config\n");
    w.write("pkg/a.py", "from .b import Config\n");
    w.write("pkg/b.py", "from .a import Config\n");
    w.write(
        "consumer.py",
        "from pkg import Config\ndef consume(): return Config()\n",
    );
    w.build();
    assert_eq!(status(&w.open(), "Config"), "unresolved");
    assert!(call_target_paths(&w.open()).is_empty());
    w.write("pkg/a.py", "from .missing import Config\n");
    w.delta("pkg/a.py");
    assert_eq!(status(&w.open(), "Config"), "unresolved");
    w.assert_cold_equivalent();
}

#[test]
fn literal_lazy_exports_resolve_existing_providers_and_track_delta() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/models.py", "class Config: pass\n");
    w.write("pkg/api.py", "_EXPORTS = frozenset({'Config'})\ndef __getattr__(name):\n    if name in _EXPORTS:\n        from . import models\n        return getattr(models, name)\n    raise AttributeError(name)\n");
    w.write(
        "consumer.py",
        "from pkg.api import Config\ndef consume(): return Config()\n",
    );
    w.build();
    assert_eq!(status(&w.open(), "Config"), "resolved");
    assert_eq!(call_target_paths(&w.open()), ["pkg/models.py"]);
    w.write("pkg/api.py", "_EXPORTS = frozenset({'Config'})\ndef __getattr__(name):\n    if name in _EXPORTS:\n        from . import missing\n        return getattr(missing, name)\n    raise AttributeError(name)\n");
    w.delta("pkg/api.py");
    assert_eq!(status(&w.open(), "Config"), "unresolved");
    assert!(call_target_paths(&w.open()).is_empty());
    w.assert_cold_equivalent();
}

#[test]
fn lazy_importlib_module_relative_namespace_is_preserved_in_delta() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.py", "class Config: pass\n");
    w.write("child.py", "class Config: pass\n");
    w.write("pkg/service.py", "import importlib\ndef __getattr__(name):\n    if name == 'child':\n        return importlib.import_module('..child', __name__)\n    raise AttributeError(name)\n");
    w.write(
        "consumer.py",
        "from pkg.service import child\ndef consume(): return child.Config()\n",
    );
    w.build();
    assert_eq!(call_target_paths(&w.open()), ["pkg/child.py"]);
    w.write(
        "consumer.py",
        "from pkg.service import child\ndef consume():\n    return child.Config()\n",
    );
    w.delta("consumer.py");
    assert_eq!(call_target_paths(&w.open()), ["pkg/child.py"]);
    w.assert_cold_equivalent();
    w.write("pkg/service.py", "import importlib\ndef __getattr__(name):\n    if name == 'child':\n        return importlib.import_module('.child', __name__)\n    raise AttributeError(name)\n");
    w.delta("pkg/service.py");
    assert!(call_target_paths(&w.open()).is_empty());
    w.assert_cold_equivalent();
}

#[test]
fn src_layout_linked_reexports_preserve_owner_workspace_priority() {
    let w = Workspace::new();
    let linked = Workspace::new();
    linked.write("src/catalogue/__init__.py", "from .api import Config\n");
    linked.write("src/catalogue/api.py", "from .models import Config\n");
    linked.write("src/catalogue/models.py", "class Config: pass\n");
    w.write("forge-mcp.yaml", &format!("roots: [src]\nlinked_workspaces:\n  - name: catalogue\n    path: '{}'\n    roots: [src]\n", linked.0.display()));
    w.write(
        "src/consumer.py",
        "from catalogue.api import Config\ndef consume(): return Config()\n",
    );
    w.build();
    let targets = |conn: &Connection| {
        rows(conn, "SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edges e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE src.path_id=(SELECT path_id FROM path_dictionary WHERE path='src/consumer.py') AND e.kind='calls' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)")
    };
    assert_eq!(targets(&w.open()), ["[catalogue]/src/catalogue/models.py"]);
    w.write("src/catalogue/__init__.py", "from .api import Config\n");
    w.write("src/catalogue/api.py", "from .models import Config\n");
    w.write("src/catalogue/models.py", "class Config: pass\n");
    w.build();
    assert_eq!(targets(&w.open()), ["src/catalogue/models.py"]);
    w.write("src/catalogue/api.py", "from .missing import Config\n");
    w.delta("src/catalogue/api.py");
    assert!(targets(&w.open()).is_empty());
    w.assert_cold_equivalent();
}

#[test]
fn bare_dotted_import_does_not_export_child_under_package_binding() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.py", "def value(): return 1\n");
    w.write("api.py", "import pkg.child\n");
    w.write(
        "consumer.py",
        "from api import pkg\ndef consume(): return pkg.value()\n",
    );
    w.build();
    assert!(call_target_paths(&w.open()).is_empty());
    w.write("api.py", "import pkg.child as alias\n");
    w.write(
        "consumer.py",
        "from api import alias\ndef consume(): return alias.value()\n",
    );
    w.build();
    assert_eq!(call_target_paths(&w.open()), ["pkg/child.py"]);
}

#[test]
fn reassigned_module_export_does_not_retain_original_transitive_target() {
    let w = Workspace::new();
    w.write("provider.py", "class Config: pass\n");
    w.write("api.py", "from provider import Config\n");
    w.write("outer.py", "from api import Config\n");
    w.write(
        "consumer.py",
        "from outer import Config\ndef consume(): return Config()\n",
    );
    w.build();
    assert_eq!(call_target_paths(&w.open()), ["provider.py"]);
    w.write(
        "api.py",
        "from provider import Config\ndef factory(): return None\nConfig = factory()\n",
    );
    w.delta("api.py");
    assert!(call_target_paths(&w.open()).is_empty());
    w.assert_cold_equivalent();
    w.write("api.py", "from provider import Config\n");
    w.delta("api.py");
    assert_eq!(call_target_paths(&w.open()), ["provider.py"]);
    w.assert_cold_equivalent();
}

#[test]
fn explicit_package_reexport_resolves_import_and_call() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "from .logging import get_log\n");
    w.write("pkg/logging.py", "def get_log(): return 1\n");
    w.write("other.py", "def helper(): return 2\n");
    w.write(
        "consumer.py",
        "from pkg import get_log\ndef consume(): return get_log()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "get_log"), "resolved");
    assert_eq!(
        import_target_paths(&conn),
        ["pkg/__init__.py", "pkg/logging.py"]
    );
    assert_eq!(call_targets(&conn), ["pkg.logging.get_log"]);
    drop(conn);

    w.write("pkg/__init__.py", "# export removed\n");
    w.delta("pkg/__init__.py");
    assert_eq!(status(&w.open(), "get_log"), "unresolved");
    w.assert_cold_equivalent();
    w.write("pkg/__init__.py", "from .logging import get_log\n");
    w.delta("pkg/__init__.py");
    assert_eq!(status(&w.open(), "get_log"), "resolved");
    w.assert_cold_equivalent();
    w.write("other.py", "def get_log(): return 2\n");
    w.delta("other.py");
    assert_eq!(status(&w.open(), "get_log"), "resolved");
    w.assert_cold_equivalent();
}

#[test]
fn duplicate_package_reexports_do_not_choose_a_target() {
    let w = Workspace::new();
    w.write(
        "pkg/__init__.py",
        "from .a import get_log\nfrom .b import get_log\n",
    );
    w.write("pkg/a.py", "def get_log(): return 1\n");
    w.write("pkg/b.py", "def get_log(): return 2\n");
    w.write(
        "consumer.py",
        "from pkg import get_log\ndef consume(): return get_log()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "get_log"), "unresolved");
    assert!(call_targets(&conn).is_empty());
}

#[test]
fn paired_runtime_and_stub_choose_runtime_child() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.py", "def value(): return 1\n");
    w.write("pkg/child.pyi", "def value() -> int: ...\n");
    w.write(
        "consumer.py",
        "from pkg import child\ndef consume(): return child.value()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "resolved");
    assert_eq!(
        import_target_paths(&conn),
        ["pkg/__init__.py", "pkg/child.py"]
    );
    assert_eq!(call_targets(&conn), ["pkg.child.value"]);
    let stub: i64 = conn
        .query_row(
            "SELECT count(*) FROM nodes WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path='pkg/child.pyi')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(stub > 0);
}

#[test]
fn paired_runtime_and_stub_choose_runtime_for_direct_module_import() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.py", "def value(): return 1\n");
    w.write("pkg/child.pyi", "def value() -> int: ...\n");
    w.write(
        "consumer.py",
        "import pkg.child as child\ndef consume(): return child.value()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "pkg.child"), "resolved");
    assert_eq!(import_target_paths(&conn), ["pkg/child.py"]);
    assert_eq!(call_targets(&conn), ["pkg.child.value"]);
}

#[test]
fn paired_stub_supplies_declaration_missing_from_generated_runtime() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.py", "def __getattr__(name): return None\n");
    w.write(
        "pkg/child.pyi",
        "class ContextUnit:\n    def value(self) -> int: ...\n",
    );
    w.write(
        "consumer.py",
        "from pkg.child import ContextUnit\ndef consume(): return ContextUnit()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "ContextUnit"), "resolved");
    assert_eq!(
        import_target_paths(&conn),
        ["pkg/child.py", "pkg/child.pyi"]
    );
    assert_eq!(call_target_paths(&conn), ["pkg/child.pyi"]);
    let evidence: String = conn.query_row("SELECT (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.py' AND line=1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='ContextUnit'", [], |row| row.get(0)).unwrap();
    assert!(evidence.contains("type stub"), "{evidence}");
    drop(conn);
    w.write("pkg/child.py", "def __getattr__(name): return name\n");
    w.delta("pkg/child.py");
    w.assert_cold_equivalent();
    w.write("pkg/child.py", "def __getattr__(name): return None\n");
    w.delta("pkg/child.py");
    w.assert_cold_equivalent();
}

#[test]
fn child_module_call_uses_stub_only_member() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.py", "def __getattr__(name): return None\n");
    w.write("pkg/child.pyi", "class ContextUnit: ...\n");
    w.write(
        "consumer.py",
        "from pkg import child\ndef consume(): return child.ContextUnit()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "resolved");
    assert_eq!(
        import_target_paths(&conn),
        ["pkg/__init__.py", "pkg/child.py"]
    );
    assert_eq!(call_target_paths(&conn), ["pkg/child.pyi"]);
}

#[test]
fn stub_only_module_remains_navigable() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.pyi", "def value() -> int: ...\n");
    w.write(
        "consumer.py",
        "from pkg import child\ndef consume(): return child.value()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "resolved");
    assert_eq!(
        import_target_paths(&conn),
        ["pkg/__init__.py", "pkg/child.pyi"]
    );
    assert_eq!(call_targets(&conn), ["pkg.child.value"]);
}

#[test]
fn multiple_runtime_modules_remain_ambiguous() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.py", "def value(): return 1\n");
    w.write("src/pkg/child.py", "def value(): return 2\n");
    w.write(
        "consumer.py",
        "from pkg import child\ndef consume(): return child.value()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "ambiguous");
    assert_eq!(import_target_paths(&conn), ["pkg/__init__.py"]);
    assert!(call_targets(&conn).is_empty());
}

#[test]
fn package_symbol_takes_precedence_over_same_named_child_module() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "def child(): return 1\n");
    w.write("pkg/child.py", "def value(): return 2\n");
    w.write(
        "consumer.py",
        "from pkg import child\ndef consume(): return child()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "resolved");
    assert_eq!(import_targets(&conn), ["pkg", "pkg.child"]);
    assert_eq!(
        import_target_paths(&conn),
        ["pkg/__init__.py", "pkg/__init__.py"]
    );
    assert_eq!(call_targets(&conn), ["pkg.child"]);
}

#[test]
fn unrelated_child_module_is_not_a_provider() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("other/child.py", "def value(): return 1\n");
    w.write(
        "consumer.py",
        "from pkg import child\ndef consume(): return child.value()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "unresolved");
    assert_eq!(import_targets(&conn), ["pkg"]);
    assert!(call_targets(&conn).is_empty());
}

#[test]
fn child_edit_and_restore_match_cold_graph() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.py", "def value(): return 1\n");
    w.write("pkg/child.pyi", "def value() -> int: ...\n");
    w.write(
        "consumer.py",
        "from pkg import child\ndef consume(): return child.value()\n",
    );
    w.build();
    w.write("pkg/child.py", "def value(arg=1): return arg\n");
    w.delta("pkg/child.py");
    w.assert_cold_equivalent();
    w.write("pkg/child.py", "def value(): return 1\n");
    w.delta("pkg/child.py");
    w.assert_cold_equivalent();
}
