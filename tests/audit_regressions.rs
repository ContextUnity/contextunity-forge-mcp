use contextunity_forge_mcp::{
    db::{reader, writer},
    mcp::server::Server,
};
#[cfg(any(
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript",
    feature = "lang-vue",
    feature = "lang-proto"
))]
use serde_json::Value;
#[cfg(any(
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript",
    feature = "lang-vue",
    feature = "lang-proto"
))]
use std::process::Command;
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
        let root = std::env::temp_dir().join(format!("forge_audit_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fn db(&self) -> PathBuf {
        self.0.join(".forge/code-map.sqlite")
    }
    fn build(&self) {
        writer::build(&self.0, &self.db(), None).unwrap();
    }
    #[cfg(any(
        feature = "lang-python",
        feature = "lang-rust",
        feature = "lang-typescript",
        feature = "lang-vue",
        feature = "lang-proto"
    ))]
    fn query(&self, args: &[&str]) -> Value {
        let out = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .arg("--root")
            .arg(&self.0)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn mcp_read_preserves_foreign_and_invalid_databases() {
    let a = Workspace::new();
    let b = Workspace::new();
    a.write("a.py", "def a(): pass\n");
    b.write("b.py", "def b(): pass\n");
    b.write("forge-mcp.yaml", "roots: [b.py]\n");
    a.build();
    let before = fs::read(a.db()).unwrap();
    let error = Server::new(b.0.clone(), a.db())
        .read(reader::overview)
        .unwrap_err();
    assert!(error.to_string().contains("different workspace"), "{error}");
    assert_eq!(before, fs::read(a.db()).unwrap());
    assert!(reader::open(&a.db(), &a.0).is_ok());

    for malformed in [b"not sqlite".as_slice(), b"".as_slice()] {
        fs::write(b.db(), malformed).unwrap_or_else(|_| {
            fs::create_dir_all(b.db().parent().unwrap()).unwrap();
            fs::write(b.db(), malformed).unwrap();
        });
        assert!(Server::new(b.0.clone(), b.db())
            .read(reader::overview)
            .is_err());
        assert_eq!(malformed, fs::read(b.db()).unwrap());
    }
}

#[cfg(feature = "lang-python")]
#[test]
fn cli_delta_updates_the_index() {
    let w = Workspace::new();
    w.write("a.py", "def before(): pass\n");
    w.build();
    w.write("a.py", "def after(): pass\n");
    w.query(&["delta", w.0.to_str().unwrap(), "a.py"]);
    assert_eq!(
        w.query(&["query", "inspect", "after"])["node"]["name"],
        "after"
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn unknown_receivers_and_shadowed_builtins_never_prove_removal() {
    for source in [
        "class Service:\n    def get(self): return 1\ndef make_service(): return Service()\ndef use(): return make_service().get()\n",
        "class Service:\n    def get(self): return 1\ndef use(client): return client.get()\n",
        "def unrelated(): pass\ndef use(client): return client.get()\n",
        "def unrelated(): pass\ndef use(get): return get()\n",
        "def unrelated(): pass\ndef use(): return unknown.clone()\n",
        "def unrelated(): pass\ndef use(): return console()\n",
        "len = callback\ndef use(): return len([])\n",
        "class Service:\n    def get(self): pass\ndef use(): return get()\n",
        "def other():\n    def get(): pass\ndef use(): return get()\n",
        "def use(resource):\n    with resource as len: return len([])\n",
        "def use():\n    try: pass\n    except Exception as len: return len([])\n",
        "class Service:\n    def get(self): pass\n    def use(self, other):\n        self = other\n        return self.get()\n",
        "def use(): return (lambda len: len([]))\n",
        "class Service:\n    def get(self): pass\n    @staticmethod\n    def use(self): return self.get()\n",
        "def use(funcs): return [len([]) for len in funcs]\n",
        "def use(f): return (len := f)([])\n",
    ] {
        let w = Workspace::new();
        w.write("service.py", source);
        w.build();
        let result = w.query(&["query", "remove", "service.py"]);
        assert_eq!(result["safe_to_remove"], false, "{source}\n{result}");
        assert!(
            result["unresolved_references"].as_u64().unwrap() > 0,
            "{source}\n{result}"
        );
    }
    let w = Workspace::new();
    w.write("service.py", "def size(): return len([])\n");
    w.build();
    assert_eq!(
        w.query(&["query", "analyze", "service.py"])["total_unresolved"],
        0
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn stale_analysis_semantics_requires_reindexing() {
    let w = Workspace::new();
    w.write("a.py", "def before(): pass\n");
    w.build();
    {
        let conn = rusqlite::Connection::open(w.db()).unwrap();
        conn.execute(
            "DELETE FROM metadata WHERE key='index_semantics_version'",
            [],
        )
        .unwrap();
    }
    assert!(reader::open(&w.db(), &w.0).is_err());
    w.write("a.py", "def after(): pass\n");
    let result = Server::new(w.0.clone(), w.db())
        .read(|c| reader::inspect(c, "after", false))
        .unwrap();
    assert_eq!(result["node"]["name"], "after");
}

#[cfg(any(
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript"
))]
fn graph_snapshot(w: &Workspace, db: &std::path::Path) -> Value {
    let conn = reader::open(db, &w.0).unwrap();
    let dangling: u64 = conn.query_row("SELECT count(*) FROM edges e LEFT JOIN nodes a ON a.id=e.src_public_id LEFT JOIN nodes b ON b.id=e.dst_public_id WHERE a.id IS NULL OR b.id IS NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(dangling, 0);
    serde_json::json!({
        "edges": reader::rows(&conn, "SELECT src_public_id,dst_public_id,kind,path,line,confidence,occurrence_count FROM edges ORDER BY src_public_id,dst_public_id,kind", &[], 10000).unwrap(),
        "coverage": reader::rows(&conn, "SELECT * FROM resolution_coverage ORDER BY path,line,expression,status,evidence", &[], 10000).unwrap()
    })
}

#[cfg(feature = "lang-rust")]
#[test]
fn delta_relinks_both_endpoints_of_rich_edges() {
    let w = Workspace::new();
    w.write("a_anchor.rs", "fn anchor() {}\n");
    w.write(
        "lib.rs",
        "pub struct Record<T> { value: T }\ninclude!(\"impls.rs\");\n",
    );
    w.write("impls.rs", "trait Save {}\nimpl<T> Save for Record<T> {}\n");
    w.build();
    for source in [
        "\npub struct Record<T> { value: T }\ninclude!(\"impls.rs\");\n",
        "pub struct Renamed<T> { value: T }\ninclude!(\"impls.rs\");\n",
        "pub struct Record<T> { value: T }\ninclude!(\"impls.rs\");\n",
    ] {
        w.write("lib.rs", source);
        writer::delta(&w.0, &w.db(), &[PathBuf::from("lib.rs")]).unwrap();
        let incremental = graph_snapshot(&w, &w.db());
        let cold = w.0.join(".forge/cold.sqlite");
        writer::build(&w.0, &cold, None).unwrap();
        assert_eq!(incremental, graph_snapshot(&w, &cold));
    }
}

#[cfg(feature = "lang-rust")]
#[test]
fn rust_implementation_resolves_the_imported_type_alias() {
    let w = Workspace::new();
    w.write(
        "lib.rs",
        "mod models;\nuse models::Record as Alias;\ntrait Save {}\nimpl<T> Save for Alias<T> {}\n",
    );
    w.write("models.rs", "pub struct Record<T> { pub value: T }\n");
    w.build();
    let conn = reader::open(&w.db(), &w.0).unwrap();
    let endpoints: (String, String) = conn.query_row("SELECT a.name,b.name FROM edges e JOIN nodes a ON a.id=e.src_public_id JOIN nodes b ON b.id=e.dst_public_id WHERE e.kind='implements'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(endpoints, ("Record".into(), "Save".into()));
}

#[cfg(feature = "lang-rust")]
#[test]
fn nested_rust_imports_retain_module_identity_and_external_scope() {
    let w = Workspace::new();
    w.write("src/engine/scanner.rs", "pub fn scan_reusing() {}\n");
    w.write(
        "src/db/writer.rs",
        "use crate::{engine::{scanner::{self, scan_reusing as inspect}}};\nuse std::fmt::Debug;\nuse missing::Widget;\npub fn build() { scanner::scan_reusing(); inspect(); }\n",
    );
    w.build();
    let conn = reader::open(&w.db(), &w.0).unwrap();
    let calls: i64 = conn.query_row(
        "SELECT coalesce(sum(occurrence_count),0) FROM edges WHERE kind='calls' AND path='src/db/writer.rs' AND dst_public_id LIKE '%scanner.rs%'",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(
        calls, 2,
        "both nested imports must reach the indexed scanner"
    );
    for (expression, expected) in [
        ("crate::engine::scanner", "resolved"),
        ("crate::engine::scanner::scan_reusing", "resolved"),
        ("std::fmt::Debug", "external"),
        ("missing::Widget", "external"),
    ] {
        let (status, evidence): (String, String) = conn.query_row(
            "SELECT status,evidence FROM resolution_coverage WHERE path='src/db/writer.rs' AND expression=?1",
            [expression],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(status, expected, "{expression}");
        if expression == "missing::Widget" {
            assert!(evidence.contains("no indexed provider"), "{evidence}");
        }
    }
    let overview = reader::overview(&conn).unwrap();
    assert_eq!(overview["counts"][0]["unresolved"], 0);
    assert_eq!(overview["counts"][0]["external_imports"], 2);
    let analysis = reader::analyze(&conn, "src/db/writer.rs").unwrap();
    assert_eq!(analysis["total_unresolved"], 0);
    assert_eq!(analysis["total_external_imports"], 2);
    let options = contextunity_forge_mcp::core::response::QueryOptions {
        limit: 30,
        offset: 0,
        detail: contextunity_forge_mcp::core::response::Detail::Compact,
        generation: None,
    };
    let paged = reader::analyze_paged(&conn, "src/db/writer.rs", None, &options).unwrap();
    assert_eq!(paged["external_imports"]["total"], 2);
    assert_eq!(paged["resolution"]["total"], 0);
    drop(conn);
    w.write("src/engine/scanner.rs", "\npub fn scan_reusing() {}\n");
    writer::delta(&w.0, &w.db(), &[PathBuf::from("src/engine/scanner.rs")]).unwrap();
    let incremental = graph_snapshot(&w, &w.db());
    let cold = w.0.join(".forge/cold.sqlite");
    writer::build(&w.0, &cold, None).unwrap();
    assert_eq!(incremental, graph_snapshot(&w, &cold));
}

#[cfg(feature = "lang-typescript")]
#[test]
fn explicit_node_builtin_import_is_separate_from_missing_package() {
    let w = Workspace::new();
    w.write(
        "src/main.ts",
        "import { readFileSync } from 'node:fs';\nimport { unknown } from 'missing-package';\nexport function run() { return readFileSync('x'); }\n",
    );
    w.build();
    let conn = reader::open(&w.db(), &w.0).unwrap();
    let statuses = reader::rows(
        &conn,
        "SELECT expression,status,evidence FROM resolution_coverage WHERE path='src/main.ts' AND line IN (1,2) ORDER BY expression",
        &[],
        10,
    ).unwrap();
    assert_eq!(
        statuses,
        vec![
            serde_json::json!({"expression":"readFileSync","status":"external","evidence":"standard library; no indexed provider for node:fs"}),
            serde_json::json!({"expression":"unknown","status":"external","evidence":"external dependency; no indexed provider for missing-package"}),
        ]
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn explicit_python_stdlib_and_external_dependency_is_separate_from_missing_package() {
    let w = Workspace::new();
    w.write(
        "src/main.py",
        "import json\nfrom typing import Any\nimport pytest\nimport nonexistent_xyz_package\n\ndef run():\n    raise ValueError('error')\n",
    );
    w.build();
    let conn = reader::open(&w.db(), &w.0).unwrap();
    let statuses = reader::rows(
        &conn,
        "SELECT expression,status,evidence FROM resolution_coverage WHERE path='src/main.py' ORDER BY line,expression",
        &[],
        10,
    ).unwrap();
    assert_eq!(
        statuses,
        vec![
            serde_json::json!({"expression":"json","status":"external","evidence":"standard library; no indexed provider for json"}),
            serde_json::json!({"expression":"Any","status":"external","evidence":"standard library; no indexed provider for typing"}),
            serde_json::json!({"expression":"pytest","status":"external","evidence":"external dependency; no indexed provider for pytest"}),
            serde_json::json!({"expression":"nonexistent_xyz_package","status":"external","evidence":"external dependency; no indexed provider for nonexistent_xyz_package"}),
            serde_json::json!({"expression":"ValueError","status":"resolved","evidence":"standard library or built-in callee: ValueError"}),
        ]
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn decorators_have_consistent_dependency_direction() {
    let w = Workspace::new();
    w.write(
        "service.py",
        "def deco(f): return f\n@deco\ndef target(): pass\n",
    );
    w.write(
        "test_service.py",
        "from service import target\ndef test_target(): target()\n",
    );
    w.build();
    let impact = w.query(&["query", "impact", "deco"]);
    for name in ["target", "test_target"] {
        assert!(
            impact["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n["name"] == name),
            "{impact}"
        );
    }
    let tests = w.query(&["query", "tests", "deco"]);
    assert!(tests["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["name"] == "test_target"));
    assert_eq!(
        w.query(&["query", "remove", "deco"])["safe_to_remove"],
        false
    );
}

#[cfg(feature = "lang-python")]
#[test]
fn decorators_resolve_in_the_definition_scope() {
    let w = Workspace::new();
    w.write("service.py", "class Service:\n    def deco(f): return f\n    @deco\n    def target(self, deco=None): pass\n    def use(self): return deco()\n");
    w.build();
    let conn = reader::open(&w.db(), &w.0).unwrap();
    let edges = reader::rows(&conn, "SELECT a.name,b.name target FROM edges e JOIN nodes a ON a.id=e.src_public_id JOIN nodes b ON b.id=e.dst_public_id WHERE e.kind='decorates'", &[], 10).unwrap();
    assert_eq!(
        edges,
        vec![serde_json::json!({"name":"deco","target":"target"})]
    );
    let status: String = conn
        .query_row(
            "SELECT status FROM resolution_coverage WHERE expression='deco' AND line=5",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "unresolved");
    let impact = contextunity_forge_mcp::db::traversal::traverse(
        &conn,
        "service.Service.deco",
        1,
        true,
        100,
    )
    .unwrap();
    assert!(impact["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["name"] == "target"));
}

#[cfg(feature = "lang-python")]
#[test]
fn analyze_uses_literal_paths_and_preserves_cross_file_cycles() {
    let w = Workspace::new();
    w.write("pkg_one/a.py", "def first(): missing()\n");
    w.write("pkgXone/a.py", "def second(): missing()\n");
    w.write("PKG_ONE/a.py", "def third(): missing()\n");
    w.write("a:b.py", "def recursive(): recursive()\n");
    w.write("left.py", "from right import right\ndef left(): right()\n");
    w.write("right.py", "from left import left\ndef right(): left()\n");
    w.build();
    for path in ["pkg_one", "pkg_one/"] {
        let result = w.query(&["query", "analyze", path]);
        assert_eq!(result["total_unresolved"], 1, "{result}");
    }
    assert!(!w.query(&["query", "analyze", "a:b.py"])["cycles"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!w.query(&["query", "analyze", "left.py"])["cycles"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[cfg(feature = "lang-python")]
#[test]
fn scoped_analysis_does_not_serialize_the_entire_graph() {
    let w = Workspace::new();
    w.write("tiny.py", "def tiny(): pass\n");
    let suffix = "x".repeat(200);
    let large: String = (0..3500)
        .map(|i| format!("def f{i}_{suffix}(): f{}_{suffix}()\n", (i + 1) % 3500))
        .collect();
    w.write("large.py", &large);
    w.build();
    let result = w.query(&["query", "analyze", "tiny.py"]);
    assert_eq!(result["cycles"], serde_json::json!([]));
}

#[cfg(feature = "lang-typescript")]
#[test]
fn routes_bind_inline_callbacks_and_every_middleware() {
    let w = Workspace::new();
    w.write("routes.ts", "function auth() {}\nfunction view() {}\napp.get('/inline', (() => view()));\napp.get('/chain', auth, view);\napp.post('/two', () => auth(), () => view());\nconst routes = [{path: '/object', component: (() => view())}];\n");
    w.build();
    let conn = reader::open(&w.db(), &w.0).unwrap();
    for (path, expected) in [("/inline", 1), ("/chain", 2), ("/two", 2), ("/object", 1)] {
        let count: u64 = conn.query_row("SELECT count(*) FROM edges e JOIN nodes r ON r.id=e.src_public_id JOIN nodes h ON h.id=e.dst_public_id WHERE e.kind='handles' AND json_extract(r.details,'$.route_path')=?1 AND h.kind='function'", [path], |r| r.get(0)).unwrap();
        assert_eq!(count, expected, "{path}");
    }
    drop(conn);
    w.write(
        "routes.ts",
        "\nfunction view() {}\napp.get('/inline', () => view());\n",
    );
    writer::delta(&w.0, &w.db(), &[PathBuf::from("routes.ts")]).unwrap();
    let incremental = graph_snapshot(&w, &w.db());
    let cold = w.0.join(".forge/cold.sqlite");
    writer::build(&w.0, &cold, None).unwrap();
    assert_eq!(incremental, graph_snapshot(&w, &cold));
}

#[cfg(feature = "lang-python")]
#[test]
fn local_import_aliases_keep_their_lexical_owner() {
    let w = Workspace::new();
    w.write("first.py", "def target(): pass\n");
    w.write("second.py", "def target(): pass\n");
    w.write("main.py", "def one():\n    from first import target as invoke\n    invoke()\ndef two():\n    from second import target as invoke\n    invoke()\n");
    w.build();
    let conn = reader::open(&w.db(), &w.0).unwrap();
    let targets = reader::rows(&conn, "SELECT a.name,b.path FROM edges e JOIN nodes a ON a.id=e.src_public_id JOIN nodes b ON b.id=e.dst_public_id WHERE e.kind='calls' ORDER BY a.name", &[], 10).unwrap();
    assert_eq!(
        targets,
        vec![
            serde_json::json!({"name":"one","path":"first.py"}),
            serde_json::json!({"name":"two","path":"second.py"})
        ]
    );
    drop(conn);
    assert_eq!(
        w.query(&["query", "remove", "first.target"])["safe_to_remove"],
        false
    );
    w.write("first.py", "\ndef target(): pass\n");
    writer::delta(&w.0, &w.db(), &[PathBuf::from("first.py")]).unwrap();
    let incremental = graph_snapshot(&w, &w.db());
    let cold = w.0.join(".forge/cold.sqlite");
    writer::build(&w.0, &cold, None).unwrap();
    assert_eq!(incremental, graph_snapshot(&w, &cold));
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_script_blocks_use_absolute_coordinates() {
    let w = Workspace::new();
    w.write("Component.vue", "<script>app.get('/a', () => {});</script><script setup>app.get('/b', () => {});</script>\n");
    w.build();
    let conn = reader::open(&w.db(), &w.0).unwrap();
    let handlers = reader::rows(&conn, "SELECT a.name,b.id FROM edges e JOIN nodes a ON a.id=e.src_public_id JOIN nodes b ON b.id=e.dst_public_id WHERE e.kind='handles' ORDER BY a.name", &[], 10).unwrap();
    assert_eq!(handlers.len(), 2);
    assert_eq!(handlers[0]["name"], "GET /a");
    assert_eq!(handlers[1]["name"], "GET /b");
    assert_ne!(handlers[0]["id"], handlers[1]["id"]);
}

#[cfg(feature = "lang-proto")]
#[test]
fn proto_rpc_type_references_participate_in_dependency_proofs() {
    let w = Workspace::new();
    w.write("a.proto", "syntax = \"proto3\";\nmessage Request {}\nmessage Reply {}\nservice Api { rpc Run(Request) returns (Reply); }\n");
    w.build();
    let result = w.query(&["query", "remove", "class:Request"]);
    assert_eq!(result["safe_to_remove"], false);
    assert_eq!(result["incoming_dependencies"][0]["kind"], "references");
    let impact = w.query(&["query", "impact", "class:Request"]);
    assert!(impact["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["name"] == "Run"));
}
