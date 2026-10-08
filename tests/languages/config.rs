use contextunity_forge_mcp::{
    core::response::{QueryOptions, ResponsePolicy, SourceOptions},
    db::{reader, symbols},
    engine::{ast, scanner},
};
use rusqlite::Connection;
use super::support::Workspace;
use std::collections::BTreeSet;

fn rows(conn: &Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn assert_snippet(workspace: &Workspace, conn: &Connection, selector: &str, expected: &str) {
    let policy = ResponsePolicy::default();
    let options = QueryOptions::resolve(&policy, None, 0, None, None).unwrap();
    let source = SourceOptions::resolve(&policy, Some(true), Some(0), None, 0).unwrap();
    let snippet = symbols::snippet_paged(conn, &workspace.0, selector, &source, &options).unwrap();
    assert!(
        snippet["source"].as_str().unwrap().contains(expected),
        "{snippet}"
    );
}

#[test]
fn yaml_indexes_nested_quoted_flow_and_sequence_keys_with_unique_ids() {
    let w = Workspace::new();
    let source = "service:\n  port: 8080\n  \"literal.dot\": yes\nflow: {left: {port: 1}, right: {port: 2}}\nitems:\n  - name: first\n  - name: second\n";
    w.write("settings.yml", source);
    w.build();
    let conn = reader::open(&w.db(), &w.0).unwrap();
    let names = rows(
        &conn,
        "SELECT qualname FROM nodes WHERE kind='variable' ORDER BY qualname",
    );
    for expected in [
        "settings.service",
        "settings.service.port",
        "settings.service.\"literal.dot\"",
        "settings.flow.left.port",
        "settings.flow.right.port",
        "settings.items.name",
    ] {
        assert!(
            names.iter().any(|name| name == expected),
            "missing {expected}: {names:?}"
        );
    }
    let ids = rows(&conn, "SELECT id FROM nodes");
    assert_eq!(ids.len(), ids.iter().collect::<BTreeSet<_>>().len());
    assert!(rows(&conn, "SELECT message FROM errors").is_empty());
    let node = reader::select(&conn, "settings.yml:2").unwrap();
    assert_eq!(node["name"], "port");
    assert_snippet(&w, &conn, "settings.yml:2", "port: 8080");
    drop(conn);
    w.write("settings.yml", &source.replace("port: 8080", "timeout: 15"));
    w.cold_parity("settings.yml");
}

#[test]
fn toml_indexes_tables_array_tables_dotted_and_inline_keys() {
    let w = Workspace::new();
    let source = "title = \"app\"\n[server]\nport = 8080\n\"literal.dot\" = true\nlimits.cpu = 2\ninline = { left = { value = 1 }, right = { value = 2 } }\n[[workers]]\nname = \"first\"\n[[workers]]\nname = \"second\"\n";
    w.write("settings.toml", source);
    w.build();
    let conn = reader::open(&w.db(), &w.0).unwrap();
    let names = rows(&conn, "SELECT qualname FROM nodes ORDER BY qualname");
    for expected in [
        "settings.title",
        "settings.server",
        "settings.server.port",
        "settings.server.\"literal.dot\"",
        "settings.server.limits.cpu",
        "settings.server.inline.left.value",
        "settings.workers.name",
    ] {
        assert!(
            names.iter().any(|name| name == expected),
            "missing {expected}: {names:?}"
        );
    }
    assert_eq!(
        rows(
            &conn,
            "SELECT id FROM nodes WHERE kind='namespace' AND name='workers'"
        )
        .len(),
        2
    );
    let ids = rows(&conn, "SELECT id FROM nodes");
    assert_eq!(ids.len(), ids.iter().collect::<BTreeSet<_>>().len());
    assert!(rows(&conn, "SELECT message FROM errors").is_empty());
    assert_snippet(&w, &conn, "settings.toml:3", "port = 8080");
    drop(conn);
    w.write(
        "settings.toml",
        &source.replace("port = 8080", "timeout = 15"),
    );
    w.cold_parity("settings.toml");
}

#[test]
fn config_ast_search_finds_inner_pairs_and_key_captures() {
    let w = Workspace::new();
    for (path, language, source, patterns) in [
        (
            "settings.yml",
            "yaml",
            "service:\n  port: 8080\n  host: local\n",
            ["port: 8080", "$KEY: 8080"],
        ),
        (
            "settings.toml",
            "toml",
            "[server]\nport = 8080\nhost = 'local'\n",
            ["port = 8080", "$KEY = 8080"],
        ),
    ] {
        w.write(path, source);
        for pattern in patterns {
            assert!(
                !ast::search(source, path, language, pattern, 20)
                    .unwrap()
                    .is_empty(),
                "{language}: {pattern}"
            );
        }
    }
    assert!(ast::search("key: 1\n", "settings.yaml", "yaml", "key: [", 20).is_err());
    assert!(ast::search("key = 1\n", "settings.toml", "toml", "key = [", 20).is_err());
}

#[test]
fn config_syntax_errors_are_recorded_and_repaired_by_delta() {
    let w = Workspace::new();
    w.write("broken.yaml", "key: [one, two\n");
    w.write("broken.toml", "[server\nport =\n");
    w.build();
    let conn = reader::open(&w.db(), &w.0).unwrap();
    assert!(!rows(&conn, "SELECT message FROM errors WHERE path='broken.yaml'").is_empty());
    assert!(!rows(&conn, "SELECT message FROM errors WHERE path='broken.toml'").is_empty());
    drop(conn);
    w.write("broken.yaml", "key: [one, two]\n");
    w.cold_parity("broken.yaml");
    w.write("broken.toml", "[server]\nport = 80\n");
    w.cold_parity("broken.toml");
    let conn = reader::open(&w.db(), &w.0).unwrap();
    assert!(rows(&conn, "SELECT message FROM errors").is_empty());
    let adapter = scanner::load_adapter(&w.0, None).unwrap();
    let scan = scanner::scan_with_adapter(&w.0, &adapter).unwrap();
    assert!(scan.entries.iter().any(|entry| entry.language == "yaml"));
    assert!(scan.entries.iter().any(|entry| entry.language == "toml"));
}
