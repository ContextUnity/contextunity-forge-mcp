#![cfg(all(feature = "lang-python", feature = "lang-rust"))]

use crate::common::Workspace;
use contextunity_forge_mcp::{
    core::response::{QueryOptions, ResponsePolicy},
    db::symbols,
    engine::ast,
};
use serde_json::Value;

fn stored_node(conn: &rusqlite::Connection, path: &str, name: &str) -> (String, Value) {
    let (kind, details): (String, String) = conn
        .query_row(
            "SELECT n.kind, n.details FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.name=?2",
            [path, name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    (kind, serde_json::from_str(&details).unwrap())
}

#[test]
fn rust_and_python_methods_are_classified_persisted_and_searchable() {
    let rust = r#"
struct Record;
impl Marker for Record {
    fn persist_record(&self) {}
    fn create_record() {}
}
impl Record {
    fn direct_record(&mut self) {}
    fn static_record() {}
    fn outer_record(&self) {
        fn nested_record() {}
    }
}
fn module_record(self) {}
"#;
    let python = r#"
class PythonRecord:
    def persist_python(self): pass

    @staticmethod
    def create_python(value): pass

    def implicit_static_python(value): pass

    @classmethod
    def class_python(cls): pass

    def outer_python(self):
        def nested_python(): pass
        return nested_python()

def module_python(self): pass
"#;

    let typescript = r#"
class TsRecord {
    persist_ts(): void {}
    static create_ts(): void {}
}
function module_ts(): void {}
"#;

    for (path, language, source) in [
        ("record.rs", "rust", rust),
        ("record.py", "python", python),
        ("record.ts", "typescript", typescript),
    ] {
        let facts = ast::extract(path, language, source).unwrap();
        assert!(facts.errors.is_empty(), "{path}: {:?}", facts.errors);
    }

    let workspace = Workspace::new();
    workspace.write("record.rs", rust);
    workspace.write("record.py", python);
    workspace.write("record.ts", typescript);
    workspace.build();
    let conn = workspace.open();

    for (name, receiver_type, is_static) in [
        ("persist_record", "Record", false),
        ("create_record", "Record", true),
        ("direct_record", "Record", false),
        ("static_record", "Record", true),
        ("persist_python", "PythonRecord", false),
        ("create_python", "PythonRecord", true),
        ("implicit_static_python", "PythonRecord", true),
        ("class_python", "PythonRecord", false),
        ("persist_ts", "TsRecord", false),
        ("create_ts", "TsRecord", true),
    ] {
        let path = if name.contains("_python") {
            "record.py"
        } else if name.contains("_ts") {
            "record.ts"
        } else {
            "record.rs"
        };
        let (kind, details) = stored_node(&conn, path, name);
        assert_eq!(kind, "method", "{path}: {name}");
        assert_eq!(details["receiver_type"], receiver_type, "{path}: {name}");
        assert_eq!(details["is_method"], true, "{path}: {name}");
        assert_eq!(details["is_static"], is_static, "{path}: {name}");
    }

    for (path, name) in [
        ("record.rs", "nested_record"),
        ("record.rs", "module_record"),
        ("record.py", "nested_python"),
        ("record.py", "module_python"),
        ("record.ts", "module_ts"),
    ] {
        let (kind, details) = stored_node(&conn, path, name);
        assert_eq!(kind, "function", "{path}: {name}");
        assert_ne!(details["is_method"], true, "{path}: {name}");
    }

    for pattern in ["persist_record*", "persist_python*", "persist_ts*"] {
        let page =
            QueryOptions::resolve(&ResponsePolicy::default(), Some(10), 0, None, None).unwrap();
        let found = symbols::search_with_options(
            &conn,
            pattern,
            &symbols::SearchOptions {
                kind: Some("method"),
                path: None,
                include_docs: false,
                exact: false,
                page: &page,
            },
        )
        .unwrap();
        assert!(
            found["nodes"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|node| {
                    node["kind"] == "method"
                        && node["name"]
                            .as_str()
                            .is_some_and(|name| name == pattern.trim_end_matches('*'))
                }),
            "method search missed {pattern}: {found}"
        );
    }
}
