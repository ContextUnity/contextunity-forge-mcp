use super::*;

pub(in crate::languages::html::manifest_relations) fn configured_template_rules_follow_effective_rules_and_delta(
) {
    let workspace = Workspace::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname='template-proof'\ndependencies=['atelier']\n",
    );
    workspace.write("app/models.py", "class Record: pass\n");
    workspace.write("app/views.py", "from atelier.views import emit\nfrom atelier.paths import lookup\nfrom atelier.router import Table\nfrom app.models import Record\nregistry = Table()\nregistry.add('configured/', Record, alias='configured')\ndef page(): return emit('screens/card.html', {'headline':'Title'})\ndef route(): return lookup('configured')\ndef helper(): return 1\ndef ordinary(): return helper()\n");
    workspace.write("app/environment.py", "from pathlib import Path\nfrom atelier.templates import Engine, Reader\nROOT = Path(__file__).parent / 'screens'\nengine = Engine(source=Reader(str(ROOT)))\n");
    workspace.write("app/screens/card.html", "<h1>{{ headline }}</h1>\n{% pull 'part.html' %}\n{% address 'configured' %}\n{% assign local = 'value' %}\n{{ local }}\n{% use custom %}\n{% shout 'hello' %}\n{{ 'hello'|caps }}\n");
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
    let write =
        |value: &serde_json::Value| workspace.write(manifest, &toml::to_string(value).unwrap());
    write(&pack);
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let assert_active = |active: bool| {
        let conn = workspace.open();
        assert_valid(&conn);
        for (label,sql,expected) in [
            ("import-proven receiver registration", "SELECT count(*) FROM nodes WHERE kind='route' AND json_extract(details,'$.route_name')='configured'",1),
            ("call and tag consume typed route keys", "SELECT count(*) FROM edge_occurrences WHERE kind='references_route'",2),
            ("configured include", "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE e.kind='includes' AND p.path='app/screens/card.html'",1),
            ("configured template library", "SELECT count(*) FROM edge_occurrences e JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='references' AND d.name='speak'",1),
            ("configured template filter", "SELECT count(*) FROM edge_occurrences e JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='references' AND d.name='capitalize'",1),
        ] {
            let actual:usize=conn.query_row(sql,[],|row|row.get(0)).unwrap();
            assert_eq!(actual,if active{expected}else{0},"{label}, active={active}");
        }
        if active {
            for (path, expression) in [
                ("app/screens/card.html", "template.variable.headline"),
                ("app/screens/part.html", "template.variable.headline"),
                ("app/screens/card.html", "template.variable.local"),
            ] {
                let status:String=conn.query_row("SELECT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND x.expression=?2",params![path,expression],|row|row.get(0)).unwrap();
                assert_eq!(
                    status, "resolved",
                    "render context and configured dialect {path}:{expression}"
                );
            }
        }
        let ordinary:usize=conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='ordinary' AND d.name='helper'",[],|row|row.get(0)).unwrap();
        assert_eq!(ordinary, 1);
        assert_eq!(conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=s.path_id JOIN path_dictionary q ON q.path_id=d.path_id WHERE e.kind='includes' AND p.path='app/templates/default.html' AND q.path='app/templates/part.html'", [], |row|row.get::<_, usize>(0)).unwrap(), 1, "bundled and configured dialects coexist");
    };
    assert_active(true);
    complete_parity(&workspace);
    let late_manifest = ".forge/frameworks/zcustom.toml";
    fs::rename(workspace.0.join(manifest), workspace.0.join(late_manifest)).unwrap();
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from(manifest), PathBuf::from(late_manifest)],
    )
    .unwrap();
    assert_active(true);
    complete_parity(&workspace);
    let cached = persisted_facts(&workspace.open());
    assert!(cached.iter().flat_map(|facts| &facts.references).any(|reference| reference.kind == "includes" && reference.expression == "part.html" && matches!(&reference.receiver_hint, Some(contextunity_forge_mcp::core::models::ReceiverHint::ConfiguredReference { origin }) if origin.pack == "zcustom" && origin.rule == "dialect")), "late pack retains actual typed include origin");
    fs::rename(workspace.0.join(late_manifest), workspace.0.join(manifest)).unwrap();
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from(manifest), PathBuf::from(late_manifest)],
    )
    .unwrap();
    assert_active(true);
    let mut conflicting = pack.clone();
    conflicting["syntax_rules"] = serde_json::json!([pack["syntax_rules"][7].clone()]);
    conflicting["route_calls"] = serde_json::json!([]);
    conflicting["syntax_rules"][0]["id"] = "conflict".into();
    conflicting["syntax_rules"][0]["operation"]["dialect"]["include"] = "different".into();
    let conflict_path = ".forge/frameworks/ambiguous-dialect.toml";
    workspace.write(conflict_path, &toml::to_string(&conflicting).unwrap());
    workspace.delta(conflict_path);
    assert_eq!(workspace.open().query_row("SELECT count(*) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='app/screens/card.html' AND n.name='local'", [], |row|row.get::<_, usize>(0)).unwrap(), 0, "conflicting directory-owning dialects fail closed");
    complete_parity(&workspace);
    fs::remove_file(workspace.0.join(conflict_path)).unwrap();
    workspace.delta(conflict_path);
    assert_active(true);
    complete_parity(&workspace);
    let card_source = fs::read_to_string(workspace.0.join("app/screens/card.html")).unwrap();
    let mut renamed_dialect = pack.clone();
    renamed_dialect["syntax_rules"][7]["operation"]["dialect"]["include"] = "draw".into();
    write(&renamed_dialect);
    workspace.delta(manifest);
    assert!(
        edge_occurrence_rows(&workspace.open(), "app/screens/card.html", "includes").is_empty(),
        "removed configured directive cannot retain an exact include"
    );
    complete_parity(&workspace);
    workspace.write(
        "app/screens/card.html",
        &card_source.replace("{% pull ", "{% draw "),
    );
    workspace.delta("app/screens/card.html");
    assert_active(true);
    complete_parity(&workspace);
    write(&pack);
    workspace.write("app/screens/card.html", &card_source);
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[
            PathBuf::from(manifest),
            PathBuf::from("app/screens/card.html"),
        ],
    )
    .unwrap();
    assert_active(true);
    complete_parity(&workspace);
    let mut empty = pack.clone();
    empty["replace"] = true.into();
    empty["syntax_rules"] = serde_json::json!([]);
    empty["route_calls"] = serde_json::json!([]);
    write(&empty);
    workspace.delta(manifest);
    assert_active(false);
    complete_parity(&workspace);
    write(&pack);
    workspace.delta(manifest);
    assert_active(true);
    complete_parity(&workspace);
}
