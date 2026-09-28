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

fn call_target_paths(conn: &Connection) -> Vec<String> {
    rows(conn, "SELECT dst.path FROM edges e JOIN nodes src ON src.id=e.src_public_id JOIN nodes dst ON dst.id=e.dst_public_id WHERE src.qualname='consumer.consume' AND e.kind='calls' ORDER BY dst.path")
}

fn import_targets(conn: &Connection) -> Vec<String> {
    rows(conn, "SELECT dst.qualname FROM edges e JOIN nodes src ON src.id=e.src_public_id JOIN nodes dst ON dst.id=e.dst_public_id WHERE src.qualname='consumer' AND e.kind='imports' ORDER BY dst.qualname")
}

fn import_target_paths(conn: &Connection) -> Vec<String> {
    rows(conn, "SELECT dst.path FROM edges e JOIN nodes src ON src.id=e.src_public_id JOIN nodes dst ON dst.id=e.dst_public_id WHERE src.qualname='consumer' AND e.kind='imports' ORDER BY dst.path")
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
fn explicit_package_reexport_resolves_import_and_call() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "from .logging import get_log\n");
    w.write("pkg/logging.py", "def get_log(): return 1\n");
    w.write("other.py", "def helper(): return 2\n");
    w.write(
        "consumer.py",
        "from pkg import get_log\ndef consume(): return get_log()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "get_log"), "resolved");
    assert_eq!(
        import_target_paths(&conn),
        ["pkg/__init__.py", "pkg/logging.py"]
    );
    assert_eq!(call_targets(&conn), ["pkg.logging.get_log"]);
    drop(conn);

    w.write("pkg/__init__.py", "# export removed\n");
    w.delta("pkg/__init__.py");
    assert_eq!(status(&w.open(), "get_log"), "unresolved");
    w.assert_cold_equivalent();
    w.write("pkg/__init__.py", "from .logging import get_log\n");
    w.delta("pkg/__init__.py");
    assert_eq!(status(&w.open(), "get_log"), "resolved");
    w.assert_cold_equivalent();
    w.write("other.py", "def get_log(): return 2\n");
    w.delta("other.py");
    assert_eq!(status(&w.open(), "get_log"), "resolved");
    w.assert_cold_equivalent();
}

#[test]
fn duplicate_package_reexports_do_not_choose_a_target() {
    let w = Workspace::new();
    w.write(
        "pkg/__init__.py",
        "from .a import get_log\nfrom .b import get_log\n",
    );
    w.write("pkg/a.py", "def get_log(): return 1\n");
    w.write("pkg/b.py", "def get_log(): return 2\n");
    w.write(
        "consumer.py",
        "from pkg import get_log\ndef consume(): return get_log()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "get_log"), "unresolved");
    assert!(call_targets(&conn).is_empty());
}

#[test]
fn paired_runtime_and_stub_choose_runtime_child() {
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
    assert_eq!(status(&conn, "child"), "resolved");
    assert_eq!(
        import_target_paths(&conn),
        ["pkg/__init__.py", "pkg/child.py"]
    );
    assert_eq!(call_targets(&conn), ["pkg.child.value"]);
    let stub: i64 = conn
        .query_row(
            "SELECT count(*) FROM nodes WHERE path='pkg/child.pyi'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(stub > 0);
}

#[test]
fn paired_runtime_and_stub_choose_runtime_for_direct_module_import() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.py", "def value(): return 1\n");
    w.write("pkg/child.pyi", "def value() -> int: ...\n");
    w.write(
        "consumer.py",
        "import pkg.child as child\ndef consume(): return child.value()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "pkg.child"), "resolved");
    assert_eq!(import_target_paths(&conn), ["pkg/child.py"]);
    assert_eq!(call_targets(&conn), ["pkg.child.value"]);
}

#[test]
fn paired_stub_supplies_declaration_missing_from_generated_runtime() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.py", "def __getattr__(name): return None\n");
    w.write(
        "pkg/child.pyi",
        "class ContextUnit:\n    def value(self) -> int: ...\n",
    );
    w.write(
        "consumer.py",
        "from pkg.child import ContextUnit\ndef consume(): return ContextUnit()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "ContextUnit"), "resolved");
    assert_eq!(
        import_target_paths(&conn),
        ["pkg/child.py", "pkg/child.pyi"]
    );
    assert_eq!(call_target_paths(&conn), ["pkg/child.pyi"]);
    let evidence: String = conn.query_row("SELECT evidence FROM resolution_coverage WHERE path='consumer.py' AND line=1 AND expression='ContextUnit'", [], |row| row.get(0)).unwrap();
    assert!(evidence.contains("type stub"), "{evidence}");
    drop(conn);
    w.write("pkg/child.py", "def __getattr__(name): return name\n");
    w.delta("pkg/child.py");
    w.assert_cold_equivalent();
    w.write("pkg/child.py", "def __getattr__(name): return None\n");
    w.delta("pkg/child.py");
    w.assert_cold_equivalent();
}

#[test]
fn child_module_call_uses_stub_only_member() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.py", "def __getattr__(name): return None\n");
    w.write("pkg/child.pyi", "class ContextUnit: ...\n");
    w.write(
        "consumer.py",
        "from pkg import child\ndef consume(): return child.ContextUnit()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "resolved");
    assert_eq!(
        import_target_paths(&conn),
        ["pkg/__init__.py", "pkg/child.py"]
    );
    assert_eq!(call_target_paths(&conn), ["pkg/child.pyi"]);
}

#[test]
fn stub_only_module_remains_navigable() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.pyi", "def value() -> int: ...\n");
    w.write(
        "consumer.py",
        "from pkg import child\ndef consume(): return child.value()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "resolved");
    assert_eq!(
        import_target_paths(&conn),
        ["pkg/__init__.py", "pkg/child.pyi"]
    );
    assert_eq!(call_targets(&conn), ["pkg.child.value"]);
}

#[test]
fn multiple_runtime_modules_remain_ambiguous() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "# package\n");
    w.write("pkg/child.py", "def value(): return 1\n");
    w.write("src/pkg/child.py", "def value(): return 2\n");
    w.write(
        "consumer.py",
        "from pkg import child\ndef consume(): return child.value()\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(status(&conn, "child"), "ambiguous");
    assert_eq!(import_target_paths(&conn), ["pkg/__init__.py"]);
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
    w.write("pkg/child.pyi", "def value() -> int: ...\n");
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
