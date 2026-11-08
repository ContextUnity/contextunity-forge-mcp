type ModuleVisibilityCase =
    (&'static str, &'static str, &'static [(&'static str, &'static str)], &'static str, &'static str);
const MODULE_VISIBILITY_CASES: &[ModuleVisibilityCase] = &[
    ("file-item", "VIS mod inner;", &[("inner.rs", "pub fn work() {}")], "use left::inner::work;", "work()"),
    ("file-module", "VIS mod inner;", &[("inner.rs", "pub fn work() {}")], "use left::inner;", "inner::work()"),
    ("inline-item", "VIS mod inner { pub fn work() {} }", &[], "use left::inner::work;", "work()"),
    ("inline-module", "VIS mod inner { pub fn work() {} }", &[], "use left::inner;", "inner::work()"),
    (
        "file-reexport",
        "pub mod inner; VIS mod api;",
        &[("inner.rs", "pub fn work() {}"), ("api.rs", "pub use crate::inner::work;")],
        "use left::api::work;",
        "work()",
    ),
    (
        "ancestor-module",
        "VIS mod outer;",
        &[("outer.rs", "pub mod inner;"), ("outer/inner.rs", "pub fn work() {}")],
        "use left::outer::inner::work;",
        "work()",
    ),
    (
        "public-facade",
        "VIS mod inner; pub use crate::inner::work;",
        &[("inner.rs", "pub fn work() {}")],
        "use left::work;",
        "work()",
    ),
];

const OWNER_PATH_CASES: &[(&str, &str, &str)] = &[
    ("crates/ordinary/src/probe.rs", "crates/ordinary/src/core.rs", "crate::core::work"),
    ("crates/ordinary/src/probe.rs", "crates/ordinary/src/engine.rs", "crate::engine::work"),
    ("examples/probe.rs", "examples/core.rs", "crate::core::work"),
    ("crates/forge-index/src/probe.rs", "crates/forge-core/src/models.rs", "forge_core::models::work"),
    ("crates/forge-index/src/probe.rs", "crates/forge-index/src/ast.rs", "crate::ast::work"),
    ("crates/forge-tasks/src/probe.rs", "crates/forge-tasks/src/store.rs", "crate::store::work"),
    ("crates/ordinary/src/probe.rs", "crates/ordinary/src/db/tasks_store.rs", "crate::db::tasks_store::work"),
];

fn two_crate_workspace() -> crate::languages::PersistedWorkspaceHarness {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write("Cargo.toml", "[workspace]\nmembers=['crates/left','crates/right']\n");
    workspace.write("crates/left/Cargo.toml", "[package]\nname='left'\nversion='0.1.0'\n");
    workspace.write(
        "crates/right/Cargo.toml",
        "[package]\nname='right'\nversion='0.1.0'\n[dependencies]\nleft={path='../left'}\n",
    );
    workspace
}

use super::coverage;
use crate::languages::harness::ExtractorHarness;


#[test]
fn cargo_policy_controls_statement_return_authority() {
    for (label, manifest, source, expression) in [
        (
            "standard",
            "enabled = false\n",
            "use rusqlite::Connection;\nfn run(conn: Connection) { let mut statement = conn.prepare(\"SELECT 1\").unwrap(); statement.execute([]); }",
            "statement.execute",
        ),
        (
            "whitespace",
            "enabled  =  false\n",
            "use rusqlite::Connection;\nfn run(conn: Connection) { let s = conn.prepare(\"SELECT 1\").unwrap(); s.execute([]); }",
            "s.execute",
        ),
        (
            "no-import",
            "enabled = false\n",
            "fn run(conn: rusqlite::Connection) { let s = conn.prepare(\"SELECT 1\").unwrap(); s.execute([]); }",
            "s.execute",
        ),
    ] {
        let workspace = crate::languages::PersistedWorkspaceHarness::new();
        workspace.write(".forge/frameworks/rust-cargo.toml", manifest);
        let catalog = ExtractorHarness::linked_with_root("rust", &[("src/lib.rs", source)], Some(workspace.root()));
        let graph = &catalog.graph;
        let row = graph
            .coverage
            .iter()
            .find(|r| r.expression == expression)
            .unwrap_or_else(|| panic!("missing {expression}"));
        assert!(
            !row.evidence.contains("rusqlite::Statement"),
            "{label}: disabled rust-cargo pack must NOT infer rusqlite::Statement: {row:#?}"
        );
    }
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write("Cargo.toml", "[package]\nname='seam-r2'\nversion='0.1.0'\n[dependencies]\nrusqlite='0.31'\n");
    let manifest = r#"enabled = true
replace = true
[[class_hierarchy_receivers]]
bases = ['rusqlite::Connection', 'Connection']
provider = 'rusqlite'
members = ['prepare', 'prepare_cached']
[class_hierarchy_receivers.returns]
prepare_cached = { kind = 'receiver', base = 'rusqlite::Row' }
[[class_hierarchy_receivers]]
bases = ['rusqlite::Row', 'Row']
provider = 'rusqlite'
members = ['get']
"#;
    workspace.write(".forge/frameworks/rust-cargo.toml", manifest);
    let source = "use rusqlite::Connection;\nfn f(conn: Connection) { let value = conn.prepare_cached(\"SELECT 1\").unwrap(); value.get(0); }";
    let catalog = ExtractorHarness::linked_with_root("rust", &[("src/lib.rs", source)], Some(workspace.root()));
    let graph = &catalog.graph;
    let row = graph.coverage.iter().find(|r| r.expression == "value.get").unwrap();
    assert!(
        row.evidence.contains("rusqlite::Row"),
        "effective return policy for prepare_cached -> Row must resolve on value.get: {row:#?}"
    );
}
#[test]
fn crate_paths_link_to_their_actual_workspace_owner() {
    for &(owner, target, import) in OWNER_PATH_CASES {
        let source = format!("use {import};\nfn probe() {{ work(); }}\n");
        let workspace = crate::languages::PersistedWorkspaceHarness::new();
        workspace.write(owner, &source);
        workspace.write(target, "pub fn work() {}\n");
        let catalog = ExtractorHarness::linked_with_root(
            "rust",
            &[
                (owner, source.as_str()),
                (
                    target,
                    "pub fn work() {}
",
                ),
            ],
            Some(workspace.root()),
        );
        let graph = &catalog.graph;
        let caller = catalog.facts[owner].nodes.iter().find(|node| node.name == "probe").unwrap();
        let callee = catalog.facts[target].nodes.iter().find(|node| node.name == "work").unwrap();
        assert!(
            graph.edges.iter().any(|edge| edge.kind == "calls" && edge.src == caller.id && edge.dst == callee.id),
            "{owner}: {import}: {:#?}",
            graph.coverage
        );
    }
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write("Cargo.toml", "[package]\nname='my_root_crate'\nversion='0.1.0'\nedition='2021'\n");
    workspace.write("src/lib.rs", "pub mod engine;");
    let catalog = ExtractorHarness::linked_with_root(
        "rust",
        &[
            (
                "src/main.rs",
                "use my_root_crate::engine::ast; fn run() { ast::extract(\"test.rs\", \"rust\", \"fn main() {}\"); }",
            ),
            ("src/engine/ast.rs", "pub fn extract(_path: &str, _lang: &str, _code: &str) {}"),
            ("src/lib.rs", "pub mod engine;"),
        ],
        Some(workspace.root()),
    );
    let resolved_cov = catalog
        .graph
        .coverage
        .iter()
        .find(|row| row.expression == "ast::extract")
        .expect("missing coverage for ast::extract with manifest");
    assert_eq!(
        resolved_cov.status, "resolved",
        "ast::extract with valid manifest and src/lib.rs must resolve locally: {resolved_cov:#?}"
    );
}

#[test]
fn rows_next_obeys_effective_return_policy_after_persistence() {
    for (policy, manifest, expected) in [
            ("default", None, Some("rusqlite::Row")),
            ("disabled", Some("enabled = false\n".to_owned()), None),
            ("no-return", Some("enabled = true\nreplace = true\n[[class_hierarchy_receivers]]\nbases = ['rusqlite::Rows']\nprovider = 'rusqlite'\nmembers = ['next']\n".to_owned()), None),
            ("remapped", Some("enabled = true\nreplace = true\n[[class_hierarchy_receivers]]\nbases = ['rusqlite::Rows']\nprovider = 'rusqlite'\nmembers = ['next']\n[class_hierarchy_receivers.returns]\nnext = { kind = 'receiver', base = 'tree_sitter::Node' }\n[[class_hierarchy_receivers]]\nbases = ['tree_sitter::Node']\nprovider = 'tree-sitter'\nmembers = ['kind']\n".to_owned()), Some("tree_sitter::Node")),
        ] {
            let workspace = crate::languages::PersistedWorkspaceHarness::new();
            workspace.write("Cargo.toml", "[package]\nname='row-policy'\nversion='0.1.0'\n[dependencies]\nrusqlite='0.31'\ntree-sitter='0.22'\n");
            if let Some(manifest) = manifest { workspace.write(".forge/frameworks/rust-cargo.toml", &manifest); }
            workspace.write("src/lib.rs", "use rusqlite::{Rows, Row};\nfn assigned(rows: &mut Rows<'_>) { let row = rows.next().unwrap().unwrap(); row.get(0); row.kind(); }\nfn computed(rows: &mut Rows<'_>) { rows.next().unwrap().unwrap().get(0); rows.next().unwrap().unwrap().kind(); }\nfn generic(row: &Row) { let value = row.get(0).unwrap(); value.kind(); }\n");
            workspace.build();
            let db = workspace.open();
            for expression in ["row.get", "rows.next().unwrap().unwrap().get"] {
                let evidence: String = db.query_row("SELECT v.evidence FROM resolution_coverage c JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence v ON v.evidence_id=c.evidence_id WHERE x.expression=?1", [expression], |row| row.get(0)).unwrap();
                assert_eq!(evidence.contains("rusqlite::Row"), expected == Some("rusqlite::Row"), "{policy}: {expression}: {evidence}");
            }
            if expected == Some("tree_sitter::Node") {
                let status: String = db.query_row("SELECT c.status FROM resolution_coverage c JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE x.expression='row.kind'", [], |row| row.get(0)).unwrap();
                assert_eq!(status, "external", "{policy}");
            }
            let status: String = db.query_row("SELECT c.status FROM resolution_coverage c JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE x.expression='value.kind'", [], |row| row.get(0)).unwrap();
            assert_eq!(status, "unresolved", "{policy}: generic Row.get payload must remain unknown");
        }
}

#[test]
fn visibility_and_public_facades_bound_cross_crate_calls() {
    for (visibility, outer_allowed, root_allowed, external_allowed) in [
        ("pub", true, true, true),
        ("pub(crate)", true, true, false),
        ("pub(super)", true, false, false),
        ("pub(in crate::outer)", true, false, false),
        ("pub(self)", false, false, false),
    ] {
        let workspace = two_crate_workspace();
        workspace.write(
            "crates/left/src/lib.rs",
            "pub mod outer;\nuse crate::outer::api::work;\nfn root_probe(){ work(); }\n",
        );
        workspace.write(
            "crates/left/src/outer/mod.rs",
            "pub mod api;\npub mod inner;\nuse self::api::work;\nfn allowed(){ work(); }\n",
        );
        workspace.write("crates/left/src/outer/inner.rs", "pub fn work() {}\n");
        workspace.write(
            "crates/left/src/outer/api.rs",
            &format!("{visibility} use crate::outer::inner::work;\nmod leaf;\n"),
        );
        workspace.write("crates/left/src/outer/api/leaf.rs", "use super::work;\nfn nested(){ work(); }\n");
        workspace.write("crates/right/src/lib.rs", "use left::outer::api::work;\nfn foreign(){ work(); }\n");
        workspace.build();
        let db = workspace.open();
        for (caller, allowed) in
            [("allowed", outer_allowed), ("root_probe", root_allowed), ("foreign", external_allowed), ("nested", true)]
        {
            let count: i64 = db
                .query_row(
                    "SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name=?1 AND d.name='work'",
                    [caller],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, i64::from(allowed), "{visibility}: {caller}");
        }
    }
    for visibility in ["pub", "pub(crate)"] {
        for &(case, declarations, providers, foreign_import, foreign_call) in MODULE_VISIBILITY_CASES {
            let workspace = two_crate_workspace();
            let local_call = match case {
                "file-reexport" => "api::work()",
                "ancestor-module" => "work()",
                "public-facade" => "work()",
                _ => "inner::work()",
            };
            workspace.write(
                "crates/left/src/lib.rs",
                &format!(
                    "{}\nfn local() {{ {local_call}; }}\n",
                    declarations.replace("VIS", visibility)
                        + if case == "ancestor-module" { " use crate::outer::inner::work;" } else { "" }
                ),
            );
            for (path, source) in providers {
                workspace.write(format!("crates/left/src/{path}"), source);
            }
            workspace
                .write("crates/right/src/lib.rs", &format!("{foreign_import}\nfn foreign() {{ {foreign_call}; }}\n"));
            workspace.build();
            let db = workspace.open();
            for (caller, expected) in [("local", true), ("foreign", visibility == "pub" || case == "public-facade")] {
                let count: i64 = db
                    .query_row(
                        "SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name=?1 AND d.name='work'",
                        [caller],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(count, i64::from(expected), "{visibility} {case}: {caller}");
            }
        }
    }
}

#[test]
fn explicit_root_is_independent_of_process_cwd() {
    let workspace = two_crate_workspace();
    let unrelated = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write("crates/left/src/lib.rs", "pub mod models; pub use models::*; pub fn work() {}\n");
    workspace.write("crates/left/src/models.rs", "pub struct Node { pub visible: String, pub(crate) internal: String, private: String } impl Node { pub fn new() -> Self { Self { visible: String::new(), internal: String::new(), private: String::new() } } pub fn finish(&self) {} }\n");
    workspace.write(
                "crates/right/src/lib.rs",
                "use left::*;\nfn probe(){ work(); let node = Node::new(); node.finish(); }\nfn borrowed(node: &Node) { node.finish(); let _ = &node.visible; let _ = &node.internal; let _ = &node.private; }\n",
            );
    for (index, cwd) in [workspace.root(), unrelated.root()].into_iter().enumerate() {
        let db_path = workspace.path(format!(".forge/cwd-{index}.sqlite"));
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .arg("--root")
            .arg(workspace.root())
            .arg("--db")
            .arg(&db_path)
            .args(["--format", "json", "build"])
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(output.status.success(), "cwd {cwd:?}: {}", String::from_utf8_lossy(&output.stderr));
        let db = rusqlite::Connection::open(db_path).unwrap();
        let calls: i64 = db
            .query_row(
                "SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='probe' AND d.name='work'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(calls, 1, "explicit-root import resolution at cwd {cwd:?}");
        let methods: i64 = db
            .query_row(
                "SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='probe' AND d.name='finish'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(methods, 1, "cross-crate imported Node receiver at cwd {cwd:?}");
        let borrowed: i64 = db
            .query_row(
                "SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='borrowed' AND d.name='finish'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(borrowed, 1, "cross-crate glob parameter identity at cwd {cwd:?}");
        for expression in ["node.visible", "node.internal", "node.private"] {
            let reads: i64 = db.query_row("SELECT count(*) FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) JOIN path_dictionary p USING(path_id) WHERE p.path='crates/right/src/lib.rs' AND x.expression=?1", [expression], |row| row.get(0)).unwrap();
            assert_eq!(
                reads, 0,
                "ordinary field reads must not expand reference inventory at cwd {cwd:?}: {expression}"
            );
        }
    }
}

#[test]
fn workspace_factories_preserve_return_identity_across_aliases() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write("tests/common/mod.rs", "#[path = \"workspace.rs\"] mod workspace; pub use workspace::Workspace;");
    workspace.write("tests/common/workspace.rs", include_str!("../../common/workspace.rs"));
    workspace.write("tests/acdd/mod.rs", "pub(crate) mod support;");
    workspace.write("tests/acdd/support.rs", include_str!("../../acdd/support.rs"));
    workspace.write("tests/acdd/support/builder.rs", include_str!("../../acdd/support/builder.rs"));
    workspace.write("tests/acdd/support/fixtures.rs", include_str!("../../acdd/support/fixtures.rs"));
    workspace.write("tests/acdd/support/git_seed.rs", include_str!("../../acdd/support/git_seed.rs"));
    workspace.write(
        "tests/helpers.rs",
        r#"mod common; mod acdd;
use common::Workspace;
use acdd::support::{acdd_workspace, DEFAULT_SEAM};
fn probe() {
    let workspace = Workspace::new();
    workspace.write("a.rs", "");
    workspace.delta(&["a.rs"]);
    workspace.build();
    workspace.open();
    let scoped = acdd_workspace(DEFAULT_SEAM, "");
    scoped.write("a.rs", "");
    scoped.root().join("child");
    let projections = (scoped.root(), &scoped);
    projections.0.join("child");
    projections.1.write("wrapped.rs", "");
}"#,
    );
    workspace.build();
    let db = workspace.open();
    for (expression, expected) in [
        ("workspace.write", "resolved"),
        ("workspace.delta", "resolved"),
        ("workspace.build", "resolved"),
        ("workspace.open", "resolved"),
        ("scoped.write", "resolved"),
        ("scoped.root", "resolved"),
        ("scoped.root().join", "external"),
        ("projections.0.join", "external"),
        ("projections.1.write", "resolved"),
    ] {
        let status: String = db
            .query_row(
                "SELECT c.status FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) JOIN path_dictionary p USING(path_id) WHERE p.path='tests/helpers.rs' AND x.expression=?1",
                [expression],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, expected, "workspace factory return: {expression}");
    }
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace
        .write("src/lib.rs", "use tree_sitter::Node as Syntax; mod a; pub mod support; mod consumer; mod shadow;\n");
    workspace.write("src/a/mod.rs", "use super::Syntax as Tree; mod b;\n");
    workspace.write("src/a/b/mod.rs", "use super::Tree as Syntax; mod named; mod glob; mod shadow;\n");
    for (file, import) in [("named", "use super::{Syntax};"), ("glob", "use super::*;")] {
        workspace.write(format!("src/a/b/{file}.rs"), &format!("{import} fn probe(node: Syntax<'_>) {{ node.kind(); node.child_by_field_name(\"body\"); node.walk(); }}"));
    }
    workspace.write("src/a/b/shadow.rs", "use super::*; struct Syntax; fn probe(node: Syntax) { node.kind(); }");
    workspace.write("src/support.rs", "pub struct Root(pub std::path::PathBuf); impl Root { pub fn write(&self) {} } pub struct Store; impl Store { pub fn sync(&mut self) {} } pub struct Milestone; impl Milestone { pub fn task_id(&self) {} } pub fn fixture() -> (Root, Store, Milestone) { panic!() }");
    workspace.write(
        "src/consumer.rs",
        "use crate::support::fixture as setup; fn probe() { let (root, mut store, milestone) = setup(); root.write(); root.0.join(\"child\"); store.sync(); milestone.task_id(); }",
    );
    workspace.write(
        "src/shadow.rs",
        "use crate::support::fixture; fn negative() { let fixture = || ((), (), ()); let (root, store, milestone) = fixture(); root.write(); store.sync(); milestone.task_id(); }",
    );
    workspace.build();
    let db = workspace.open();
    for (path, expression, expected) in [
        ("src/a/b/named.rs", "node.kind", "external"),
        ("src/a/b/named.rs", "node.child_by_field_name", "external"),
        ("src/a/b/named.rs", "node.walk", "external"),
        ("src/a/b/glob.rs", "node.kind", "external"),
        ("src/a/b/glob.rs", "node.walk", "external"),
        ("src/a/b/shadow.rs", "node.kind", "unresolved"),
        ("src/consumer.rs", "root.write", "resolved"),
        ("src/consumer.rs", "root.0.join", "external"),
        ("src/consumer.rs", "store.sync", "resolved"),
        ("src/consumer.rs", "milestone.task_id", "resolved"),
        ("src/shadow.rs", "root.write", "unresolved"),
    ] {
        let status: String = db
            .query_row(
                "SELECT c.status FROM resolution_coverage c JOIN coverage_expressions x USING(expression_id) JOIN path_dictionary p USING(path_id) WHERE p.path=?1 AND x.expression=?2",
                [path, expression],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, expected, "{path}: {expression}");
    }
    for method in ["write", "sync", "task_id"] {
        let calls: i64 = db.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes n ON n.node_hash=e.dst_hash JOIN path_dictionary owner ON owner.path_id=e.owner_id JOIN path_dictionary target ON target.path_id=n.path_id WHERE owner.path='src/consumer.rs' AND target.path='src/support.rs' AND e.kind='calls' AND n.name=?1", [method], |row| row.get(0)).unwrap();
        assert_eq!(calls, 1, "imported tuple field must call canonical {method}");
    }
    let shadow_calls: i64 = db.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes n ON n.node_hash=e.dst_hash JOIN path_dictionary owner ON owner.path_id=e.owner_id JOIN path_dictionary target ON target.path_id=n.path_id WHERE owner.path='src/shadow.rs' AND target.path='src/support.rs' AND e.kind='calls'", [], |row| row.get(0)).unwrap();
    assert_eq!(shadow_calls, 0, "shadowed factory must not acquire tuple payloads");
}

#[test]
fn module_imports_preserve_visibility_and_receiver_boundaries() {
    let files = [
        ("crates/ordinary/src/lib.rs", "mod inner; pub(crate) use crate::inner::work;"),
        ("crates/ordinary/src/inner.rs", "pub(crate) fn work() {}"),
        ("crates/ordinary/src/probe.rs", "use crate::work; fn root_probe() { work(); }"),
        (
            "tests/probe.rs",
            "mod common; use crate::common::Workspace; fn probe() { let w = Workspace::new(); w.write(); }",
        ),
        ("tests/common/mod.rs", "mod workspace;\npub use workspace::Workspace;\n"),
        (
            "tests/common/workspace.rs",
            "pub struct Workspace;\nimpl Workspace {\n pub fn new() -> Self { Self }\n pub fn write(&self) {}\n}\n",
        ),
        ("src/lang/mod.rs", "pub use tree_sitter::Node as Syntax;\npub fn helper() {}\nmod sub;\n"),
        ("src/lang/sub/mod.rs", "use super::*;\nmod leaf;\n"),
        ("src/lang/sub/leaf.rs", "use super::*;\nfn run(s: &Syntax) { s.kind(); helper(); }\n"),
        ("src/parallel/mod.rs", "use rayon::prelude::*;\npub fn par_work() {}\nmod child;\n"),
        ("src/parallel/child.rs", "use super::*;\nfn check() { par_work(); }\n"),
        (
            "src/chains.rs",
            "use rusqlite::Connection;\nfn test_db(conn: &Connection) {\n    let mut stmt = conn.prepare(\"SELECT 1\").unwrap();\n    let mut rows = stmt.query([]).unwrap();\n    let row = rows.next().unwrap().unwrap();\n    let _val: i64 = row.get(0).unwrap();\n}\nfn test_syntax(node: &tree_sitter::Node<'_>) {\n    let child = node.child_by_field_name(\"test\").unwrap();\n    let named = child.named_child(0).unwrap();\n    let _ = named.kind();\n}\n",
        ),
        (
            "src/fields.rs",
            "struct Store { conn: rusqlite::Connection } impl Store { fn run(&self) { let stmt = self.conn.prepare_cached(\"SELECT 1\").unwrap(); stmt.column_count(); } } fn siblings(node: &tree_sitter::Node<'_>) { node.parent().unwrap().kind(); node.next_sibling().unwrap().kind(); node.prev_sibling().unwrap().kind(); node.next_named_sibling().unwrap().kind(); node.prev_named_sibling().unwrap().kind(); let mut cursor = node.walk(); node.children(&mut cursor).next().unwrap().kind(); node.named_children(&mut cursor).next().unwrap().kind(); }",
        ),
        ("crates/left/src/lib.rs", "mod inner;\npub(crate) use inner::work;\n"),
        ("crates/left/src/inner.rs", "pub fn work() {}\n"),
        ("crates/right/src/lib.rs", "use left::work;\nfn probe() { work(); }\n"),
        (
            "src/opt_and_then.rs",
            "struct A;\nimpl A { fn finish(&self) {} }\nstruct B;\nimpl B { fn finish(&self) {} }\nfn probe(opt: Option<A>) { let result = opt.and_then(|_| Some(B)).unwrap(); result.finish(); }\n",
        ),
    ];
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    for (path, source) in &files {
        workspace.write(path, source);
    }
    let graph = ExtractorHarness::linked_with_root("rust", &files, Some(workspace.root())).graph;
    for expression in ["Workspace::new", "w.write", "helper", "par_work", "work"] {
        let entry = coverage(&graph, expression);
        assert_eq!(entry.status, "resolved", "{entry:#?}; {:#?}", graph.coverage);
    }
    for expression in [
        "s.kind",
        "conn.prepare(\"SELECT 1\").unwrap",
        "stmt.query([]).unwrap",
        "rows.next().unwrap().unwrap",
        "row.get(0).unwrap",
        "node.child_by_field_name(\"test\").unwrap",
        "child.named_child(0).unwrap",
        "named.kind",
        "self.conn.prepare_cached(\"SELECT 1\").unwrap",
        "stmt.column_count",
        "node.parent().unwrap().kind",
        "node.next_sibling().unwrap().kind",
        "node.prev_sibling().unwrap().kind",
        "node.next_named_sibling().unwrap().kind",
        "node.prev_named_sibling().unwrap().kind",
        "node.children(&mut cursor).next().unwrap().kind",
        "node.named_children(&mut cursor).next().unwrap().kind",
    ] {
        let entry = coverage(&graph, expression);
        assert_eq!(entry.status, "external", "{entry:#?}");
    }
    let cross_crate_work = graph
        .coverage
        .iter()
        .find(|c| c.path == "crates/right/src/lib.rs" && c.expression == "work")
        .expect("right crate work call");
    assert_ne!(
        cross_crate_work.status, "resolved",
        "pub(crate) re-export in left crate must not resolve in right crate: {cross_crate_work:#?}"
    );
    let and_then_finish = graph
        .coverage
        .iter()
        .find(|c| c.path == "src/opt_and_then.rs" && c.expression == "result.finish")
        .expect("and_then finish call");
    assert_ne!(
        and_then_finish.status, "resolved",
        "type-changing and_then must not resolve to input type method: {and_then_finish:#?}"
    );
}
