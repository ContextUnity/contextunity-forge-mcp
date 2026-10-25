use super::*;

#[cfg(feature = "lang-html")]
#[test]
fn framework_manifest_accepts_generic_typed_rules_and_formats() {
    let mut failures = Vec::new();
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("skills/contextunity-forge/references/frameworks");
    for (name, extension) in [
        ("django", "toml"),
        ("starlette", "toml"),
        ("fastapi", "toml"),
        ("nuxt", "toml"),
        ("vue-runtime", "toml"),
        ("htmx", "toml"),
        ("alpinejs", "toml"),
        ("gridviewspec", "toml"),
        ("declarative-rules", "toml"),
        ("declarative-rules", "yaml"),
    ] {
        let path = examples.join(format!("{name}.{extension}.example"));
        let example = match std::fs::read_to_string(&path) {
            Ok(example) => example,
            Err(error) => {
                failures.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        let workspace = Workspace::new();
        workspace.write("index.html", "<p>Example manifest</p>");
        let identity = if name == "declarative-rules" {
            "project-rules"
        } else {
            name
        };
        workspace.write(
            format!(".forge/frameworks/{identity}.{extension}"),
            &example,
        );
        if let Err(error) = writer::build(&workspace.0, &workspace.db(), None) {
            failures.push(format!(
                "{} failed production loading: {error:#}",
                path.display()
            ));
        }
    }
    for (language, operation) in [
        (
            "python",
            serde_json::json!({"kind":"named_binding","domain":"paint","binding":{"kind":"call_reference","callee":{"kind":"imported","module":"tablekit","export":"Column"},"key":{"kind":"keyword","name":"renderer"},"provider_scopes":[{"workspace":"","project":"../frontend","package":"runtime"}]}}),
        ),
        (
            "python",
            serde_json::json!({"kind":"named_binding","domain":"paint","binding":{"kind":"call_reference","callee":{"kind":"imported","module":"tablekit","export":"Column"},"key":{"kind":"keyword","name":"renderer"},"provider_scopes":[{"workspace":"unknown","project":"frontend","package":"runtime"}]}}),
        ),
        (
            "python",
            serde_json::json!({"kind":"named_binding","domain":"filters","binding":{"kind":"assignment_definition","receiver_type":{"module":"jinja2","export":"Environment","extra":true},"member":"filters","template_root":"templates"}}),
        ),
        (
            "python",
            serde_json::json!({"kind":"named_binding","domain":"filters","binding":{"kind":"assignment_definition","receiver_type":{"module":"jinja2","export":"Environment"},"member":"filters","template_root":"../templates"}}),
        ),
        (
            "html",
            serde_json::json!({"kind":"named_binding","domain":"filters","binding":{"kind":"template_reference","symbol_kind":"tag"}}),
        ),
        (
            "python",
            serde_json::json!({"kind":"http_client_call","callee":"fetch","receivers":[],"methods":{},"default_method":"GET","url_key":"url","method_key":"method","relative_prefixes":["/"],"target_context":{"request_owners":[],"route_owners":["app.py"]}}),
        ),
        (
            "python",
            serde_json::json!({"kind":"http_client_call","callee":"fetch","receivers":[],"methods":{},"default_method":"GET","url_key":"url","method_key":"method","relative_prefixes":["/"],"target_context":{"request_owners":["test.py"],"route_owners":["../app.py"]}}),
        ),
        (
            "python",
            serde_json::json!({"kind":"http_client_call","callee":"fetch","receivers":[],"methods":{},"default_method":"GET","url_key":"url","method_key":"method","relative_prefixes":["/"],"target_context":{"request_owners":["test.py"],"route_owners":["app.py","app.py"]}}),
        ),
    ] {
        let pack = serde_json::json!({"syntax_rules":[{"id":"strict","language":language,"operation":operation}]});
        if assert_manifest_build_rejected(
            ".forge/frameworks/strict.toml",
            &toml::to_string(&pack).unwrap(),
            &[("index.html", "<p>Strict residual schema</p>")],
        )
        .is_err()
        {
            failures.push(format!(
                "malformed residual descriptor accepted/published: {pack}"
            ));
        }
    }
    let complete_toml = r#"replace = true
[[syntax_rules]]
id = "template-symbols"
language = "html"
activation = [{ kind = "package", name = "django" }]
operation = { kind = "template_symbols", requires_loader = false, tags = ["csrf_token"], filters = ["safe"], libraries = [], library_routes = { static = "static" } }
"#;
    let complete_yaml = r#"replace: true
syntax_rules:
  - id: template-symbols
    language: html
    activation:
      - kind: package
        name: django
    operation:
      kind: template_symbols
      requires_loader: false
      tags: [csrf_token]
      filters: [safe]
      libraries: []
      library_routes:
        static: static
"#;

    for (extension, complete_manifest) in [
        ("toml", complete_toml),
        ("yaml", complete_yaml),
        ("yml", complete_yaml),
    ] {
        let workspace = Workspace::new();
        workspace.write(
            "pyproject.toml",
            "[project]\nname = 'sample'\ndependencies = ['django>=5']\n",
        );
        workspace.write("index.html", "{% csrf_token %}{{ value|safe }}\n");
        workspace.write(
            format!(".forge/frameworks/django.{extension}"),
            complete_manifest,
        );
        if let Err(error) = writer::build(&workspace.0, &workspace.db(), None) {
            failures.push(format!(
                "valid .{extension} generic manifest failed to build: {error:#}"
            ));
            continue;
        }

        let dependencies = DependencyRegistry::collect_with_scan_config(&workspace.0, None);
        if dependencies
            .framework_manifest_for_path(LanguageFamily("python"), "index.html", "django", "django")
            .is_none()
        {
            failures.push(format!(
                "generic .{extension} manifest did not reach its linker"
            ));
        }
        let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
        let indexed_file = query_count(
            &conn,
            "SELECT count(*) FROM path_dictionary WHERE path='index.html'",
            [],
        );
        if indexed_file != 1 {
            failures.push(format!(
                "generic .{extension} manifest published {indexed_file} index rows"
            ));
        }
    }

    for extension in ["toml", "yaml", "yml"] {
        for legacy_section in ["receivers", "builtins", "filters", "routes"] {
            let manifest = if extension == "toml" {
                format!("{legacy_section} = []\n")
            } else {
                format!("{legacy_section}: []\n")
            };
            if assert_manifest_build_rejected(
                &format!(".forge/frameworks/django.{extension}"),
                &manifest,
                &[
                    (
                        "pyproject.toml",
                        "[project]\nname = 'sample'\ndependencies = ['django>=5']\n",
                    ),
                    ("index.html", "<p>Legacy manifest section</p>\n"),
                ],
            )
            .is_err()
            {
                failures.push(format!(
                    ".{extension} legacy section {legacy_section} was accepted or published index"
                ));
            }
        }
    }

    let empty = Workspace::new();
    empty.write("index.html", "<p>Empty typed rule manifest</p>\n");
    empty.write(".forge/frameworks/django.toml", "enabled = false\n");
    if let Err(error) = writer::build(&empty.0, &empty.db(), None) {
        failures.push(format!("empty disabled manifest was rejected: {error:#}"));
    }

    let undeclared = Workspace::new();
    undeclared.write("index.html", "<p>No framework declaration</p>\n");
    undeclared.write(".forge/frameworks/django.toml", complete_toml);
    let undeclared_dependencies = DependencyRegistry::collect_with_scan_config(&undeclared.0, None);
    if undeclared_dependencies
        .framework_manifest_for_path(LanguageFamily("python"), "index.html", "django", "django")
        .is_none()
    {
        failures
            .push("an undeclared project did not activate its local Django manifest".to_owned());
    }
    assert!(
        failures.is_empty(),
        "manifest contract failures:\n{}",
        failures.join("\n")
    );
}
#[cfg(feature = "lang-html")]
#[test]
fn invalid_user_framework_manifest_fails_before_index_publication() {
    let complete = "";
    let mut failures = Vec::new();
    let invalid_manifests = [
        ("nonboolean enabled", "toml", format!("enabled = 'false'\n{complete}")),
        ("nonboolean enabled", "yaml", "enabled: 'false'\n".to_owned()),
        ("nonboolean enabled", "yml", "enabled: 0\n".to_owned()),
        ("removed legacy section", "toml", "receivers = []\n".to_owned()),
        ("removed legacy section", "yaml", "builtins: []\n".to_owned()),
        ("wrong syntax rule shape", "toml", "syntax_rules = {}\n".to_owned()),
        ("conflicting modes", "toml", "extend = true\nreplace = true\n".to_owned()),
        ("wrong mode shape", "toml", "extend = 'true'\n".to_owned()),
        ("malformed TOML syntax", "toml", "syntax_rules = [\n".to_owned()),
        (
            "duplicate table headers",
            "toml",
            format!("{complete}[unused]\nfirst = 1\n[unused]\nsecond = 2\n"),
        ),
        (
            "duplicate keys",
            "toml",
            format!("{complete}[unused]\nfirst = 1\nfirst = 2\n"),
        ),
        (
            "removed legacy section redeclared as a table",
            "toml",
            format!("{complete}[receivers]\nname = 'duplicate-shape'\n"),
        ),
        (
            "nested table before dotted key",
            "toml",
            format!(
                "{complete}[unused.receivers]\nname = 'declared-as-table'\n[unused]\nreceivers = []\n"
            ),
        ),
        (
            "dotted key before nested table",
            "toml",
            format!(
                "{complete}[unused]\nreceivers = []\n[unused.receivers]\nname = 'declared-as-table'\n"
            ),
        ),
        (
            "malformed YAML syntax",
            "yaml",
            "syntax_rules: [\n".to_owned(),
        ),
        (
            "malformed YAML syntax",
            "yml",
            "syntax_rules: [\n".to_owned(),
        ),
    ];

    for (invalid_kind, extension, manifest) in invalid_manifests {
        if assert_manifest_build_rejected(
            &format!(".forge/frameworks/django.{extension}"),
            &manifest,
            &[
                ("pyproject.toml", "[project]\nname = 'sample'\ndependencies = ['django>=5']\n"),
                ("shop/views.py", "from django.shortcuts import render\ndef page(request):\n    return render(request, 'templates/page.html', {'title': 'Welcome'})\n"),
                ("shop/templates/page.html", "<h1>{{ title }}</h1>\n"),
            ],
        )
        .is_err()
        {
            failures.push(format!("a declared Django project with {invalid_kind} in its .{extension} manifest was accepted or published index"));
        }
    }
    assert!(
        failures.is_empty(),
        "invalid manifest contract failures:\n{}",
        failures.join("\n")
    );
}
