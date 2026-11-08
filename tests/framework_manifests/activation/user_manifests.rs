use super::*;

#[cfg(feature = "lang-html")]
#[test]
fn framework_manifest_accepts_generic_typed_rules_and_formats() {
    let mut failures = Vec::new();
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("skills/contextunity-forge/references/frameworks");
    for (name, extension, example) in [
        ("django","toml",include_str!("../../../skills/contextunity-forge/references/frameworks/django.toml.example")),
        ("starlette","toml",include_str!("../../../skills/contextunity-forge/references/frameworks/starlette.toml.example")),
        ("fastapi","toml",include_str!("../../../skills/contextunity-forge/references/frameworks/fastapi.toml.example")),
        ("nuxt","toml",include_str!("../../../skills/contextunity-forge/references/frameworks/nuxt.toml.example")),
        ("vue-runtime","toml",include_str!("../../../skills/contextunity-forge/references/frameworks/vue-runtime.toml.example")),
        ("htmx","toml",include_str!("../../../skills/contextunity-forge/references/frameworks/htmx.toml.example")),
        ("alpinejs","toml",include_str!("../../../skills/contextunity-forge/references/frameworks/alpinejs.toml.example")),
        ("gridviewspec","toml",include_str!("../../../skills/contextunity-forge/references/frameworks/gridviewspec.toml.example")),
        ("declarative-rules","toml",include_str!("../../../skills/contextunity-forge/references/frameworks/declarative-rules.toml.example")),
        ("declarative-rules","yaml",include_str!("../../../skills/contextunity-forge/references/frameworks/declarative-rules.yaml.example")),
    ] {
        let path = examples.join(format!("{name}.{extension}.example"));
        let identity = if name == "declarative-rules" {
            "project-rules"
        } else {
            name
        };
        let manifest = format!(".forge/frameworks/{identity}.{extension}");
        let result =
            FrameworkHarness::from_manifests(&[(&manifest, example)]).and_then(|harness| {
                harness.link_sources(&[("index.html", "html", "<p>Example manifest</p>")])
            });
        if let Err(error) = result {
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
        let mut sources =
            VirtualWorkspace::from_files(&[("index.html", "<p>Strict residual schema</p>")]);
        sources.write(
            ".forge/frameworks/strict.toml",
            &toml::to_string(&pack).unwrap(),
        );
        if sources.index().is_ok() {
            failures.push(format!(
                "malformed residual descriptor accepted by the in-memory production linker: {pack}"
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
        let mut sources = VirtualWorkspace::from_files(&[
            (
                "pyproject.toml",
                "[project]\nname = 'sample'\ndependencies = ['django>=5']\n",
            ),
            ("index.html", "{% csrf_token %}{{ value|safe }}\n"),
        ]);
        sources.write(
            format!(".forge/frameworks/django.{extension}"),
            complete_manifest,
        );
        let indexed = match sources.index() {
            Ok(indexed) => indexed,
            Err(error) => {
                failures.push(format!(
                    "valid .{extension} generic manifest failed to build: {error:#}"
                ));
                continue;
            }
        };
        if indexed
            .registry
            .framework_manifest_for_path(LanguageFamily("python"), "index.html", "django", "django")
            .is_none()
        {
            failures.push(format!(
                "generic .{extension} manifest did not reach its linker"
            ));
        }
        let indexed_file = query_count(
            &indexed.conn,
            "SELECT count(*) FROM path_dictionary WHERE path=?1",
            ["index.html"],
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
            let manifest_path = format!(".forge/frameworks/django.{extension}");
            if FrameworkHarness::from_manifests(&[(manifest_path.as_str(), manifest.as_str())])
                .is_ok()
            {
                failures.push(format!(
                    ".{extension} legacy section {legacy_section} was accepted by the production manifest parser"
                ));
            }
        }
    }

    let empty =
        FrameworkHarness::from_manifests(&[(".forge/frameworks/django.toml", "enabled = false\n")])
            .and_then(|harness| {
                harness.link_sources(&[(
                    "index.html",
                    "html",
                    "<p>Empty typed rule manifest</p>\n",
                )])
            });
    if let Err(error) = empty {
        failures.push(format!("empty disabled manifest was rejected: {error:#}"));
    }

    let undeclared =
        FrameworkHarness::from_manifests(&[(".forge/frameworks/django.toml", complete_toml)])
            .unwrap();
    undeclared
        .link_sources(&[("index.html", "html", "<p>No framework declaration</p>\n")])
        .unwrap();
    if undeclared
        .registry
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
    let project_files = [
        (
            "pyproject.toml",
            "[project]\nname = 'sample'\ndependencies = ['django>=5']\n",
        ),
        (
            "shop/views.py",
            "from django.shortcuts import render\ndef page(request):\n    return render(request, 'templates/page.html', {'title': 'Welcome'})\n",
        ),
        ("shop/templates/page.html", "<h1>{{ title }}</h1>\n"),
    ];

    for (invalid_kind, extension, manifest) in invalid_manifests {
        let manifest_path = format!(".forge/frameworks/django.{extension}");
        let workspace = FrameworkHarness::persisted(&[]);
        workspace.write(manifest_path.as_str(), manifest.as_str());
        workspace.write_all(&project_files);
        let result = contextunity_forge_mcp::engine::pipeline::build_with_cache_dir(
            workspace.root(),
            &workspace.db(),
            None,
            &workspace.path(".forge/extraction-cache"),
        );
        if result.is_ok() || workspace.db().exists() {
            failures.push(format!("a declared Django project with {invalid_kind} in its .{extension} manifest was accepted or published index"));
        }
    }
    assert!(
        failures.is_empty(),
        "invalid manifest contract failures:\n{}",
        failures.join("\n")
    );
}
