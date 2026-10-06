#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::{
    core::models::ReceiverHint,
    engine::{ast, linker},
};
use serde_json::json;
use std::collections::BTreeMap;
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct PythonWorkspace(PathBuf);

impl PythonWorkspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "forge_python_semantics_{}_{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn write(&self, path: &str, source: &str) {
        let target = self.0.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, source).unwrap();
    }

    fn build(&self) -> rusqlite::Connection {
        let db = self.0.join(".forge/code-map.sqlite");
        contextunity_forge_mcp::db::writer::build(&self.0, &db, None).unwrap();
        contextunity_forge_mcp::db::reader::open(&db, &self.0).unwrap()
    }
}

impl Drop for PythonWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn linked_python_roots() -> (PythonWorkspace, PythonWorkspace) {
    let primary = PythonWorkspace::new();
    let second = PythonWorkspace::new();
    primary.write(
        "forge-mcp.yaml",
        &format!(
        "roots: [src]\nlinked_workspaces:\n  - name: second\n    path: '{}'\n    roots: [src]\n",
        second.0.display()
    ),
    );
    (primary, second)
}

fn persisted_import(
    conn: &rusqlite::Connection,
    path: &str,
    line: i64,
    name: &str,
) -> (String, Vec<String>) {
    let status = conn.query_row(
        "SELECT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND c.line=?2 AND x.expression=?3",
        rusqlite::params![path, line, name], |row| row.get(0),
    ).unwrap_or_else(|_| panic!("missing import coverage {path}:{line} {name}"));
    let targets = conn.prepare(
        "SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) || ':' || dst.name FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes dst ON dst.node_hash=e.dst_hash JOIN coverage_evidence v ON v.evidence_id=e.evidence_id WHERE p.path=?1 AND e.line=?2 AND e.kind='imports' AND v.evidence=?3 ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id),dst.name",
    ).unwrap().query_map(rusqlite::params![path, line, name], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
    (status, targets)
}

fn persisted_status(
    conn: &rusqlite::Connection,
    path: &str,
    line: i64,
    expression: &str,
) -> String {
    conn.query_row(
        "SELECT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND c.line=?2 AND x.expression=?3",
        rusqlite::params![path, line, expression], |row| row.get(0),
    ).unwrap_or_else(|_| panic!("missing coverage {path}:{line} {expression}"))
}

fn persisted_coverage(
    conn: &rusqlite::Connection,
    path: &str,
    line: i64,
    expression: &str,
) -> (String, String) {
    conn.query_row(
        "SELECT c.status,e.evidence FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence e ON e.evidence_id=c.evidence_id WHERE p.path=?1 AND c.line=?2 AND x.expression=?3",
        rusqlite::params![path, line, expression], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap_or_else(|_| panic!("missing coverage {path}:{line} {expression}"))
}

fn persisted_status_rows(
    conn: &rusqlite::Connection,
    path: &str,
    line: i64,
    expression: &str,
) -> Vec<String> {
    conn.prepare(
        "SELECT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND c.line=?2 AND x.expression=?3 ORDER BY c.status",
    ).unwrap().query_map(rusqlite::params![path, line, expression], |row| row.get(0)).unwrap().map(Result::unwrap).collect()
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
fn typed_parameters_preserve_forward_qualified_and_default_types_in_their_scope() {
    let source = "class Service:\n    def execute(self): pass\ndef run(service: Service, forward: 'pkg.Service', default: pkg.Service = None, *items: Service, **options: 'pkg.Service'):\n    def nested(service: Other): pass\n    return service.execute()\nclass Client:\n    def call(self, service: Service): return service.execute()\ndef untyped(service): return service.execute()\n";
    let facts = ast::extract("parameters.py", "python", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let node = |name| facts.nodes.iter().find(|node| node.name == name).unwrap();
    assert_eq!(
        node("run").details["param_types"],
        json!({"service":"Service", "forward":"pkg.Service", "default":"pkg.Service"})
    );
    assert_eq!(
        node("nested").details["param_types"],
        json!({"service":"Other"})
    );
    assert_eq!(
        node("call").details["param_types"],
        json!({"service":"Service"})
    );
    assert!(node("untyped").details.get("param_types").is_none());
}

#[test]
fn class_assignments_are_bindings_without_leaking_nested_callable_assignments() {
    let facts = ast::extract("class_bindings.py", "python", "class Model:\n    objects = factory()\n    execute: object = custom\n    def method(self):\n        local = 1\n    class Nested:\n        nested = 1\n").unwrap();
    let model = facts
        .nodes
        .iter()
        .find(|node| node.name == "Model")
        .unwrap();
    assert_eq!(model.details["bindings"], json!(["execute", "objects"]));
}

#[test]
fn computed_receiver_hints_are_structural_and_preserve_original_expression() {
    let source = "def run():\n    'value'.upper()\n    r'value'.strip()\n    b'value'.upper()\n    f'{value}'.upper()\n    Widget().execute()\n    package.Widget().execute()\n    super().execute()\n    super(Widget, self).execute()\n    response.request().consume()\n    factory().request().consume()\n";
    let facts = ast::extract("computed.py", "python", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let hint = |expression| {
        &facts
            .references
            .iter()
            .find(|reference| reference.expression == expression)
            .unwrap()
            .receiver_hint
    };
    assert!(
        matches!(hint("'value'.upper"), Some(ReceiverHint::StringLiteral { member }) if member == "upper")
    );
    assert!(
        matches!(hint("r'value'.strip"), Some(ReceiverHint::StringLiteral { member }) if member == "strip")
    );
    assert!(hint("b'value'.upper").is_none());
    assert!(hint("f'{value}'.upper").is_none());
    assert!(
        matches!(hint("Widget().execute"), Some(ReceiverHint::CallResult { callee, member }) if callee == "Widget" && member == "execute")
    );
    assert!(
        matches!(hint("package.Widget().execute"), Some(ReceiverHint::CallResult { callee, .. }) if callee == "package.Widget")
    );
    assert!(
        matches!(hint("super().execute"), Some(ReceiverHint::Super { member }) if member == "execute")
    );
    assert!(hint("super(Widget, self).execute").is_none());
    assert!(
        matches!(hint("response.request().consume"), Some(ReceiverHint::CallResult { callee, .. }) if callee == "response.request")
    );
    assert!(hint("factory().request().consume").is_none());
}

#[test]
fn verified_constructor_super_and_known_string_methods_resolve_at_the_linker() {
    let source = "class Base:\n    def execute(self): pass\nclass Child(Base):\n    def execute(self): pass\n    def run(self): return super().execute()\ndef direct(): return Base().execute()\ndef literal(): return 'value'.upper()\ndef invalid(): return 'value'.invented_method()\n";
    let facts = ast::extract("resolved.py", "python", source).unwrap();
    let base_method = facts
        .nodes
        .iter()
        .find(|node| node.qualname == "resolved.Base.execute")
        .unwrap()
        .id
        .clone();
    let graph = linker::link(&BTreeMap::from([("resolved.py".to_owned(), facts)]));
    for expression in ["super().execute", "Base().execute", "'value'.upper"] {
        assert_eq!(
            graph
                .coverage
                .iter()
                .find(|coverage| coverage.expression == expression)
                .unwrap()
                .status,
            "resolved",
            "{expression}"
        );
    }
    assert_eq!(
        graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == "'value'.invented_method")
            .unwrap()
            .status,
        "unresolved"
    );
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == "calls" && edge.dst == base_method)
            .count(),
        2
    );
}

#[test]
fn local_receiver_dictionary_and_logger_methods_resolve_in_function_scope() {
    let source = r#"
from typing import Mapping
import logging

def decode(raw: Mapping[str, object], logger: logging.LoggerAdapter):
    val = raw.get("field")
    items = raw.items()
    keys = raw.keys()
    values = raw.values()
    logger.info("decoded")
    logger.warning("retry")
    logger.debug("details")
    logger.error("failed")

def helper():
    row = {"id": 1}
    row.get("id")
    row.keys()
    row.values()
    row.update({"next": 2})
    return row.pop("id")

class LocalMapping:
    def get(self, key): return key

def local(mapping: LocalMapping):
    return mapping.get("field")

def conventional(logger, log):
    logger.exception("failed")
    log.debug("details")
"#;
    let facts = ast::extract("decode.py", "python", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let local_get = facts
        .nodes
        .iter()
        .find(|node| node.qualname == "decode.LocalMapping.get")
        .unwrap()
        .id
        .clone();
    let graph = linker::link(&BTreeMap::from([("decode.py".to_owned(), facts)]));
    let local = graph
        .coverage
        .iter()
        .find(|coverage| coverage.expression == "mapping.get")
        .unwrap();
    assert_eq!(local.status, "resolved", "{local:#?}");
    assert!(graph.edges.iter().any(|edge| edge.kind == "calls"
        && edge.evidence == "mapping.get"
        && edge.dst == local_get));
    for expression in [
        "raw.get",
        "raw.items",
        "raw.keys",
        "raw.values",
        "row.get",
        "row.keys",
        "row.values",
        "row.update",
        "row.pop",
    ] {
        let coverage = graph
            .coverage
            .iter()
            .find(|c| c.expression == expression)
            .unwrap_or_else(|| panic!("missing coverage for {expression}"));
        assert_eq!(
            coverage.status, "external",
            "expression {expression} should be external, evidence: {}",
            coverage.evidence
        );
        assert!(
            coverage.evidence.contains("Python standard library"),
            "{expression}: {}",
            coverage.evidence
        );
    }
    for expression in [
        "logger.info",
        "logger.warning",
        "logger.debug",
        "logger.error",
    ] {
        let coverage = graph
            .coverage
            .iter()
            .find(|c| c.expression == expression)
            .unwrap_or_else(|| panic!("missing coverage for {expression}"));
        assert_eq!(
            coverage.status, "external",
            "expression {expression} should be external, evidence: {}",
            coverage.evidence
        );
        assert!(coverage.evidence.contains("logging.LoggerAdapter"));
        assert!(
            coverage.evidence.contains("line 3"),
            "{}",
            coverage.evidence
        );
    }
    for expression in ["logger.exception", "log.debug"] {
        let coverage = graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == expression)
            .unwrap();
        assert_eq!(coverage.status, "unresolved", "{expression}: {coverage:#?}");
    }
}

#[test]
fn untyped_logger_parameters_remain_unresolved_in_persisted_coverage() {
    let w = PythonWorkspace::new();
    w.write("untyped.py", "def conventional(logger, log):\n    logger.exception('failed')\n    log.debug('details')\n");
    let conn = w.build();
    for (line, expression) in [(2, "logger.exception"), (3, "log.debug")] {
        assert_eq!(
            persisted_status(&conn, "untyped.py", line, expression),
            "unresolved",
            "{expression}"
        );
    }
}

#[test]
fn logging_factory_methods_have_verified_builtin_origin_after_persistence() {
    let w = PythonWorkspace::new();
    let source = "import logging\nlogging.basicConfig(level=logging.INFO)\nlogger = logging.getLogger(__name__)\nlogger.info('i')\nlogger.warning('w')\nlogger.error('e')\nlogger.debug('d')\nlogger.exception('x')\ndef unknown(logger):\n    logger.info('x')\n";
    w.write("src/factory.py", source);
    w.write("src/rebound.py", "import logging\nlogging = object()\nlogger = logging.getLogger(__name__)\nlogger.info('x')\n");
    let db = w.0.join(".forge/code-map.sqlite");
    let check = |conn: &rusqlite::Connection| {
        for (line, method) in [
            (4, "info"),
            (5, "warning"),
            (6, "error"),
            (7, "debug"),
            (8, "exception"),
        ] {
            let expression = format!("logger.{method}");
            let (status, evidence) = persisted_coverage(conn, "src/factory.py", line, &expression);
            assert_eq!(status, "external", "{expression}: {evidence}");
            assert_eq!(
                evidence,
                format!("builtin:logging.Logger built-in: {expression}")
            );
        }
        assert_eq!(
            persisted_status(conn, "src/factory.py", 10, "logger.info"),
            "unresolved"
        );
        assert_eq!(
            persisted_status(conn, "src/rebound.py", 4, "logger.info"),
            "unresolved"
        );
    };
    contextunity_forge_mcp::db::writer::build(&w.0, &db, None).unwrap();
    let cold = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();
    check(&cold);
    drop(cold);
    w.write("src/factory.py", &format!("{source}# index delta\n"));
    contextunity_forge_mcp::db::writer::delta(&w.0, &db, &[PathBuf::from("src/factory.py")])
        .unwrap();
    let delta = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();
    check(&delta);
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
fn with_statement_as_target_uses_the_context_value() {
    let source = "class Conn:\n    def execute(self):\n        return 1\n\ndef use():\n    with Conn() as db:\n        db.execute()\n    db.execute()\n";
    let facts = ast::extract("ctx.py", "python", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let execute = facts
        .nodes
        .iter()
        .find(|node| node.qualname == "ctx.Conn.execute")
        .unwrap()
        .id
        .clone();
    let graph = linker::link(&BTreeMap::from([("ctx.py".to_owned(), facts)]));
    let rows: Vec<_> = graph
        .coverage
        .iter()
        .filter(|coverage| coverage.expression == "db.execute")
        .collect();
    assert_eq!(rows.len(), 2, "{rows:#?}");
    assert!(
        rows.iter().all(|coverage| coverage.status == "resolved"),
        "{rows:#?}"
    );
    let calls = graph
        .edges
        .iter()
        .filter(|edge| edge.kind == "calls" && edge.evidence == "db.execute" && edge.dst == execute)
        .count();
    assert_eq!(calls, 2);
}

#[test]
fn stdlib_sqlite_and_logger_adapter_receivers_resolve_persisted_coverage() {
    let w = PythonWorkspace::new();
    let source = "import sqlite3\nimport logging\n\ndef run(conn: sqlite3.Connection, adapter: logging.LoggerAdapter):\n    conn.execute('SELECT 1')\n    conn.commit()\n    conn.rollback()\n    conn.cursor()\n    conn.close()\n    conn.unknown_method()\n    adapter.info('i')\n    adapter.warning('w')\n    adapter.error('e')\n    adapter.debug('d')\n    adapter.critical('c')\n    adapter.exception('x')\n    adapter.unknown_call()\n";
    w.write("src/service.py", source);
    let db = w.0.join(".forge/code-map.sqlite");
    contextunity_forge_mcp::db::writer::build(&w.0, &db, None).unwrap();
    let reader = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();

    for (line, method) in [
        (5, "execute"),
        (6, "commit"),
        (7, "rollback"),
        (8, "cursor"),
        (9, "close"),
    ] {
        let expression = format!("conn.{method}");
        let (status, evidence) = persisted_coverage(&reader, "src/service.py", line, &expression);
        assert_eq!(status, "external", "{expression}: {evidence}");
        assert!(
            evidence.contains("builtin:sqlite3"),
            "{expression}: {evidence}"
        );
    }
    assert_eq!(
        persisted_status(&reader, "src/service.py", 10, "conn.unknown_method"),
        "unresolved"
    );

    for (line, method) in [
        (11, "info"),
        (12, "warning"),
        (13, "error"),
        (14, "debug"),
        (15, "critical"),
        (16, "exception"),
    ] {
        let expression = format!("adapter.{method}");
        let (status, evidence) = persisted_coverage(&reader, "src/service.py", line, &expression);
        assert_eq!(status, "external", "{expression}: {evidence}");
        assert!(
            evidence.contains("builtin:logging.LoggerAdapter"),
            "{expression}: {evidence}"
        );
    }
    assert_eq!(
        persisted_status(&reader, "src/service.py", 17, "adapter.unknown_call"),
        "unresolved"
    );
}

#[test]
fn inherited_self_member_resolves_cross_file_mixin_methods() {
    let w = PythonWorkspace::new();
    w.write("src/pkg/__init__.py", "");
    w.write(
        "src/pkg/connection.py",
        "class SqliteConnectionMixin:\n    def _get_connection(self):\n        return self\n",
    );
    w.write(
        "src/pkg/projection.py",
        "from .connection import SqliteConnectionMixin\nclass SqliteCellEdgeProjectionLayer(SqliteConnectionMixin):\n    pass\n",
    );
    w.write(
        "src/pkg/validation.py",
        "from .projection import SqliteCellEdgeProjectionLayer\nclass SqliteCellEdgeValidationLayer(SqliteCellEdgeProjectionLayer):\n    pass\n",
    );
    w.write(
        "src/pkg/mutations.py",
        "from .validation import SqliteCellEdgeValidationLayer\nclass SqliteCellEdgeMutationLayer(SqliteCellEdgeValidationLayer, UnindexedBase):\n    def execute(self):\n        self._get_connection()\n        self._missing_method()\n",
    );
    let db = w.0.join(".forge/code-map.sqlite");
    contextunity_forge_mcp::db::writer::build(&w.0, &db, None).unwrap();
    let reader = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();

    let (status, evidence) =
        persisted_coverage(&reader, "src/pkg/mutations.py", 4, "self._get_connection");
    assert_eq!(status, "resolved", "{evidence}");
    assert_eq!(
        persisted_status(&reader, "src/pkg/mutations.py", 5, "self._missing_method"),
        "unresolved"
    );
}

#[test]
fn call_return_type_and_context_manager_propagation_resolves_persisted_coverage() {
    let w = PythonWorkspace::new();
    w.write(
        "src/db_factory.py",
        "import sqlite3\n\ndef get_db() -> sqlite3.Connection:\n    return sqlite3.connect(':memory:')\n\ndef unannotated_db():\n    return sqlite3.connect(':memory:')\n",
    );
    w.write(
        "src/consumer.py",
        "import sqlite3\nfrom db_factory import get_db, unannotated_db\n\ndef run():\n    with get_db() as db:\n        db.execute('SELECT 1')\n    with unannotated_db() as raw:\n        raw.execute('SELECT 2')\n    conn = get_db()\n    conn.execute('SELECT 3')\n",
    );
    w.write(
        "src/mixin_consumer.py",
        "import sqlite3\nclass BaseRepo:\n    def get_conn(self) -> sqlite3.Connection:\n        return sqlite3.connect(':memory:')\n\nclass ChildRepo(BaseRepo):\n    def perform(self):\n        with self.get_conn() as db:\n            db.execute('SELECT 4')\n",
    );
    let db = w.0.join(".forge/code-map.sqlite");
    contextunity_forge_mcp::db::writer::build(&w.0, &db, None).unwrap();
    let reader = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();

    // 1. with get_db() as db: db.execute(...) -> external builtin:sqlite3
    let (status, evidence) = persisted_coverage(&reader, "src/consumer.py", 6, "db.execute");
    assert_eq!(status, "external", "db.execute: {evidence}");
    assert!(
        evidence.contains("builtin:sqlite3"),
        "db.execute evidence: {evidence}"
    );

    // 2. with unannotated_db() as raw: raw.execute(...) -> stays unresolved
    assert_eq!(
        persisted_status(&reader, "src/consumer.py", 8, "raw.execute"),
        "unresolved"
    );

    // 3. conn = get_db(); conn.execute(...) -> external builtin:sqlite3
    let (status, evidence) = persisted_coverage(&reader, "src/consumer.py", 10, "conn.execute");
    assert_eq!(status, "external", "conn.execute: {evidence}");
    assert!(
        evidence.contains("builtin:sqlite3"),
        "conn.execute evidence: {evidence}"
    );

    // 4. with self.get_conn() as db: db.execute(...) in subclass -> external builtin:sqlite3
    let (status, evidence) = persisted_coverage(&reader, "src/mixin_consumer.py", 9, "db.execute");
    assert_eq!(status, "external", "self.get_conn() db.execute: {evidence}");
    assert!(
        evidence.contains("builtin:sqlite3"),
        "self.get_conn() db.execute evidence: {evidence}"
    );
}

#[test]
fn loop_iterable_annotation_element_inference_resolves_persisted_coverage() {
    let w = PythonWorkspace::new();
    w.write(
        "src/loop_service.py",
        "from typing import Sequence, Mapping, Iterable\n\ndef process_list(items: list[dict]):\n    for row in items:\n        row.get('key')\n\ndef process_seq(items: Sequence[Mapping]):\n    for row in items:\n        row.get('key')\n\ndef process_iter(items: Iterable[dict]):\n    for row in items:\n        row.get('key')\n\ndef process_untyped(items):\n    for row in items:\n        row.get('key')\n",
    );
    let db = w.0.join(".forge/code-map.sqlite");
    contextunity_forge_mcp::db::writer::build(&w.0, &db, None).unwrap();
    let reader = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();

    // 1. list[dict] -> row.get is external builtin:dict
    let (status, evidence) = persisted_coverage(&reader, "src/loop_service.py", 5, "row.get");
    assert_eq!(status, "external", "process_list row.get: {evidence}");
    assert!(
        evidence.contains("Python standard library"),
        "process_list evidence: {evidence}"
    );

    // 2. Sequence[Mapping] -> row.get is external builtin:Mapping
    let (status, evidence) = persisted_coverage(&reader, "src/loop_service.py", 9, "row.get");
    assert_eq!(status, "external", "process_seq row.get: {evidence}");
    assert!(
        evidence.contains("Python standard library"),
        "process_seq evidence: {evidence}"
    );

    // 3. Iterable[dict] -> row.get is external builtin:dict
    let (status, evidence) = persisted_coverage(&reader, "src/loop_service.py", 13, "row.get");
    assert_eq!(status, "external", "process_iter row.get: {evidence}");
    assert!(
        evidence.contains("Python standard library"),
        "process_iter evidence: {evidence}"
    );

    // 4. untyped -> row.get stays unresolved
    assert_eq!(
        persisted_status(&reader, "src/loop_service.py", 17, "row.get"),
        "unresolved"
    );
}

#[test]
fn loop_element_types_stop_at_rebind_and_shadowed_sequence() {
    let w = PythonWorkspace::new();
    w.write(
        "src/loop_guard.py",
        "class Sequence:\n    pass\n\ndef rebound(items: list[dict]):\n    items = None\n    for row in items:\n        row.get('key')\n\ndef shadowed(items: Sequence[dict]):\n    for row in items:\n        row.get('key')\n",
    );
    let db = w.0.join(".forge/code-map.sqlite");
    contextunity_forge_mcp::db::writer::build(&w.0, &db, None).unwrap();
    let reader = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();
    assert_eq!(
        persisted_status(&reader, "src/loop_guard.py", 7, "row.get"),
        "unresolved"
    );
    assert_eq!(
        persisted_status(&reader, "src/loop_guard.py", 11, "row.get"),
        "unresolved"
    );
}

#[test]
fn nested_self_and_unbound_cls_do_not_take_the_enclosing_class() {
    let w = PythonWorkspace::new();
    w.write(
        "src/repo.py",
        "import sqlite3\n\nclass Repo:\n    def connect(cls) -> sqlite3.Connection:\n        return sqlite3.connect(':memory:')\n\n    def save(self) -> sqlite3.Connection:\n        return sqlite3.connect(':memory:')\n\n    def run(self):\n        def helper(self):\n            nested = self.save()\n            nested.execute('SELECT 1')\n        borrowed = cls.connect()\n        borrowed.execute('SELECT 2')\n\n    def make(cls):\n        owned = cls.connect()\n        owned.execute('SELECT 3')\n",
    );
    let db = w.0.join(".forge/code-map.sqlite");
    contextunity_forge_mcp::db::writer::build(&w.0, &db, None).unwrap();
    let reader = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();
    assert_eq!(
        persisted_status(&reader, "src/repo.py", 13, "nested.execute"),
        "unresolved"
    );
    assert_eq!(
        persisted_status(&reader, "src/repo.py", 15, "borrowed.execute"),
        "unresolved"
    );
    let (status, evidence) = persisted_coverage(&reader, "src/repo.py", 19, "owned.execute");
    assert_eq!(status, "external", "{evidence}");
    assert!(evidence.contains("builtin:sqlite3"), "{evidence}");
}

#[test]
fn logger_process_is_not_a_logger_method() {
    let w = PythonWorkspace::new();
    w.write(
        "src/log_service.py",
        "import logging\n\ndef run():\n    log = logging.getLogger('app')\n    log.info('ok')\n    log.process('no')\n",
    );
    let db = w.0.join(".forge/code-map.sqlite");
    contextunity_forge_mcp::db::writer::build(&w.0, &db, None).unwrap();
    let reader = contextunity_forge_mcp::db::reader::open(&db, &w.0).unwrap();
    let (status, evidence) = persisted_coverage(&reader, "src/log_service.py", 5, "log.info");
    assert_eq!(status, "external", "{evidence}");
    assert!(evidence.contains("builtin:logging.Logger"), "{evidence}");
    assert_eq!(
        persisted_status(&reader, "src/log_service.py", 6, "log.process"),
        "unresolved"
    );
}

#[test]
fn global_name_declaration_does_not_mask_same_named_instance_field() {
    let workspace = PythonWorkspace::new();
    workspace.write(
        "src/fields.py",
        "class Client:\n    def run(self):\n        pass\n\nclass Box:\n    def __init__(self):\n        global client\n        self.client = Client()\n\ndef invoke(box: Box):\n    box.client.run()\n",
    );
    workspace.write(
        "src/nonlocal_fields.py",
        "class LocalClient:\n    def run(self):\n        pass\n\ndef outer():\n    client = None\n    class Box:\n        def __init__(self):\n            nonlocal client\n            self.client = LocalClient()\n        def invoke(self):\n            self.client.run()\n",
    );
    let reader = workspace.build();

    let (status, evidence) = persisted_coverage(&reader, "src/fields.py", 11, "box.client.run");
    assert_eq!(status, "resolved", "{evidence}");
    let targets: Vec<String> = reader
        .prepare(
            "SELECT dst.qualname FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE p.path=?1 AND e.line=?2 AND e.kind='calls' ORDER BY dst.qualname",
        )
        .unwrap()
        .query_map(rusqlite::params!["src/fields.py", 11], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(
        targets
            .iter()
            .any(|target| target == "src.fields.Client.run"),
        "{targets:#?}"
    );
    assert_eq!(
        persisted_status(&reader, "src/nonlocal_fields.py", 12, "self.client.run"),
        "resolved"
    );
}

#[test]
fn awaited_generic_returns_provide_only_proven_loop_element_types() {
    let workspace = PythonWorkspace::new();
    workspace.write(
        "src/providers.py",
        "def passthrough(fn):\n    return fn\n\nasync def typed_rows() -> list[dict[str, object]]:\n    return []\n\nasync def untyped_rows():\n    return []\n\n@passthrough\nasync def decorated_rows() -> list[dict[str, object]]:\n    return []\n",
    );
    workspace.write(
        "src/consumer.py",
        "from providers import typed_rows, untyped_rows, decorated_rows\n\nasync def run():\n    typed_items = await typed_rows()\n    for item in typed_items:\n        item.get('typed')\n    untyped_items = await untyped_rows()\n    for unknown in untyped_items:\n        unknown.get('unknown')\n    decorated_items = await decorated_rows()\n    for decorated in decorated_items:\n        decorated.get('decorated')\n",
    );
    let reader = workspace.build();

    let (status, evidence) = persisted_coverage(&reader, "src/consumer.py", 6, "item.get");
    assert_eq!(status, "external", "{evidence}");
    assert!(evidence.contains("Python standard library"), "{evidence}");
    assert_eq!(
        persisted_status(&reader, "src/consumer.py", 9, "unknown.get"),
        "unresolved"
    );
    assert_eq!(
        persisted_status(&reader, "src/consumer.py", 12, "decorated.get"),
        "unresolved"
    );
}

#[test]
fn awaited_typed_receiver_methods_preserve_async_return_types() {
    let workspace = PythonWorkspace::new();
    workspace.write(
        "src/models.py",
        "class Worker:\n    def work(self) -> None:\n        pass\n\nclass Client:\n    async def fetch(self) -> Worker:\n        return Worker()\n\n    async def fetch_all(self) -> list[Worker]:\n        return []\n",
    );
    workspace.write(
        "src/consumer.py",
        "from models import Client\n\nasync def typed(client: Client):\n    result = await client.fetch()\n    result.work()\n\nasync def untyped(client):\n    result = await client.fetch()\n    result.work()\n\nasync def shadowed(client: Client):\n    client = object()\n    result = await client.fetch()\n    result.work()\n\nasync def spaced(client: Client):\n    result = await client . fetch()\n    result.work()\n\nasync def typed_collection(client: Client):\n    results = await client.fetch_all()\n    for worker in results:\n        worker.work()\n",
    );
    let reader = workspace.build();

    let (status, evidence) = persisted_coverage(&reader, "src/consumer.py", 5, "result.work");
    assert_eq!(status, "resolved", "{evidence}");
    assert!(
        evidence.contains("src.models.Worker.work"),
        "expected exact Worker.work provider evidence, got {evidence}"
    );
    let (spaced_status, spaced_evidence) =
        persisted_coverage(&reader, "src/consumer.py", 18, "result.work");
    assert_eq!(spaced_status, "resolved", "{spaced_evidence}");
    assert!(
        spaced_evidence.contains("src.models.Worker.work"),
        "expected exact Worker.work provider evidence after whitespace normalization, got {spaced_evidence}"
    );
    let (collection_status, collection_evidence) =
        persisted_coverage(&reader, "src/consumer.py", 23, "worker.work");
    assert_eq!(collection_status, "resolved", "{collection_evidence}");
    assert!(
        collection_evidence.contains("src.models.Worker.work"),
        "expected list element type from the typed async receiver, got {collection_evidence}"
    );
    assert_eq!(
        persisted_status(&reader, "src/consumer.py", 9, "result.work"),
        "unresolved",
        "an untyped async receiver must remain unknown"
    );
    assert_eq!(
        persisted_status(&reader, "src/consumer.py", 14, "result.work"),
        "unresolved",
        "a rebound async receiver must remain unknown"
    );
}
