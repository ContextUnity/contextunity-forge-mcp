#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::{
    core::response::{CoverageOptions, Detail, QueryOptions, ResponsePolicy, SourceOptions},
    db::{reader, symbols, traversal, writer},
};
use rusqlite::{
    hooks::{AuthAction, AuthContext, Authorization},
    Connection,
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
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
        let path = std::env::temp_dir().join(format!(
            "forge_query_context_{}_{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, contents: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    fn db(&self) -> PathBuf {
        self.0.join(".forge/code-map.sqlite")
    }
    fn build(&self) -> Connection {
        writer::build(&self.0, &self.db(), None).unwrap();
        reader::open(&self.db(), &self.0).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn options(limit: usize) -> QueryOptions {
    QueryOptions::resolve(&ResponsePolicy::default(), Some(limit), 0, None, None).unwrap()
}

fn continue_page(options: &mut QueryOptions, page: &Value) -> bool {
    assert_eq!(page["limit"], options.limit);
    assert_eq!(page["offset"], options.offset);
    if !page["has_more"].as_bool().unwrap() {
        assert!(page["next_offset"].is_null());
        return false;
    }
    let emitted = page["items"].as_array().unwrap().len();
    assert!(emitted > 0);
    assert_eq!(page["next_offset"], options.offset + emitted);
    options.offset += emitted;
    options.generation = Some(page["generation"].as_str().unwrap().to_owned());
    true
}

fn persisted_state(conn: &Connection) -> Value {
    let schema = reader::rows(
        conn,
        "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name,tbl_name",
        &[],
        100_000,
    )
    .unwrap();
    let table_names = {
        let mut statement = conn
            .prepare(
                "SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .unwrap();
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    let tables = table_names
        .into_iter()
        .map(|name| {
            let quoted_name = name.replace('"', "\"\"");
            let mut rows = reader::rows(
                conn,
                &format!("SELECT * FROM \"{quoted_name}\""),
                &[],
                100_000,
            )
            .unwrap();
            rows.sort_by_key(|row| row.to_string());
            (name, rows)
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "schema": schema,
        "tables": tables,
        "pragmas": {
            "application_id": conn.query_row("PRAGMA application_id", [], |row| row.get::<_, i64>(0)).unwrap(),
            "freelist_count": conn.query_row("PRAGMA freelist_count", [], |row| row.get::<_, i64>(0)).unwrap(),
            "journal_mode": conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0)).unwrap(),
            "page_count": conn.query_row("PRAGMA page_count", [], |row| row.get::<_, i64>(0)).unwrap(),
            "schema_version": conn.query_row("PRAGMA schema_version", [], |row| row.get::<_, i64>(0)).unwrap(),
            "user_version": conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0)).unwrap(),
        },
    })
}

#[path = "query_context/diagnostics_and_source.rs"]
mod diagnostics_and_source;
#[path = "query_context/ranking_and_coverage.rs"]
mod ranking_and_coverage;
#[path = "query_context/search_and_paging.rs"]
mod search_and_paging;
#[path = "query_context/symbol_context.rs"]
mod symbol_context;
