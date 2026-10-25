use super::*;

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
