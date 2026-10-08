use crate::common::Workspace;
use contextunity_forge_mcp::db::{reader, writer};

#[cfg(feature = "lang-python")]
#[test]
fn hybrid_search_ranks_connected_exact_symbols_before_path_order() {
    use contextunity_forge_mcp::core::response::{Detail, QueryOptions, ResponsePolicy};
    use contextunity_forge_mcp::db::symbols;

    let ws = Workspace::new();
    ws.write(
        "a.py",
        "def process():\n    return 1\ndef process_work():\n    return 1\n",
    );
    ws.write(
        "z.py",
        "def process():\n    return 1\ndef process_work():\n    return 1\n",
    );
    ws.write(
        "caller.py",
        "from z import process, process_work\n\ndef run():\n    process_work()\n    return process()\n",
    );
    let db_path = ws.root().join(".forge/code-map.sqlite");
    writer::build(ws.root(), &db_path, None).unwrap();
    let conn = reader::open(&db_path, ws.root()).unwrap();
    let options = QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(10),
        0,
        Some(Detail::Full),
        None,
    )
    .unwrap();
    let result = symbols::search_with_options(
        &conn,
        "process",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &options,
        },
    )
    .unwrap();
    let nodes = result["nodes"]["items"].as_array().unwrap();
    assert_eq!(result["nodes"]["total"], 4);
    assert_eq!(
        nodes[0]["path"], "z.py",
        "inbound graph connectivity breaks equivalent exact-name scores"
    );
    assert_eq!(nodes[1]["path"], "a.py");
    assert_eq!(nodes[0]["name"], "process");
    assert_eq!(nodes[1]["name"], "process");
    assert_eq!(nodes[2]["name"], "process_work");
    assert_eq!(nodes[2]["path"], "z.py");
    assert_eq!(nodes[3]["name"], "process_work");
    assert_eq!(nodes[3]["path"], "a.py");
    let prefix = symbols::search_with_options(
        &conn,
        "process_*",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &options,
        },
    )
    .unwrap();
    let nodes = prefix["nodes"]["items"].as_array().unwrap();
    assert_eq!(prefix["nodes"]["total"], 2);
    assert_eq!(nodes[0]["path"], "z.py", "connected prefix matches receive the same graph ranking regardless of identifier punctuation");
    assert_eq!(nodes[1]["path"], "a.py");
    drop(conn);

    let catalog: String = (0..501)
        .map(|index| format!("def rankterm_{index:03}():\n    return 1\n"))
        .collect();
    ws.write("catalog.py", &catalog);
    ws.write("exact_a.py", "def rankterm():\n    return 1\n");
    ws.write("exact_z.py", "def rankterm():\n    return 1\n");
    ws.write("rank_caller.py", "from exact_z import rankterm\nfrom catalog import rankterm_500\ndef run_ranked():\n    rankterm_500()\n    return rankterm()\n");
    writer::build(ws.root(), &db_path, None).unwrap();
    let conn = reader::open(&db_path, ws.root()).unwrap();
    let mut page_options = QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(100),
        0,
        Some(Detail::Full),
        None,
    )
    .unwrap();
    let mut all = Vec::new();
    loop {
        let result = symbols::search_with_options(
            &conn,
            "rankterm",
            &symbols::SearchOptions {
                kind: Some("function"),
                path: None,
                include_docs: false,
                exact: false,
                page: &page_options,
            },
        )
        .unwrap();
        let page = &result["nodes"];
        assert_eq!(page["total"], 503);
        all.extend(page["items"].as_array().unwrap().iter().cloned());
        if page["has_more"] == false {
            break;
        }
        page_options.offset = page["next_offset"].as_u64().unwrap() as usize;
        page_options.generation = Some(page["generation"].as_str().unwrap().to_owned());
    }
    assert_eq!(all.len(), 503);
    let ids: std::collections::BTreeSet<_> = all
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 503);
    assert_eq!(all[0]["path"], "exact_z.py");
    assert_eq!(all[1]["path"], "exact_a.py");
    assert!(all[2]["name"].as_str().unwrap().starts_with("rankterm_"));
    page_options.offset = 501;
    page_options.limit = 2;
    let tail = symbols::search_with_options(
        &conn,
        "rankterm",
        &symbols::SearchOptions {
            kind: Some("function"),
            path: None,
            include_docs: false,
            exact: false,
            page: &page_options,
        },
    )
    .unwrap();
    assert_eq!(tail["nodes"]["items"], serde_json::json!(&all[501..]));
    assert_eq!(tail["nodes"]["total"], 503);
    assert_eq!(tail["nodes"]["has_more"], false);
}
