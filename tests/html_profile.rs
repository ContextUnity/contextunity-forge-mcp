#![cfg(feature = "lang-html")]

use contextunity_forge_mcp::{
    core::{
        commitments,
        response::{Detail, QueryOptions, ResponsePolicy, SourceOptions},
    },
    db::{reader, symbols, writer},
    engine::{ast, scanner},
};
use std::{
    collections::BTreeMap,
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
        let root = std::env::temp_dir().join(format!("forge_html_{}_{nonce}", std::process::id()));
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
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(feature = "lang-typescript")]
fn coverage_aggregate_rows(conn: &rusqlite::Connection) -> (Vec<String>, Vec<String>) {
    let owner_language = conn
        .prepare(
            "SELECT json_array(p.path, c.line, e.expression, c.status, v.evidence, c.language) \
             FROM coverage_owner_language c \
             JOIN path_dictionary p ON p.path_id=c.path_id \
             JOIN coverage_expressions e ON e.expression_id=c.expression_id \
             JOIN coverage_evidence v ON v.evidence_id=c.evidence_id \
             ORDER BY p.path, c.line, e.expression, c.status, v.evidence, c.language",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let language_counts = conn
        .prepare(
            "SELECT json_array(p.path, c.language, c.status, c.records) \
             FROM coverage_language_counts c \
             JOIN path_dictionary p ON p.path_id=c.path_id \
             ORDER BY p.path, c.language, c.status",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    (owner_language, language_counts)
}

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

#[cfg(feature = "lang-typescript")]
#[test]
fn root_relative_browser_assets_do_not_resolve_into_another_workspace() {
    let workspace = Workspace::new();
    let linked = Workspace::new();
    workspace.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: assets\n    path: '{}'\n    roots: [.]\n",
            linked.0.display()
        ),
    );
    workspace.write("index.html", "<script src='/assets/app.js'></script>");
    linked.write("assets/app.js", "export const answer = 42;\n");
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let imports: i64 = conn
        .query_row(
            "SELECT count(*) FROM edges WHERE (SELECT id FROM nodes WHERE node_hash=src_hash)='module:index.html' AND kind='imports'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(imports, 0);
    let unresolved: i64 = conn
        .query_row(
            "SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='index.html' AND status='unresolved'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(unresolved, 1);
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
            &format!("site/{name}.html"),
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
            &format!("site/{name}.html"),
            &format!("{script}\n{{% include 'parts/{name}.html' %}}\n"),
        );
        workspace.write(
            &format!("site/parts/{name}.html"),
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
            &format!("site/chain{index}.html"),
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

#[cfg(feature = "lang-typescript")]
#[test]
fn classic_require_rebindings_do_not_change_independent_module_imports() {
    for declaration in ["const require = customLoader;", "function require() {}"] {
        let source = format!("<script>{declaration}</script>\n<script>const fs = require('node:fs'); fs.readFile();</script>\n<script type='module'>import fs from 'node:fs'; fs.readFile();</script>");
        let facts = ast::extract("loaders.html", "html", &source).unwrap();
        let loader = facts
            .nodes
            .iter()
            .find(|node| node.kind == "function" && node.name == "require")
            .map(|node| node.id.clone());
        let imports: Vec<_> = facts
            .references
            .iter()
            .filter(|reference| reference.kind == "imports")
            .map(|reference| {
                (
                    reference.line,
                    reference.expression.as_str(),
                    reference.alias.as_deref(),
                    reference.module.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            imports,
            vec![(3, "default", Some("fs"), Some("node:fs"))],
            "{facts:#?}"
        );
        let graph = contextunity_forge_mcp::engine::linker::link(
            &std::collections::BTreeMap::from([("loaders.html".to_owned(), facts)]),
        );
        if declaration.starts_with("function") {
            let loader = loader.expect("classic loader function");
            assert!(
                graph.edges.iter().any(|edge| edge.kind == "calls"
                    && edge.line == 2
                    && edge.evidence == "require"
                    && edge.dst == loader),
                "{graph:#?}"
            );
        } else {
            assert!(
                graph.coverage.iter().any(|coverage| coverage.line == 2
                    && coverage.expression == "require"
                    && coverage.status == "unresolved"),
                "{graph:#?}"
            );
        }
        assert!(
            graph.coverage.iter().any(|coverage| coverage.line == 2
                && coverage.expression == "fs.readFile"
                && coverage.status == "unresolved"),
            "{graph:#?}"
        );
        assert!(
            graph.coverage.iter().any(|coverage| coverage.line == 3
                && coverage.expression == "fs.readFile"
                && coverage.status == "external"),
            "{graph:#?}"
        );
    }
}

#[cfg(feature = "lang-typescript")]
#[path = "html_profile/classic_wire.rs"]
mod classic_wire;

#[cfg(feature = "lang-python")]
#[path = "html_profile/template_origins.rs"]
mod template_origins;

#[path = "html_profile/template_masking.rs"]
mod template_masking;

#[test]
fn django_jinja_directives_are_external_while_template_targets_remain_local() {
    let path = "templates/page.html";
    let source = r#"{% extends "base.html" %}
{% block content %}
<section id="card">
  {% url 'product-detail' product.id %}
  {% static 'css/site.css' %}
  {% trans 'Details' %}
  {% csrf_token %}
  {% custom_widget product %}
  {{ product.name|default:'Untitled'|slugify }}
  {{ product.name|custom_widget }}
  {{ product.created|date:'Y-m-d' }}
  {{ product.tags|length }}
  {{ product.metadata|json_script:'metadata' }}
  {% include "partials/card.html" %}
</section>
{% endblock %}
"#;
    let facts = ast::extract(path, "html", source).unwrap();
    assert!(facts.errors.is_empty(), "{:#?}", facts.errors);
    assert!(facts.nodes.iter().any(|node| node.kind == "module"));
    for (kind, target) in [("extends", "base.html"), ("includes", "partials/card.html")] {
        assert!(
            facts
                .references
                .iter()
                .any(|reference| reference.kind == kind && reference.expression == target),
            "{kind} {target}: {:#?}",
            facts.references
        );
    }

    let mut all = BTreeMap::from([(path.to_owned(), facts)]);
    for target in ["templates/base.html", "templates/partials/card.html"] {
        all.insert(
            target.to_owned(),
            ast::extract(target, "html", "<main>Local template</main>").unwrap(),
        );
    }
    let graph = contextunity_forge_mcp::engine::linker::link(&all);
    for (kind, target) in [
        ("extends", "templates/base.html"),
        ("includes", "templates/partials/card.html"),
    ] {
        let module = all[target]
            .nodes
            .iter()
            .find(|node| node.kind == "module")
            .unwrap();
        assert!(
            graph
                .edges
                .iter()
                .any(|edge| { edge.path == path && edge.kind == kind && edge.dst == module.id }),
            "{kind} {target}: {graph:#?}"
        );
    }
    for (kind, names) in [
        ("tag", &["block", "include", "extends"][..]),
        ("filter", &["default", "length"][..]),
    ] {
        for name in names {
            let expression = format!("template.{kind}.{name}");
            assert!(
                graph.coverage.iter().any(|coverage| {
                    coverage.path == path
                        && coverage.expression == expression
                        && coverage.status == "external"
                        && coverage.evidence.contains("builtin:shared_template")
                }),
                "{expression}: {graph:#?}"
            );
        }
    }
    for expression in [
        "template.tag.url",
        "template.tag.static",
        "template.tag.trans",
        "template.tag.csrf_token",
        "template.filter.date",
        "template.filter.json_script",
        "template.filter.slugify",
    ] {
        assert!(
            graph.coverage.iter().any(|coverage| {
                coverage.path == path
                    && coverage.expression == expression
                    && coverage.status == "unresolved"
            }),
            "{expression} requires verified Django context or a library load: {graph:#?}"
        );
    }
    for expression in [
        "template.tag.custom_widget",
        "template.filter.custom_widget",
    ] {
        assert!(
            !graph.coverage.iter().any(|coverage| {
                coverage.path == path
                    && coverage.expression == expression
                    && coverage.status == "external"
            }),
            "custom directive {expression} is not a framework built-in: {graph:#?}"
        );
    }
}

#[test]
fn template_targets_resolve_within_the_owning_template_tree() {
    let workspace = Workspace::new();
    workspace.write(
        "commerce/templates/page.html",
        "{% extends 'base.html' %}\n{% include 'partials/card.html' %}\n{% import 'macros/forms.html' as forms %}\n{% from 'macros/forms.html' import field %}\n",
    );
    workspace.write("commerce/templates/base.html", "<main></main>");
    workspace.write("commerce/templates/partials/card.html", "<div></div>");
    workspace.write(
        "commerce/templates/macros/forms.html",
        "{% macro field() %}{% endmacro %}",
    );
    workspace.write("other/templates/base.html", "<main></main>");
    workspace.write("other/templates/partials/card.html", "<div></div>");
    workspace.write(
        "other/templates/macros/forms.html",
        "{% macro field() %}{% endmacro %}",
    );
    workspace.write(
        "other/templates/page.html",
        "{% include 'partials/card.html' %}",
    );
    workspace.write(
        "isolated/templates/page.html",
        "{% include 'partials/card.html' %}",
    );

    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    for (path, target, status, expected_target) in [
        (
            "commerce/templates/page.html",
            "base.html",
            "resolved",
            "commerce/templates/base.html",
        ),
        (
            "commerce/templates/page.html",
            "partials/card.html",
            "resolved",
            "commerce/templates/partials/card.html",
        ),
        (
            "commerce/templates/page.html",
            "macros/forms.html",
            "resolved",
            "commerce/templates/macros/forms.html",
        ),
        (
            "other/templates/page.html",
            "partials/card.html",
            "resolved",
            "other/templates/partials/card.html",
        ),
        (
            "isolated/templates/page.html",
            "partials/card.html",
            "unresolved",
            "",
        ),
    ] {
        let mut statement = conn
            .prepare("SELECT rc.status, ce.evidence FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions cx ON cx.expression_id=rc.expression_id JOIN coverage_evidence ce ON ce.evidence_id=rc.evidence_id WHERE pd.path=?1 AND cx.expression=?2")
            .unwrap();
        let rows = statement
            .query_map([path, target], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(!rows.is_empty(), "missing coverage for {path}: {target}");
        assert!(
            rows.iter().all(|(actual, evidence)| {
                actual == status
                    && (expected_target.is_empty() || evidence.contains(expected_target))
            }),
            "{path}: {target}: {rows:?}"
        );
    }
}

#[test]
fn registered_django_template_symbols_use_loaded_project_library() {
    let workspace = Workspace::new();
    let library = "from django import template\nregister = template.Library()\n@register.filter(name='display_price')\ndef format_price(value):\n    return value\n@register.simple_tag(name='badge')\ndef render_badge(value):\n    return value\n";
    for project in ["commerce", "other"] {
        workspace.write(
            &format!("{project}/pyproject.toml"),
            "[project]\nname = 'sample'\ndependencies = ['django>=5']\n",
        );
        workspace.write(&format!("{project}/shop/templatetags/pricing.py"), library);
    }
    workspace.write(
        "commerce/templates/page.html",
        "{% load pricing %}\n{{ total|display_price }}\n{% badge total %}\n{% absent_tag total %}\n",
    );
    workspace.write(
        "other/templates/page.html",
        "{% load pricing %}\n{{ total|display_price }}\n",
    );
    workspace.write(
        "commerce/templates/unloaded.html",
        "{{ total|display_price }}\n",
    );
    workspace.write(
        "isolated/pyproject.toml",
        "[project]\nname = 'isolated'\ndependencies = ['django>=5']\n",
    );
    workspace.write(
        "isolated/templates/page.html",
        "{% load pricing %}\n{{ total|display_price }}\n",
    );
    workspace.write(
        "commerce/templates/selected.html",
        "{% load display_price from pricing %}\n{{ total|display_price }}\n{% badge total %}\n",
    );
    workspace.write(
        "ambiguous/pyproject.toml",
        "[project]\nname = 'ambiguous'\ndependencies = ['django>=5']\n",
    );
    for package in ["shop", "billing"] {
        workspace.write(
            &format!("ambiguous/{package}/templatetags/pricing.py"),
            library,
        );
    }
    workspace.write(
        "ambiguous/templates/page.html",
        "{% load pricing %}\n{{ total|display_price }}\n",
    );
    workspace.write(
        "shadow/pyproject.toml",
        "[project]\nname = 'shadow'\ndependencies = ['django>=5']\n",
    );
    workspace.write(
        "shadow/shop/templatetags/pricing.py",
        "from django import template\nregister = template.Library()\nregister = object()\n@register.filter\ndef format_price(value):\n    return value\n",
    );
    workspace.write(
        "shadow/templates/page.html",
        "{% load pricing %}\n{{ total|format_price }}\n",
    );

    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    for (path, expression, status, provider, function) in [
        (
            "commerce/templates/page.html",
            "template.filter.display_price",
            "resolved",
            "commerce/shop/templatetags/pricing.py",
            "format_price",
        ),
        (
            "commerce/templates/page.html",
            "template.tag.badge",
            "resolved",
            "commerce/shop/templatetags/pricing.py",
            "render_badge",
        ),
        (
            "other/templates/page.html",
            "template.filter.display_price",
            "resolved",
            "other/shop/templatetags/pricing.py",
            "format_price",
        ),
        (
            "commerce/templates/selected.html",
            "template.filter.display_price",
            "resolved",
            "commerce/shop/templatetags/pricing.py",
            "format_price",
        ),
        (
            "commerce/templates/selected.html",
            "template.tag.badge",
            "unresolved",
            "",
            "",
        ),
        (
            "commerce/templates/page.html",
            "template.tag.absent_tag",
            "unresolved",
            "",
            "",
        ),
        (
            "commerce/templates/unloaded.html",
            "template.filter.display_price",
            "unresolved",
            "",
            "",
        ),
        (
            "isolated/templates/page.html",
            "template.filter.display_price",
            "unresolved",
            "",
            "",
        ),
        (
            "ambiguous/templates/page.html",
            "template.filter.display_price",
            "ambiguous",
            "",
            "",
        ),
        (
            "shadow/templates/page.html",
            "template.filter.format_price",
            "unresolved",
            "",
            "",
        ),
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
        assert_eq!(rows.len(), 1, "{path}: {expression}: {rows:?}");
        assert_eq!(rows[0].0, status, "{path}: {expression}: {rows:?}");
        if !provider.is_empty() {
            let target_count: i64 = conn
                .query_row(
                    "SELECT count(*) FROM edge_occurrences e JOIN path_dictionary pd ON pd.path_id=e.owner_id JOIN nodes dst ON dst.node_hash=e.dst_hash JOIN path_dictionary target_path ON target_path.path_id=dst.path_id WHERE pd.path=?1 AND e.kind='references' AND target_path.path=?2 AND dst.name=?3",
                    [path, provider, function],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(
                target_count > 0,
                "{path}: {expression} has no edge to {provider}"
            );
        }
    }
}

#[test]
fn template_expressions_follow_proven_local_and_imported_bindings() {
    let workspace = Workspace::new();
    for project in ["commerce", "other"] {
        workspace.write(
            &format!("{project}/templates/macros/forms.html"),
            "{% macro field(label) %}<label>{{ label }}</label>{% endmacro %}\n",
        );
        workspace.write(
            &format!("{project}/templates/page.html"),
            "{% import 'macros/forms.html' as forms %}\n{% from 'macros/forms.html' import field as quick_field %}\n{% set title = 'Checkout' %}\n<h1>{{ title }}</h1>\n{{ forms.field('Name') }}\n{{ quick_field('Email') }}\n{{ missing_value }}\n",
        );
    }
    workspace.write(
        "isolated/templates/page.html",
        "{% import 'macros/forms.html' as forms %}\n{{ forms.field('Name') }}\n",
    );

    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    for (path, expression, status, provider, symbol) in [
        (
            "commerce/templates/page.html",
            "template.variable.title",
            "resolved",
            "commerce/templates/page.html",
            "title",
        ),
        (
            "commerce/templates/page.html",
            "template.macro.forms.field",
            "resolved",
            "commerce/templates/macros/forms.html",
            "field",
        ),
        (
            "commerce/templates/page.html",
            "template.macro.quick_field",
            "resolved",
            "commerce/templates/macros/forms.html",
            "field",
        ),
        (
            "other/templates/page.html",
            "template.variable.title",
            "resolved",
            "other/templates/page.html",
            "title",
        ),
        (
            "other/templates/page.html",
            "template.macro.forms.field",
            "resolved",
            "other/templates/macros/forms.html",
            "field",
        ),
        (
            "other/templates/page.html",
            "template.macro.quick_field",
            "resolved",
            "other/templates/macros/forms.html",
            "field",
        ),
        (
            "commerce/templates/page.html",
            "template.variable.missing_value",
            "unresolved",
            "",
            "",
        ),
        (
            "isolated/templates/page.html",
            "template.macro.forms.field",
            "unresolved",
            "",
            "",
        ),
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
        assert_eq!(rows.len(), 1, "{path}: {expression}: {rows:?}");
        assert_eq!(rows[0].0, status, "{path}: {expression}: {rows:?}");
        if !provider.is_empty() {
            let target_count: i64 = conn
                .query_row(
                    "SELECT count(*) FROM edge_occurrences e JOIN path_dictionary pd ON pd.path_id=e.owner_id JOIN nodes dst ON dst.node_hash=e.dst_hash JOIN path_dictionary target_path ON target_path.path_id=dst.path_id WHERE pd.path=?1 AND e.kind='references' AND target_path.path=?2 AND dst.name=?3",
                    [path, provider, symbol],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(
                target_count > 0,
                "{path}: {expression} has no edge to {provider}:{symbol}"
            );
        }
    }
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

#[cfg(feature = "lang-typescript")]
#[test]
fn overview_attributes_disjoint_script_islands_without_claiming_template_lines() {
    let workspace = Workspace::new();
    workspace.write(
        "templates/page.html",
        "<script>\nunknownFirst();\n</script>\n<div>{{ missingTemplate }}</div>\n<script>\nunknownSecond();\n</script>\n",
    );
    workspace.write(
        "templates/inline.html",
        "<script>unknownInline();</script>\n",
    );
    workspace.write(
        "templates/mixed.html",
        "<div>{{ mixedTemplate }}</div><script>unknownMixed();</script>\n",
    );
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let options = QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(20),
        0,
        Some(Detail::Full),
        None,
    )
    .unwrap();
    let overview = reader::overview_with_options(
        &conn,
        &reader::OverviewOptions {
            aspects: Some(&["languages".into()]),
            page: &options,
        },
    )
    .unwrap();
    let languages = overview["languages"]["items"].as_array().unwrap();
    let by_language = |language: &str| {
        languages
            .iter()
            .find(|entry| entry["language"] == language)
            .unwrap()
    };
    assert_eq!(by_language("html")["unresolved"], 2, "{overview}");
    assert_eq!(by_language("javascript")["unresolved"], 4, "{overview}");
    assert_eq!(by_language("javascript")["files"], 0, "{overview}");

    let analysis = reader::analyze_paged(&conn, "templates/page.html", None, &options).unwrap();
    assert_eq!(
        analysis["resolution_languages"]["html"]["unresolved"], 1,
        "{analysis}"
    );
    assert_eq!(
        analysis["resolution_languages"]["javascript"]["unresolved"], 2,
        "{analysis}"
    );
    let inline = reader::analyze_paged(&conn, "templates/inline.html", None, &options).unwrap();
    assert_eq!(
        inline["resolution_languages"]["javascript"]["unresolved"], 1,
        "{inline}"
    );
    let mixed = reader::analyze_paged(&conn, "templates/mixed.html", None, &options).unwrap();
    assert_eq!(
        mixed["resolution_languages"]["html"]["unresolved"], 1,
        "{mixed}"
    );
    assert_eq!(
        mixed["resolution_languages"]["javascript"]["unresolved"], 1,
        "{mixed}"
    );
    let raw_count: i64 = conn
        .query_row("SELECT count(*) FROM resolution_coverage", [], |row| {
            row.get(0)
        })
        .unwrap();
    let sidecar_count: i64 = conn
        .query_row("SELECT count(*) FROM coverage_owner_language", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        sidecar_count, 1,
        "only the mixed script reference needs an override"
    );
    let projected_count: i64 = conn
        .query_row(
            "SELECT coalesce(sum(records),0) FROM coverage_language_counts",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(projected_count, raw_count);
    drop(conn);

    workspace.write(
        "templates/mixed.html",
        "<div>{{ changedTemplate }}</div><script>changedScript();</script>\n",
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("templates/mixed.html")],
    )
    .unwrap();
    let cold_db = workspace.0.join("cold.sqlite");
    writer::build(&workspace.0, &cold_db, None).unwrap();
    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = reader::open(&cold_db, &workspace.0).unwrap();
    let report = |conn: &rusqlite::Connection| {
        reader::analyze_paged(conn, "templates/mixed.html", None, &options).unwrap()
            ["resolution_languages"]
            .clone()
    };
    let overview_languages = |conn: &rusqlite::Connection| {
        reader::overview_with_options(
            conn,
            &reader::OverviewOptions {
                aspects: Some(&["languages".into()]),
                page: &options,
            },
        )
        .unwrap()["languages"]["items"]
            .clone()
    };
    assert_eq!(overview_languages(&delta), overview_languages(&cold));
    assert_eq!(report(&delta), report(&cold));
    assert_eq!(report(&delta)["html"]["unresolved"], 1);
    assert_eq!(report(&delta)["javascript"]["unresolved"], 1);
    let owner_rows = |conn: &rusqlite::Connection| {
        conn.prepare("SELECT pd.path, ol.line, ce.expression, ol.status, ev.evidence, ol.language FROM coverage_owner_language ol JOIN path_dictionary pd ON pd.path_id=ol.path_id JOIN coverage_expressions ce ON ce.expression_id=ol.expression_id JOIN coverage_evidence ev ON ev.evidence_id=ol.evidence_id ORDER BY pd.path,ol.line,ce.expression,ol.status,ev.evidence")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert_eq!(owner_rows(&delta), owner_rows(&cold));
    assert_eq!(owner_rows(&delta).len(), 1);
    let projected_rows = |conn: &rusqlite::Connection| {
        conn.prepare("SELECT p.path,c.language,c.status,c.records FROM coverage_language_counts c JOIN path_dictionary p ON p.path_id=c.path_id ORDER BY p.path,c.language,c.status")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert_eq!(projected_rows(&delta), projected_rows(&cold));
    let coverage_rows = |conn: &rusqlite::Connection| {
        conn.prepare("SELECT pd.path, rc.line, ce.expression, rc.status, ev.evidence FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id JOIN coverage_evidence ev ON ev.evidence_id=rc.evidence_id ORDER BY pd.path,rc.line,ce.expression,rc.status,ev.evidence")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert_eq!(coverage_rows(&delta), coverage_rows(&cold));
    let coverage_leaves = |conn: &rusqlite::Connection| {
        conn.prepare("SELECT coalesce(pd.path,''), hex(dc.digest) FROM domain_commitments dc LEFT JOIN path_dictionary pd ON pd.path_id=dc.owner_id WHERE dc.domain='resolution_coverage' ORDER BY pd.path")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert_eq!(coverage_leaves(&delta), coverage_leaves(&cold));
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
    assert_eq!(
        delta
            .query_row("SELECT count(*) FROM resolution_coverage", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        raw_count,
    );
    drop(delta);
    drop(cold);
    let writable = rusqlite::Connection::open(workspace.db()).unwrap();
    writable
        .execute(
            "UPDATE metadata SET value='11' WHERE key='schema_version'",
            [],
        )
        .unwrap();
    drop(writable);
    let error = reader::open(&workspace.db(), &workspace.0).unwrap_err();
    assert!(
        error.to_string().contains("incompatible database schema"),
        "{error}"
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("templates/mixed.html")],
    )
    .unwrap();
    let rebuilt = reader::open(&workspace.db(), &workspace.0).unwrap();
    assert_eq!(report(&rebuilt)["javascript"]["unresolved"], 1);
}

#[cfg(feature = "lang-typescript")]
#[test]
fn same_line_coverage_with_distinct_ast_owners_fails_closed() {
    let workspace = Workspace::new();
    workspace.write(
        "templates/ambiguous.html",
        "{% include 'same' %}<script>same();</script>\n",
    );
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let options = QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(20),
        0,
        Some(Detail::Full),
        None,
    )
    .unwrap();
    let analysis =
        reader::analyze_paged(&conn, "templates/ambiguous.html", None, &options).unwrap();
    assert_eq!(
        analysis["resolution_languages"]["html"]["unresolved"], 2,
        "{analysis}"
    );
    assert!(
        analysis["resolution_languages"].get("javascript").is_none(),
        "{analysis}"
    );
    let overrides: i64 = conn
        .query_row("SELECT count(*) FROM coverage_owner_language", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(overrides, 0);
}

#[test]
fn jinja_builtins_follow_imported_filesystem_loader_roots() {
    let workspace = Workspace::new();
    workspace.write(
        "shop/pyproject.toml",
        "[project]\nname = 'shop'\ndependencies = ['jinja2>=3']\n",
    );
    workspace.write(
        "shop/site/environment.py",
        "from pathlib import Path\nfrom jinja2 import Environment, FileSystemLoader\nTEMPLATES = Path(__file__).parent / 'templates'\nenv = Environment(loader=FileSystemLoader(str(TEMPLATES)))\n",
    );
    let source = "{% set rows = [] %}\n{{ rows|selectattr('active')|list }}\n";
    workspace.write("shop/site/templates/page.html", source);
    workspace.write("shop/site/other/templates/page.html", source);
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let status = |conn: &rusqlite::Connection, path: &str, expression: &str| -> String {
        conn.query_row(
            "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path=?1 AND ce.expression=?2",
            [path, expression],
            |row| row.get(0),
        )
        .unwrap()
    };
    for expression in [
        "template.tag.set",
        "template.filter.selectattr",
        "template.filter.list",
    ] {
        assert_eq!(
            status(&conn, "shop/site/templates/page.html", expression),
            "external",
            "{expression}"
        );
        assert_eq!(
            status(&conn, "shop/site/other/templates/page.html", expression),
            "unresolved",
            "{expression}"
        );
    }
    drop(conn);
    workspace.write(
        "shop/site/templates/page.html",
        "{% set rows = [] %}\n{{ rows|selectattr('active')|list }}\n<p>Changed</p>\n",
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("shop/site/templates/page.html")],
    )
    .unwrap();
    let cold_db = workspace.0.join("jinja-cold.sqlite");
    writer::build(&workspace.0, &cold_db, None).unwrap();
    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = reader::open(&cold_db, &workspace.0).unwrap();
    for expression in [
        "template.tag.set",
        "template.filter.selectattr",
        "template.filter.list",
    ] {
        assert_eq!(
            status(&delta, "shop/site/templates/page.html", expression),
            "external",
            "{expression}"
        );
        assert_eq!(
            status(&delta, "shop/site/templates/page.html", expression),
            status(&cold, "shop/site/templates/page.html", expression)
        );
    }
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
    drop(delta);
    drop(cold);
    workspace.write(
        "shop/site/environment.py",
        "from pathlib import Path\nfrom jinja2 import Environment, FileSystemLoader\nTEMPLATES = Path(__file__).parent / 'other' / 'templates'\nenv = Environment(loader=FileSystemLoader(str(TEMPLATES)))\n",
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("shop/site/environment.py")],
    )
    .unwrap();
    writer::build(&workspace.0, &cold_db, None).unwrap();
    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = reader::open(&cold_db, &workspace.0).unwrap();
    for (path, expected) in [
        ("shop/site/templates/page.html", "unresolved"),
        ("shop/site/other/templates/page.html", "external"),
    ] {
        for expression in [
            "template.tag.set",
            "template.filter.selectattr",
            "template.filter.list",
        ] {
            assert_eq!(
                status(&delta, path, expression),
                expected,
                "{path} {expression}"
            );
            assert_eq!(
                status(&delta, path, expression),
                status(&cold, path, expression)
            );
        }
    }
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
}

#[test]
fn jinja_loader_roots_fail_closed_without_a_unique_imported_provider() {
    let workspace = Workspace::new();
    let source = "{% set rows = [] %}\n{{ rows|selectattr('active')|list }}\n";
    for project in [
        "ambiguous",
        "rebound",
        "annotated",
        "tuple",
        "local_shadow",
        "src_jinja_shadow",
        "src_path_shadow",
        "missing",
    ] {
        workspace.write(
            &format!("{project}/pyproject.toml"),
            &format!("[project]\nname = '{project}'\ndependencies = ['jinja2>=3']\n"),
        );
        workspace.write(&format!("{project}/templates/page.html"), source);
    }
    let provider = "from pathlib import Path\nfrom jinja2 import Environment, FileSystemLoader\nTEMPLATES = Path(__file__).parent / 'templates'\nenv = Environment(loader=FileSystemLoader(str(TEMPLATES)))\n";
    workspace.write("ambiguous/first.py", provider);
    workspace.write("ambiguous/second.py", provider);
    workspace.write("rebound/environment.py", "from pathlib import Path\nfrom jinja2 import Environment, FileSystemLoader\nFileSystemLoader = object()\nTEMPLATES = Path(__file__).parent / 'templates'\nenv = Environment(loader=FileSystemLoader(str(TEMPLATES)))\n");
    workspace.write("annotated/environment.py", "from pathlib import Path\nfrom jinja2 import Environment, FileSystemLoader\nFileSystemLoader: object = object()\nTEMPLATES = Path(__file__).parent / 'templates'\nenv = Environment(loader=FileSystemLoader(str(TEMPLATES)))\n");
    workspace.write("tuple/environment.py", "from pathlib import Path\nfrom jinja2 import Environment, FileSystemLoader\nFileSystemLoader, unused = (object(), None)\nTEMPLATES = Path(__file__).parent / 'templates'\nenv = Environment(loader=FileSystemLoader(str(TEMPLATES)))\n");
    workspace.write("local_shadow/jinja2.py", "def Environment(*args, **kwargs):\n    return None\n\ndef FileSystemLoader(*args, **kwargs):\n    return None\n");
    workspace.write("local_shadow/environment.py", provider);
    workspace.write("src_jinja_shadow/src/jinja2.py", "def Environment(*args, **kwargs):\n    return None\n\ndef FileSystemLoader(*args, **kwargs):\n    return None\n");
    workspace.write("src_jinja_shadow/environment.py", provider);
    workspace.write("src_path_shadow/src/pathlib.py", "class Path:\n    pass\n");
    workspace.write("src_path_shadow/environment.py", provider);
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    for project in [
        "ambiguous",
        "rebound",
        "annotated",
        "tuple",
        "local_shadow",
        "src_jinja_shadow",
        "src_path_shadow",
        "missing",
    ] {
        let path = format!("{project}/templates/page.html");
        for expression in [
            "template.tag.set",
            "template.filter.selectattr",
            "template.filter.list",
        ] {
            let status: String = conn.query_row(
                "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path=?1 AND ce.expression=?2",
                [path.as_str(), expression],
                |row| row.get(0),
            ).unwrap();
            assert_eq!(status, "unresolved", "{project} {expression}");
        }
    }
}

#[test]
fn jinja_loader_roots_follow_literal_parent_chains_and_local_loader_flow() {
    let workspace = Workspace::new();
    let template = "{% set rows = [] %}\n{{ rows|e }}\n";
    for project in [
        "proven",
        "dynamic_parent",
        "bare_file_parent",
        "string_resolve",
        "rebound_loader",
        "shadowed_str",
        "shadowed_path",
        "shadowed_root",
        "conditional_return",
    ] {
        workspace.write(
            &format!("{project}/pyproject.toml"),
            &format!("[project]\nname = '{project}'\ndependencies = ['jinja2>=3']\n"),
        );
        workspace.write(
            &format!("{project}/src/pkg/templates/spec/page.html"),
            template,
        );
    }
    workspace.write("proven/src/pkg/templates/other/page.html", template);
    let provider = "from functools import lru_cache\nfrom pathlib import Path\nfrom jinja2 import Environment, FileSystemLoader\n_PACKAGE_ROOT = Path(__file__).resolve().parents[1]\n_TEMPLATE_DIR = _PACKAGE_ROOT / 'templates' / 'spec'\n\n@lru_cache(maxsize=1)\ndef environment():\n    loader = FileSystemLoader(str(_TEMPLATE_DIR))\n    return Environment(loader=loader)\n";
    workspace.write("proven/src/pkg/render/jinja.py", provider);
    workspace.write(
        "dynamic_parent/src/pkg/render/jinja.py",
        &provider
            .replace("parents[1]", "parents[INDEX]")
            .replace("_PACKAGE_ROOT =", "INDEX = 1\n_PACKAGE_ROOT ="),
    );
    workspace.write(
        "bare_file_parent/src/pkg/render/jinja.py",
        &provider.replace(
            "Path(__file__).resolve().parents[1]",
            "__file__.resolve().parents[1]",
        ),
    );
    workspace.write(
        "string_resolve/src/pkg/render/jinja.py",
        &provider.replace(
            "Path(__file__).resolve().parents[1]",
            "str(Path(__file__)).resolve().parents[1]",
        ),
    );
    workspace.write(
        "rebound_loader/src/pkg/render/jinja.py",
        &provider.replace(
            "    return Environment(loader=loader)",
            "    loader = unknown\n    return Environment(loader=loader)",
        ),
    );
    workspace.write(
        "shadowed_str/src/pkg/render/jinja.py",
        &provider.replace(
            "    loader = FileSystemLoader",
            "    str = lambda value: unknown\n    loader = FileSystemLoader",
        ),
    );
    workspace.write("shadowed_path/src/pkg/render/jinja.py", &provider.replace("    loader = FileSystemLoader(str(_TEMPLATE_DIR))", "    Path = lambda value: unknown\n    loader = FileSystemLoader(str(Path(__file__).resolve().parents[1] / 'templates' / 'spec'))"));
    workspace.write(
        "shadowed_root/src/pkg/render/jinja.py",
        &provider.replace(
            "    loader = FileSystemLoader",
            "    _TEMPLATE_DIR = unknown\n    loader = FileSystemLoader",
        ),
    );
    workspace.write(
        "conditional_return/src/pkg/render/jinja.py",
        &provider.replace(
            "    return Environment(loader=loader)",
            "    if False:\n        return Environment(loader=loader)",
        ),
    );
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let status = |conn: &rusqlite::Connection, path: &str, expression: &str| -> String {
        conn.query_row(
            "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path=?1 AND ce.expression=?2",
            [path, expression],
            |row| row.get(0),
        ).unwrap()
    };
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    for expression in ["template.tag.set", "template.filter.e"] {
        assert_eq!(
            status(&conn, "proven/src/pkg/templates/spec/page.html", expression),
            "external",
            "{expression}"
        );
        for path in [
            "proven/src/pkg/templates/other/page.html",
            "dynamic_parent/src/pkg/templates/spec/page.html",
            "bare_file_parent/src/pkg/templates/spec/page.html",
            "string_resolve/src/pkg/templates/spec/page.html",
            "rebound_loader/src/pkg/templates/spec/page.html",
            "shadowed_str/src/pkg/templates/spec/page.html",
            "shadowed_path/src/pkg/templates/spec/page.html",
            "shadowed_root/src/pkg/templates/spec/page.html",
            "conditional_return/src/pkg/templates/spec/page.html",
        ] {
            assert_eq!(
                status(&conn, path, expression),
                "unresolved",
                "{path} {expression}"
            );
        }
    }
    drop(conn);
    workspace.write(
        "proven/src/pkg/templates/spec/page.html",
        "{% set rows = [] %}\n{{ rows|e }}\n<p>Changed</p>\n",
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("proven/src/pkg/templates/spec/page.html")],
    )
    .unwrap();
    let cold_db = workspace.0.join("parent-chain-cold.sqlite");
    writer::build(&workspace.0, &cold_db, None).unwrap();
    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = reader::open(&cold_db, &workspace.0).unwrap();
    for expression in ["template.tag.set", "template.filter.e"] {
        let path = "proven/src/pkg/templates/spec/page.html";
        assert_eq!(status(&delta, path, expression), "external", "{expression}");
        assert_eq!(
            status(&delta, path, expression),
            status(&cold, path, expression)
        );
    }
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
}

#[test]
fn unrelated_html_delta_does_not_hydrate_jinja_loader_providers() {
    let workspace = Workspace::new();
    workspace.write(
        "shop/pyproject.toml",
        "[project]\nname = 'shop'\ndependencies = ['jinja2>=3']\n",
    );
    workspace.write(
        "shop/environment.py",
        "from pathlib import Path\nfrom jinja2 import Environment, FileSystemLoader\nTEMPLATES = Path(__file__).parent / 'templates'\nenv = Environment(loader=FileSystemLoader(str(TEMPLATES)))\n",
    );
    workspace.write(
        "shop/templates/page.html",
        "{% set title = 'Welcome' %}{{ title }}\n",
    );
    workspace.write("shop/unrelated.html", "<p>Before</p>\n");
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    workspace.write("shop/unrelated.html", "<p>After</p>\n");
    let delta = writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("shop/unrelated.html")],
    )
    .unwrap();
    assert_eq!(
        delta["loaded_fact_files"], 0,
        "unrelated HTML must not load Jinja providers: {delta}"
    );
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    commitments::verify(&conn).unwrap();
}

#[test]
fn html_delta_does_not_reinsert_render_dependencies_for_context_only_loader_facts() {
    let workspace = Workspace::new();
    workspace.write(
        "shop/pyproject.toml",
        "[project]\nname = 'shop'\ndependencies = ['django>=5', 'jinja2>=3']\n",
    );
    workspace.write(
        "shop/views.py",
        "from pathlib import Path\nfrom django.shortcuts import render\nfrom jinja2 import Environment, FileSystemLoader\nTEMPLATES = Path(__file__).parent / 'templates'\nenv = Environment(loader=FileSystemLoader(str(TEMPLATES)))\n\ndef page(request):\n    return render(request, 'templates/other.html', {'title': 'Other'})\n",
    );
    workspace.write("shop/templates/page.html", "<p>Before</p>\n");
    workspace.write("shop/templates/other.html", "<p>Other</p>\n");
    writer::build(&workspace.0, &workspace.db(), None).unwrap();

    let render_dependencies = |conn: &rusqlite::Connection| {
        conn.query_row(
            "SELECT count(*) FROM dependencies d JOIN path_dictionary p ON p.path_id=d.owner_id WHERE p.path='shop/views.py' AND d.kind='renders'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
    };
    let before = reader::open(&workspace.db(), &workspace.0).unwrap();
    let before_count = render_dependencies(&before);
    assert!(
        before_count > 0,
        "the fixture must persist a render dependency"
    );
    drop(before);

    workspace.write("shop/templates/page.html", "<p>After</p>\n");
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("shop/templates/page.html")],
    )
    .unwrap();

    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    assert_eq!(render_dependencies(&delta), before_count);
    commitments::verify(&delta).unwrap();
}

#[test]
fn rendered_template_context_keys_follow_exact_targets_and_includes() {
    let workspace = Workspace::new();
    for project in ["shop", "other"] {
        workspace.write(
            &format!("{project}/pyproject.toml"),
            &format!("[project]\nname = '{project}'\ndependencies = ['django>=5']\n"),
        );
    }
    workspace.write(
        "shop/views.py",
        "from django.shortcuts import render\n\ndef page(request):\n    return render(request, 'templates/page.html', {'title': 'Welcome'})\n\ndef later(request):\n    return render(request, 'templates/later.html', {'late': 'Ready'})\n",
    );
    workspace.write(
        "shop/templates/page.html",
        "<h1>{{ title }}</h1>\n{% include 'partials/card.html' %}\n",
    );
    workspace.write(
        "shop/templates/partials/card.html",
        "<strong>{{ title }}</strong>\n",
    );
    workspace.write(
        "shop/templates/next.html",
        "<h1>{{ title }}</h1><h2>{{ headline }}</h2>\n",
    );
    workspace.write("other/templates/page.html", "<h1>{{ title }}</h1>\n");
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    for (path, expected) in [
        ("shop/templates/page.html", "resolved"),
        ("shop/templates/partials/card.html", "resolved"),
        ("other/templates/page.html", "unresolved"),
    ] {
        let (status, evidence): (String, String) = conn.query_row(
            "SELECT rc.status,ev.evidence FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id JOIN coverage_evidence ev ON ev.evidence_id=rc.evidence_id WHERE pd.path=?1 AND ce.expression='template.variable.title'",
            [path], |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(status, expected, "{path}: {evidence}");
        if status == "resolved" {
            assert!(evidence.contains("shop/views.py"), "{path}: {evidence}");
        }
    }
    drop(conn);

    let rows = |conn: &rusqlite::Connection| {
        conn.prepare("SELECT pd.path,ce.expression,rc.status,ev.evidence FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id JOIN coverage_evidence ev ON ev.evidence_id=rc.evidence_id WHERE ce.expression LIKE 'template.variable.%' ORDER BY pd.path,ce.expression,rc.status,ev.evidence")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    workspace.write("shop/templates/later.html", "<p>{{ late }}</p>\n");
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("shop/templates/later.html")],
    )
    .unwrap();
    let added_cold = workspace.0.join("render-added-cold.sqlite");
    writer::build(&workspace.0, &added_cold, None).unwrap();
    let added_delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let added_cold = reader::open(&added_cold, &workspace.0).unwrap();
    assert_eq!(
        rows(&added_delta),
        rows(&added_cold),
        "new exact target must hydrate unchanged provider"
    );
    let later = rows(&added_delta)
        .into_iter()
        .find(|row| row.0 == "shop/templates/later.html" && row.1 == "template.variable.late")
        .unwrap();
    assert_eq!(later.2, "resolved", "{later:?}");
    commitments::verify(&added_delta).unwrap();
    commitments::verify(&added_cold).unwrap();
    drop(added_delta);
    drop(added_cold);

    workspace.write(
        "shop/templates/page.html",
        "<h1>{{ title }}</h1><p>Changed</p>\n{% include 'partials/card.html' %}\n",
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("shop/templates/page.html")],
    )
    .unwrap();
    let html_cold = workspace.0.join("render-html-cold.sqlite");
    writer::build(&workspace.0, &html_cold, None).unwrap();
    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = reader::open(&html_cold, &workspace.0).unwrap();
    assert_eq!(
        rows(&delta),
        rows(&cold),
        "HTML-only delta must retain unchanged render provider facts"
    );
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
    drop(delta);
    drop(cold);

    workspace.write(
        "shop/views.py",
        "from django.shortcuts import render\n\ndef page(request):\n    return render(request, 'templates/next.html', {'title': 'Welcome'})\n\ndef later(request):\n    return render(request, 'templates/later.html', {'late': 'Ready'})\n",
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("shop/views.py")],
    )
    .unwrap();
    let target_cold = workspace.0.join("render-target-cold.sqlite");
    writer::build(&workspace.0, &target_cold, None).unwrap();
    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = reader::open(&target_cold, &workspace.0).unwrap();
    assert_eq!(
        rows(&delta),
        rows(&cold),
        "old and new literal targets must be invalidated"
    );
    for path in [
        "shop/templates/page.html",
        "shop/templates/partials/card.html",
    ] {
        let old = rows(&delta)
            .into_iter()
            .find(|row| row.0 == path && row.1 == "template.variable.title")
            .unwrap();
        assert_eq!(old.2, "unresolved", "{path}: {old:?}");
    }
    let next = rows(&delta)
        .into_iter()
        .find(|row| row.0 == "shop/templates/next.html" && row.1 == "template.variable.title")
        .unwrap();
    assert_eq!(next.2, "resolved", "{next:?}");
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
    drop(delta);
    drop(cold);

    workspace.write(
        "shop/views.py",
        "from django.shortcuts import render\n\ndef page(request):\n    return render(request, 'templates/next.html', {'headline': 'Welcome'})\n\ndef later(request):\n    return render(request, 'templates/later.html', {'late': 'Ready'})\n",
    );
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from("shop/views.py")],
    )
    .unwrap();
    let key_cold = workspace.0.join("render-key-cold.sqlite");
    writer::build(&workspace.0, &key_cold, None).unwrap();
    let delta = reader::open(&workspace.db(), &workspace.0).unwrap();
    let cold = reader::open(&key_cold, &workspace.0).unwrap();
    assert_eq!(
        rows(&delta),
        rows(&cold),
        "changed literal context keys must be invalidated"
    );
    for (expression, expected) in [
        ("template.variable.title", "unresolved"),
        ("template.variable.headline", "resolved"),
    ] {
        let row = rows(&delta)
            .into_iter()
            .find(|row| row.0 == "shop/templates/next.html" && row.1 == expression)
            .unwrap();
        assert_eq!(row.2, expected, "{row:?}");
    }
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
}

#[test]
fn render_context_requires_a_unique_unshadowed_import_provider() {
    for (local_provider, views) in [
        (
            true,
            "from django.shortcuts import render\n\ndef page(request):\n    return render(request, 'templates/page.html', {'title': 'Welcome'})\n",
        ),
        (
            false,
            "from django.shortcuts import render\n\ndef replacement(request, target, context):\n    return None\n\nrender: object = replacement\n\ndef page(request):\n    return render(request, 'templates/page.html', {'title': 'Welcome'})\n",
        ),
    ] {
        let workspace = Workspace::new();
        workspace.write("pyproject.toml", "[project]\nname = 'shadowed'\ndependencies = ['django>=5']\n");
        if local_provider {
            workspace.write("django/shortcuts.py", "def render(request, target, context):\n    return None\n");
        }
        workspace.write("views.py", views);
        workspace.write("templates/page.html", "<h1>{{ title }}</h1>\n");
        writer::build(&workspace.0, &workspace.db(), None).unwrap();
        let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
        let (status, evidence): (String, String) = conn.query_row(
            "SELECT rc.status,ev.evidence FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id JOIN coverage_evidence ev ON ev.evidence_id=rc.evidence_id WHERE pd.path='templates/page.html' AND ce.expression='template.variable.title'",
            [], |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(status, "unresolved", "local_provider={local_provider}: {evidence}");
        commitments::verify(&conn).unwrap();
        drop(conn);
        workspace.write("templates/page.html", "<h1>{{ title }}</h1><p>Changed</p>\n");
        writer::delta(&workspace.0, &workspace.db(), &[PathBuf::from("templates/page.html")]).unwrap();
        let cold_db = workspace.0.join("shadow-render-cold.sqlite");
        writer::build(&workspace.0, &cold_db, None).unwrap();
        for db in [workspace.db(), cold_db] {
            let conn = reader::open(&db, &workspace.0).unwrap();
            let status: String = conn.query_row(
                "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path='templates/page.html' AND ce.expression='template.variable.title'",
                [], |row| row.get(0),
            ).unwrap();
            assert_eq!(status, "unresolved", "local_provider={local_provider}, db={db:?}");
            commitments::verify(&conn).unwrap();
        }
    }
}

#[test]
fn django_core_template_builtins_require_verified_load_context() {
    let workspace = Workspace::new();
    workspace.write(
        "shop/pyproject.toml",
        "[project]\nname = 'shop'\ndependencies = ['django>=5', 'jinja2>=3']\n",
    );
    workspace.write(
        "shop/templates/loaded.html",
        "{% load i18n static l10n %}\n{% translate 'Hello' %}\n{% static 'site.css' %}\n{% with title='Hello' %}\n{% endwith %}\n{% block content %}\n{% if title %}\n{% elif fallback %}\n{% else %}\n{% for item in items %}\n{{ item|escape|safe|upper|lower|urlencode|escapejs|json_script:'item' }}\n{% empty %}\n{% endfor %}\n{% endif %}\n{% autoescape off %}\n{% endautoescape %}\n{% url 'item' %}\n{% csrf_token %}\n{% endblock %}\n",
    );
    workspace.write(
        "shop/templates/unloaded.html",
        "{% translate 'Hello' %}\n{% static 'site.css' %}\n{% load missing_library %}\n{% unknown_tag %}\n",
    );
    workspace.write(
        "shop/templates/selected.html",
        "{% load trans from i18n %}\n{% trans 'Hello' %}\n{% translate 'Hello' %}\n",
    );
    workspace.write(
        "shop/templates/jinja.html",
        "{% macro badge(label) %}{{ label|e }}{% endmacro %}\n{% set title = 'Hello' %}\n{{ title|e|selectattr('active')|list }}\n",
    );
    workspace.write(
        "shop/environment.py",
        "from pathlib import Path\nfrom jinja2 import Environment, FileSystemLoader\nTEMPLATES = Path(__file__).parent / 'templates'\nenv = Environment(loader=FileSystemLoader(str(TEMPLATES)))\n",
    );
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let status = |path: &str, expression: &str| -> String {
        conn.query_row(
            "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path=?1 AND ce.expression=?2",
            [path, expression],
            |row| row.get(0),
        )
        .unwrap()
    };
    let loaded = "shop/templates/loaded.html";
    for expression in [
        "i18n",
        "static",
        "l10n",
        "template.tag.translate",
        "template.tag.static",
        "template.tag.with",
        "template.tag.endwith",
        "template.tag.block",
        "template.tag.endblock",
        "template.tag.if",
        "template.tag.endif",
        "template.tag.for",
        "template.tag.endfor",
        "template.tag.elif",
        "template.tag.else",
        "template.tag.empty",
        "template.tag.autoescape",
        "template.tag.endautoescape",
        "template.tag.url",
        "template.tag.csrf_token",
        "template.filter.escape",
        "template.filter.safe",
        "template.filter.upper",
        "template.filter.lower",
        "template.filter.urlencode",
        "template.filter.escapejs",
        "template.filter.json_script",
    ] {
        assert_eq!(status(loaded, expression), "external", "{expression}");
    }
    let unloaded = "shop/templates/unloaded.html";
    for expression in [
        "template.tag.translate",
        "template.tag.static",
        "missing_library",
        "template.tag.unknown_tag",
    ] {
        assert_eq!(status(unloaded, expression), "unresolved", "{expression}");
    }
    let selected = "shop/templates/selected.html";
    assert_eq!(status(selected, "i18n"), "external");
    assert_eq!(status(selected, "template.tag.trans"), "external");
    assert_eq!(status(selected, "template.tag.translate"), "unresolved");
    for expression in [
        "template.tag.macro",
        "template.tag.endmacro",
        "template.tag.set",
        "template.filter.e",
        "template.filter.selectattr",
        "template.filter.list",
    ] {
        assert_eq!(
            status("shop/templates/jinja.html", expression),
            "external",
            "{expression}"
        );
    }
    let evidence = |path: &str, expression: &str| -> String {
        conn.query_row(
            "SELECT ce.evidence FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions cx ON cx.expression_id=rc.expression_id JOIN coverage_evidence ce ON ce.evidence_id=rc.evidence_id WHERE pd.path=?1 AND cx.expression=?2",
            [path, expression],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert!(evidence(loaded, "i18n").contains("builtin:django_template_library"));
    assert!(evidence(loaded, "template.tag.translate").contains("builtin:django_template"));
    assert!(evidence("shop/templates/jinja.html", "template.filter.e")
        .contains("builtin:jinja_template"));
}

#[test]
fn rendered_template_with_multiple_views_stays_ambiguous() {
    let workspace = Workspace::new();
    workspace.write(
        "shop/pyproject.toml",
        "[project]\nname = 'shop'\ndependencies = ['django>=5']\n",
    );
    workspace.write(
        "shop/views.py",
        "from django.shortcuts import render\n\ndef page(request):\n    return render(request, 'templates/page.html', {'title': 'Welcome'})\n\ndef other(request):\n    return render(request, 'templates/page.html', {'headline': 'Other'})\n",
    );
    workspace.write("shop/templates/page.html", "<h1>{{ title }}</h1>\n");
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let (status, evidence): (String, String) = conn
        .query_row(
            "SELECT rc.status,ev.evidence FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id JOIN coverage_evidence ev ON ev.evidence_id=rc.evidence_id WHERE pd.path='shop/templates/page.html' AND ce.expression='template.variable.title'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "ambiguous", "{evidence}");
    let edges: i64 = conn
        .query_row(
            "SELECT count(*) FROM edges e JOIN path_dictionary pd ON pd.path_id=e.path_id WHERE e.kind='context_provider' AND pd.path='shop/templates/page.html'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        edges, 0,
        "a shared template must not emit a context_provider edge"
    );
}
