use super::*;

pub(in crate::languages::html::manifest_relations) fn configured_syntax_rules_follow_effective_rules_and_delta(
) {
    let workspace = Workspace::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname='syntax-proof'\ndependencies=['atelier']\n",
    );
    workspace.write(
        "package.json",
        r#"{"name":"syntax-proof","dependencies":{"atelier":"*"}}"#,
    );
    let registration_source = "from atelier import Service\nservice = Service('proof')\n@service.publish(label='advertised')\ndef exported(): return 1\ndef helper(): return 1\ndef ordinary(): return helper()\n";
    workspace.write("app.py", registration_source);
    workspace.write("page.html", "<button zz-read='/configured' zz-state='item' zz-values='{\"key\":1}' z-event:click='act()' z-object='{count: 1}'></button>\n");
    workspace.write("app.ts", "const entries = [{location:'/configured',handler:page}];\nfunction page() {}\nfunction client() { return ask('/configured'); }\n");
    let mut pack = serde_json::json!({
        "syntax_rules":[
            {"id":"tools","language":"python","operation":{"kind":"decorated_registration","modules":["atelier"],"constructor":"Service","member":"publish","name_keyword":"label","provider":"configured"}},
            {"id":"requests","language":"html","operation":{"kind":"html_request_attributes","request_node":"configured_request","context_node":"configured_context","namespace":"configured","context_label":"Configured context","prefixes":["zz-"],"methods":{"read":"GET"},"contexts":["state"],"values":"values"}},
            {"id":"scripts","language":"html","operation":{"kind":"html_script_attributes","event_prefixes":["z-event:"],"object_attribute":"z-object","event_scope":"event_attribute","object_scope":"object_attribute"}},
            {"id":"routes","language":"typescript","operation":{"kind":"route_object","collection":"entries","path_key":"location","handler_key":"handler","method":"GET"}},
            {"id":"client","language":"typescript","operation":{"kind":"http_client_call","callee":"ask","receivers":[],"methods":{},"default_method":"GET","url_key":"destination","method_key":"verb","relative_prefixes":["/"]}}
        ]
    });
    let manifest = ".forge/frameworks/atelier.toml";
    let write =
        |value: &serde_json::Value| workspace.write(manifest, &toml::to_string(value).unwrap());
    write(&pack);
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let assert_active = |active: bool| {
        let conn = workspace.open();
        assert_valid(&conn);
        for (label, sql, expected) in [
            ("decorated constructor/member/name", "SELECT count(*) FROM nodes WHERE kind='tool_registration' AND json_extract(details,'$.advertised_name')='advertised' AND json_extract(details,'$.pack')='atelier' AND json_extract(details,'$.rule')='tools'", 1),
            ("request attribute prefix/method", "SELECT count(*) FROM nodes WHERE kind='configured_request'", 1),
            ("script attribute prefixes", "SELECT count(*) FROM nodes WHERE kind='template_scope' AND (name LIKE 'event_attribute@%' OR name LIKE 'object_attribute@%')", 2),
            ("object collection/key", "SELECT count(*) FROM nodes WHERE kind='route'", 1),
            ("configured client and request", "SELECT count(*) FROM edge_occurrences WHERE kind='calls_endpoint'", 2),
        ] {
            let actual: usize = conn.query_row(sql, [], |row|row.get(0)).unwrap();
            assert_eq!(actual, if active {expected} else {0}, "{label}, active={active}");
        }
        let cached = persisted_facts(&conn);
        let public_cached: Vec<_> = cached
            .iter()
            .map(crate::common::assertions::projected_facts)
            .collect();
        for (rule, expected) in [("tools", 1), ("requests", 1), ("scripts", 2), ("routes", 1)] {
            let origins = public_cached
                .iter()
                .flat_map(|facts| facts["nodes"].as_array().into_iter().flatten())
                .filter(|node| {
                    node["details"]["value_flow"]["rule_origins"]
                        .as_array()
                        .is_some_and(|origins| {
                            origins
                                .iter()
                                .any(|origin| origin["pack"] == "atelier" && origin["rule"] == rule)
                        })
                })
                .count();
            assert_eq!(
                origins,
                if active { expected } else { 0 },
                "durable configured origin: {rule}"
            );
        }
        let clients=cached.iter().flat_map(|facts| &facts.references).filter(|reference| reference.kind=="calls_endpoint").filter(|reference| matches!(&reference.receiver_hint,Some(contextunity_forge_mcp::core::models::ReceiverHint::ConfiguredReference { origin }) if origin.pack=="atelier" && origin.rule=="client")).count();
        assert_eq!(
            clients,
            usize::from(active),
            "client reference preserves actual origin"
        );
        let ordinary: usize = conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='ordinary' AND d.name='helper'",[],|row|row.get(0)).unwrap();
        assert_eq!(
            ordinary, 1,
            "ordinary call survives syntax rule transitions"
        );
    };
    assert_active(true);
    complete_parity(&workspace);
    for suffix in [
        "def unrelated(service):\n    @service.publish(label='shadow')\n    def shadowed(): return helper()\n    return shadowed\n",
        "def unrelated():\n    service = object()\n    @service.publish(label='shadow')\n    def shadowed(): return helper()\n    return shadowed\n",
        "def unrelated():\n    service = Service('local')\n    @service.publish(label='shadow')\n    def shadowed(): return helper()\n    return shadowed\n",
    ] {
        workspace.write("app.py", &format!("{registration_source}\n{suffix}"));
        workspace.delta("app.py");
        assert_active(true);
        assert_eq!(workspace.open().query_row("SELECT count(*) FROM nodes WHERE kind='tool_registration'", [], |row| row.get::<_, usize>(0)).unwrap(), 1, "a different lexical receiver cannot borrow the module instance: {suffix}");
        complete_parity(&workspace);
    }
    workspace.write("app.py", registration_source);
    workspace.delta("app.py");
    for rule in pack["syntax_rules"].as_array_mut().unwrap() {
        rule["activation"] = serde_json::json!([{"kind":"package","name":"missing-runtime"}]);
    }
    write(&pack);
    workspace.delta(manifest);
    assert_active(false);
    complete_parity(&workspace);
    for rule in pack["syntax_rules"].as_array_mut().unwrap() {
        rule["activation"] = serde_json::json!([]);
    }
    write(&pack);
    workspace.delta(manifest);
    assert_active(true);
    complete_parity(&workspace);
    let mut empty = pack.clone();
    empty["replace"] = true.into();
    empty["syntax_rules"] = serde_json::json!([]);
    write(&empty);
    workspace.delta(manifest);
    assert_active(false);
    complete_parity(&workspace);
    write(&pack);
    workspace.delta(manifest);
    assert_active(true);
    fs::remove_file(workspace.0.join(manifest)).unwrap();
    workspace.delta(manifest);
    assert_active(false);
    complete_parity(&workspace);
    for invalid in [
        serde_json::json!({"kind":"execute","script":"anything"}),
        serde_json::json!({"kind":"http_client_call","callee":"ask","receivers":[],"methods":{},"default_method":"GET","url_key":"destination","method_key":"verb","relative_prefixes":["/"],"extra":"unchecked"}),
    ] {
        pack["syntax_rules"][0]["operation"] = invalid;
        write(&pack);
        assert!(
            writer::build(&workspace.0, &workspace.db(), None).is_err(),
            "unknown operation/field must fail closed"
        );
    }
}
