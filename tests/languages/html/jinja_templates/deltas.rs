use super::*;

#[test]
fn unrelated_html_delta_does_not_hydrate_jinja_loader_providers() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
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
    workspace.build();
    workspace.write("shop/unrelated.html", "<p>After</p>\n");
    let delta = workspace.delta(&["shop/unrelated.html"]);
    assert_eq!(
        delta["loaded_fact_files"], 0,
        "unrelated HTML must not load Jinja providers: {delta}"
    );
    let conn = workspace.open();
    commitments::verify(&conn).unwrap();
}

#[test]
fn html_delta_does_not_reinsert_render_dependencies_for_context_only_loader_facts() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
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
    workspace.build();

    let render_dependencies = |conn: &rusqlite::Connection| {
        conn.query_row(
            "SELECT count(*) FROM dependencies d JOIN path_dictionary p ON p.path_id=d.owner_id WHERE p.path='shop/views.py' AND d.kind='renders'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
    };
    let before = workspace.open();
    let before_count = render_dependencies(&before);
    assert!(
        before_count > 0,
        "the fixture must persist a render dependency"
    );
    drop(before);

    workspace.write("shop/templates/page.html", "<p>After</p>\n");
    workspace.delta(&["shop/templates/page.html"]);

    let delta = workspace.open();
    assert_eq!(render_dependencies(&delta), before_count);
    commitments::verify(&delta).unwrap();
}
