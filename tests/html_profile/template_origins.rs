use super::Workspace;
use contextunity_forge_mcp::engine::{ast, linker};
use std::collections::BTreeMap;

#[test]
fn django_framework_templates_require_scoped_dependencies_and_yield_to_local_files() {
    let workspace = Workspace::new();
    workspace.write(
        "commerce/pyproject.toml",
        "[project]\nname = 'commerce'\ndependencies = ['django>=5.0.0']\n",
    );
    workspace.write(
        "other/pyproject.toml",
        "[project]\nname = 'other'\ndependencies = ['requests>=2.0']\n",
    );
    workspace.write(
        "commerce/isolated/pyproject.toml",
        "[project]\nname = 'isolated'\ndependencies = ['requests>=2.0']\n",
    );

    let django_path = "commerce/templates/references.html";
    let django_source = r#"{% extends "admin/change_form.html" %}
{% include "admin/base_site.html" %}
{% include "django/forms/widgets/input.html" %}
{% include "django/forms/widgets/textarea.html" %}
{% include "custom/missing.html" %}"#;
    let other_path = "other/templates/references.html";
    let other_source = r#"{% include "admin/change_form.html" %}"#;
    let isolated_path = "commerce/isolated/templates/references.html";
    let isolated_source = r#"{% include "admin/change_form.html" %}"#;
    let mut facts = BTreeMap::from([
        (
            django_path.to_owned(),
            ast::extract(django_path, "html", django_source).unwrap(),
        ),
        (
            other_path.to_owned(),
            ast::extract(other_path, "html", other_source).unwrap(),
        ),
        (
            isolated_path.to_owned(),
            ast::extract(isolated_path, "html", isolated_source).unwrap(),
        ),
    ]);
    let graph = linker::link_with_root(&facts, None, Some(&workspace.0));

    for target in [
        "admin/change_form.html",
        "admin/base_site.html",
        "django/forms/widgets/input.html",
        "django/forms/widgets/textarea.html",
    ] {
        assert!(
            graph.coverage.iter().any(|coverage| {
                coverage.path == django_path
                    && coverage.expression == target
                    && coverage.status == "external"
                    && coverage.evidence
                        == format!(
                            "framework built-in template from django: {target}; target unindexed"
                        )
            }),
            "{target}: {graph:#?}"
        );
    }
    assert!(
        graph.coverage.iter().any(|coverage| {
            coverage.path == django_path
                && coverage.expression == "custom/missing.html"
                && coverage.status == "unresolved"
        }),
        "{graph:#?}"
    );
    assert!(
        graph.coverage.iter().any(|coverage| {
            coverage.path == other_path
                && coverage.expression == "admin/change_form.html"
                && coverage.status == "unresolved"
        }),
        "{graph:#?}"
    );
    assert!(
        graph.coverage.iter().any(|coverage| {
            coverage.path == isolated_path
                && coverage.expression == "admin/change_form.html"
                && coverage.status == "unresolved"
        }),
        "{graph:#?}"
    );

    let local_path = "commerce/templates/admin/change_form.html";
    facts.insert(
        local_path.to_owned(),
        ast::extract(local_path, "html", "<form></form>").unwrap(),
    );
    let local_graph = linker::link_with_root(&facts, None, Some(&workspace.0));
    let local_module = facts[local_path]
        .nodes
        .iter()
        .find(|node| node.kind == "module")
        .unwrap();
    assert!(
        local_graph.edges.iter().any(|edge| {
            edge.path == django_path
                && edge.kind == "extends"
                && edge.evidence == format!("{django_path}: extends {local_path}")
                && edge.dst == local_module.id
        }),
        "{local_graph:#?}"
    );
    assert!(
        local_graph.coverage.iter().any(|coverage| {
            coverage.path == django_path
                && coverage.expression == "admin/change_form.html"
                && coverage.status == "resolved"
                && coverage.evidence == format!("template target: {local_path}")
        }),
        "{local_graph:#?}"
    );
}
