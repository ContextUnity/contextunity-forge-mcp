#![cfg(any(feature = "lang-python", feature = "lang-rust", feature = "lang-vue"))]

use contextunity_forge_mcp::{
    core::commitments,
    db::reader,
};
use rusqlite::Connection;
use std::fs;

use crate::common::Workspace;

fn assert_cold_equivalent(workspace: &Workspace) {
    let receipt = workspace.db().with_extension("sqlite.verified.json");
    if receipt.exists() {
        fs::remove_file(receipt).unwrap();
    }
    let incremental = workspace.open();
    commitments::verify(&incremental).unwrap();
    let dangling: i64 = incremental.query_row("SELECT count(*) FROM edges e LEFT JOIN nodes s ON s.node_hash=e.src_hash LEFT JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.id IS NULL OR d.id IS NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(dangling, 0);
    let cold_path = workspace.path(".forge/cold.sqlite");
    workspace.build_to(&cold_path);
    let cold = reader::open(&cold_path, workspace.root()).unwrap();
    commitments::verify(&cold).unwrap();
    for conn in [&incremental, &cold] {
        let mut statement = conn
            .prepare("SELECT path,facts_blob FROM local_facts")
            .unwrap();
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .unwrap();
        for row in rows {
            let (path, facts_blob) = row.unwrap();
            let facts_json = zstd::stream::decode_all(&facts_blob[..]).unwrap();
            assert!(!commitments::hash(&facts_json).is_empty(), "{path}");
        }
    }
    for sql in [
        "SELECT p.path||'|'||n.id||'|'||n.kind||'|'||np.path FROM nodes n JOIN path_dictionary p ON p.path_id=n.owner_path_id JOIN path_dictionary np ON np.path_id=n.path_id ORDER BY p.path,n.id",
        "SELECT path||'|'||hex(facts_blob) FROM local_facts ORDER BY path",
        "SELECT p.path||'|'||o.ordinal||'|'||s.id||'|'||d.id||'|'||o.kind||'|'||o.line||'|'||v.evidence FROM edge_occurrences o JOIN path_dictionary p ON p.path_id=o.owner_id JOIN nodes s ON s.node_hash=o.src_hash JOIN nodes d ON d.node_hash=o.dst_hash JOIN coverage_evidence v ON v.evidence_id=o.confidence_id ORDER BY p.path,o.ordinal",
        "SELECT (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)||'|'||line||'|'||(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)||'|'||status||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)",
        "SELECT p.path||'|'||c.domain||'|'||hex(c.digest) FROM domain_commitments c JOIN path_dictionary p ON p.path_id=c.owner_id ORDER BY p.path,c.domain",
    ] {
        assert_eq!(strings(&incremental, sql), strings(&cold, sql), "{sql}");
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
            format!("part{component:02}/a.py"),
            &format!("def value_{component}():\n    return {component}\n"),
        );
        w.write(
            format!("part{component:02}/b.py"),
            &format!("def extra_{component}():\n    return {component}\n"),
        );
    }
    w.build();
    let sql =
        "SELECT id||'|'||node_id FROM nodes WHERE path_id NOT IN(SELECT path_id FROM path_dictionary WHERE path IN('part23','part23/b.py')) ORDER BY id";
    let before = strings(&w.open(), sql);
    w.write("part23/b.py", "def extra_23():\n    return 999\n");
    let report = w.delta(&["part23/b.py"]);
    assert_eq!(report["affected_owners"], 1, "{report}");
    assert_eq!(before, strings(&w.open(), sql));
    assert_eq!(report["reparsed_files"], 1);
    assert_eq!(report["loaded_fact_files"], 0);
    assert_eq!(report["rewritten_files"], 1);
    assert_cold_equivalent(&w);
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
            format!("services/other/src/contextunity/other/helper_{n}.py"),
            &format!("def helper_{n}(): return {n}\n"),
        );
        w.write(
            format!("services/other/src/contextunity/other/view_{n}.py"),
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
    assert_cold_equivalent(&w);
}

#[test]
#[cfg(feature = "lang-python")]
fn linked_component_transfer_preserves_unmodified_containment_edges() {
    let w = Workspace::new();
    let linked = Workspace::new();
    w.write("local/a.py", "def local(): pass\n");
    linked.write("src/b.py", "def bravo(): pass\n");
    linked.write("src/c.py", "def charlie(): pass\n");
    w.write("forge-mcp.yaml", &format!("roots: [local]\nlinked_workspaces:\n  - name: linked\n    path: {}\n    roots: [src]\n", linked.root().display()));
    w.build();
    let untouched_sql = "SELECT id||'|'||node_id FROM nodes WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path='local/a.py') ORDER BY id";
    let before = strings(&w.open(), untouched_sql);
    linked.write("src/a.py", "def alpha(): pass\n");
    let report = w.delta(&["[linked]/src/a.py"]);
    assert_eq!(report["rewritten_files"], 2, "{report}");
    assert_eq!(report["loaded_fact_files"], 1, "{report}");
    assert_eq!(before, strings(&w.open(), untouched_sql));
    assert_cold_equivalent(&w);
    for path in ["src/a.py", "src/b.py", "src/c.py"] {
        fs::remove_file(linked.path(path)).unwrap();
        w.delta(&[&format!("[linked]/{path}")]);
        assert_cold_equivalent(&w);
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
        assert_cold_equivalent(&w);
    }
    assert_eq!(strings(&w.open(), "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.py' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='provide' ORDER BY line"), ["ambiguous", "unresolved"]);
    for path in ["lib/__init__.py", "lib.py"] {
        fs::remove_file(w.path(path)).unwrap();
        w.delta(&[path]);
        assert_cold_equivalent(&w);
    }
    assert_eq!(
        strings(
            &w.open(),
            "SELECT (SELECT id FROM nodes WHERE node_hash=dst_hash) FROM edges WHERE kind='documents'"
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
        assert_cold_equivalent(&w);
    }
    fs::rename(w.path("part/a.py"), w.path("part/d.py")).unwrap();
    w.delta(&["part/a.py", "part/d.py"]);
    assert_cold_equivalent(&w);
    for path in ["part/b.py", "part/c.py", "part/d.py"] {
        fs::remove_file(w.path(path)).unwrap();
        w.delta(&[path]);
        assert_cold_equivalent(&w);
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
        "SELECT (SELECT path FROM path_dictionary WHERE path_id=edges.path_id)||'|'||occurrence_count FROM edges WHERE kind='implements'",
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
        fs::remove_file(w.path(deleted)).unwrap();
        let report = w.delta(&[deleted]);
        assert_eq!(report["affected_owners"], 1, "{report}");
        assert_eq!(report["reparsed_files"], 0, "{report}");
        assert_eq!(report["loaded_fact_files"], 0, "{report}");
        assert_eq!(report["rewritten_files"], 0, "{report}");
        assert_aggregate(&w, surviving, 1);
        assert_cold_equivalent(&w);
        fs::remove_file(w.path(surviving)).unwrap();
        w.delta(&[surviving]);
        assert_aggregate(&w, "", 0);
        assert_cold_equivalent(&w);
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
        assert_cold_equivalent(&w);
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
        assert_cold_equivalent(&w);
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
        fs::create_dir_all(w.path(new).parent().unwrap()).unwrap();
        fs::rename(w.path(old), w.path(new)).unwrap();
        let report = w.delta(&[old, new]);
        assert_eq!(report["affected_owners"], 2, "{report}");
        assert_eq!(report["loaded_fact_files"], 0, "{report}");
        assert_eq!(report["rewritten_files"], 1, "{report}");
        assert_aggregate(&w, owner, 2);
        assert_cold_equivalent(&w);
    }
}

#[test]
#[cfg(feature = "lang-vue")]
fn vue_setup_body_edits_refresh_calls_and_preserve_consumer_contracts() {
    let w = Workspace::new();
    w.write(
        "Provider.vue",
        "<script setup>\nexport function run() { defineProps(); return 1; }\n</script>\n",
    );
    w.write(
        "consumer.ts",
        "import { run } from './Provider.vue';\nrun();\n",
    );
    w.build();
    let consumer_hashes = |conn: &Connection| {
        strings(conn,
        "SELECT domain||'|'||hex(digest) FROM domain_commitments WHERE owner_id=(SELECT path_id FROM path_dictionary WHERE path='consumer.ts') ORDER BY domain")
    };
    let before = consumer_hashes(&w.open());
    assert_eq!(
        strings(
            &w.open(),
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='run' AND line=2"
        ),
        ["resolved"]
    );
    w.write(
        "Provider.vue",
        "<script setup>\nexport function run() {\n  defineOptions();\n  return 1;\n}\n</script>\n",
    );
    let report = w.delta(&["Provider.vue"]);
    assert_eq!(report["affected_owners"], 1, "{report}");
    assert_eq!(report["reparsed_files"], 1, "{report}");
    assert_eq!(consumer_hashes(&w.open()), before);
    assert_eq!(strings(&w.open(), "SELECT status||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='Provider.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='defineOptions'"), ["external|builtin:vue_macro built-in: defineOptions"]);
    assert_cold_equivalent(&w);
}
