#![cfg(feature = "lang-python")]

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
        let root =
            std::env::temp_dir().join(format!("forge_python_child_{}_{nonce}", std::process::id()));
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
    fn delta(&self, path: &str) {
        writer::delta(&self.0, &self.db(), &[PathBuf::from(path)]).unwrap();
    }
    fn open(&self) -> Connection {
        reader::open(&self.db(), &self.0).unwrap()
    }

    fn assert_cold_equivalent(&self) {
        let incremental = self.open();
        commitments::verify(&incremental).unwrap();
        let cold_path = self.0.join(".forge/cold.sqlite");
        writer::build(&self.0, &cold_path, None).unwrap();
        let cold = reader::open(&cold_path, &self.0).unwrap();
        commitments::verify(&cold).unwrap();
        for sql in [
            "SELECT src_public_id||'|'||dst_public_id||'|'||kind||'|'||path||'|'||line FROM edges ORDER BY src_public_id,dst_public_id,kind,path,line",
            "SELECT path||'|'||line||'|'||expression||'|'||status||'|'||evidence FROM resolution_coverage ORDER BY path,line,expression,status,evidence",
        ] {
            assert_eq!(rows(&incremental, sql), rows(&cold, sql), "{sql}");
        }
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
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn status(conn: &Connection, expression: &str) -> String {
    conn.query_row(
        "SELECT status FROM resolution_coverage WHERE path='consumer.py' AND expression=?1 AND line=1",
        [expression],
        |row| row.get(0),
    ).unwrap()
}

fn call_targets(conn: &Connection) -> Vec<String> {
    rows(conn, "SELECT dst.qualname FROM edges e JOIN nodes src ON src.id=e.src_public_id JOIN nodes dst ON dst.id=e.dst_public_id WHERE src.qualname='consumer.consume' AND e.kind='calls' ORDER BY dst.qualname")
}

fn import_targets(conn: &Connection) -> Vec<String> {
    rows(conn, "SELECT dst.qualname FROM edges e JOIN nodes src ON src.id=e.src_public_id JOIN nodes dst ON dst.id=e.dst_public_id WHERE src.qualname='consumer' AND e.kind='imports' ORDER BY dst.qualname")
}

#[test]
fn unique_child_module_is_imported_and_its_calls_resolve() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.py", "def value(): return 1\n");
    w.write(
        "consumer.py",
        "from pkg import child\ndef consume(): return child.value()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "resolved");
    assert_eq!(import_targets(&conn), ["pkg", "pkg.child"]);
    assert_eq!(call_targets(&conn), ["pkg.child.value"]);
}

#[test]
fn duplicate_child_modules_do_not_choose_a_target() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.py", "def value(): return 1\n");
    w.write("pkg/child.pyi", "def value() -> int: ...\n");
    w.write(
        "consumer.py",
        "from pkg import child\ndef consume(): return child.value()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "ambiguous");
    assert_eq!(import_targets(&conn), ["pkg"]);
    assert!(call_targets(&conn).is_empty());
}

#[test]
fn package_symbol_takes_precedence_over_same_named_child_module() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "def child(): return 1\n");
    w.write("pkg/child.py", "def value(): return 2\n");
    w.write(
        "consumer.py",
        "from pkg import child\ndef consume(): return child()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "resolved");
    assert_eq!(import_targets(&conn), ["pkg"]);
    assert_eq!(call_targets(&conn), ["pkg.child"]);
}

#[test]
fn unrelated_child_module_is_not_a_provider() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("other/child.py", "def value(): return 1\n");
    w.write(
        "consumer.py",
        "from pkg import child\ndef consume(): return child.value()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "unresolved");
    assert_eq!(import_targets(&conn), ["pkg"]);
    assert!(call_targets(&conn).is_empty());
}

#[test]
fn child_edit_and_restore_match_cold_graph() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.py", "def value(): return 1\n");
    w.write(
        "consumer.py",
        "from pkg import child\ndef consume(): return child.value()\n",
    );
    w.build();
    w.write("pkg/child.py", "def value(arg=1): return arg\n");
    w.delta("pkg/child.py");
    w.assert_cold_equivalent();
    w.write("pkg/child.py", "def value(): return 1\n");
    w.delta("pkg/child.py");
    w.assert_cold_equivalent();
}
