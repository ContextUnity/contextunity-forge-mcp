#![cfg(all(feature = "lang-python", feature = "lang-typescript"))]

use crate::common::Workspace;
use contextunity_forge_mcp::{core::commitments, db::reader};
use rusqlite::Connection;

fn assert_cold_equivalent(workspace: &Workspace) {
    let incremental = workspace.open();
    commitments::verify(&incremental).unwrap();
    let cold = workspace.path(".forge/cold.sqlite");
    workspace.build_to(&cold);
    let cold = reader::open(&cold, workspace.root()).unwrap();
    for query in [
        "SELECT id||'|'||qualname FROM nodes ORDER BY id",
        "SELECT (SELECT id FROM nodes WHERE node_hash=src_hash)||'|'||(SELECT id FROM nodes WHERE node_hash=dst_hash)||'|'||kind||'|'||(SELECT path FROM path_dictionary WHERE path_id=edges.path_id)||'|'||line||'|'||occurrence_count FROM edges ORDER BY (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind",
        "SELECT (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)||'|'||line||'|'||(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)||'|'||status||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)",
    ] {
        assert_eq!(rows(&incremental, query), rows(&cold, query), "{query}");
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
fn stable_module_edit_keeps_relative_consumers_and_docs_without_basename_fanout() {
    let w = Workspace::new();
    w.write("pkg/types.py", "def present(): return 1\n");
    w.write(
        "pkg/consumer.py",
        "from .types import present, future\ndef consume(): return present(), future()\n",
    );
    w.write("docs/api.md", "# API\nUse `types.future` and `present`.\n");
    w.write("other/types.py", "def unrelated(): return 1\n");
    w.write(
        "other/consumer.py",
        "from .types import unrelated\ndef call(): return unrelated()\n",
    );
    w.write(
        "web/consumer.ts",
        "export function run(types: any) { return types.unrelated(); }\n",
    );
    w.build();
    w.write(
        "pkg/types.py",
        "def present(): return 2\ndef future(): return 3\n",
    );
    let delta = w.delta(&["pkg/types.py"]);
    assert_eq!(delta["affected_owners"], 3, "{delta}");
    let conn = w.open();
    let statuses = rows(&conn, "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='pkg/consumer.py' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='future'");
    assert!(!statuses.is_empty());
    assert!(statuses.iter().all(|s| s == "resolved"));
    assert!(!rows(
        &conn,
        "SELECT (SELECT id FROM nodes WHERE node_hash=dst_hash) FROM edges WHERE kind='documents' AND (SELECT id FROM nodes WHERE node_hash=dst_hash) LIKE '%future%'"
    )
    .is_empty());
    drop(conn);
    assert_cold_equivalent(&w);
}

#[test]
fn module_addition_ambiguity_and_deletion_keep_relative_imports_and_doc_suffixes() {
    let w = Workspace::new();
    w.write(
        "pkg/consumer.py",
        "from .types import provide\ndef consume(): return provide()\n",
    );
    w.write("docs/api.md", "# API\nUse `types.provide`.\n");
    w.build();
    for path in ["pkg/types.py", "pkg/types/__init__.py"] {
        w.write(path, "def provide(): return 1\n");
        w.delta(&[path]);
        assert_cold_equivalent(&w);
    }
    for path in ["pkg/types/__init__.py", "pkg/types.py"] {
        std::fs::remove_file(w.path(path)).unwrap();
        w.delta(&[path]);
        assert_cold_equivalent(&w);
    }
}

#[test]
fn removing_a_populated_module_directory_prunes_all_its_files() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "from .types import provide\n");
    w.write("pkg/types.py", "def provide(): return 1\n");
    w.write("pkg/subpkg/__init__.py", "def helper(): return 2\n");
    w.write(
        "consumer.py",
        "from pkg.types import provide\ndef use(): return provide()\n",
    );
    w.build();

    std::fs::remove_dir_all(w.path("pkg")).unwrap();
    let deleted = ["pkg/__init__.py", "pkg/types.py", "pkg/subpkg/__init__.py"]
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect::<Vec<_>>();
    w.delta(&deleted);
    assert_cold_equivalent(&w);

    let conn = w.open();
    assert!(rows(&conn, "SELECT path FROM files WHERE path LIKE 'pkg/%'").is_empty());
}
