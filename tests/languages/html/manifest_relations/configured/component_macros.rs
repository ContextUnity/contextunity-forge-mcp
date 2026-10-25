use super::*;

pub(in crate::languages::html::manifest_relations) fn configured_component_macros_follow_effective_rules_and_delta(
) {
    let workspace = Workspace::new();
    let manifest = ".forge/frameworks/atelier-macros.toml";
    let source = |name: &str| {
        format!("<script setup lang=\"ts\">\nconst input = {name}<{{title: string}}>();\nconst send = publishSignal<{{(event: 'ready'): void}}>();\ndeclareOutlet<{{default(props: {{item: string}}): unknown}}>();\nsend('ready');\n</script><template><p>{{{{input.title}}}}</p></template>\n")
    };
    workspace.write("macro.vue", &source("inspectInput"));
    workspace.write("shadow.vue", "<script setup lang=\"ts\">\nfunction inspectInput() { return {title:'local'}; }\nconst input = inspectInput();\n</script><template><p>{{input.title}}</p></template>\n");
    let pack = serde_json::json!({"syntax_rules":[{"id":"macros","language":"vue","operation":{"kind":"component_macros","props":"inspectInput","emits":"publishSignal","slots":"declareOutlet","markers":["markComponent"]}}]});
    let write =
        |value: &serde_json::Value| workspace.write(manifest, &toml::to_string(value).unwrap());
    write(&pack);
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let assert_state = |props: bool, other: bool| {
        let conn = workspace.open();
        assert_valid(&conn);
        let cached = persisted_facts(&conn);
        let public_cached: Vec<_> = cached
            .iter()
            .map(crate::common::assertions::projected_facts)
            .collect();
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
        assert!(macros
            .iter()
            .all(|fact| fact["origin"]["pack"] == "atelier-macros"
                && fact["origin"]["rule"] == "macros"));
        let fields:usize=conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes d ON d.node_hash=e.dst_hash JOIN nodes s ON s.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='macro.vue' AND e.kind='references' AND d.kind='field' AND d.name='title'",[],|row|row.get(0)).unwrap();
        assert_eq!(
            fields,
            usize::from(props),
            "configured typed prop targets its canonical field"
        );
        let emits:usize=conn.query_row("SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='macro.vue' AND x.expression='send' AND c.status='external'",[],|row|row.get(0)).unwrap();
        assert_eq!(
            emits,
            usize::from(other),
            "configured emits role owns callable admission"
        );
        let ordinary:usize=conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes d ON d.node_hash=e.dst_hash JOIN nodes s ON s.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='shadow.vue' AND e.kind='calls' AND d.name='inspectInput'",[],|row|row.get(0)).unwrap();
        assert_eq!(
            ordinary, 1,
            "same-name local call retains ordinary lexical target"
        );
        assert!(!cached
            .iter()
            .map(crate::common::assertions::projected_facts)
            .flat_map(|facts| facts["nodes"].as_array().cloned().unwrap_or_default())
            .filter(|node| node["path"] == "shadow.vue")
            .any(|node| node["details"]["value_flow"]["component_macros"]
                .as_array()
                .is_some_and(|facts| !facts.is_empty())));
    };
    assert_state(true, true);
    complete_parity(&workspace);
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
        workspace.delta("macro.vue");
        assert_state(false, false);
        let cached = persisted_facts(&workspace.open());
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
        complete_parity(&workspace);
    }
    workspace.write("macro.vue", &source("inspectInput"));
    workspace.delta("macro.vue");
    assert_state(true, true);
    let mut renamed = pack.clone();
    renamed["syntax_rules"][0]["operation"]["props"] = "readInput".into();
    write(&renamed);
    workspace.delta(manifest);
    assert_state(false, true);
    complete_parity(&workspace);
    workspace.write("macro.vue", &source("readInput"));
    workspace.delta("macro.vue");
    assert_state(true, true);
    complete_parity(&workspace);
    let mut inactive = renamed.clone();
    inactive["syntax_rules"][0]["activation"] =
        serde_json::json!([{"kind":"package","name":"missing-runtime"}]);
    write(&inactive);
    workspace.delta(manifest);
    assert_state(false, false);
    complete_parity(&workspace);
    let mut empty = renamed.clone();
    empty["replace"] = true.into();
    empty["syntax_rules"] = serde_json::json!([]);
    write(&empty);
    workspace.delta(manifest);
    assert_state(false, false);
    complete_parity(&workspace);
    write(&renamed);
    workspace.delta(manifest);
    assert_state(true, true);
    complete_parity(&workspace);
}
