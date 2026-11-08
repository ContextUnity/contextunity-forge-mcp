use super::*;
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
        workspace.write(
            "src/conditional_alias.py",
            "def implementation(): return 'ready'\nif bool(input()):\n    dispatch = implementation\n",
        );
        workspace.write("src/consumer.py", "def use():\n    dispatch()\n    from pkg import dispatch\n    dispatch()\ndef uncertain():\n    from ambiguous import dispatch\n    dispatch()\ndef changed():\n    from rebound_alias import dispatch\n    dispatch()\ndef maybe():\n    from conditional_alias import dispatch\n    dispatch()\n");
    }
    primary.build();
    let conn = primary.open();
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
        let called: Vec<String> = query_strings(
            &conn,
            "SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) || ':' || dst.name FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes dst ON dst.node_hash=e.dst_hash JOIN coverage_evidence v ON v.evidence_id=e.evidence_id WHERE p.path=?1 AND e.line=4 AND e.kind='calls' AND v.evidence='dispatch' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id),dst.name",
            [path],
        );
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
            let direct_targets: i64 = query_scalar(
                &conn,
                "SELECT count(*) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE p.path=?1 AND e.line=?2 AND e.kind='calls' AND dst.name='implementation'",
                rusqlite::params![path, line],
            );
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
        w.write("pyproject.toml", "[project]\nname='scripts'\nversion='1'\n[project.scripts]\nclean='clean:run'\nloop='loop:run'\naug='aug:run'\nassignment='assignment:run'\nimported='imported:run'\nduplicate='duplicate_script:run'\nlambda='lambda_script:run'\ndeleted='deleted:run'\nconditional='conditional:run'\nexception='exception:run'\ndecorated='decorated:run'\n");
        w.write("src/clean.py", "def run(): pass\n");
        w.write("src/loop.py", "def run(): pass\nfor run in []: pass\n");
        w.write("src/aug.py", "def run(): pass\nrun += 1\n");
        w.write("src/assignment.py", "def run(): pass\nrun = other\n");
        w.write(
            "src/imported.py",
            "def run(): pass\nfrom other import run\n",
        );
        w.write(
            "src/duplicate_script.py",
            "def run(): pass\ndef run(): pass\n",
        );
        w.write(
            "src/lambda_script.py",
            "def run(): pass\nrun = lambda: None\n",
        );
        w.write("src/deleted.py", "def run(): pass\ndel run\n");
        w.write("src/conditional.py", "if False:\n    def run(): pass\n");
        w.write(
            "src/exception.py",
            "try:\n    def run(): pass\nexcept Exception: pass\n",
        );
        w.write("src/decorated.py", "@identity\ndef run(): pass\n");
        w.write(
            "src/guard_conditional.py",
            "if False:\n    def run(): pass\nif __name__ == '__main__':\n    run()\n",
        );
        w.write(
            "src/guard_try.py",
            "try:\n    def run(): pass\nexcept Exception: pass\nif __name__ == '__main__':\n    run()\n",
        );
        w.write(
            "src/guard_decorated.py",
            "@identity\ndef run(): pass\nif __name__ == '__main__':\n    run()\n",
        );
        w.write("src/pkg/__init__.py", "# package\n");
        w.write("src/pkg/catalog.py", "CONSTANT = 7\nfactory_value = make_value()\ndef implementation(): return 1\ncallable_alias = implementation\ndef named(): return 2\n");
        w.write("src/pkg/facade.py", "from .catalog import CONSTANT\n");
        w.write("src/pkg/duplicate.py", "VALUE = 1\nVALUE = 2\n");
        w.write(
            "src/pkg/rebound.py",
            "from .catalog import CONSTANT\nCONSTANT = 99\n",
        );
        w.write(
            "src/pkg/rebound_declarations.py",
            "class Foo: pass\nFoo = 1\ndef make(): return 1\nmake = lambda: 2\nPRE = 0\nclass PRE: pass\n",
        );
        w.write("src/rebound_pkg/__init__.py", "class Foo: pass\nFoo = 1\n");
        w.write("src/rebound_pkg/Foo.py", "def unrelated(): return 1\n");
        w.write("src/consumer.py", "from pkg.catalog import CONSTANT\nfrom pkg.catalog import factory_value\nfrom pkg.catalog import callable_alias\nfrom pkg.facade import CONSTANT as reexported\nfrom pkg.catalog import named\nfrom pkg.catalog import absent\nfrom pkg.duplicate import VALUE\nfrom pkg.rebound import CONSTANT as rebound\nfrom pkg.rebound_declarations import Foo\nfrom pkg.rebound_declarations import make\nfrom pkg.rebound_declarations import PRE\nfrom rebound_pkg import Foo as rebound_foo\n");
    }
    primary.build();
    let conn = primary.open();
    for (path, expected) in [
        ("src/guard_conditional.py", 0),
        ("src/clean.py", 1),
        ("src/loop.py", 0),
        ("src/aug.py", 0),
        ("src/assignment.py", 0),
        ("src/imported.py", 0),
        ("src/duplicate_script.py", 0),
        ("src/lambda_script.py", 0),
        ("src/deleted.py", 0),
        ("src/conditional.py", 0),
        ("src/exception.py", 0),
        ("src/decorated.py", 1),
        ("src/guard_try.py", 0),
        ("src/guard_decorated.py", 1),
    ] {
        let actual: i64 = query_scalar(
            &conn,
            "SELECT count(*) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.kind='function' AND n.name='run' AND json_extract(n.details, '$.entrypoint')='main'",
            [path],
        );
        assert_eq!(actual, expected, "{path}");
    }
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
        let class_targets: i64 = query_scalar(
            &conn,
            "SELECT count(*) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE p.path=?1 AND e.line=11 AND e.kind='imports' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)=?2 AND dst.name='PRE' AND dst.kind='class'",
            rusqlite::params![consumer, format!("{prefix}pkg/rebound_declarations.py")],
        );
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
    drop(conn);
    primary.write("src/clean.py", "def run(): pass\nfor run in []: pass\n");
    primary.write("src/loop.py", "def run(): pass\n");
    primary.write("src/conditional.py", "@identity\ndef run(): pass\n");
    primary.write("src/decorated.py", "if False:\n    def run(): pass\n");
    primary.write(
        "src/guard_conditional.py",
        "@identity\ndef run(): pass\nif __name__ == '__main__':\n    run()\n",
    );
    primary.write(
        "src/guard_decorated.py",
        "if False:\n    def run(): pass\nif __name__ == '__main__':\n    run()\n",
    );
    primary.delta(&[
        "src/clean.py",
        "src/loop.py",
        "src/conditional.py",
        "src/decorated.py",
        "src/guard_conditional.py",
        "src/guard_decorated.py",
    ]);
    let conn = primary.open();
    for (path, expected) in [
        ("src/clean.py", 0),
        ("src/loop.py", 1),
        ("src/conditional.py", 1),
        ("src/decorated.py", 0),
        ("src/guard_conditional.py", 1),
        ("src/guard_decorated.py", 0),
    ] {
        let count: i64 = query_scalar(
            &conn,
            "SELECT count(*) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.kind='function' AND n.name='run' AND json_extract(n.details, '$.entrypoint')='main'",
            [path],
        );
        assert_eq!(count, expected, "delta {path}");
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
        w.write(
            "src/pkg/rebound.py",
            "from . import selection as _selection\n_selection = None\nSelection = _selection.Selection\n",
        );
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
    primary.build();
    let conn = primary.open();
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
        let calls: i64 = query_scalar(
            &conn,
            "SELECT count(*) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE p.path=?1 AND e.line=5 AND e.kind='calls' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)=?2 AND dst.name='Selection' AND dst.kind='class'",
            rusqlite::params![consumer, format!("{prefix}pkg/selection.py")],
        );
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

    let facts_map = [
        ("packages/core/src/contextunity/core/types.py", types_src),
        (
            "packages/core/src/contextunity/core/grpc_error_decorators.py",
            decorators_src,
        ),
        (
            "packages/core/src/contextunity/core/grpc_errors.py",
            grpc_errors_src,
        ),
        (
            "packages/core/src/contextunity/core/__init__.py",
            core_init_src,
        ),
        (
            "extensions/commerce/src/contextunity/commerce/typing.py",
            commerce_typing_src,
        ),
        ("consumer.py", consumer_src),
    ]
    .into_iter()
    .map(|(path, source)| {
        (
            path.to_owned(),
            ExtractorHarness::for_lang("python")
                .at_path(path)
                .extract(source),
        )
    })
    .collect::<BTreeMap<_, _>>();

    let graph = ExtractorHarness::link_graph(&facts_map);
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
