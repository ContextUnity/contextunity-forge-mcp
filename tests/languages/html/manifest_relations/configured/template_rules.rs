use super::*;

#[test]
fn configured_template_rules_follow_effective_rules_and_delta() {
    let workspace = HtmlWorkspaceHarness::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname='template-proof'\ndependencies=['atelier']\n",
    );
    workspace.write("app/models.py", "class Record: pass\n");
    workspace.write("app/views.py", "from atelier.views import emit\nfrom atelier.paths import lookup\nfrom atelier.router import Table\nfrom app.models import Record\nregistry = Table()\nregistry.add('configured/', Record, alias='configured')\ndef page(): return emit('screens/card.html', {'headline':'Title'})\ndef route(): return lookup('configured')\ndef helper(): return 1\ndef ordinary(): return helper()\n");
    workspace
        .write("app/environment.py", "from pathlib import Path\nfrom atelier.templates import Engine, Reader\nROOT = Path(__file__).parent / 'screens'\nengine = Engine(source=Reader(str(ROOT)))\n");
    workspace.write(
        "app/screens/card.html",
        "<h1>{{ headline }}</h1>\n{% pull 'part.html' %}\n{% address 'configured' %}\n{% assign local = 'value' %}\n{{ local }}\n{% use custom %}\n{% shout 'hello' %}\n{{ 'hello'|caps }}\n",
    );
    workspace.write("app/screens/part.html", "<p>{{ headline }}</p>\n");
    workspace.write(
        "app/templates/default.html",
        "{% include 'part.html' %}\n{% set standard = 'value' %}\n{{ standard }}\n",
    );
    workspace.write("app/templates/part.html", "<p>Default</p>\n");
    workspace.write("app/libraries/custom.py", "from atelier.tags import Library\nregistry = Library()\n@registry.command(label='shout')\ndef speak(value): return value\n@registry.decorate(label='caps')\ndef capitalize(value): return value\n");
    let pack = serde_json::json!({
        "route_calls":[{"callee":".add","method":"ANY","path_argument":0,"handler_argument":1,"receiver_names":["registry"],"name_keyword":"alias","registration_guard":"register","allow_relative_path":true}],
        "syntax_rules":[
            {"id":"register","language":"python","operation":{"kind":"receiver_registration","module":"atelier.router","constructors":["Table"],"member":"add","path_argument":0,"handler_argument":1,"name_keyword":"alias"}},
            {"id":"render","language":"python","operation":{"kind":"render_context","package":"atelier","module":"atelier.views","export":"emit","target_argument":0,"context_argument":1,"argument_count":2}},
            {"id":"loader","language":"python","operation":{"kind":"template_loader","package":"atelier","module":"atelier.templates","environment":"Engine","loader":"Reader","loader_keyword":"source"}},
            {"id":"library","language":"python","operation":{"kind":"template_library","directory":"libraries","module":"atelier.tags","constructor":"Library","members":{"command":"tag","decorate":"filter"},"name_keyword":"label"}},
            {"id":"named-call","language":"python","operation":{"kind":"named_route_call","module":"atelier.paths","export":"lookup","argument":0,"expression_prefix":"route-key:"}},
            {"id":"named-tag","language":"html","operation":{"kind":"template_named_route","tag":"address","expression_prefix":"route-key:"}},
            {"id":"symbols","language":"html","activation":[{"kind":"package","name":"atelier"}],"operation":{"kind":"template_symbols","requires_loader":true,"tags":[],"filters":[],"libraries":[],"library_routes":{}}},
            {"id":"dialect","language":"html","operation":{"kind":"template_dialect","dialect":{"directory":"screens","assignment":"assign","function":"function","import":"fetch","from":"using","alias":"called","load":"use","include":"pull","extends":"inherit","scope_open":["when"],"scope_close":["endwhen"]}}}
        ]
    });
    let manifest = ".forge/frameworks/atelier-templates.toml";
    let write = |value: &serde_json::Value| workspace.write(manifest, &toml::to_string(value).unwrap());
    write(&pack);
    workspace.build();
    let assert_active = |active: bool| {
        let observations = workspace.observe();
        let validity = &observations.validity();
        validity.assert_valid();
        for (label, actual, expected) in [
            (
                "import-proven receiver registration",
                observations.nodes()
                    .iter()
                    .filter(|node| node.kind == "route")
                    .filter(|node| {
                        serde_json::from_str::<serde_json::Value>(&node.details).unwrap()["route_name"] == "configured"
                    })
                    .count(),
                1,
            ),
            (
                "call and tag consume typed route keys",
                observations.edges()
                    .iter()
                    .filter(|edge| edge.kind == "references_route")
                    .count(),
                2,
            ),
            (
                "configured include",
                observations.edges()
                    .iter()
                    .filter(|edge| edge.kind == "includes" && edge.owner_path == "app/screens/card.html")
                    .count(),
                1,
            ),
            (
                "configured template library",
                observations.edges()
                    .iter()
                    .filter(|edge| edge.kind == "references" && edge.target_name == "speak")
                    .count(),
                1,
            ),
            (
                "configured template filter",
                observations.edges()
                    .iter()
                    .filter(|edge| edge.kind == "references" && edge.target_name == "capitalize")
                    .count(),
                1,
            ),
        ] {
            assert_eq!(actual, if active { expected } else { 0 }, "{label}, active={active}");
        }
        if active {
            for (path, expression) in [
                ("app/screens/card.html", "template.variable.headline"),
                ("app/screens/part.html", "template.variable.headline"),
                ("app/screens/card.html", "template.variable.local"),
            ] {
                let statuses: Vec<_> = observations.coverage()
                    .iter()
                    .filter(|coverage| coverage.path == path && coverage.expression == expression)
                    .map(|coverage| coverage.status.as_str())
                    .collect();
                assert_eq!(
                    statuses,
                    ["resolved"],
                    "render context and configured dialect {path}:{expression}"
                );
            }
        }
        let ordinary = observations.edges()
            .iter()
            .filter(|edge| edge.kind == "calls" && edge.source_name == "ordinary" && edge.target_name == "helper")
            .count();
        assert_eq!(ordinary, 1);
        assert_eq!(
            observations.edges()
                .iter()
                .filter(|edge| {
                    edge.kind == "includes"
                        && edge.source_path == "app/templates/default.html"
                        && edge.target_path == "app/templates/part.html"
                })
                .count(),
            1,
            "bundled and configured dialects coexist"
        );
    };
    assert_active(true);
    workspace.assert_cold_parity();
    let late_manifest = ".forge/frameworks/zcustom.toml";
    workspace.rename_file(manifest, late_manifest).unwrap();
    workspace.delta(&[manifest, late_manifest]);
    assert_active(true);
    let observations = workspace.observe();
    let cached = observations.persisted_facts();
    assert!(
        cached.iter().flat_map(|facts| &facts.references).any(|reference| reference.kind == "includes"
            && reference.expression == "part.html"
            && matches!(&reference.receiver_hint, Some(contextunity_forge_mcp::core::models::ReceiverHint::ConfiguredReference { origin }) if origin.pack == "zcustom" && origin.rule == "dialect")),
        "late pack retains actual typed include origin"
    );
    workspace.rename_file(late_manifest, manifest).unwrap();
    workspace.delta(&[manifest, late_manifest]);
    assert_active(true);
    let mut conflicting = pack.clone();
    conflicting["syntax_rules"] = serde_json::json!([pack["syntax_rules"][7].clone()]);
    conflicting["route_calls"] = serde_json::json!([]);
    conflicting["syntax_rules"][0]["id"] = "conflict".into();
    conflicting["syntax_rules"][0]["operation"]["dialect"]["include"] = "different".into();
    let conflict_path = ".forge/frameworks/ambiguous-dialect.toml";
    workspace.write(conflict_path, &toml::to_string(&conflicting).unwrap());
    workspace.delta(&[conflict_path]);
    let observations = workspace.observe();
    let validity = &observations.validity();
    validity.assert_valid();
    let conflicting_locals = observations.nodes()
        .iter()
        .filter(|node| node.path == "app/screens/card.html" && node.name == "local")
        .count();
    assert_eq!(
        conflicting_locals, 0,
        "conflicting directory-owning dialects fail closed"
    );
    workspace.remove_file(conflict_path).unwrap();
    workspace.delta(&[conflict_path]);
    assert_active(true);
    let card_source = workspace.read("app/screens/card.html").unwrap();
    let mut renamed_dialect = pack.clone();
    renamed_dialect["syntax_rules"][7]["operation"]["dialect"]["include"] = "draw".into();
    write(&renamed_dialect);
    workspace.delta(&[manifest]);
    let observations = workspace.observe();
    let validity = &observations.validity();
    validity.assert_valid();
    assert!(
        observations.edges()
            .iter()
            .find(|edge| edge.owner_path == "app/screens/card.html" && edge.kind == "includes")
            .is_none(),
        "removed configured directive cannot retain an exact include"
    );
    workspace.write("app/screens/card.html", &card_source.replace("{% pull ", "{% draw "));
    workspace.delta(&["app/screens/card.html"]);
    assert_active(true);
    write(&pack);
    workspace.write("app/screens/card.html", &card_source);
    workspace.delta(&[manifest, "app/screens/card.html"]);
    assert_active(true);
    let mut empty = pack.clone();
    empty["replace"] = true.into();
    empty["syntax_rules"] = serde_json::json!([]);
    empty["route_calls"] = serde_json::json!([]);
    write(&empty);
    workspace.delta(&[manifest]);
    assert_active(false);
    write(&pack);
    workspace.delta(&[manifest]);
    assert_active(true);
    workspace.assert_cold_parity();
}
