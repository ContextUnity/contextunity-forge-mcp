use crate::common::Workspace;
use contextunity_forge_mcp::{
    core::response::{QueryOptions, ResponsePolicy},
    db::{reader, writer},
};

#[test]
fn database_roundtrip_build_and_query() {
    let ws = Workspace::new();
    ws.write(
        "src/main.py",
        r#"
class Service:
    def execute(self):
        return helper()

def helper():
    return 42
"#,
    );
    ws.write(
        "README.md",
        r#"# Workspace Documentation
## Standards
All handlers must be asynchronous and tested.
See `ServiceClient`.
"#,
    );

    let db_path = ws.root().join("code_map.sqlite");
    let report = writer::build(ws.root(), &db_path, None).expect("cold build failed");
    assert!(report["nodes"].as_i64().unwrap_or(0) > 0);
    assert!(report["files"].as_i64().unwrap_or(0) >= 2);
    assert_eq!(
        report["node_row_count"].as_u64(),
        report["nodes"].as_u64(),
        "cold build reports the number of node rows inserted"
    );
    assert!(
        report["node_row_insert_ms"].as_f64().is_some(),
        "cold build reports node-row batch insertion time"
    );
    assert!(report["rows_ms"].as_f64().is_some());
    assert!(report["persist_ms"].as_f64().is_some());

    let conn = reader::open(&db_path, ws.root()).expect("reader open failed");
    let page = QueryOptions::resolve(&ResponsePolicy::default(), Some(10), 0, None, None).unwrap();
    let overview = reader::overview_with_options(
        &conn,
        &reader::OverviewOptions {
            aspects: None,
            page: &page,
        },
    )
    .expect("overview failed");
    assert!(overview["counts"]["files"].as_i64().unwrap_or(0) >= 2);

    let docs = reader::search_docs_with_options(
        &conn,
        "Standards",
        &reader::DocSearchOptions {
            doc_type: None,
            component: None,
            include_excerpt: false,
            page: &page,
        },
    )
    .expect("search docs failed");
    assert!(!docs["sections"]["items"].as_array().unwrap().is_empty());

    let sections: Vec<(String, String, String, i64)> = conn
        .prepare(
            "SELECT section_title, content, referenced_symbols, is_invariant FROM doc_sections WHERE path='README.md' ORDER BY rowid",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(sections.len(), 2, "{sections:?}");
    assert_eq!(sections[0].0, "Workspace Documentation");
    assert_eq!(sections[1].0, "Standards");
    assert!(sections[1].1.contains("asynchronous"), "{}", sections[1].1);
    assert_eq!(sections[1].2, "[\"ServiceClient\"]");
    assert_eq!(sections[0].3, 0);
    assert_eq!(sections[1].3, 0);
}
