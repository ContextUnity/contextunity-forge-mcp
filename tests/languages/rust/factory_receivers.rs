use crate::languages::harness::LinkedCatalog;
use std::sync::OnceLock;

fn factory_catalog() -> &'static LinkedCatalog {
    static CATALOG: OnceLock<LinkedCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let main_source = r#"
mod common;
mod acdd;
mod rev7_client;
use rev7_client::StdioClientRev7;
use acdd::support::DEFAULT_SEAM;
use common::Workspace;

fn probe_rev7_gaps() {
    let workspace = acdd::support::acdd_workspace(DEFAULT_SEAM, "");
    workspace.write("a.rs", "");
    workspace.root().join("child");
    let projections = (workspace.root(), &workspace);
    projections.0.join("child");
    projections.1.write("wrapped.rs", "");

    let b = Workspace::new();
    let r = Workspace::new();
    for w in [&b, &r] {
        w.write("loop.rs", "");
    }
    let pair = (&b, &r);
    let (first, second) = pair;
    first.write("first.rs", "");
    second.write("second.rs", "");

    let mut cmd = std::process::Command::new("git");
    let output = cmd.current_dir(".").output().unwrap();
    output.status.success();
    output.stdout.len();
    let json = serde_json::from_str::<serde_json::Value>("{}").unwrap();
    json.as_object();
    let yaml = serde_yaml::from_str::<serde_yaml::Value>("{}").unwrap();
    yaml.as_mapping();
}
fn unknown_array(factory: impl Fn() -> Workspace) {
    let known = Workspace::new();
    for unknown in [&known, &factory()] {
        unknown.write("unknown.rs", "");
    }
}
fn database(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    let mut statement = conn.prepare("SELECT 1")?;
    statement.query([])?;
    let mut unwrapped = conn.prepare("SELECT 1").unwrap();
    unwrapped.query([]).unwrap();
    Ok(())
}
fn transaction(tx: &rusqlite::Transaction<'_>) {
    tx.query_row("SELECT 1", [], |transaction_row| transaction_row.get::<_, i64>(0));
}
struct Outer {}
impl Outer { fn outer(&self) {} }
struct Inner {}
impl Inner { fn inner(&self) {} }
fn lexical(flag: bool) {
    let value = Outer {};
    if flag {
        let value = Inner {};
        value.inner();
        let (tuple_local, _) = (Outer {}, ());
        tuple_local.outer();
    }
    value.outer();
    for _ in [1] {
        let nested = Outer {};
        nested.outer();
    }
    let owned = "known".to_owned();
    "known".len();
    owned.len();
    std::path::Path::new(".").join("child").exists();
    String::from_utf8_lossy(&[]).len();
    u32::try_from(1u64);
}
struct GenericHolder<'a> { leaf: LeafRev7, label: &'a str }
struct LeafRev7 {}
impl LeafRev7 { fn finish(&self) {} }
impl<'a> GenericHolder<'a> { fn run(&self) { self.leaf.finish(); } }
type ClientRev7 = StdioClientRev7;
type ChainedClientRev7 = ClientRev7;
type CycleARev7 = CycleBRev7;
type CycleBRev7 = CycleARev7;
fn next_gaps(response: serde_json::Value, metadata: std::fs::Metadata, unrelated: UnknownRev7) {
    let client = ChainedClientRev7::new();
    client.payload();
    response["result"]["tools"].as_array();
    let indexed = response["result"]["tools"];
    indexed.as_array();
    unrelated["result"].as_array();
    CycleARev7::new();
    metadata.file_type().is_file();
    std::time::Instant::now().elapsed().as_millis();
    tempfile::Builder::new().prefix("probe").tempdir().unwrap().path();
}
struct BatchFacts<'a> { nodes: Vec<LeafRev7>, leaf: &'a LeafRev7 }
impl<'a> BatchFacts<'a> {
    fn shared(&self) {
        let this = self;
        this.leaf.finish();
        self.nodes.iter().next().unwrap().finish();
    }
    fn mutable(&mut self) {
        let that = self;
        that.nodes.iter_mut().next().unwrap().finish();
    }
}
fn batch_parameters<'a>(plain: &LeafRev7, mutable: &mut LeafRev7, named: &'a LeafRev7,
    anonymous: &'_ LeafRev7, facts: &BatchFacts<'a>, path: &'a std::path::Path,
    nodes: &[LeafRev7], optional: &Option<Vec<LeafRev7>>, result: &Result<LeafRev7, ()>) {
    plain.finish(); mutable.finish(); named.finish(); anonymous.finish();
    facts.leaf.finish();
    facts.nodes.iter().next().unwrap().finish();
    nodes.into_iter().next().unwrap().finish();
    optional.as_deref().unwrap().iter().next().unwrap().finish();
    result.as_ref().unwrap().finish();
    path.join("child").exists();
    let collection: Vec<LeafRev7> = Vec::new();
    collection.as_ref().iter().next().unwrap().finish();
}
fn batch_unknown<T>(unknown: &T) { unknown.finish(); }
struct BatchMaker {}
impl BatchMaker {
    fn current() -> Self { Self {} }
    fn ready(&self) {}
    fn new() -> LeafRev7 { LeafRev7 {} }
}
fn batch_tuple() -> (LeafRev7, Outer) { (LeafRev7 {}, Outer {}) }
struct MissingConstructor {}
impl MissingConstructor { fn finish(&self) {} }
struct __rust_closure {}
fn batch_six() {
    let maker = BatchMaker::current(); maker.ready();
    let changed = BatchMaker::new(); changed.finish();
    let (tuple_leaf, mut tuple_outer) = batch_tuple();
    tuple_leaf.finish(); tuple_outer.outer();
    let git = |args: &[&str]| {};
    git(&["status"]);
    let invoke = || LeafRev7 {};
    let again = invoke;
    invoke(); again();
    let unproven_result = invoke(); unproven_result.finish();
    unproven_result();
    let missing = MissingConstructor::new(); missing.finish();
    let fake = __rust_closure {}; fake();
    let number = 1; number();
}
struct BatchHub {}
impl BatchHub { fn leaf(&self) -> LeafRev7 { LeafRev7 {} } }
fn make_hub() -> BatchHub { BatchHub {} }
fn result_hub() -> Result<BatchHub, ()> { Ok(BatchHub {}) }
struct Collision { finish: LeafRev7 }
impl Collision { fn finish(&self) {} fn inspect(&self) { let _ = &self.finish; self.finish(); } }
fn batch_seven<'a>(facts: &BatchFacts<'a>, collision: &Collision, name: &str, target: &std::path::PathBuf) -> Result<(), ()> {
    let _ = &facts.nodes;
    let _ = &facts.leaf;
    let _ = &collision.finish;
    collision.finish();
    facts.leaf();
    make_hub().leaf().finish();
    result_hub()?.leaf().finish();
    name.trim().is_empty();
    target.parent().unwrap().exists();
    std::env::temp_dir().join("child").exists();
    Ok(())
}
"#;
        let sources = &[
            (
                "tests/common/mod.rs",
                "#[path = \"workspace.rs\"] mod workspace; pub use workspace::Workspace;",
            ),
            (
                "tests/common/workspace.rs",
                include_str!("../../common/workspace.rs"),
            ),
            (
                "tests/acdd/mod.rs",
                "pub(crate) mod support; mod tuple_probe;",
            ),
            (
                "tests/acdd/support.rs",
                include_str!("../../acdd/support.rs"),
            ),
            (
                "tests/acdd/support/builder.rs",
                include_str!("../../acdd/support/builder.rs"),
            ),
            (
                "tests/acdd/support/fixtures.rs",
                include_str!("../../acdd/support/fixtures.rs"),
            ),
            (
                "tests/acdd/support/git_seed.rs",
                include_str!("../../acdd/support/git_seed.rs"),
            ),
            (
                "tests/acdd/tuple_probe.rs",
                "use super::support::*; fn probe() { let (root, _, _) = fixture(DEFAULT_SEAM); root.write(\"tuple.rs\", \"\"); root.root().join(\"child\"); }",
            ),
            (
                "tests/rev7_client.rs",
                "pub struct StdioClientRev7 {} impl StdioClientRev7 { pub fn new() -> Self { Self {} } pub fn payload(&self) {} }",
            ),
            ("tests/rev7_gap_probe.rs", main_source),
        ];
        let facts = sources
            .iter()
            .map(|(path, source)| {
                let parsed = contextunity_forge_mcp::engine::ast::extract(path, "rust", source)
                    .unwrap_or_else(|error| panic!("{path} (rust): {error}"));
                ((*path).to_owned(), parsed)
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let registry_root = Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR")));
        let registry = contextunity_forge_mcp::engine::languages::dependency_registry::DependencyRegistry::collect(registry_root);
        let graph = contextunity_forge_mcp::engine::linker::link_with_registry(&facts, None, None, &registry);
        LinkedCatalog { facts, graph }
    })
}

fn probe_status(expression: &str) -> String {
    factory_catalog()
        .graph
        .coverage
        .iter()
        .find(|row| row.path == "tests/rev7_gap_probe.rs" && row.expression == expression)
        .map(|row| row.status.to_string())
        .unwrap_or_else(|| panic!("missing {expression}"))
}

#[test]
fn acdd_workspace_factory_propagates_write_and_tuple_projections() {
    assert_eq!(probe_status("workspace.write"), "resolved");
    for (expression, expected) in [
        ("workspace.root", "resolved"),
        ("workspace.root().join", "external"),
        ("projections.0.join", "external"),
        ("projections.1.write", "resolved"),
    ] {
        assert_eq!(probe_status(expression), expected, "{expression}");
    }
}

#[test]
fn loop_pair_and_command_receivers_stay_bounded() {
    for expression in [
        "w.write",
        "first.write",
        "second.write",
        "cmd.current_dir",
        "output.status.success",
        "output.stdout.len",
        "json.as_object",
        "yaml.as_mapping",
    ] {
        let status = probe_status(expression);
        assert!(
            matches!(status.as_str(), "resolved" | "external"),
            "{expression}: {status}"
        );
    }
}

#[test]
fn lexical_std_generic_and_alias_receivers_stay_bounded() {
    for expression in [
        "value.inner",
        "value.outer",
        "tuple_local.outer",
        "nested.outer",
        "owned.len",
        "\"known\".len",
        "std::path::Path::new(\".\").join(\"child\").exists",
        "String::from_utf8_lossy(&[]).len",
        "u32::try_from",
        "self.leaf.finish",
        "ChainedClientRev7::new",
        "client.payload",
        "response[\"result\"][\"tools\"].as_array",
        "indexed.as_array",
        "metadata.file_type().is_file",
        "std::time::Instant::now().elapsed().as_millis",
        "tempfile::Builder::new().prefix(\"probe\").tempdir().unwrap().path",
    ] {
        let status = probe_status(expression);
        assert!(
            matches!(status.as_str(), "resolved" | "external"),
            "{expression}: {status}"
        );
    }
}

#[test]
fn batch_parameter_constructor_and_hub_receivers_stay_bounded() {
    for expression in [
        "this.leaf.finish",
        "self.nodes.iter().next().unwrap().finish",
        "that.nodes.iter_mut().next().unwrap().finish",
        "plain.finish",
        "mutable.finish",
        "named.finish",
        "anonymous.finish",
        "facts.leaf.finish",
        "facts.nodes.iter().next().unwrap().finish",
        "nodes.into_iter().next().unwrap().finish",
        "optional.as_deref().unwrap().iter().next().unwrap().finish",
        "result.as_ref().unwrap().finish",
        "path.join(\"child\").exists",
        "collection.as_ref().iter().next().unwrap().finish",
        "maker.ready",
        "changed.finish",
        "tuple_leaf.finish",
        "tuple_outer.outer",
        "git",
        "invoke",
        "again",
        "self.nodes",
        "self.leaf",
        "collision.finish",
        "make_hub().leaf().finish",
        "result_hub()?.leaf().finish",
        "name.trim().is_empty",
        "target.parent().unwrap().exists",
        "std::env::temp_dir().join(\"child\").exists",
        "transaction_row.get",
    ] {
        let status = probe_status(expression);
        assert!(
            matches!(status.as_str(), "resolved" | "external"),
            "{expression}: {status}"
        );
    }
}

#[test]
fn unknown_cycles_and_unproven_results_fail_closed() {
    for expression in [
        "unrelated[\"result\"].as_array",
        "CycleARev7::new",
        "unknown.finish",
        "unproven_result.finish",
        "unproven_result",
        "missing.finish",
        "fake",
        "number",
        "unknown.write",
    ] {
        assert_eq!(probe_status(expression), "unresolved", "{expression}");
    }
}

#[test]
fn field_read_prefers_declared_field_over_same_name_method() {
    let catalog = factory_catalog();
    let edge_count = |source_path: &str,
                      source_name: &str,
                      edge_kind: &str,
                      target_kind: &str,
                      target_name: Option<&str>| {
        let source_ids: Vec<_> = catalog.facts[source_path]
            .nodes
            .iter()
            .filter(|node| node.name == source_name)
            .map(|node| node.id.as_str())
            .collect();
        let target_ids: Vec<_> = catalog
            .facts
            .values()
            .flat_map(|facts| &facts.nodes)
            .filter(|node| {
                node.kind == target_kind && target_name.is_none_or(|name| node.name == name)
            })
            .map(|node| node.id.as_str())
            .collect();
        catalog
            .graph
            .edges
            .iter()
            .filter(|edge| {
                edge.kind == edge_kind
                    && source_ids.contains(&edge.src.as_str())
                    && target_ids.contains(&edge.dst.as_str())
            })
            .count()
    };
    assert_eq!(
        edge_count(
            "tests/rev7_gap_probe.rs",
            "inspect",
            "references",
            "field",
            Some("finish"),
        ),
        1,
        "field reads must prefer the declared field over a same-name method"
    );
    assert_eq!(
        edge_count(
            "tests/rev7_gap_probe.rs",
            "batch_seven",
            "calls",
            "field",
            None,
        ),
        0,
        "a field read must not manufacture a callable edge"
    );
}

#[test]
fn rusqlite_prepare_query_stays_external() {
    for expression in ["statement.query", "unwrapped.query"] {
        assert_eq!(probe_status(expression), "external", "{expression}");
    }
}

#[test]
fn imported_tuple_fixture_keeps_root_receiver() {
    let catalog = factory_catalog();
    for (expression, expected) in [
        ("root.write", "resolved"),
        ("root.root", "resolved"),
        ("root.root().join", "external"),
    ] {
        assert_eq!(
            catalog.coverage_status("tests/acdd/tuple_probe.rs", expression),
            expected,
            "imported tuple fixture: {expression}"
        );
    }
}
