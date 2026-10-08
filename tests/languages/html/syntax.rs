use super::*;

#[test]
fn html_files_are_indexed_and_snippets_preserve_markup_after_delta() {
    let workspace = Workspace::new();
    let source = "<!doctype html>\n<html><body>\n<p>Привіт</p>\n</body></html>\n";
    workspace.write("index.html", source);
    workspace.write("other.htm", "<p>Other</p>");
    assert_eq!(
        scanner::language(std::path::Path::new("index.html")),
        Some(("html", false))
    );
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let options =
        QueryOptions::resolve(&ResponsePolicy::default(), Some(10), 0, None, None).unwrap();
    let preview = SourceOptions {
        enabled: true,
        leading_lines: 0,
        max_body_lines: 100,
        offset: 0,
    };
    let snippet =
        symbols::snippet_paged(&conn, &workspace.0, "index.html", &preview, &options).unwrap();
    assert_eq!(snippet["node"]["language"], "html");
    assert_eq!(snippet["source"], source);
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM files WHERE language='html'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    drop(conn);
    let updated = "<html>\n<body>Updated</body>\n</html>\n";
    workspace.write("index.html", updated);
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("index.html")],
    )
    .unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let snippet =
        symbols::snippet_paged(&conn, &workspace.0, "index.html", &preview, &options).unwrap();
    assert_eq!(snippet["source"], updated);
    let cold = workspace.0.join("cold.sqlite");
    writer::build(&workspace.0, &cold, None).unwrap();
    let rebuilt = reader::open(&cold, &workspace.0).unwrap();
    let graph = |conn: &rusqlite::Connection| {
        conn.prepare("SELECT id, details FROM nodes ORDER BY id")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert_eq!(graph(&conn), graph(&rebuilt));
}

#[test]
fn html_diagnostics_and_whole_document_ast_search_use_html_grammar() {
    let facts = ast::extract("index.html", "html", "<div attr=\"").unwrap();
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
    let facts = ast::extract(
        "index.html",
        "html",
        "<script src='app.js'></script><link rel='modulepreload' href='./chunk.mjs'>",
    )
    .unwrap();
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

#[cfg(feature = "lang-typescript")]
#[test]
fn inline_javascript_imports_resolve_without_html_namespace_collision() {
    let workspace = Workspace::new();
    workspace.write("app.html", "<html>\n<script type=\"module\">\nimport { answer } from './app.js';\n</script>\n<script type=\"application/ld+json\">{\"invalidJS\": true}</script>\n<script src=\"other.js\">import ignored from 'ignored';</script>\n</html>\n");
    workspace.write("app.js", "export function answer() { return 42; }\n");
    let facts = ast::extract(
        "app.html",
        "html",
        &fs::read_to_string(workspace.0.join("app.html")).unwrap(),
    )
    .unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    assert_eq!(facts.references.len(), 2);
    let inline = facts
        .references
        .iter()
        .find(|reference| reference.module.as_deref() == Some("./app.js"))
        .unwrap();
    assert_eq!(inline.line, 3);
    assert!(!facts
        .references
        .iter()
        .any(|reference| reference.module.as_deref() == Some("ignored")));
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let resolved: i64 = conn.query_row("SELECT count(*) FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=n.path_id WHERE e.src_hash=(SELECT node_hash FROM nodes WHERE id='module:app.html') AND e.kind='imports' AND p.path='app.js'", [], |row| row.get(0)).unwrap();
    assert!(resolved > 0);
    let self_imports: i64 = conn.query_row("SELECT count(*) FROM edges WHERE (SELECT id FROM nodes WHERE node_hash=src_hash)='module:app.html' AND (SELECT id FROM nodes WHERE node_hash=dst_hash)='module:app.html' AND kind='imports'", [], |row| row.get(0)).unwrap();
    assert_eq!(self_imports, 0);
}

#[cfg(feature = "lang-typescript")]
#[test]
fn html_local_script_and_link_assets_resolve_and_remote_assets_stay_out() {
    let workspace = Workspace::new();
    let source = "<script src='app.js?v=1'></script>\n<link rel='modulepreload' href='/app.js#module'>\n<script src='https://cdn.invalid/remote.js'></script>\n<link rel='stylesheet' href='app.css'>\n";
    workspace.write("app.html", source);
    workspace.write("app.js", "export const answer = 42;\n");
    let facts = ast::extract("app.html", "html", source).unwrap();
    assert_eq!(facts.references.len(), 2);
    assert!(facts
        .references
        .iter()
        .any(|reference| reference.module.as_deref() == Some("./app.js")));
    assert!(facts
        .references
        .iter()
        .any(|reference| reference.module.as_deref() == Some("/app.js")));
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let occurrences: i64 = conn.query_row(
        "SELECT occurrence_count FROM edges WHERE (SELECT id FROM nodes WHERE node_hash=src_hash)='module:app.html' AND (SELECT id FROM nodes WHERE node_hash=dst_hash)='module:app.js' AND kind='imports'",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(occurrences, 2);
}

#[test]
fn htmx_declarations_are_searchable_without_inventing_handler_edges() {
    let workspace = Workspace::new();
    let source = concat!(
        "<div hx-target='#result' hx-swap='outerHTML'>\n",
        "<button hx-get='/orders?state=open' hx-trigger='click' hx-include='#filter' hx-vals='{\"token\":\"secret\"}'>Load</button>\n",
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

    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
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
    let workspace = Workspace::new();
    workspace.write("page.html", "<button hx-get='/before'>Before</button>");
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let mut updated = String::new();
    for index in 0..70 {
        updated.push_str(&format!("<button hx-get='/route-{index}'>Go</button>\n"));
    }
    updated.push_str(&format!(
        "<button hx-post='{}'>Large</button>",
        "x".repeat(300)
    ));
    workspace.write("page.html", &updated);
    writer::delta(&workspace.0, &workspace.db(), &[PathBuf::from("page.html")]).unwrap();
    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = workspace.0.join("cold.sqlite");
    writer::build(&workspace.0, &cold, None).unwrap();
    let rebuilt = reader::open(&cold, &workspace.0).unwrap();
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

#[cfg(feature = "lang-typescript")]
#[test]
fn embedded_javascript_scopes_preserve_symbols_positions_and_receiver_edges() {
    let source = "<script type='module'>class Client { work() {} }\nconst client = new Client(); client.work();\n</script>\n<script type='module'>client.work();</script>\n<button onclick=\"function launch() {} launch();\">Run</button>";
    let facts = ast::extract("islands.html", "html", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let class = facts
        .nodes
        .iter()
        .find(|node| node.name == "Client" && node.kind == "class")
        .unwrap();
    assert_eq!(class.line, 1);
    assert_eq!(
        class.details["column"],
        source.find("class Client").unwrap()
    );
    let work = facts
        .nodes
        .iter()
        .find(|node| node.name == "work" && node.kind == "method")
        .unwrap();
    let launch = facts
        .nodes
        .iter()
        .find(|node| node.name == "launch")
        .unwrap();
    assert_eq!(launch.line, 5);
    assert_eq!(
        launch.details["column"],
        source
            .lines()
            .nth(4)
            .unwrap()
            .find("function launch")
            .unwrap()
    );
    let all = std::collections::BTreeMap::from([("islands.html".to_owned(), facts.clone())]);
    let graph = contextunity_forge_mcp::engine::linker::link(&all);
    assert!(
        graph.edges.iter().any(|edge| edge.kind == "calls"
            && edge.evidence == "client.work"
            && edge.dst == work.id),
        "{graph:#?}"
    );
    assert!(
        graph
            .edges
            .iter()
            .any(|edge| edge.kind == "calls" && edge.evidence == "launch" && edge.dst == launch.id),
        "{graph:#?}"
    );
    assert!(
        graph.coverage.iter().any(|coverage| coverage.line == 4
            && coverage.expression == "client.work"
            && coverage.status == "unresolved"),
        "{graph:#?}"
    );
    assert_eq!(
        facts
            .nodes
            .iter()
            .filter(|node| node.kind == "template_scope")
            .count(),
        4
    );
}

#[cfg(feature = "lang-typescript")]
#[test]
fn html_javascript_import_scope_and_boolean_handlers_survive_delta() {
    let workspace = Workspace::new();
    workspace.write("provider.js", "export function answer() { return 42; }");
    workspace.write("page.html", "<script type='module'>import { answer } from './provider.js'; answer();</script>\n<script type='module'>answer();</script>\n<button onclick=\"function check() {} if (true && true) check();\">Run</button>");
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let imports: i64 = conn.query_row("SELECT count(*) FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=n.path_id WHERE e.src_hash=(SELECT node_hash FROM nodes WHERE id='module:page.html') AND e.kind='imports' AND p.path='provider.js'", [], |row| row.get(0)).unwrap();
    assert!(imports > 0);
    let resolved: i64 = conn.query_row("SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='page.html' AND line=1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='answer' AND status='resolved'", [], |row| row.get(0)).unwrap();
    let unresolved: i64 = conn.query_row("SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='page.html' AND line=2 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='answer' AND status='unresolved'", [], |row| row.get(0)).unwrap();
    assert_eq!(resolved, 2);
    assert_eq!(unresolved, 1);
    drop(conn);
    workspace.write("page.html", "<script>function changed() {} changed();</script><button onclick='function click() {} click();'>Run</button>");
    writer::delta(&workspace.0, &workspace.db(), &[PathBuf::from("page.html")]).unwrap();
    let cold = workspace.0.join("cold.sqlite");
    writer::build(&workspace.0, &cold, None).unwrap();
    let rows = |db: &std::path::Path| {
        let conn = reader::open(db, &workspace.0).unwrap();
        let mut statement = conn
            .prepare("SELECT id,details FROM nodes ORDER BY id")
            .unwrap();
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert_eq!(rows(&workspace.db()), rows(&cold));
}

#[cfg(feature = "lang-typescript")]
#[test]
fn same_line_javascript_islands_have_distinct_linked_symbol_identities() {
    let source = "<script type='module'>function same() {} same();</script><script type='module'>function same() {} same();</script><button onclick='function same() {} same();'>Run</button>";
    let facts = ast::extract("same-line.html", "html", source).unwrap();
    let functions: std::collections::BTreeSet<_> = facts
        .nodes
        .iter()
        .filter(|node| node.kind == "function" && node.name == "same")
        .map(|node| node.id.clone())
        .collect();
    assert_eq!(functions.len(), 3);
    let graph =
        contextunity_forge_mcp::engine::linker::link(&std::collections::BTreeMap::from([(
            "same-line.html".to_owned(),
            facts,
        )]));
    let targets: std::collections::BTreeSet<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.kind == "calls" && edge.evidence == "same")
        .map(|edge| edge.dst.clone())
        .collect();
    assert_eq!(targets, functions, "{graph:#?}");
}

#[cfg(feature = "lang-typescript")]
#[test]
fn classic_javascript_globals_are_shared_by_scripts_and_inline_handlers() {
    let source = "<script>class Worker { run() {} } const worker = new Worker(); function launch() {}</script>\n<script>worker.run(); launch();</script>\n<button onclick='launch(); worker.run();'>Run</button>\n<script type='module'>const privateWorker = new Worker(); privateWorker.run();</script>\n<script type='module'>privateWorker.run();</script>";
    let facts = ast::extract("classic.html", "html", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let work = facts
        .nodes
        .iter()
        .find(|node| node.kind == "method" && node.name == "run")
        .unwrap();
    let launch = facts
        .nodes
        .iter()
        .find(|node| node.name == "launch")
        .unwrap();
    let graph =
        contextunity_forge_mcp::engine::linker::link(&std::collections::BTreeMap::from([(
            "classic.html".to_owned(),
            facts.clone(),
        )]));
    for line in [2, 3] {
        assert!(
            graph.edges.iter().any(|edge| edge.kind == "calls"
                && edge.line == line
                && edge.evidence == "launch"
                && edge.dst == launch.id),
            "{graph:#?}"
        );
        assert!(
            graph.edges.iter().any(|edge| edge.kind == "calls"
                && edge.line == line
                && edge.evidence == "worker.run"
                && edge.dst == work.id),
            "{graph:#?}"
        );
    }
    assert!(
        graph.edges.iter().any(|edge| edge.kind == "calls"
            && edge.line == 4
            && edge.evidence == "privateWorker.run"
            && edge.dst == work.id),
        "{graph:#?}"
    );
    assert!(
        graph.coverage.iter().any(|coverage| coverage.line == 5
            && coverage.expression == "privateWorker.run"
            && coverage.status == "unresolved"),
        "{graph:#?}"
    );
    assert_eq!(
        facts
            .nodes
            .iter()
            .filter(|node| node.details["classic_global"] == true)
            .count(),
        1
    );
}

#[cfg(feature = "lang-typescript")]
#[test]
fn inline_handlers_resolve_exact_local_classic_script_providers() {
    let workspace = Workspace::new();
    workspace.write("site/assets/cart.js", "function openCart() {}\n");
    workspace.write(
        "site/index.html",
        "<script src='./assets/cart.js'></script><button onclick='openCart()'>Cart</button>\n",
    );
    for (name, script) in [
        (
            "module",
            "<script type='module' src='./assets/cart.js'></script>",
        ),
        ("async", "<script async src='./assets/cart.js'></script>"),
        ("defer", "<script defer src='./assets/cart.js'></script>"),
        ("dynamic", "<script src='./assets/{name}.js'></script>"),
        ("unimported", ""),
    ] {
        workspace.write(
            format!("site/{name}.html"),
            &format!("{script}<button onclick='openCart()'>Cart</button>\n"),
        );
    }
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let linked: i64 = conn.query_row(
        "SELECT count(*) FROM edges e JOIN path_dictionary owner ON owner.path_id=e.path_id JOIN coverage_evidence evidence ON evidence.evidence_id=e.evidence_id JOIN nodes target ON target.node_hash=e.dst_hash JOIN path_dictionary target_path ON target_path.path_id=target.path_id WHERE owner.path='site/index.html' AND e.kind='calls' AND evidence.evidence='openCart' AND target_path.path='site/assets/cart.js' AND target.kind='function'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        linked, 1,
        "the handler call links to the exact indexed script provider"
    );
    for name in ["module", "async", "defer", "dynamic", "unimported"] {
        let path = format!("site/{name}.html");
        let status: String = conn.query_row(
            "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path=?1 AND ce.expression='openCart'",
            [path.as_str()], |row| row.get(0),
        ).unwrap();
        assert_eq!(status, "unresolved", "{name}");
    }
    drop(conn);
    workspace.write("site/assets/duplicate.js", "function openCart() {}\n");
    workspace.write(
        "site/index.html",
        "<script src='./assets/cart.js'></script><script src='./assets/duplicate.js'></script><button onclick='openCart()'>Cart</button>\n",
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[
            PathBuf::from("site/index.html"),
            PathBuf::from("site/assets/duplicate.js"),
        ],
    )
    .unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let status: String = conn.query_row(
        "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path='site/index.html' AND ce.expression='openCart'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(status, "ambiguous");
    let incremental_aggregates = coverage_aggregate_rows(&conn);
    assert!(
        !incremental_aggregates.0.is_empty(),
        "the inline JavaScript handler exercises owner-language override persistence"
    );
    commitments::verify(&conn).unwrap();
    drop(conn);
    let cold_db = workspace.0.join("bridge-cold.sqlite");
    writer::build(&workspace.0, &cold_db, None).unwrap();
    let cold = reader::open(&cold_db, &workspace.0).unwrap();
    let cold_status: String = cold.query_row(
        "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path='site/index.html' AND ce.expression='openCart'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(cold_status, status);
    assert_eq!(incremental_aggregates, coverage_aggregate_rows(&cold));
    commitments::verify(&cold).unwrap();
}

#[cfg(feature = "lang-typescript")]
#[test]
fn inline_handler_provider_rebinding_fails_closed_after_exact_declaration() {
    let workspace = Workspace::new();
    let script = "function openCart() {}\nopenCart = unknown;\n";
    workspace.write("site/assets/cart.js", script);
    let extracted = ast::extract("site/assets/cart.js", "javascript", script).unwrap();
    assert!(
        extracted.nodes.iter().any(|node| {
            node.kind == "module"
                && node.details["rebindings"]
                    .as_array()
                    .is_some_and(|bindings| bindings.iter().any(|binding| binding == "openCart"))
        }),
        "the indexed module records the AST binding write"
    );
    workspace.write(
        "site/index.html",
        "<script src='./assets/cart.js'></script><button onclick='openCart()'>Cart</button>\n",
    );
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let status: String = conn.query_row(
        "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path='site/index.html' AND ce.expression='openCart'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        status, "unresolved",
        "the call target is rebound after the indexed declaration"
    );
    commitments::verify(&conn).unwrap();
}

#[cfg(feature = "lang-typescript")]
#[test]
fn included_inline_handlers_follow_only_unique_prior_classic_scripts() {
    let workspace = Workspace::new();
    workspace.write("site/assets/cart.js", "function openCart() {}\n");
    workspace.write("site/assets/other.js", "function openCart() {}\n");
    for (name, script) in [
        ("unique", "<script src='./assets/cart.js'></script>"),
        (
            "ambiguous",
            "<script src='./assets/cart.js'></script><script src='./assets/other.js'></script>",
        ),
        ("async", "<script async src='./assets/cart.js'></script>"),
    ] {
        workspace.write(
            format!("site/{name}.html"),
            &format!("{script}\n{{% include 'parts/{name}.html' %}}\n"),
        );
        workspace.write(
            format!("site/parts/{name}.html"),
            "<button onclick='openCart()'>Cart</button>\n",
        );
    }
    workspace.write(
        "site/parts/shared.html",
        "<button onclick='openCart()'>Cart</button>\n",
    );
    workspace.write(
        "site/first.html",
        "<script src='./assets/cart.js'></script>\n{% include 'parts/shared.html' %}\n",
    );
    workspace.write(
        "site/second.html",
        "<script src='./assets/other.js'></script>\n{% include 'parts/shared.html' %}\n",
    );
    workspace.write(
        "site/parts/same_line.html",
        "<button onclick='openCart()'>Cart</button>\n",
    );
    workspace.write(
        "site/same_line.html",
        "<script src='./assets/cart.js'></script>{% include 'parts/same_line.html' %}\n",
    );
    workspace.write(
        "site/parts/late.html",
        "<button onclick='openCart()'>Cart</button>\n",
    );
    workspace.write(
        "site/late.html",
        "{% include 'parts/late.html' %}\n<script src='./assets/cart.js'></script>\n",
    );
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let status = |conn: &rusqlite::Connection, name: &str| -> String {
        conn.query_row(
            "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path=?1 AND ce.expression='openCart'",
            [format!("site/parts/{name}.html")], |row| row.get(0),
        ).unwrap()
    };
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    for (name, expected) in [
        ("unique", "resolved"),
        ("ambiguous", "ambiguous"),
        ("async", "unresolved"),
        ("shared", "ambiguous"),
        ("same_line", "unresolved"),
        ("late", "unresolved"),
    ] {
        assert_eq!(status(&conn, name), expected, "{name}");
    }
    commitments::verify(&conn).unwrap();
    drop(conn);
    workspace.write(
        "site/unique.html",
        "<script async src='./assets/cart.js'></script>\n{% include 'parts/unique.html' %}\n",
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("site/unique.html")],
    )
    .unwrap();
    let cold_db = workspace.0.join("include-bridge-cold.sqlite");
    writer::build(&workspace.0, &cold_db, None).unwrap();
    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = reader::open(&cold_db, &workspace.0).unwrap();
    assert_eq!(status(&delta, "unique"), "unresolved");
    for name in [
        "unique",
        "ambiguous",
        "async",
        "shared",
        "same_line",
        "late",
    ] {
        assert_eq!(status(&delta, name), status(&cold, name), "{name}");
    }
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
    drop(delta);
    drop(cold);
    workspace.write(
        "site/second.html",
        "<script async src='./assets/other.js'></script>\n{% include 'parts/shared.html' %}\n",
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("site/second.html")],
    )
    .unwrap();
    writer::build(&workspace.0, &cold_db, None).unwrap();
    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = reader::open(&cold_db, &workspace.0).unwrap();
    assert_eq!(
        status(&delta, "shared"),
        "unresolved",
        "one include parent lacks a proven provider"
    );
    assert_eq!(status(&delta, "shared"), status(&cold, "shared"));
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
}

#[cfg(feature = "lang-typescript")]
#[test]
fn included_inline_handlers_respect_manifest_scope_and_bounded_ancestry() {
    let workspace = Workspace::new();
    workspace.write(
        "site/package.json",
        "{\"name\":\"site\",\"version\":\"1.0.0\"}\n",
    );
    workspace.write(
        "site/sub/package.json",
        "{\"name\":\"sub\",\"version\":\"1.0.0\"}\n",
    );
    workspace.write("site/assets/cart.js", "function openCart() {}\n");
    workspace.write(
        "site/sub/child.html",
        "<button onclick='openCart()'>Cart</button>\n",
    );
    workspace.write(
        "site/scoped.html",
        "<script src='./assets/cart.js'></script>\n{% include 'sub/child.html' %}\n",
    );
    workspace.write(
        "site/chain0.html",
        "<script src='./assets/cart.js'></script>\n{% include 'chain1.html' %}\n",
    );
    for index in 1..17 {
        let handler = if index == 15 {
            "<button onclick='openCart()'>Cart</button>\n"
        } else {
            ""
        };
        workspace.write(
            format!("site/chain{index}.html"),
            &format!("{{% include 'chain{}.html' %}}\n{handler}", index + 1),
        );
    }
    workspace.write(
        "site/chain17.html",
        "<button onclick='openCart()'>Cart</button>\n",
    );
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    for (path, expected) in [
        ("site/sub/child.html", "unresolved"),
        ("site/chain15.html", "resolved"),
        ("site/chain17.html", "unresolved"),
    ] {
        let status: String = conn.query_row(
            "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path=?1 AND ce.expression='openCart'",
            [path], |row| row.get(0),
        ).unwrap();
        assert_eq!(status, expected, "{path}");
    }
    commitments::verify(&conn).unwrap();
}

#[test]
fn embedded_javascript_uses_its_owner_profile_inside_html() {
    let workspace = Workspace::new();
    let source = "<div>{{ encodeURIComponent }}</div>\n<script>\nfunction make() {\n  return { count: 0, inc() { this.count += 1; this.reset(); }, reset() { this.count = 0; } };\n}\nfunction render() { encodeURIComponent('x'); document.querySelector('#card'); return Date.now(); }\n</script>\n";
    let path = "templates/island.html";
    let facts = ast::extract(path, "html", source).unwrap();
    for expression in [
        "this.count",
        "this.reset",
        "encodeURIComponent",
        "document.querySelector",
        "Date.now",
    ] {
        assert!(
            facts.references.iter().any(|reference| {
                reference.expression == expression
                    && facts
                        .nodes
                        .iter()
                        .any(|owner| owner.id == reference.source && owner.language == "javascript")
            }),
            "{expression} does not belong to a JavaScript island owner"
        );
    }
    workspace.write(path, source);
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    for (expression, status) in [
        ("this.count", "resolved"),
        ("this.reset", "resolved"),
        ("encodeURIComponent", "external"),
        ("document.querySelector", "external"),
        ("Date.now", "external"),
        ("template.variable.encodeURIComponent", "unresolved"),
    ] {
        let mut statement = conn
            .prepare("SELECT rc.status, ce.evidence FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions cx ON cx.expression_id=rc.expression_id JOIN coverage_evidence ce ON ce.evidence_id=rc.evidence_id WHERE pd.path=?1 AND cx.expression=?2")
            .unwrap();
        let rows = statement
            .query_map([path, expression], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(!rows.is_empty(), "{expression} has no persisted coverage");
        assert!(
            rows.iter().all(|(actual, _)| actual == status),
            "{expression}: {rows:?}"
        );
        if matches!(expression, "document.querySelector") {
            assert!(
                rows.iter()
                    .all(|(_, evidence)| evidence.contains("builtin:web_api")),
                "{rows:?}"
            );
        }
    }
    let field_edge: i64 = conn
        .query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash JOIN path_dictionary target_path ON target_path.path_id=dst.path_id WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.line=4 AND e.kind='mutates' AND target_path.path=?1 AND dst.name='count' AND dst.kind='field'",
            [path],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        field_edge > 0,
        "this.count must link to its returned object field"
    );
}
