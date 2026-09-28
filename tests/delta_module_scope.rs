#![cfg(all(feature = "lang-python", feature = "lang-typescript"))]

use contextunity_forge_mcp::{core::commitments, db::{reader, writer}};
use rusqlite::Connection;
use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("forge_delta_modules_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    fn db(&self) -> PathBuf { self.0.join(".forge/code-map.sqlite") }
    fn build(&self) { writer::build(&self.0, &self.db(), None).unwrap(); }
    fn delta(&self, path: &str) -> serde_json::Value {
        writer::delta(&self.0, &self.db(), &[PathBuf::from(path)]).unwrap()
    }
    fn assert_cold_equivalent(&self) {
        let incremental = reader::open(&self.db(), &self.0).unwrap();
        commitments::verify(&incremental).unwrap();
        let cold = self.0.join(".forge/cold.sqlite");
        writer::build(&self.0, &cold, None).unwrap();
        let cold = reader::open(&cold, &self.0).unwrap();
        for query in [
            "SELECT id||'|'||qualname FROM nodes ORDER BY id",
            "SELECT src_public_id||'|'||dst_public_id||'|'||kind||'|'||path||'|'||line||'|'||occurrence_count FROM edges ORDER BY src_public_id,dst_public_id,kind",
            "SELECT path||'|'||line||'|'||expression||'|'||status||'|'||evidence FROM resolution_coverage ORDER BY path,line,expression,status,evidence",
        ] { assert_eq!(rows(&incremental, query), rows(&cold, query), "{query}"); }
    }
}
impl Drop for Workspace { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
fn rows(conn: &Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql).unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
}

#[test]
fn stable_module_edit_keeps_relative_consumers_and_docs_without_basename_fanout() {
    let w = Workspace::new();
    w.write("pkg/types.py", "def present(): return 1\n");
    w.write("pkg/consumer.py", "from .types import present, future\ndef consume(): return present(), future()\n");
    w.write("docs/api.md", "# API\nUse `types.future` and `present`.\n");
    w.write("other/types.py", "def unrelated(): return 1\n");
    w.write("other/consumer.py", "from .types import unrelated\ndef call(): return unrelated()\n");
    w.write("web/consumer.ts", "export function run(types: any) { return types.unrelated(); }\n");
    w.build();
    w.write("pkg/types.py", "def present(): return 2\ndef future(): return 3\n");
    let delta = w.delta("pkg/types.py");
    assert_eq!(delta["affected_owners"], 3, "{delta}");
    let conn = reader::open(&w.db(), &w.0).unwrap();
    let statuses = rows(&conn, "SELECT status FROM resolution_coverage WHERE path='pkg/consumer.py' AND expression='future'");
    assert!(!statuses.is_empty());
    assert!(statuses.iter().all(|s| s == "resolved"));
    assert!(!rows(&conn, "SELECT dst_public_id FROM edges WHERE kind='documents' AND dst_public_id LIKE '%future%'").is_empty());
    drop(conn);
    w.assert_cold_equivalent();
}

#[test]
fn module_addition_ambiguity_and_deletion_keep_relative_imports_and_doc_suffixes() {
    let w = Workspace::new();
    w.write("pkg/consumer.py", "from .types import provide\ndef consume(): return provide()\n");
    w.write("docs/api.md", "# API\nUse `types.provide`.\n");
    w.build();
    for path in ["pkg/types.py", "pkg/types/__init__.py"] {
        w.write(path, "def provide(): return 1\n");
        w.delta(path);
        w.assert_cold_equivalent();
    }
    for path in ["pkg/types/__init__.py", "pkg/types.py"] {
        fs::remove_file(w.0.join(path)).unwrap();
        w.delta(path);
        w.assert_cold_equivalent();
    }
}
