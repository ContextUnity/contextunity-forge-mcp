use super::*;
use crate::languages::harness::{query_scalar, ExtractorHarness};

fn full_query_options() -> QueryOptions {
    QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(20),
        0,
        Some(Detail::Full),
        None,
    )
    .unwrap()
}

#[cfg(feature = "lang-typescript")]
#[test]
fn root_relative_browser_assets_do_not_resolve_into_another_workspace() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    let linked = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: assets\n    path: '{}'\n    roots: [.]\n",
            linked.root().display()
        ),
    );
    workspace.write("index.html", "<script src='/assets/app.js'></script>");
    linked.write("assets/app.js", "export const answer = 42;\n");
    let conn = workspace.build_and_open();
    let imports: i64 = query_scalar(
        &conn,
        "SELECT count(*) FROM edges WHERE (SELECT id FROM nodes WHERE node_hash=src_hash)='module:index.html' AND kind='imports'",
        [],
    );
    assert_eq!(imports, 0);
    let unresolved: i64 = query_scalar(
        &conn,
        "SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='index.html' AND status='unresolved'",
        [],
    );
    assert_eq!(unresolved, 1);
}

#[cfg(feature = "lang-typescript")]
#[test]
fn classic_require_rebindings_do_not_change_independent_module_imports() {
    for declaration in ["const require = customLoader;", "function require() {}"] {
        let source = format!(
            "<script>{declaration}</script>\n<script>const fs = require('node:fs'); fs.readFile();</script>\n<script type='module'>import fs from 'node:fs'; fs.readFile();</script>"
        );
        let facts = ExtractorHarness::for_lang("html")
            .at_path("loaders.html")
            .extract(&source);
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
        let graph = ExtractorHarness::link_graph(&std::collections::BTreeMap::from([(
            "loaders.html".to_owned(),
            facts,
        )]));
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
#[test]
fn overview_attributes_disjoint_script_islands_without_claiming_template_lines() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write("templates/page.html", "<script>\nunknownFirst();\n</script>\n<div>{{ missingTemplate }}</div>\n<script>\nunknownSecond();\n</script>\n");
    workspace.write(
        "templates/inline.html",
        "<script>unknownInline();</script>\n",
    );
    workspace.write(
        "templates/mixed.html",
        "<div>{{ mixedTemplate }}</div><script>unknownMixed();</script>\n",
    );
    workspace.build();
    let conn = workspace.open();
    let options = full_query_options();
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
    let raw_count: i64 = query_scalar(&conn, "SELECT count(*) FROM resolution_coverage", []);
    let sidecar_count: i64 =
        query_scalar(&conn, "SELECT count(*) FROM coverage_owner_language", []);
    assert_eq!(
        sidecar_count, 1,
        "only the mixed script reference needs an override"
    );
    let projected_count: i64 = query_scalar(
        &conn,
        "SELECT coalesce(sum(records),0) FROM coverage_language_counts",
        [],
    );
    assert_eq!(projected_count, raw_count);
    drop(conn);

    workspace.write(
        "templates/mixed.html",
        "<div>{{ changedTemplate }}</div><script>changedScript();</script>\n",
    );
    let cold = workspace.delta_cold("templates/mixed.html");
    let delta = cold.incremental_connection;
    let cold = cold.cold_connection;
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
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?)))
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
        conn.prepare(
            "SELECT coalesce(pd.path,''), hex(dc.digest) FROM domain_commitments dc LEFT JOIN path_dictionary pd ON pd.path_id=dc.owner_id WHERE dc.domain='resolution_coverage' ORDER BY pd.path",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
    };
    assert_eq!(coverage_leaves(&delta), coverage_leaves(&cold));
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
    assert_eq!(
        query_scalar::<i64>(&delta, "SELECT count(*) FROM resolution_coverage", []),
        raw_count,
    );
    drop(delta);
    drop(cold);
    rusqlite::Connection::open(workspace.db())
        .unwrap()
        .execute(
            "UPDATE metadata SET value=?1 WHERE key=?2",
            rusqlite::params!["11", "schema_version"],
        )
        .unwrap();
    let error =
        contextunity_forge_mcp::db::reader::open(&workspace.db(), workspace.root()).unwrap_err();
    assert!(
        error.to_string().contains("incompatible database schema"),
        "{error}"
    );
    workspace.delta(&["templates/mixed.html"]);
    let rebuilt = workspace.open();
    assert_eq!(report(&rebuilt)["javascript"]["unresolved"], 1);
}

#[cfg(feature = "lang-typescript")]
#[test]
fn same_line_coverage_with_distinct_ast_owners_fails_closed() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write(
        "templates/ambiguous.html",
        "{% include 'same' %}<script>same();</script>\n",
    );
    let conn = workspace.build_and_open();
    let options = full_query_options();
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
    let overrides: i64 = query_scalar(&conn, "SELECT count(*) FROM coverage_owner_language", []);
    assert_eq!(overrides, 0);
}

#[test]
fn html_files_are_indexed_and_snippets_preserve_markup_after_delta() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    let source = "<!doctype html>\n<html><body>\n<p>Привіт</p>\n</body></html>\n";
    workspace.write("index.html", source);
    workspace.write("other.htm", "<p>Other</p>");
    assert_eq!(
        scanner::language(std::path::Path::new("index.html")),
        Some(("html", false))
    );
    workspace.build();
    let conn = workspace.open();
    let options =
        QueryOptions::resolve(&ResponsePolicy::default(), Some(10), 0, None, None).unwrap();
    let preview = SourceOptions {
        enabled: true,
        leading_lines: 0,
        max_body_lines: 100,
        offset: 0,
    };
    let snippet =
        symbols::snippet_paged(&conn, workspace.root(), "index.html", &preview, &options).unwrap();
    assert_eq!(snippet["node"]["language"], "html");
    assert_eq!(snippet["source"], source);
    assert_eq!(
        query_scalar::<i64>(
            &conn,
            "SELECT count(*) FROM files WHERE language='html'",
            []
        ),
        2
    );
    drop(conn);
    let updated = "<html>\n<body>Updated</body>\n</html>\n";
    workspace.write("index.html", updated);
    workspace.delta(&["index.html"]);
    let conn = workspace.open();
    let snippet =
        symbols::snippet_paged(&conn, workspace.root(), "index.html", &preview, &options).unwrap();
    assert_eq!(snippet["source"], updated);
    let rebuilt = crate::languages::support::fresh_cold(&workspace);
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

#[cfg(feature = "lang-typescript")]
#[test]
fn inline_javascript_imports_resolve_without_html_namespace_collision() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    let source = "<html>\n<script type=\"module\">\nimport { answer } from './app.js';\n</script>\n<script type=\"application/ld+json\">{\"invalidJS\": true}</script>\n<script src=\"other.js\">import ignored from 'ignored';</script>\n</html>\n";
    workspace.write("app.html", source);
    workspace.write("app.js", "export function answer() { return 42; }\n");
    let facts = ExtractorHarness::for_lang("html")
        .at_path("app.html")
        .extract(source);
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
    workspace.build();
    let conn = workspace.open();
    let resolved: i64 = query_scalar(
        &conn,
        "SELECT count(*) FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=n.path_id WHERE e.src_hash=(SELECT node_hash FROM nodes WHERE id='module:app.html') AND e.kind='imports' AND p.path='app.js'",
        [],
    );
    assert!(resolved > 0);
    let self_imports: i64 = query_scalar(
        &conn,
        "SELECT count(*) FROM edges WHERE (SELECT id FROM nodes WHERE node_hash=src_hash)='module:app.html' AND (SELECT id FROM nodes WHERE node_hash=dst_hash)='module:app.html' AND kind='imports'",
        [],
    );
    assert_eq!(self_imports, 0);
}

#[cfg(feature = "lang-typescript")]
#[test]
fn html_local_script_and_link_assets_resolve_and_remote_assets_stay_out() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    let source = "<script src='app.js?v=1'></script>\n<link rel='modulepreload' href='/app.js#module'>\n<script src='https://cdn.invalid/remote.js'></script>\n<link rel='stylesheet' href='app.css'>\n";
    workspace.write("app.html", source);
    workspace.write("app.js", "export const answer = 42;\n");
    let facts = ExtractorHarness::for_lang("html")
        .at_path("app.html")
        .extract(source);
    assert_eq!(facts.references.len(), 2);
    assert!(facts
        .references
        .iter()
        .any(|reference| reference.module.as_deref() == Some("./app.js")));
    assert!(facts
        .references
        .iter()
        .any(|reference| reference.module.as_deref() == Some("/app.js")));
    workspace.build();
    let conn = workspace.open();
    let occurrences: i64 = query_scalar(
        &conn,
        "SELECT occurrence_count FROM edges WHERE (SELECT id FROM nodes WHERE node_hash=src_hash)='module:app.html' AND (SELECT id FROM nodes WHERE node_hash=dst_hash)='module:app.js' AND kind='imports'",
        [],
    );
    assert_eq!(occurrences, 2);
}

#[cfg(feature = "lang-typescript")]
#[test]
fn embedded_javascript_scopes_preserve_symbols_positions_and_receiver_edges() {
    let source = "<script type='module'>class Client { work() {} }\nconst client = new Client(); client.work();\n</script>\n<script type='module'>client.work();</script>\n<button onclick=\"function launch() {} launch();\">Run</button>";
    let graph = ExtractorHarness::for_lang("html")
        .at_path("islands.html")
        .linked(source);
    let facts = &graph.facts;
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
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write("provider.js", "export function answer() { return 42; }");
    workspace.write("page.html", "<script type='module'>import { answer } from './provider.js'; answer();</script>\n<script type='module'>answer();</script>\n<button onclick=\"function check() {} if (true && true) check();\">Run</button>");
    workspace.build();
    let conn = workspace.open();
    let imports: i64 = query_scalar(
        &conn,
        "SELECT count(*) FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=n.path_id WHERE e.src_hash=(SELECT node_hash FROM nodes WHERE id='module:page.html') AND e.kind='imports' AND p.path='provider.js'",
        [],
    );
    assert!(imports > 0);
    let resolved: i64 = query_scalar(
        &conn,
        "SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='page.html' AND line=1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='answer' AND status='resolved'",
        [],
    );
    let unresolved: i64 = query_scalar(
        &conn,
        "SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='page.html' AND line=2 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='answer' AND status='unresolved'",
        [],
    );
    assert_eq!(resolved, 2);
    assert_eq!(unresolved, 1);
    drop(conn);
    workspace.write(
        "page.html",
        "<script>function changed() {} changed();</script><button onclick='function click() {} click();'>Run</button>",
    );
    workspace.delta(&["page.html"]);
    let cold = crate::languages::support::fresh_cold(&workspace);
    let rows = |conn: &rusqlite::Connection| {
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
    assert_eq!(rows(&workspace.open()), rows(&cold));
}

#[cfg(feature = "lang-typescript")]
#[test]
fn same_line_javascript_islands_have_distinct_linked_symbol_identities() {
    let source = "<script type='module'>function same() {} same();</script><script type='module'>function same() {} same();</script><button onclick='function same() {} same();'>Run</button>";
    let graph = ExtractorHarness::for_lang("html")
        .at_path("same-line.html")
        .linked(source);
    let facts = &graph.facts;
    let functions: std::collections::BTreeSet<_> = facts
        .nodes
        .iter()
        .filter(|node| node.kind == "function" && node.name == "same")
        .map(|node| node.id.clone())
        .collect();
    assert_eq!(functions.len(), 3);
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
    let graph = ExtractorHarness::for_lang("html")
        .at_path("classic.html")
        .linked(source);
    let facts = &graph.facts;
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

#[test]
fn embedded_javascript_uses_its_owner_profile_inside_html() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    let source = "<div>{{ encodeURIComponent }}</div>\n<script>\nfunction make() {\n  return { count: 0, inc() { this.count += 1; this.reset(); }, reset() { this.count = 0; } };\n}\nfunction render() { encodeURIComponent('x'); document.querySelector('#card'); return Date.now(); }\n</script>\n";
    let path = "templates/island.html";
    let facts = ExtractorHarness::for_lang("html")
        .at_path(path)
        .extract(source);
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
    workspace.build();
    let conn = workspace.open();
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
    let field_edge: i64 = query_scalar(
        &conn,
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash JOIN path_dictionary target_path ON target_path.path_id=dst.path_id WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.line=4 AND e.kind='mutates' AND target_path.path=?1 AND dst.name='count' AND dst.kind='field'",
        [path],
    );
    assert!(
        field_edge > 0,
        "this.count must link to its returned object field"
    );
}
