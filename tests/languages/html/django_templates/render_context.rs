use super::*;

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
