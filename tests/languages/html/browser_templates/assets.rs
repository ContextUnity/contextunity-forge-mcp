use super::*;

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
