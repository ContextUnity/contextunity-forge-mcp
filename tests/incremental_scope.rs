#![cfg(any(feature = "lang-python", feature = "lang-rust"))]

use contextunity_forge_mcp::{
    core::commitments,
    db::{reader, writer},
};
use rusqlite::Connection;
use serde_json::Value;
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
        let root = std::env::temp_dir().join(format!("forge_scope_{}_{nonce}", std::process::id()));
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
    fn open(&self) -> Connection {
        reader::open(&self.db(), &self.0).unwrap()
    }
    fn delta(&self, paths: &[&str]) -> Value {
        writer::delta(
            &self.0,
            &self.db(),
            &paths.iter().map(PathBuf::from).collect::<Vec<_>>(),
        )
        .unwrap()
    }
    fn assert_cold_equivalent(&self) {
        let receipt = self.db().with_extension("sqlite.verified.json");
        if receipt.exists() {
            fs::remove_file(receipt).unwrap();
        }
        let incremental = self.open();
        commitments::verify(&incremental).unwrap();
        let dangling: i64 = incremental.query_row("SELECT count(*) FROM edges e LEFT JOIN nodes s ON s.id=e.src_public_id LEFT JOIN nodes d ON d.id=e.dst_public_id WHERE s.id IS NULL OR d.id IS NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(dangling, 0);
        let cold = self.0.join(".forge/cold.sqlite");
        writer::build(&self.0, &cold, None).unwrap();
        let cold = reader::open(&cold, &self.0).unwrap();
        commitments::verify(&cold).unwrap();
        for conn in [&incremental, &cold] {
            let mut statement = conn.prepare("SELECT f.path,f.facts_json,c.facts_hash FROM local_facts f JOIN file_commitments c ON c.path=f.path").unwrap();
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .unwrap();
            for row in rows {
                let (path, facts_json, facts_hash) = row.unwrap();
                assert_eq!(
                    facts_hash,
                    commitments::hash(facts_json.as_bytes()),
                    "{path}"
                );
            }
        }
        for sql in [
            "SELECT owner||'|'||public_id||'|'||kind||'|'||path FROM owned_nodes ORDER BY owner,public_id",
            "SELECT path||'|'||facts_json FROM local_facts ORDER BY path",
            "SELECT owner||'|'||ordinal||'|'||src||'|'||dst||'|'||kind||'|'||line||'|'||confidence FROM edge_occurrences ORDER BY owner,ordinal",
            "SELECT path||'|'||line||'|'||expression||'|'||status||'|'||evidence FROM resolution_coverage ORDER BY path,line,expression,status,evidence",
            "SELECT path||'|'||facts_hash||'|'||nodes_hash||'|'||edges_hash||'|'||search_hash||'|'||deps_hash FROM file_commitments ORDER BY path",
        ] {
            assert_eq!(strings(&incremental, sql), strings(&cold, sql), "{sql}");
        }
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn strings(conn: &Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn document_mtime_json_roundtrip_preserves_commitment_input() {
    // An observed timestamp that parsed one ULP lower without float_roundtrip.
    let mtime = 1_790_494_491.067_507_7_f64;
    let encoded = serde_json::to_string(&mtime).unwrap();
    let decoded: f64 = serde_json::from_str(&encoded).unwrap();
    assert_eq!(mtime.to_bits(), decoded.to_bits(), "{encoded}");
}

#[test]
#[cfg(feature = "lang-python")]
fn one_file_edit_does_not_rewrite_unrelated_component_owners() {
    let w = Workspace::new();
    for component in 0..48 {
        w.write(
            &format!("part{component:02}/a.py"),
            &format!("def value_{component}():\n    return {component}\n"),
        );
        w.write(
            &format!("part{component:02}/b.py"),
            &format!("def extra_{component}():\n    return {component}\n"),
        );
    }
    w.build();
    let sql =
        "SELECT id||'|'||node_id FROM nodes WHERE path NOT IN('part23','part23/b.py') ORDER BY id";
    let before = strings(&w.open(), sql);
    w.write("part23/b.py", "def extra_23():\n    return 999\n");
    let report = w.delta(&["part23/b.py"]);
    assert_eq!(report["affected_owners"], 1, "{report}");
    assert_eq!(before, strings(&w.open(), sql));
    assert_eq!(report["reparsed_files"], 1);
    assert_eq!(report["loaded_fact_files"], 0);
    assert_eq!(report["rewritten_files"], 1);
    w.assert_cold_equivalent();
}

#[test]
#[cfg(feature = "lang-python")]
fn src_namespace_edit_relinks_its_consumer_without_invalidating_siblings() {
    let w = Workspace::new();
    let provider = "services/shield/src/contextunity/shield/audit.py";
    w.write(provider, "def log_event(): return 1\n");
    w.write(
        "services/client/src/contextunity/client/main.py",
        "from contextunity.shield.audit import log_event\ndef run(): return log_event()\n",
    );
    for n in 0..20 {
        w.write(
            &format!("services/other/src/contextunity/other/helper_{n}.py"),
            &format!("def helper_{n}(): return {n}\n"),
        );
        w.write(
            &format!("services/other/src/contextunity/other/view_{n}.py"),
            &format!("from contextunity.other.helper_{n} import helper_{n}\ndef view(): return helper_{n}()\n"),
        );
    }
    w.build();
    w.write(provider, "def log_event_new(): return 2\n");
    let report = w.delta(&[provider]);
    assert_eq!(report["reparsed_files"], 1, "{report}");
    assert_eq!(report["rewritten_files"], 1, "{report}");
    assert_eq!(report["affected_owners"], 2, "{report}");
    assert_eq!(report["loaded_fact_files"], 1, "{report}");
    w.assert_cold_equivalent();
}

#[test]
#[cfg(feature = "lang-python")]
fn linked_component_transfer_preserves_unmodified_containment_edges() {
    let w = Workspace::new();
    let linked = Workspace::new();
    w.write("local/a.py", "def local(): pass\n");
    linked.write("src/b.py", "def bravo(): pass\n");
    linked.write("src/c.py", "def charlie(): pass\n");
    w.write("forge-mcp.yaml", &format!("roots: [local]\nlinked_workspaces:\n  - name: linked\n    path: {}\n    roots: [src]\n", linked.0.display()));
    w.build();
    let untouched_sql = "SELECT id||'|'||node_id FROM nodes WHERE path='local/a.py' ORDER BY id";
    let before = strings(&w.open(), untouched_sql);
    linked.write("src/a.py", "def alpha(): pass\n");
    let report = w.delta(&["[linked]/src/a.py"]);
    assert_eq!(report["rewritten_files"], 2, "{report}");
    assert_eq!(report["loaded_fact_files"], 1, "{report}");
    assert_eq!(before, strings(&w.open(), untouched_sql));
    w.assert_cold_equivalent();
    for path in ["src/a.py", "src/b.py", "src/c.py"] {
        fs::remove_file(linked.0.join(path)).unwrap();
        w.delta(&[&format!("[linked]/{path}")]);
        w.assert_cold_equivalent();
    }
}

#[test]
#[cfg(feature = "lang-python")]
fn provider_addition_ambiguity_and_removal_relink_consumers_and_docs() {
    let w = Workspace::new();
    w.write(
        "consumer.py",
        "from lib import provide\ndef consume():\n    return provide()\n",
    );
    w.write("docs/api.md", "# API\nUse `provide` to read the value.\n");
    w.write("other/a.py", "def untouched(): pass\n");
    w.build();
    for (path, source) in [
        ("lib.py", "def provide(): return 1\n"),
        ("lib/__init__.py", "def provide(): return 2\n"),
    ] {
        w.write(path, source);
        w.delta(&[path]);
        w.assert_cold_equivalent();
    }
    assert_eq!(strings(&w.open(), "SELECT status FROM resolution_coverage WHERE path='consumer.py' AND expression='provide' ORDER BY line"), ["ambiguous", "unresolved"]);
    for path in ["lib/__init__.py", "lib.py"] {
        fs::remove_file(w.0.join(path)).unwrap();
        w.delta(&[path]);
        w.assert_cold_equivalent();
    }
    assert_eq!(
        strings(
            &w.open(),
            "SELECT dst_public_id FROM edges WHERE kind='documents'"
        ),
        Vec::<String>::new()
    );
}

#[test]
#[cfg(feature = "lang-python")]
fn component_representative_mutations_match_cold_builds() {
    let w = Workspace::new();
    w.write("part/b.py", "def bravo(): pass\n");
    w.write("part/c.py", "def charlie(): pass\n");
    w.write("other/a.py", "def unrelated(): pass\n");
    w.build();
    for (changed, source) in [
        ("part/c.py", "def charlie(): return 1\n"),
        ("part/b.py", "def bravo(): return 2\n"),
        ("part/a.py", "def alpha(): pass\n"),
    ] {
        w.write(changed, source);
        w.delta(&[changed]);
        w.assert_cold_equivalent();
    }
    fs::rename(w.0.join("part/a.py"), w.0.join("part/d.py")).unwrap();
    w.delta(&["part/a.py", "part/d.py"]);
    w.assert_cold_equivalent();
    for path in ["part/b.py", "part/c.py", "part/d.py"] {
        fs::remove_file(w.0.join(path)).unwrap();
        w.delta(&[path]);
        w.assert_cold_equivalent();
    }
    let conn = w.open();
    assert_eq!(
        strings(
            &conn,
            "SELECT id FROM nodes WHERE kind='component' ORDER BY id"
        ),
        ["component:other"]
    );
}

#[cfg(feature = "lang-rust")]
fn shared_aggregate_workspace() -> Workspace {
    let w = Workspace::new();
    w.write("src/defs.rs", "pub struct Thing; pub trait Trait {}\n");
    w.write(
        "impl_a/a.rs",
        "use src::defs as d; use src::defs::Trait as X; impl X for d::Thing {}\n",
    );
    w.write(
        "impl_b/b.rs",
        "use src::defs as e; use src::defs::Trait as Y; impl Y for e::Thing {}\n",
    );
    w.build();
    assert_aggregate(&w, "impl_a/a.rs", 2);
    w
}

#[cfg(feature = "lang-rust")]
fn assert_aggregate(w: &Workspace, owner: &str, count: usize) {
    let actual = strings(
        &w.open(),
        "SELECT path||'|'||occurrence_count FROM edges WHERE kind='implements'",
    );
    let expected = if count == 0 {
        Vec::new()
    } else {
        vec![format!("{owner}|{count}")]
    };
    assert_eq!(actual, expected);
}

#[test]
#[cfg(feature = "lang-rust")]
fn shared_aggregate_deletion_reseals_transferred_and_unchanged_owners() {
    for (deleted, surviving) in [
        ("impl_a/a.rs", "impl_b/b.rs"),
        ("impl_b/b.rs", "impl_a/a.rs"),
    ] {
        let w = shared_aggregate_workspace();
        fs::remove_file(w.0.join(deleted)).unwrap();
        let report = w.delta(&[deleted]);
        assert_eq!(report["affected_owners"], 1, "{report}");
        assert_eq!(report["reparsed_files"], 0, "{report}");
        assert_eq!(report["loaded_fact_files"], 0, "{report}");
        assert_eq!(report["rewritten_files"], 0, "{report}");
        assert_aggregate(&w, surviving, 1);
        w.assert_cold_equivalent();
        fs::remove_file(w.0.join(surviving)).unwrap();
        w.delta(&[surviving]);
        assert_aggregate(&w, "", 0);
        w.assert_cold_equivalent();
    }
}

#[test]
#[cfg(feature = "lang-rust")]
fn shared_aggregate_addition_reseals_prior_and_resulting_owners() {
    for (added, owner) in [
        ("impl_0/new.rs", "impl_0/new.rs"),
        ("impl_z/new.rs", "impl_a/a.rs"),
    ] {
        let w = shared_aggregate_workspace();
        w.write(
            added,
            "use src::defs as f; use src::defs::Trait as Z; impl Z for f::Thing {}\n",
        );
        let report = w.delta(&[added]);
        assert_eq!(report["affected_owners"], 1, "{report}");
        assert_eq!(report["reparsed_files"], 1, "{report}");
        assert_eq!(report["loaded_fact_files"], 0, "{report}");
        assert_eq!(report["rewritten_files"], 1, "{report}");
        assert_aggregate(&w, owner, 3);
        w.assert_cold_equivalent();
    }
}

#[test]
#[cfg(feature = "lang-rust")]
fn shared_aggregate_edit_reseals_transferred_and_unchanged_owners() {
    for (edited, surviving) in [
        ("impl_a/a.rs", "impl_b/b.rs"),
        ("impl_b/b.rs", "impl_a/a.rs"),
    ] {
        let w = shared_aggregate_workspace();
        w.write(edited, "pub fn placeholder() {}\n");
        let report = w.delta(&[edited]);
        assert_eq!(report["affected_owners"], 1, "{report}");
        assert_eq!(report["loaded_fact_files"], 0, "{report}");
        assert_eq!(report["rewritten_files"], 1, "{report}");
        assert_aggregate(&w, surviving, 1);
        w.assert_cold_equivalent();
    }
}

#[test]
#[cfg(feature = "lang-rust")]
fn shared_aggregate_rename_reseals_prior_and_resulting_owners() {
    for (old, new, owner) in [
        ("impl_a/a.rs", "impl_z/z.rs", "impl_b/b.rs"),
        ("impl_b/b.rs", "impl_0/z.rs", "impl_0/z.rs"),
    ] {
        let w = shared_aggregate_workspace();
        fs::create_dir_all(w.0.join(new).parent().unwrap()).unwrap();
        fs::rename(w.0.join(old), w.0.join(new)).unwrap();
        let report = w.delta(&[old, new]);
        assert_eq!(report["affected_owners"], 2, "{report}");
        assert_eq!(report["loaded_fact_files"], 0, "{report}");
        assert_eq!(report["rewritten_files"], 1, "{report}");
        assert_aggregate(&w, owner, 2);
        w.assert_cold_equivalent();
    }
}
