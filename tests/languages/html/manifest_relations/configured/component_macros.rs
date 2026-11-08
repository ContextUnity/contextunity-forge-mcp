use super::*;

#[test]
fn configured_component_macros_follow_effective_rules_and_delta() {
    let workspace = HtmlWorkspaceHarness::new();
    let manifest = ".forge/frameworks/atelier-macros.toml";
    let source = |name: &str| {
        format!(
            "<script setup lang=\"ts\">\nconst input = {name}<{{title: string}}>();\nconst send = publishSignal<{{(event: 'ready'): void}}>();\ndeclareOutlet<{{default(props: {{item: string}}): unknown}}>();\nsend('ready');\n</script><template><p>{{{{input.title}}}}</p></template>\n"
        )
    };
    workspace.write("macro.vue", &source("inspectInput"));
    workspace
        .write("shadow.vue", "<script setup lang=\"ts\">\nfunction inspectInput() { return {title:'local'}; }\nconst input = inspectInput();\n</script><template><p>{{input.title}}</p></template>\n");
    let pack = serde_json::json!({"syntax_rules":[{"id":"macros","language":"vue","operation":{"kind":"component_macros","props":"inspectInput","emits":"publishSignal","slots":"declareOutlet","markers":["markComponent"]}}]});
    let write = |value: &serde_json::Value| workspace.write(manifest, &toml::to_string(value).unwrap());
    write(&pack);
    workspace.build();
    let assert_state = |props: bool, other: bool| {
        let observations = workspace.observe();
        let validity = &observations.validity();
        validity.assert_valid();
        let cached = &observations.persisted_facts();
        let public_cached: Vec<_> = cached.iter().map(crate::common::assertions::projected_facts).collect();
        let macros: Vec<_> = public_cached
            .iter()
            .flat_map(|facts| facts["nodes"].as_array().into_iter().flatten())
            .filter(|node| node["path"] == "macro.vue")
            .flat_map(|node| {
                node["details"]["value_flow"]["component_macros"]
                    .as_array()
                    .into_iter()
                    .flatten()
            })
            .collect();
        assert_eq!(
            macros.len(),
            usize::from(props) + 2 * usize::from(other),
            "durable origins follow effective macro roles"
        );
        assert!(
            macros
                .iter()
                .all(|fact| fact["origin"]["pack"] == "atelier-macros" && fact["origin"]["rule"] == "macros")
        );
        let fields = observations.edges()
            .iter()
            .filter(|edge| {
                edge.owner_path == "macro.vue"
                    && edge.kind == "references"
                    && edge.target_kind == "field"
                    && edge.target_name == "title"
            })
            .count();
        assert_eq!(
            fields,
            usize::from(props),
            "configured typed prop targets its canonical field"
        );
        let emits = observations.coverage()
            .iter()
            .filter(|coverage| {
                coverage.path == "macro.vue" && coverage.expression == "send" && coverage.status == "external"
            })
            .count();
        assert_eq!(
            emits,
            usize::from(other),
            "configured emits role owns callable admission"
        );
        let ordinary = observations.edges()
            .iter()
            .filter(|edge| {
                edge.owner_path == "shadow.vue" && edge.kind == "calls" && edge.target_name == "inspectInput"
            })
            .count();
        assert_eq!(ordinary, 1, "same-name local call retains ordinary lexical target");
        assert!(
            !cached
                .iter()
                .map(crate::common::assertions::projected_facts)
                .flat_map(|facts| facts["nodes"].as_array().cloned().unwrap_or_default())
                .filter(|node| node["path"] == "shadow.vue")
                .any(|node| node["details"]["value_flow"]["component_macros"]
                    .as_array()
                    .is_some_and(|facts| !facts.is_empty()))
        );
    };
    assert_state(true, true);
    workspace.assert_cold_parity();
    for mutation in [
        "inspectInput = local; publishSignal = local; declareOutlet = local;",
        "inspectInput += local; publishSignal += local; declareOutlet += local;",
        "inspectInput++; publishSignal++; declareOutlet++;",
    ] {
        let changed = source("inspectInput").replacen(
            "const input",
            &format!("function local() {{ return {{title:'local'}}; }}\n{mutation}\nconst input"),
            1,
        );
        workspace.write("macro.vue", &changed);
        workspace.delta(&["macro.vue"]);
        assert_state(false, false);
        let observations = workspace.observe();
        let cached = observations.persisted_facts();
        assert!(
            !cached
                .iter()
                .map(crate::common::assertions::projected_facts)
                .flat_map(|facts| facts["nodes"].as_array().cloned().unwrap_or_default())
                .filter(|node| node["path"] == "macro.vue")
                .any(|node| node["details"]["value_flow"]["vue_slots"]
                    .as_array()
                    .is_some_and(|slots| !slots.is_empty())),
            "producer writes invalidate derived slot facts: {mutation}"
        );
    }
    workspace.write("macro.vue", &source("inspectInput"));
    workspace.delta(&["macro.vue"]);
    assert_state(true, true);
    let mut renamed = pack.clone();
    renamed["syntax_rules"][0]["operation"]["props"] = "readInput".into();
    write(&renamed);
    workspace.delta(&[manifest]);
    assert_state(false, true);
    workspace.write("macro.vue", &source("readInput"));
    workspace.delta(&["macro.vue"]);
    assert_state(true, true);
    let mut inactive = renamed.clone();
    inactive["syntax_rules"][0]["activation"] = serde_json::json!([{"kind":"package","name":"missing-runtime"}]);
    write(&inactive);
    workspace.delta(&[manifest]);
    assert_state(false, false);
    let mut empty = renamed.clone();
    empty["replace"] = true.into();
    empty["syntax_rules"] = serde_json::json!([]);
    write(&empty);
    workspace.delta(&[manifest]);
    assert_state(false, false);
    write(&renamed);
    workspace.delta(&[manifest]);
    assert_state(true, true);
    workspace.assert_cold_parity();
}
