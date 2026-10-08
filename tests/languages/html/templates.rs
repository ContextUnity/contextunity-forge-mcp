use super::*;

#[test]
fn alpine_magic_members_require_declared_alpine_dependency() {
    let workspace = Workspace::new();
    let script = "<script>const refs = this.$refs; this.$dispatch('shown');</script>\n";
    workspace.write(
        "declared/package.json",
        r#"{"dependencies":{"alpinejs":"^3.14.0"}}"#,
    );
    workspace.write("declared/index.html", script);
    workspace.write("plain/package.json", r#"{"dependencies":{"vue":"^3.5.0"}}"#);
    workspace.write("plain/index.html", script);

    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let observations = [
        ("declared/index.html", "this.$refs", "external"),
        ("declared/index.html", "this.$dispatch", "external"),
        ("plain/index.html", "this.$refs", "unresolved"),
        ("plain/index.html", "this.$dispatch", "unresolved"),
    ]
    .map(|(path, expression, expected)| {
        let statuses: Vec<(String, String)> = conn
            .prepare("SELECT rc.status,ev.evidence FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id JOIN coverage_evidence ev ON ev.evidence_id=rc.evidence_id WHERE pd.path=?1 AND ce.expression=?2 ORDER BY rc.status,ev.evidence")
            .unwrap()
            .query_map(rusqlite::params![path, expression], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        ((path, expression), (expected, statuses))
    });
    assert!(
        observations
            .iter()
            .all(|(_, (expected, statuses))| {
                statuses.len() == 1 && statuses[0].0.as_str() == *expected
            }),
        "Alpine magic must be external only in the package that declares alpinejs: {observations:#?}"
    );
    commitments::verify(&conn).unwrap();
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
            format!("{project}/pyproject.toml"),
            "[project]\nname = 'sample'\ndependencies = ['django>=5']\n",
        );
        workspace.write(format!("{project}/shop/templatetags/pricing.py"), library);
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
            format!("ambiguous/{package}/templatetags/pricing.py"),
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
            format!("{project}/templates/macros/forms.html"),
            "{% macro field(label) %}<label>{{ label }}</label>{% endmacro %}\n",
        );
        workspace.write(
            format!("{project}/templates/page.html"),
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
    let rooted_source = format!("{source}{{% include 'partials/card.html' %}}\n");
    workspace.write("shop/site/templates/page.html", &rooted_source);
    workspace.write("shop/site/other/templates/page.html", source);
    workspace.write(
        "shop/site/templates/partials/card.html",
        "<article>{{ row.name }}</article>\n",
    );
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
    let include_targets = |conn: &rusqlite::Connection, path: &str| -> Vec<String> {
        conn.prepare("SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='includes' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)")
            .unwrap()
            .query_map([path], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let expected_include = ["shop/site/templates/partials/card.html"];
    assert_eq!(
        include_targets(&conn, "shop/site/templates/page.html"),
        expected_include,
        "the declared FileSystemLoader keeps the exact Jinja include target"
    );
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
        &format!("{rooted_source}<p>Changed</p>\n"),
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
    assert_eq!(
        include_targets(&delta, "shop/site/templates/page.html"),
        expected_include,
        "the include edge survives incremental persistence"
    );
    assert_eq!(
        include_targets(&delta, "shop/site/templates/page.html"),
        include_targets(&cold, "shop/site/templates/page.html")
    );
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
            format!("{project}/pyproject.toml"),
            &format!("[project]\nname = '{project}'\ndependencies = ['jinja2>=3']\n"),
        );
        workspace.write(format!("{project}/templates/page.html"), source);
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
            format!("{project}/pyproject.toml"),
            &format!("[project]\nname = '{project}'\ndependencies = ['jinja2>=3']\n"),
        );
        workspace.write(
            format!("{project}/src/pkg/templates/spec/page.html"),
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
            format!("{project}/pyproject.toml"),
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

#[test]
fn shared_html_template_rules_stay_external_without_framework_declarations() {
    let workspace = Workspace::new();
    workspace.write(
        "page.html",
        r#"{% if active %}
{% for row in rows %}
{{ "value"|safe|urlencode }}
{% endfor %}
{% else %}
{% csrf_token %}
{% endif %}
"#,
    );
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let status = |expression: &str| -> String {
        conn.query_row(
            "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path='page.html' AND ce.expression=?1",
            [expression],
            |row| row.get(0),
        )
        .unwrap()
    };
    let shared_expressions = [
        "template.tag.if",
        "template.tag.for",
        "template.filter.safe",
        "template.filter.urlencode",
        "template.tag.endfor",
        "template.tag.else",
        "template.tag.endif",
    ];
    for expression in shared_expressions {
        assert_eq!(
            status(expression),
            "external",
            "shared template rule {expression} must remain profile-owned without a framework package"
        );
    }
    assert_eq!(
        status("template.tag.csrf_token"),
        "unresolved",
        "Django-only tags remain gated by a declared Django package"
    );
    let (external, unresolved): (i64, i64) = conn
        .query_row(
            "SELECT SUM(status='external'), SUM(status='unresolved') FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id WHERE pd.path='page.html' AND rc.expression_id IN (SELECT expression_id FROM coverage_expressions WHERE expression IN ('template.tag.if','template.tag.for','template.filter.safe','template.filter.urlencode','template.tag.endfor','template.tag.else','template.tag.endif'))",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((external, unresolved), (7, 0));
}
