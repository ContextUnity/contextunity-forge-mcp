use super::*;

#[test]
fn html_django_template_preprocessor_avoids_syntax_errors_and_extracts_links() {
    let source = r#"
{% extends "base.html" %}
{% load static %}
{% block content %}
<div id="container-{{ item.id }}" class="card">
    <h1>{{ item.title }}</h1>
    {% if item.is_active %}
        <p>Active</p>
    {% endif %}
    {% include "partials/sidebar.html" with extra="val" %}
</div>
<script>
    let currentId = {{ item.id }};
    {% if item.is_active %}
    console.log("active: " + currentId);
    {% endif %}
</script>
{% endblock %}
"#;
    let facts = ast::extract("templates/item.html", "html", source).unwrap();
    assert_eq!(
        facts.errors.len(),
        0,
        "Django template must not produce syntax errors: {:?}",
        facts.errors
    );
    let includes: Vec<_> = facts
        .references
        .iter()
        .filter(|r| r.kind == "includes")
        .collect();
    assert_eq!(includes.len(), 1);
    assert_eq!(includes[0].expression, "partials/sidebar.html");

    let extends: Vec<_> = facts
        .references
        .iter()
        .filter(|r| r.kind == "extends")
        .collect();
    assert_eq!(extends.len(), 1);
    assert_eq!(extends[0].expression, "base.html");
}

#[test]
fn html_truncated_template_tags_do_not_panic() {
    for snippet in [
        "{%",
        "{% ",
        "{% include",
        "{#",
        "{# unclosed comment",
        "{{",
        "{{ unclosed var",
    ] {
        let facts = ast::extract("truncated.html", "html", snippet);
        assert!(facts.is_ok(), "must not panic on: {snippet}");
    }
}

#[test]
fn html_template_file_still_reports_syntax_error_in_plain_script() {
    let source = r#"
<div>{{ item.title }}</div>
<script>
    function broken( {
</script>
"#;
    let facts = ast::extract("template_with_broken_js.html", "html", source).unwrap();
    assert!(
        facts
            .errors
            .iter()
            .any(|e| e.message.contains("embedded JavaScript")),
        "plain script in template file must still report real JS syntax errors: {:?}",
        facts.errors
    );
}

#[test]
fn unicode_template_tags_keep_embedded_javascript_byte_coordinates() {
    let source = "<script>{% if λ %}function afterMask() { return 1; }{% endif %}</script>";
    let facts = ast::extract("template.html", "html", source).unwrap();
    let function = facts
        .nodes
        .iter()
        .find(|node| node.name == "afterMask")
        .expect("function following a masked Unicode tag is indexed");
    assert_eq!(function.line, 1);
    assert_eq!(
        function.details["column"].as_u64(),
        Some(source.find("function afterMask").unwrap() as u64)
    );
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_mask_keeps_interpolation_references_and_script_coordinates() {
    let source = "<template><p>{{ user.name }}</p></template>\n<script setup>function afterVue() {}</script>";
    let facts = ast::extract("component.vue", "vue", source).unwrap();
    assert!(facts
        .references
        .iter()
        .any(|reference| reference.expression == "user.name"));
    let function = facts
        .nodes
        .iter()
        .find(|node| node.name == "afterVue")
        .expect("script function remains indexed after template masking");
    assert_eq!(function.line, 2);
    assert_eq!(
        function.details["column"].as_u64(),
        Some(
            source
                .lines()
                .nth(1)
                .unwrap()
                .find("function afterVue")
                .unwrap() as u64
        )
    );
}
