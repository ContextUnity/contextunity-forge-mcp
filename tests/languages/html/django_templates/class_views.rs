use super::*;

#[test]
fn django_cbv_attributes_link_only_unique_templates_and_models() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname='django-cbv'\ndependencies=['django']\n",
    );
    workspace.write("app/models.py", "from django.db import models\nclass Product(models.Model):\n    pass\nclass Duplicate(models.Model):\n    pass\n");
    workspace.write(
        "other/models.py",
        "from django.db import models\nclass Duplicate(models.Model):\n    pass\n",
    );
    let views_source = "from app.models import Product, Missing, Duplicate\nfrom other.models import Duplicate\n\nclass ProductView:\n    template_name = 'products/detail.html'\n    model = Product\n\nclass MissingTemplateView:\n    template_name = 'missing.html'\n    model = Product\n\nclass DuplicateTemplateView:\n    template_name = 'products/duplicate.html'\n    model = Product\n\nclass MissingModelView:\n    template_name = 'products/detail.html'\n    model = Missing\n\nclass DuplicateModelView:\n    template_name = 'products/detail.html'\n    model = Duplicate\n\nclass DynamicView:\n    template_name = choose_template()\n    model = choose_model()\n";
    workspace.write("app/views.py", views_source);
    workspace.write("app/templates/products/detail.html", "<p>Detail</p>\n");
    workspace.write("app/templates/products/duplicate.html", "<p>First</p>\n");
    workspace.write("other/templates/products/duplicate.html", "<p>Second</p>\n");

    let conn = workspace.build_and_open();
    let validity = validity_observations(&conn);
    validity.assert_valid();
    let product_view = node_id_observation(&conn, "app/views.py", "ProductView")
        .expect("unique ProductView definition");
    let missing_model_view = node_id_observation(&conn, "app/views.py", "MissingModelView")
        .expect("unique MissingModelView definition");
    let duplicate_template_view =
        node_id_observation(&conn, "app/views.py", "DuplicateTemplateView")
            .expect("unique DuplicateTemplateView definition");
    let duplicate_model_view = node_id_observation(&conn, "app/views.py", "DuplicateModelView")
        .expect("unique DuplicateModelView definition");
    let product_model = node_id_observation(&conn, "app/models.py", "Product")
        .expect("unique Product model definition");
    let route_observations = html_coverage_observations(&conn);

    assert_eq!(
        edge_occurrence_rows(&conn, "app/views.py", "renders_template"),
        [
            (
                product_view.clone(),
                "module:app/templates/products/detail.html".into(),
                5
            ),
            (
                missing_model_view.clone(),
                "module:app/templates/products/detail.html".into(),
                17
            ),
            (
                duplicate_model_view.clone(),
                "module:app/templates/products/detail.html".into(),
                21
            ),
        ]
    );
    assert_eq!(
        edge_occurrence_rows(&conn, "app/views.py", "uses_model"),
        [
            (product_view.clone(), product_model.clone(), 6),
            (
                node_id_observation(&conn, "app/views.py", "MissingTemplateView")
                    .expect("unique MissingTemplateView definition"),
                product_model.clone(),
                10
            ),
            (duplicate_template_view, product_model, 14),
        ]
    );
    for (line, expression, status) in [
        (9, "'missing.html'", "unresolved"),
        (13, "'products/duplicate.html'", "ambiguous"),
        (18, "Missing", "unresolved"),
        (22, "Duplicate", "ambiguous"),
    ] {
        let actual: Vec<_> = route_observations
            .iter()
            .filter(|coverage| {
                coverage.path == "app/views.py"
                    && coverage.line == line
                    && coverage.expression == expression
            })
            .map(|coverage| coverage.status.as_str())
            .collect();
        assert_eq!(actual, [status], "{expression}");
    }
    for (line, expression) in [(25, "choose_template()"), (26, "choose_model()")] {
        let statuses: Vec<_> = route_observations
            .iter()
            .filter(|coverage| {
                coverage.path == "app/views.py"
                    && coverage.line == line
                    && coverage.expression == expression
            })
            .map(|coverage| coverage.status.as_str())
            .collect();
        assert_eq!(statuses, ["unresolved"], "{expression}");
    }
    assert!(
        edge_occurrence_rows(&conn, "app/views.py", "uses_model")
            .iter()
            .all(|(source, _, _)| source != &duplicate_model_view),
        "ambiguous duplicate model declarations must not produce an edge"
    );
}
