use super::*;
use crate::languages::harness::ExtractorHarness;

#[test]
fn htmx_declarations_are_searchable_without_inventing_handler_edges() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    let source = concat!(
        "<div hx-target='#result' hx-swap='outerHTML'>\n",
        "<button hx-get='/orders?state=open' hx-trigger='click' hx-include='#filter' hx-vals='{\"token\":\"[REDACTED:API key param]\"}'>Load</button>\n",
        "<button data-hx-post='/orders' data-hx-target='#result'>Save</button>\n",
        "<button hx-delete='https://example.invalid/orders'>Remote</button>\n",
        "<button hx-patch='{{ route }}' hx-vals='js:{token: currentToken()}'>Dynamic</button>\n",
        "<button hx-put=''>Current page</button>\n",
        "</div>\n",
    );
    workspace.write("page.html", source);
    let facts = ast::extract("page.html", "html", source).unwrap();
    assert_eq!(facts.references.len(), 1);
    assert_eq!(facts.references[0].kind, "references");
    assert_eq!(facts.references[0].expression, "template.variable.route");
    assert_eq!(facts.references[0].source, "module:page.html");
    assert!(!facts.edges.iter().any(|edge| edge.kind == "handles"));
    let requests: Vec<_> = facts
        .nodes
        .iter()
        .filter(|node| node.kind == "htmx_request")
        .collect();
    assert_eq!(requests.len(), 5);
    let get = requests
        .iter()
        .find(|node| node.name == "GET /orders?state=open")
        .unwrap();
    assert_eq!(get.line, 2);
    assert_eq!(get.details["url_status"], "local_unverified");
    assert_eq!(get.details["trigger"], "click");
    assert_eq!(get.details["include"], "#filter");
    assert_eq!(get.details["vals_key_count"], 1);
    assert!(requests.iter().any(|node| node.name == "POST /orders"));
    assert!(requests
        .iter()
        .any(|node| node.name == "DELETE https://example.invalid/orders"
            && node.details["url_status"] == "remote"));
    assert!(requests.iter().any(|node| node.name == "PATCH {{ route }}"
        && node.details["url_status"] == "dynamic_or_nonlocal"
        && node.details["vals_status"] == "dynamic"));
    assert!(requests
        .iter()
        .any(|node| node.name == "PUT " && node.details["url_status"] == "current_page"));
    assert_eq!(
        facts
            .nodes
            .iter()
            .filter(|node| node.kind == "htmx_context")
            .count(),
        1
    );
    assert!(!serde_json::to_string(&facts).unwrap().contains("secret"));

    writer::build(workspace.root(), &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), workspace.root()).unwrap();
    let options = QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(10),
        0,
        Some(Detail::Full),
        None,
    )
    .unwrap();
    let inspected = reader::inspect_paged(&conn, &get.id, false, &options).unwrap();
    assert_eq!(inspected["node"]["name"], "GET /orders?state=open");
    assert_eq!(inspected["node"]["details"]["vals_key_count"], 1);
    let inspect_bytes = serde_json::to_vec(&inspected).unwrap().len();
    assert!(inspect_bytes < 8192, "{inspect_bytes} bytes");
    println!("HTMX request full inspect={inspect_bytes} bytes");
    let searched = symbols::search_with_options(
        &conn,
        "orders",
        &symbols::SearchOptions {
            kind: Some("htmx_request"),
            path: None,
            include_docs: false,
            exact: false,
            page: &options,
        },
    )
    .unwrap();
    assert_eq!(searched["nodes"]["total"], 3);
    assert_eq!(searched["nodes"]["items"][0]["path"], "page.html");
    let edges: i64 = conn
        .query_row(
            "SELECT count(*) FROM edges WHERE (SELECT id FROM nodes WHERE node_hash=src_hash)='module:page.html' AND kind='imports'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(edges, 0);
}

#[test]
fn htmx_requests_remain_complete_after_delta_and_cold_build() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write("page.html", "<button hx-get='/before'>Before</button>");
    writer::build(workspace.root(), &workspace.db(), None).unwrap();
    let mut updated = String::new();
    for index in 0..70 {
        updated.push_str(&format!("<button hx-get='/route-{index}'>Go</button>\n"));
    }
    updated.push_str(&format!(
        "<button hx-post='{}'>Large</button>",
        "x".repeat(300)
    ));
    workspace.write("page.html", &updated);
    writer::delta(
        workspace.root(),
        &workspace.db(),
        &[std::path::PathBuf::from("page.html")],
    )
    .unwrap();
    let delta = reader::open(&workspace.db(), workspace.root()).unwrap();
    let cold = workspace.root().join("cold.sqlite");
    writer::build(workspace.root(), &cold, None).unwrap();
    let rebuilt = reader::open(&cold, workspace.root()).unwrap();
    let rows = |conn: &rusqlite::Connection| {
        conn.prepare(
            "SELECT id,name,line,details FROM nodes WHERE kind='htmx_request' ORDER BY line,id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
    };
    let requests = rows(&delta);
    assert_eq!(requests, rows(&rebuilt));
    assert_eq!(requests.len(), 71);
    assert_eq!(requests.first().unwrap().1, "GET /route-0");
    assert!(requests
        .iter()
        .any(|(_, name, line, _)| name == "GET /route-69" && *line == 70));
    let options = QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(10),
        0,
        Some(Detail::Compact),
        None,
    )
    .unwrap();
    let first = symbols::search_with_options(
        &delta,
        "route",
        &symbols::SearchOptions {
            kind: Some("htmx_request"),
            path: None,
            include_docs: false,
            exact: false,
            page: &options,
        },
    )
    .unwrap();
    assert_eq!(first["nodes"]["total"], 70);
    assert_eq!(first["nodes"]["items"].as_array().unwrap().len(), 10);
    assert_eq!(first["nodes"]["next_offset"], 10);
    let generation = first["nodes"]["generation"].as_str().unwrap().to_owned();
    let second_options = QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(10),
        10,
        Some(Detail::Compact),
        Some(generation),
    )
    .unwrap();
    let second = symbols::search_with_options(
        &delta,
        "route",
        &symbols::SearchOptions {
            kind: Some("htmx_request"),
            path: None,
            include_docs: false,
            exact: false,
            page: &second_options,
        },
    )
    .unwrap();
    assert_eq!(second["nodes"]["total"], 70);
    assert_eq!(second["nodes"]["items"].as_array().unwrap().len(), 10);
    assert_ne!(
        first["nodes"]["items"][0]["id"],
        second["nodes"]["items"][0]["id"]
    );
    let full_options = QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(10),
        0,
        Some(Detail::Full),
        None,
    )
    .unwrap();
    let inspected = reader::inspect_paged(&delta, &requests[69].0, false, &full_options).unwrap();
    let inspect_bytes = serde_json::to_vec(&inspected).unwrap().len();
    assert!(inspect_bytes < 64 * 1024, "{inspect_bytes} bytes");
    println!("HTMX 71 requests retained; page 10/70; full inspect={inspect_bytes} bytes");
}

#[test]
fn htmx_large_attribute_sets_use_bounded_values_per_request() {
    let value = "x".repeat(240);
    let source = (0..64)
        .map(|index| format!("<button hx-get='/route-{index}' hx-trigger='{value}' hx-target='{value}' hx-swap='{value}' hx-include='{value}'>Go</button>\n"))
        .collect::<String>();
    let facts = ast::extract("page.html", "html", &source).unwrap();
    let requests: Vec<_> = facts
        .nodes
        .iter()
        .filter(|node| node.kind == "htmx_request")
        .collect();
    assert_eq!(requests.len(), 64);
    assert!(requests
        .iter()
        .all(|node| node.details.to_string().len() < 2048));
    assert!(requests.iter().any(|node| node.name == "GET /route-63"));
    println!(
        "HTMX 64 requests, largest node details={} bytes",
        requests
            .iter()
            .map(|node| node.details.to_string().len())
            .max()
            .unwrap()
    );
}

#[test]
fn htmx_same_line_requests_have_distinct_stable_ids() {
    let source = "<button hx-get='/same'></button><button hx-get='/same'></button><button data-hx-get='/old' hx-get='/right' hx-post='/save'></button>";
    let first = ast::extract("page.html", "html", source).unwrap();
    let second = ast::extract("page.html", "html", source).unwrap();
    let requests: Vec<_> = first
        .nodes
        .iter()
        .filter(|node| node.kind == "htmx_request")
        .collect();
    assert_eq!(requests.len(), 4);
    assert!(requests.iter().all(|node| node.line == 1));
    let ids: std::collections::HashSet<_> = requests.iter().map(|node| &node.id).collect();
    assert_eq!(ids.len(), 4);
    assert_eq!(
        first.nodes.iter().map(|node| &node.id).collect::<Vec<_>>(),
        second.nodes.iter().map(|node| &node.id).collect::<Vec<_>>()
    );
    assert!(requests.iter().any(|node| node.name == "GET /right"));
    assert!(requests.iter().any(|node| node.name == "POST /save"));
    assert!(!requests.iter().any(|node| node.name == "GET /old"));
}

#[test]
fn html_diagnostics_and_whole_document_ast_search_use_html_grammar() {
    let facts = ExtractorHarness::for_lang("html")
        .at_path("index.html")
        .extract("<div attr=\"");
    assert!(!facts.errors.is_empty());
    assert_eq!(facts.nodes.len(), 1);
    let matches = ast::search("<p>Hello</p>", "index.html", "html", "<p>Hello</p>", 10).unwrap();
    assert_eq!(matches.len(), 1);
    let nested = ast::search(
        "<html><body><p>Hello</p><p>Hello</p></body></html>",
        "index.html",
        "html",
        "<p>Hello</p>",
        10,
    )
    .unwrap();
    assert_eq!(nested.len(), 2);
    assert!(ast::search("<p>Hello</p>", "index.html", "html", "<div attr=\"", 10).is_err());
}

#[test]
fn html_script_paths_are_preserved_without_requiring_javascript_grammar() {
    let facts = ExtractorHarness::for_lang("html")
        .at_path("index.html")
        .extract("<script src='app.js'></script><link rel='modulepreload' href='./chunk.mjs'>");
    assert_eq!(facts.references.len(), 2);
    assert!(facts
        .references
        .iter()
        .all(|reference| reference.kind == "imports"));
    assert!(facts
        .references
        .iter()
        .any(|reference| reference.module.as_deref() == Some("./app.js")));
}
