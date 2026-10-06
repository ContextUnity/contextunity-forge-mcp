use contextunity_forge_mcp::{
    core::commitments,
    db::{reader, writer},
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn assert_factory_delta(
    provider: &str,
    consumer: &str,
    before: &str,
    after: &str,
    source: &str,
    expression: &str,
    expected_initial: (&str, Option<&str>),
) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = Workspace(std::env::temp_dir().join(format!(
        "forge_receiver_delta_{}_{nonce}",
        std::process::id()
    )));
    fs::create_dir_all(&workspace.0).unwrap();
    for path in [provider, consumer] {
        fs::create_dir_all(workspace.0.join(path).parent().unwrap()).unwrap();
    }
    fs::write(workspace.0.join(provider), before).unwrap();
    fs::write(workspace.0.join(consumer), source).unwrap();
    let db = workspace.0.join(".forge/code-map.sqlite");
    writer::build(&workspace.0, &db, None).unwrap();
    let initial = reader::open(&db, &workspace.0).unwrap();
    let status = |conn: &rusqlite::Connection| -> Vec<String> {
        let mut statement = conn.prepare("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2 ORDER BY status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)").unwrap();
        statement
            .query_map([consumer, expression], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(status(&initial), [expected_initial.0]);
    if let Some(origin) = expected_initial.1 {
        let evidence: String = initial
            .query_row(
                "SELECT (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2",
                [consumer, expression],
                |row| row.get(0),
            )
            .unwrap();
        assert!(evidence.contains(origin), "{expression}: {evidence}");
    }
    drop(initial);
    fs::write(workspace.0.join(provider), after).unwrap();
    writer::delta(&workspace.0, &db, &[PathBuf::from(provider)]).unwrap();
    let delta = reader::open(&db, &workspace.0).unwrap();
    commitments::verify(&delta).unwrap();
    assert_eq!(status(&delta), ["unresolved"]);
    let cold_db = workspace.0.join("cold.sqlite");
    writer::build(&workspace.0, &cold_db, None).unwrap();
    let cold = reader::open(&cold_db, &workspace.0).unwrap();
    commitments::verify(&cold).unwrap();
    assert_eq!(status(&delta), status(&cold));
    let calls = |conn: &rusqlite::Connection| -> Vec<(String, String, String)> {
        let mut statement = conn.prepare("SELECT (SELECT id FROM nodes WHERE node_hash=edge_occurrences.src_hash),(SELECT id FROM nodes WHERE node_hash=edge_occurrences.dst_hash),(SELECT evidence FROM coverage_evidence WHERE evidence_id=edge_occurrences.confidence_id) FROM edge_occurrences WHERE (SELECT path FROM path_dictionary WHERE path_id=edge_occurrences.owner_id)=?1 AND kind='calls' ORDER BY (SELECT id FROM nodes WHERE node_hash=edge_occurrences.src_hash),(SELECT id FROM nodes WHERE node_hash=edge_occurrences.dst_hash),(SELECT evidence FROM coverage_evidence WHERE evidence_id=edge_occurrences.confidence_id)").unwrap();
        statement
            .query_map([consumer], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(calls(&delta), calls(&cold));
}

#[cfg(feature = "lang-python")]
#[test]
fn python_factory_contract_changes_relink_consumers_like_cold_builds() {
    assert_factory_delta(
        "provider.py",
        "consumer.py",
        "class Client:\n    def execute(self): return 1\ndef make_client():\n    return Client()\n",
        "class Client:\n    def execute(self): return 1\ndef make_client():\n    return 0\n",
        "from provider import make_client\nclient = make_client()\nclient.execute()\n",
        "client.execute",
        ("resolved", None),
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn python_literal_factory_changes_relink_consumers_like_cold_builds() {
    assert_factory_delta(
        "provider.py",
        "consumer.py",
        "def make():\n    return {}\n",
        "def make():\n    return []\n",
        "from provider import make\nvalue = make()\nvalue.get('key')\n",
        "value.get",
        ("external", Some("Python standard library")),
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn python_constructor_field_contract_changes_relink_consumers_like_cold_builds() {
    assert_factory_delta(
        "provider.py",
        "consumer.py",
        "class Client:\n    def execute(self): return 1\nclass Other: pass\nclass Wrapper:\n    def __init__(self):\n        client = Client()\n        self.client = client\n",
        "class Client:\n    def execute(self): return 1\nclass Other: pass\nclass Wrapper:\n    def __init__(self):\n        client = Other()\n        self.client = client\n",
        "from provider import Wrapper\nwrapper = Wrapper()\nwrapper.client.execute()\n",
        "wrapper.client.execute",
        ("resolved", None),
    );
}

#[cfg(feature = "lang-typescript")]
#[test]
fn typescript_factory_contract_changes_relink_consumers_like_cold_builds() {
    assert_factory_delta("provider.ts", "consumer.ts",
        "export class Client { execute() { return 1; } } export function makeClient() { return new Client(); }",
        "export class Client { execute() { return 1; } } export function makeClient() { return 0; }",
        "import { makeClient } from './provider'; const client = makeClient(); client.execute();", "client.execute", ("resolved", None));
}

#[cfg(feature = "lang-rust")]
#[test]
fn rust_factory_contract_changes_relink_consumers_like_cold_builds() {
    assert_factory_delta(
        "src/provider.rs",
        "src/consumer.rs",
        "pub struct Client; pub struct Other; impl Client { pub fn execute(&self) {} } pub fn make() -> Client { Client }",
        "pub struct Client; pub struct Other; impl Client { pub fn execute(&self) {} } pub fn make() -> Other { Other }",
        "use crate::provider::make; fn run() { let client = make(); client.execute(); }",
        "client.execute",
        ("resolved", None),
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn rust_tuple_field_contract_changes_relink_nominal_consumers_like_cold_builds() {
    assert_factory_delta(
        "src/provider.rs",
        "src/consumer.rs",
        "pub struct Client; pub struct Other; pub struct Holder(pub Client); impl Client { pub fn execute(&self) {} }",
        "pub struct Client; pub struct Other; pub struct Holder(pub Other); impl Client { pub fn execute(&self) {} }",
        "use crate::provider::Holder; fn run(holder: &Holder) { holder.0.execute(); }",
        "holder.0.execute",
        ("resolved", None),
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn python_declared_builtin_shadowing_relinks_consumers_like_cold_builds() {
    assert_factory_delta(
        "provider.py",
        "consumer.py",
        "def make_text() -> str:\n    return ''\n",
        "def make_text() -> str:\n    str = 0\n    return ''\n",
        "from provider import make_text\ntext = make_text()\ntext.upper()\n",
        "text.upper",
        ("external", Some("Python standard library")),
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn python_scoped_factory_import_changes_relink_consumers_like_cold_builds() {
    assert_factory_delta(
        "provider.py",
        "consumer.py",
        "class Client:\n    def execute(self): return 1\nclass Other: pass\ndef make_client():\n    from provider import Client as Product\n    return Product()\n",
        "class Client:\n    def execute(self): return 1\nclass Other: pass\ndef make_client():\n    from provider import Other as Product\n    return Product()\n",
        "from provider import make_client\nclient = make_client()\nclient.execute()\n",
        "client.execute",
        ("resolved", None),
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn python_factory_scope_name_limits_relink_consumers_like_cold_builds() {
    let before = format!("class Client:\n    def execute(self): return 1\ndef make_client():\n    {} = 0\n    return Client()\n", "x".repeat(4_096));
    let after = format!("class Client:\n    def execute(self): return 1\ndef make_client():\n    {} = 0\n    return Client()\n", "x".repeat(4_097));
    assert_factory_delta(
        "provider.py",
        "consumer.py",
        &before,
        &after,
        "from provider import make_client\nclient = make_client()\nclient.execute()\n",
        "client.execute",
        ("resolved", None),
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn large_independent_receiver_graphs_preserve_per_file_resolution_and_stable_order() {
    use contextunity_forge_mcp::engine::{ast, linker};
    use std::collections::BTreeMap;

    let mut source = "class Target:\n    def work(self): pass\n".to_owned();
    for index in 0..260 {
        source.push_str(&format!("def use_{index}(client: Target): client.work()\n"));
    }
    let files: BTreeMap<_, _> = (0..32)
        .map(|index| {
            let path = format!("file_{index:02}.py");
            let facts = ast::extract(&path, "python", &source).unwrap();
            (path, facts)
        })
        .collect();
    assert!(files.values().map(|facts| facts.nodes.len()).sum::<usize>() >= 8192);
    let mut expected = contextunity_forge_mcp::core::models::Graph::default();
    for (path, facts) in &files {
        let part = linker::link(&BTreeMap::from([(path.clone(), facts.clone())]));
        expected.edges.extend(part.edges);
        expected.coverage.extend(part.coverage);
    }
    expected.edges.sort_by(|a, b| {
        (&a.path, a.line, &a.src, &a.dst, &a.kind).cmp(&(&b.path, b.line, &b.src, &b.dst, &b.kind))
    });
    expected
        .coverage
        .sort_by(|a, b| (&a.path, a.line, &a.expression).cmp(&(&b.path, b.line, &b.expression)));
    let actual = linker::link(&files);
    assert_eq!(
        serde_json::json!([actual.edges, actual.coverage]),
        serde_json::json!([expected.edges, expected.coverage])
    );
}
