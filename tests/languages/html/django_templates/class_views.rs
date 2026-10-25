use super::*;

#[test]
fn django_cbv_attributes_link_only_unique_templates_and_models() {
    let workspace = Workspace::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname='django-cbv'\ndependencies=['django']\n",
    );
    workspace.write(
        "app/models.py",
        "from django.db import models\nclass Product(models.Model):\n    pass\nclass Duplicate(models.Model):\n    pass\n",
    );
    workspace.write(
        "other/models.py",
        "from django.db import models\nclass Duplicate(models.Model):\n    pass\n",
    );
    let views_source = "from app.models import Product, Missing, Duplicate\nfrom other.models import Duplicate\n\nclass ProductView:\n    template_name = 'products/detail.html'\n    model = Product\n\nclass MissingTemplateView:\n    template_name = 'missing.html'\n    model = Product\n\nclass DuplicateTemplateView:\n    template_name = 'products/duplicate.html'\n    model = Product\n\nclass MissingModelView:\n    template_name = 'products/detail.html'\n    model = Missing\n\nclass DuplicateModelView:\n    template_name = 'products/detail.html'\n    model = Duplicate\n\nclass DynamicView:\n    template_name = choose_template()\n    model = choose_model()\n";
    workspace.write("app/views.py", views_source);
    workspace.write("app/templates/products/detail.html", "<p>Detail</p>\n");
    workspace.write("app/templates/products/duplicate.html", "<p>First</p>\n");
    workspace.write("other/templates/products/duplicate.html", "<p>Second</p>\n");

    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = workspace.open();
    assert_valid(&conn);

    assert_eq!(
        edge_occurrence_rows(&conn, "app/views.py", "renders_template"),
        [
            (
                node_id(&conn, "app/views.py", "ProductView"),
                "module:app/templates/products/detail.html".into(),
                5
            ),
            (
                node_id(&conn, "app/views.py", "MissingModelView"),
                "module:app/templates/products/detail.html".into(),
                17
            ),
            (
                node_id(&conn, "app/views.py", "DuplicateModelView"),
                "module:app/templates/products/detail.html".into(),
                21
            ),
        ]
    );
    assert_eq!(
        edge_occurrence_rows(&conn, "app/views.py", "uses_model"),
        [
            (
                node_id(&conn, "app/views.py", "ProductView"),
                node_id(&conn, "app/models.py", "Product"),
                6
            ),
            (
                node_id(&conn, "app/views.py", "MissingTemplateView"),
                node_id(&conn, "app/models.py", "Product"),
                10
            ),
            (
                node_id(&conn, "app/views.py", "DuplicateTemplateView"),
                node_id(&conn, "app/models.py", "Product"),
                14
            ),
        ]
    );
    for (line, expression, status) in [
        (9, "'missing.html'", "unresolved"),
        (13, "'products/duplicate.html'", "ambiguous"),
        (18, "Missing", "unresolved"),
        (22, "Duplicate", "ambiguous"),
    ] {
        let actual: String = conn
            .query_row(
                "SELECT status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='app/views.py' AND c.line=?1 AND x.expression=?2",
                params![line, expression],
                |row| row.get(0),
            )
            .unwrap_or_else(|error| panic!("missing coverage at app/views.py:{line} for {expression}: {error}"));
        assert_eq!(actual, status, "{expression}");
    }
    for (line, expression) in [(25, "choose_template()"), (26, "choose_model()")] {
        let status: String = conn
            .query_row(
                "SELECT status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='app/views.py' AND c.line=?1 AND x.expression=?2",
                params![line, expression],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "unresolved", "{expression}");
    }
    assert!(
        edge_occurrence_rows(&conn, "app/views.py", "uses_model")
            .iter()
            .all(|(source, _, _)| source != &node_id(&conn, "app/views.py", "DuplicateModelView")),
        "ambiguous duplicate model declarations must not produce an edge"
    );
}
