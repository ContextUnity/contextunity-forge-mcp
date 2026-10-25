use super::*;

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
