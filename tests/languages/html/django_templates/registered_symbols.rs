use super::*;

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
    // Registry assignments and decorator libraries share the persisted template
    // seam, while retaining distinct admission and scope proof.
    for renamed in [false, true] {
        let registry = Workspace::new();
        let keys = if renamed {
            ["payload_key", "decimal_key", "short_key"]
        } else {
            ["json_script", "floatformat", "truncatechars"]
        };
        let manifest = r#"enabled = true
extend = true
[[syntax_rules]]
id = "registry"
language = "python"
activation = [{kind="package",name="jinja2"}]
operation = {kind="named_binding",domain="local_filters",binding={kind="assignment_definition",receiver_type={module="jinja2",export="Environment"},member="filters",template_root="templates"}}
[[syntax_rules]]
id = "uses"
language = "html"
package_family = "python"
activation = [{kind="package",name="jinja2"}]
operation = {kind="named_binding",domain="local_filters",binding={kind="template_reference",symbol_kind="filter"}}
"#;
        registry.write(".forge/frameworks/local-filters.toml", manifest);
        registry.write(
            "app/pyproject.toml",
            "[project]\nname='app'\ndependencies=['jinja2']\n",
        );
        registry.write("app/site/filters.py", "def json_script(value):\n    return value\ndef floatformat(value):\n    return value\ndef truncatechars(value):\n    return value\n");
        let provider = format!("from jinja2 import Environment as Env\nfrom .filters import json_script, floatformat, truncatechars\ndef install(env: Env):\n    env.filters['{}'] = json_script\n    env.filters['{}'] = floatformat\n    env.filters['{}'] = truncatechars\n", keys[0], keys[1], keys[2]);
        registry.write("app/site/jinja_env.py", &provider);
        let template = format!("{{{{ value|{} }}}}\n{{{{ value|{} }}}}\n{{{{ value|{} }}}}\n{{{{ value|{} }}}}\n{{{{ value|{} }}}}\n{{{{ value|{} }}}}\n{{{{ value|{} }}}}\n{{{{ value|safe }}}}\n", keys[0], keys[0], keys[1], keys[2], keys[2], keys[2], keys[2]);
        registry.write("app/site/templates/page.html", &template);
        registry.write("app/outside/page.html", &template);
        registry.write(
            "app/site/templates/nested/pyproject.toml",
            "[project]\nname='nested'\ndependencies=['jinja2']\n",
        );
        registry.write("app/site/templates/nested/page.html", &template);
        for (project, body) in [
            ("missing", "    env.filters['json_script'] = absent\n"),
            ("duplicate", "    env.filters['json_script'] = json_script\n    env.filters['json_script'] = floatformat\n"),
            ("conditional", "    if enabled:\n        env.filters['json_script'] = json_script\n"),
            ("dynamic", "    env.filters[key] = json_script\n"),
            ("rebound", "    env = object()\n    env.filters['json_script'] = json_script\n"),
            ("wrongtype", "    env.filters['json_script'] = json_script\n"),
            ("wrongexport", "    env.filters['json_script'] = json_script\n"),
            ("delete", "    env.filters['json_script'] = json_script\n    del env.filters['json_script']\n"),
            ("replace", "    env.filters['json_script'] = json_script\n    env.filters = {}\n"),
            ("unknownwrite", "    env.filters['json_script'] = json_script\n    env.filters[key] = floatformat\n"),
            ("augmented", "    env.filters['json_script'] = json_script\n    env.filters['json_script'] += floatformat\n"),
            ("aliasdelete", "    alias = env\n    env.filters['json_script'] = json_script\n    del alias.filters['json_script']\n"),
            ("targetwrite", "    json_script = object()\n    env.filters['json_script'] = json_script\n"),
        ] {
            registry.write(format!("{project}/pyproject.toml"), "[project]\nname='negative'\ndependencies=['jinja2']\n");
            registry.write(format!("{project}/site/filters.py"), "def json_script(value):\n    return value\ndef floatformat(value):\n    return value\n");
            let annotation = match project {
                "wrongtype" => "object",
                "wrongexport" => "Other",
                _ => "Env",
            };
            let imported = if project == "wrongexport" { "Environment as Env, FileSystemLoader as Other" } else { "Environment as Env" };
            registry.write(format!("{project}/site/registry.py"), &format!("from jinja2 import {imported}\nfrom .filters import json_script, floatformat\ndef install(env: {annotation}):\n{body}"));
            registry.write(format!("{project}/site/templates/page.html"), "{{ value|json_script }}\n");
        }
        let readback = |connection: &rusqlite::Connection, active: bool| {
            for (index, key) in [
                keys[0], keys[0], keys[1], keys[2], keys[2], keys[2], keys[2],
            ]
            .iter()
            .enumerate()
            {
                let line = index as i64 + 1;
                let expression = format!("template.filter.{key}");
                let statuses: Vec<String> = connection.prepare("SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary p ON p.path_id=rc.path_id JOIN coverage_expressions e ON e.expression_id=rc.expression_id WHERE p.path='app/site/templates/page.html' AND rc.line=?1 AND e.expression=?2").unwrap().query_map(rusqlite::params![line, expression], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
                assert_eq!(
                    statuses,
                    [if active { "resolved" } else { "unresolved" }],
                    "renamed={renamed} line={line}"
                );
                let (declaration_line, name) = if index < 2 {
                    (1, "json_script")
                } else if index == 2 {
                    (3, "floatformat")
                } else {
                    (5, "truncatechars")
                };
                let target = format!("py:app/site/filters.py:{declaration_line}:{name}");
                let count: i64 = connection.query_row("SELECT count(*) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes t ON t.node_hash=e.dst_hash WHERE p.path='app/site/templates/page.html' AND e.line=?1 AND e.kind='references' AND s.id='module:app/site/templates/page.html' AND t.id=?2", rusqlite::params![line,target], |row| row.get(0)).unwrap();
                assert_eq!(count, i64::from(active), "{target}");
            }
            for (path, expected) in [
                ("app/outside/page.html", "unresolved"),
                ("app/site/templates/nested/page.html", "unresolved"),
                ("missing/site/templates/page.html", "unresolved"),
                (
                    "duplicate/site/templates/page.html",
                    if active { "ambiguous" } else { "unresolved" },
                ),
                ("conditional/site/templates/page.html", "unresolved"),
                ("dynamic/site/templates/page.html", "unresolved"),
                ("rebound/site/templates/page.html", "unresolved"),
                ("wrongtype/site/templates/page.html", "unresolved"),
                ("wrongexport/site/templates/page.html", "unresolved"),
                ("delete/site/templates/page.html", "unresolved"),
                ("replace/site/templates/page.html", "unresolved"),
                ("unknownwrite/site/templates/page.html", "unresolved"),
                ("augmented/site/templates/page.html", "unresolved"),
                ("aliasdelete/site/templates/page.html", "unresolved"),
                ("targetwrite/site/templates/page.html", "unresolved"),
            ] {
                let expression = if path.starts_with("app/") {
                    format!("template.filter.{}", keys[0])
                } else {
                    "template.filter.json_script".into()
                };
                let statuses: Vec<String> = connection.prepare("SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary p ON p.path_id=rc.path_id JOIN coverage_expressions e ON e.expression_id=rc.expression_id WHERE p.path=?1 AND e.expression=?2").unwrap().query_map([path, &expression], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
                assert!(!statuses.is_empty(), "{path}");
                assert!(
                    statuses.iter().all(|status| status == expected),
                    "{path}: {statuses:?}"
                );
            }
            let safe: String = connection.query_row("SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary p ON p.path_id=rc.path_id JOIN coverage_expressions e ON e.expression_id=rc.expression_id WHERE p.path='app/site/templates/page.html' AND e.expression='template.filter.safe'", [], |row| row.get(0)).unwrap();
            assert_eq!(safe, "external");
            commitments::verify(connection).unwrap();
        };
        readback(&registry.build(), true);
        // A changed consumer must hydrate the unchanged registration and target.
        registry.write("app/site/templates/page.html", &format!("{template}\n"));
        registry.delta("app/site/templates/page.html");
        readback(&registry.open(), true);
        registry.assert_cold_equivalent();
        // A provider-only removal must invalidate all cached consumers.
        registry.write("app/site/jinja_env.py", "from jinja2 import Environment as Env\nfrom .filters import json_script, floatformat, truncatechars\ndef install(env: Env):\n    pass\n");
        registry.delta("app/site/jinja_env.py");
        let connection = registry.open();
        let remaining: i64 = connection.query_row("SELECT count(*) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id WHERE p.path='app/site/templates/page.html' AND e.kind='references'", [], |row| row.get(0)).unwrap();
        assert_eq!(remaining, 0);
        drop(connection);
        registry.assert_cold_equivalent();
        registry.write("app/site/jinja_env.py", &provider);
        registry.delta("app/site/jinja_env.py");
        readback(&registry.open(), true);
        registry.assert_cold_equivalent();
        registry.write(
            ".forge/frameworks/local-filters.toml",
            "enabled=true\nreplace=true\n",
        );
        registry.delta(".forge/frameworks/local-filters.toml");
        readback(&registry.open(), false);
        registry.assert_cold_equivalent();
    }
}
