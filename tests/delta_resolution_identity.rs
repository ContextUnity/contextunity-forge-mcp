#![cfg(all(feature = "lang-python", feature = "lang-typescript"))]

use contextunity_forge_mcp::{
    core::commitments,
    db::{reader, writer},
};
use rusqlite::Connection;
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "forge_resolution_identity_{}_{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    fn db(&self) -> PathBuf {
        self.0.join(".forge/code-map.sqlite")
    }
    fn build(&self) {
        writer::build(&self.0, &self.db(), None).unwrap();
    }
    fn delta(&self, path: &str) -> serde_json::Value {
        writer::delta(&self.0, &self.db(), &[PathBuf::from(path)]).unwrap()
    }
    fn assert_cold_equivalent(&self) {
        let incremental = reader::open(&self.db(), &self.0).unwrap();
        commitments::verify(&incremental).unwrap();
        let cold = self.0.join(".forge/cold.sqlite");
        writer::build(&self.0, &cold, None).unwrap();
        let cold = reader::open(&cold, &self.0).unwrap();
        commitments::verify(&cold).unwrap();
        for query in [
            "SELECT id||'|'||kind||'|'||qualname||'|'||line||'|'||end_line||'|'||details FROM nodes ORDER BY id",
            "SELECT (SELECT id FROM nodes WHERE node_hash=src_hash)||'|'||(SELECT id FROM nodes WHERE node_hash=dst_hash)||'|'||kind||'|'||(SELECT path FROM path_dictionary WHERE path_id=edges.path_id)||'|'||line||'|'||occurrence_count FROM edges ORDER BY (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind",
            "SELECT (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)||'|'||line||'|'||(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)||'|'||status||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)",
            "SELECT p.path||'|'||f.path||'|'||d.kind||'|'||d.symbol FROM dependencies d JOIN path_dictionary p ON p.path_id=d.owner_id JOIN files f ON f.path_hash=d.target_hash WHERE d.target_hash IS NOT NULL ORDER BY p.path,f.path,d.kind,d.symbol",
        ] { assert_eq!(rows(&incremental, query), rows(&cold, query), "{query}"); }
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn rows(conn: &Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn body_edits_refresh_owner_without_relinking_consumers_and_line_moves_relink() {
    let w = Workspace::new();
    w.write(
        "provider.py",
        "def left(): return 1\ndef right(): return 2\ndef value(): return left()\n",
    );
    for i in 0..30 {
        w.write(
            &format!("consumer_{i}.py"),
            "from provider import value\ndef consume(): return value()\n",
        );
    }
    w.write("docs/api.md", "# API\nUse `provider.value`.\n");
    w.build();
    w.write(
        "provider.py",
        "def left(): return 1\ndef right(): return 2\ndef value(): return right()\n\n",
    );
    let delta = w.delta("provider.py");
    assert_eq!(delta["affected_owners"], 1, "{delta}");
    assert_eq!(delta["loaded_fact_files"], 0, "{delta}");
    let conn = reader::open(&w.db(), &w.0).unwrap();
    let targets = rows(&conn, "SELECT (SELECT id FROM nodes WHERE node_hash=e.dst_hash) AS dst_public_id FROM edges e JOIN nodes n ON n.node_hash=e.src_hash WHERE n.qualname='provider.value' AND e.kind='calls'");
    assert!(
        targets.iter().any(|target| target.contains("right")),
        "{targets:?}"
    );
    assert!(
        !targets.iter().any(|target| target.contains("left")),
        "{targets:?}"
    );
    drop(conn);
    w.assert_cold_equivalent();
    w.write(
        "provider.py",
        "def left(): return 1\ndef right(): unused = 0; return 2\ndef value(): return right()\n\n",
    );
    let local_edit = w.delta("provider.py");
    assert_eq!(local_edit["affected_owners"], 1, "{local_edit}");
    assert_eq!(local_edit["loaded_fact_files"], 0, "{local_edit}");
    w.assert_cold_equivalent();
    w.write(
        "provider.py",
        "\ndef left(): return 1\ndef right(): return 2\ndef value(): return right()\n",
    );
    let moved = w.delta("provider.py");
    assert!(moved["affected_owners"].as_u64().unwrap() > 1, "{moved}");
    w.assert_cold_equivalent();
}

#[test]
fn additions_deletions_and_signature_changes_still_relink_consumers() {
    let w = Workspace::new();
    w.write("provider.py", "def value(): return 1\n");
    w.write(
        "consumer.py",
        "from provider import value, future\ndef consume(): return value(), future()\n",
    );
    w.write(
        "docs/api.md",
        "# API\nUse `provider.future` and `provider.value`.\n",
    );
    w.build();
    for source in [
        "def value(arg=1): return arg\n",
        "def value(arg=1): return arg\ndef future(): return 2\n",
        "def future(): return 2\n",
    ] {
        w.write("provider.py", source);
        let delta = w.delta("provider.py");
        assert!(delta["affected_owners"].as_u64().unwrap() > 1, "{delta}");
        w.assert_cold_equivalent();
    }
}

#[test]
fn default_export_swap_updates_importers() {
    let w = Workspace::new();
    w.write(
        "provider.ts",
        "export default function first() { return 1; }\nexport function second() { return 2; }\n",
    );
    w.write(
        "consumer.ts",
        "import value from './provider';\nexport function consume() { return value(); }\n",
    );
    w.build();
    w.write(
        "provider.ts",
        "export function first() { return 1; }\nexport default function second() { return 2; }\n",
    );
    let delta = w.delta("provider.ts");
    assert_eq!(delta["affected_owners"], 2, "{delta}");
    w.assert_cold_equivalent();
}

#[test]
fn linked_body_edit_preserves_cross_workspace_consumers() {
    let w = Workspace::new();
    let linked = Workspace::new();
    linked.write("src/provider.py", "def value(): return 1\n");
    w.write(
        "local/consumer.py",
        "from provider import value\ndef consume(): return value()\n",
    );
    w.write("forge-mcp.yaml", &format!("roots: [local]\nlinked_workspaces:\n  - name: library\n    path: {}\n    roots: [src]\n", linked.0.display()));
    w.build();
    linked.write("src/provider.py", "def value(): return 2\n\n");
    let delta = w.delta("[library]/src/provider.py");
    assert_eq!(delta["affected_owners"], 1, "{delta}");
    w.assert_cold_equivalent();
}
