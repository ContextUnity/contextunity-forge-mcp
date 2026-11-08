use super::*;

#[test]
fn configured_component_and_receiver_rules_follow_effective_rules_and_delta() {
    let workspace = HtmlWorkspaceHarness::new();
    workspace.write(
        "package.json",
        r#"{"name":"configured-components","dependencies":{"@atelier/ui-kit":"*"}}"#,
    );
    workspace.write(
        "atelier.config.ts",
        "export default configureAtelier({widgets:true, addons:[]});\n",
    );
    workspace.write("widgets/CardView.vue", "<template><p>Card</p></template>\n");
    workspace.write("registered.vue", "<script>\nimport Card from './widgets/CardView.vue';\nimport { wrapWidget } from '@atelier/ui-kit';\nexport default wrapWidget({items:{ConfiguredCard: Card}});\n</script><template><ConfiguredCard/></template>\n");
    workspace.write("bare.vue", "<script>\nimport Card from './widgets/CardView.vue';\nexport default {items:{ConfiguredCard: Card}};\n</script><template><ConfiguredCard/></template>\n");
    workspace.write(
        "page.vue",
        "<template><CardView/><LateView/><ConfiguredShell/></template>\n",
    );
    workspace.write("app.ts","import { CardView } from '#widgets';\nfunction page() { return configuredRef(1); }\nfunction callback(event: ConfiguredEvent) { return event.payload.get('key'); }\nfunction implicit() { return $signal(); }\nfunction helper() { return 1; }\nfunction ordinary() { return helper(); }\nfunction shadow(configuredRef: () => number) { return configuredRef(); }\n");
    workspace.write(
        "shadow.ts",
        "function configuredRef() { return 1; }\nfunction ordinary() { return configuredRef(); }\n",
    );
    for (path, source) in [
        (
            "receiver-local.ts",
            "function $signal() { return 1; }\nexport function localReceiver() { return $signal(); }\n",
        ),
        (
            "receiver-parameter.ts",
            "export function parameterReceiver($signal: () => number) { return $signal(); }\n",
        ),
        (
            "receiver-provider.ts",
            "export function providedSignal() { return 1; }\n",
        ),
        (
            "receiver-import.ts",
            "import {providedSignal as $signal} from './receiver-provider';\nexport function importedReceiver() { return $signal(); }\n",
        ),
        (
            "receiver-write.ts",
            "export function writtenReceiver(input: () => number) { let $signal = input; $signal = replacement; return $signal(); }\n",
        ),
        (
            "receiver-member.ts",
            "class Receiver { $signal() { return 1; } runReceiver() { return this.$signal(); } }\n",
        ),
    ] {
        workspace.write(path, source);
    }
    workspace.write(
        "factory.js",
        "Widget.bind('panel', {start() {this.finish();},finish() {}});\n",
    );
    let pack = serde_json::json!({"syntax_rules":[
        {"id":"components","language":"typescript","operation":{"kind":"component_environment","package":"@atelier/ui-kit","config_file":"atelier.config.ts","config_callee":"configureAtelier","enabled_key":"widgets","modules_key":"addons","directory":"widgets","extension":".vue","virtual_module":"#widgets","autoimports":["configuredRef"],"builtins":["ConfiguredShell"],"modules":{}}},
        {"id":"options","language":"vue","operation":{"kind":"component_options","module":"@atelier/ui-kit","export":"wrapWidget","components_key":"items"}},
        {"id":"receiver","language":"typescript","operation":{"kind":"receiver_type","receiver":{"name":"ConfiguredEvent","members":["payload"],"properties":{"payload":"Headers"},"callback_callees":["configuredCallback"],"generic":false}}},
        {"id":"factory","language":"javascript","operation":{"kind":"object_factory","callees":[".bind"],"unshadowed_globals":["Widget"]}},
        {"id":"globals","language":"javascript","operation":{"kind":"implicit_builtins","names":["Widget","Widget.bind"]}},
        {"id":"implicit","language":"typescript","activation":[{"kind":"package","name":"@atelier/ui-kit"}],"operation":{"kind":"implicit_receivers","names":["$signal"]}}
    ]});
    let manifest = ".forge/frameworks/atelier-components.toml";
    let write = |value: &serde_json::Value| workspace.write(manifest, &toml::to_string(value).unwrap());
    write(&pack);
    workspace.build();
    let assert_active = |active: bool| {
        let observations = workspace.observe();
        let validity = &observations.validity();
        validity.assert_valid();
        for (path, expression) in [
            ("page.vue", "CardView"),
            ("page.vue", "ConfiguredShell"),
            ("app.ts", "CardView"),
            ("app.ts", "configuredRef"),
            ("app.ts", "event.payload.get"),
            ("app.ts", "$signal"),
        ] {
            let statuses: Vec<_> = observations.coverage()
                .iter()
                .filter(|coverage| coverage.path == path && coverage.expression == expression)
                .map(|coverage| coverage.status.as_str())
                .collect();
            assert!(!statuses.is_empty(), "coverage {path}:{expression}");
            if path == "app.ts" && expression == "CardView" {
                assert_eq!(
                    statuses,
                    vec![if active { "resolved" } else { "external" }],
                    "ordinary virtual-module import survives removal"
                );
                continue;
            }
            let admitted = statuses
                .iter()
                .any(|status| *status == "resolved" || *status == "external");
            assert_eq!(
                admitted, active,
                "configured components/receiver {path}:{expression}: {statuses:?}"
            );
        }
        let shadow = observations.edges()
            .iter()
            .filter(|edge| {
                edge.source_path == "shadow.ts"
                    && edge.source_name == "ordinary"
                    && edge.kind == "calls"
                    && edge.target_name == "configuredRef"
            })
            .count();
        assert_eq!(
            shadow, 1,
            "configured builtin spelling preserves local declaration calls"
        );
        for (path, caller, target_path, target) in [
            ("receiver-local.ts", "localReceiver", "receiver-local.ts", "$signal"),
            (
                "receiver-import.ts",
                "importedReceiver",
                "receiver-provider.ts",
                "providedSignal",
            ),
            ("receiver-member.ts", "runReceiver", "receiver-member.ts", "$signal"),
        ] {
            let canonical_ids: Vec<_> = observations.nodes()
                .iter()
                .filter(|node| node.path == target_path && node.name == target)
                .map(|node| node.id.as_str())
                .collect();
            assert_eq!(canonical_ids.len(), 1, "unique lexical target: {target_path}:{target}");
            let targets: Vec<_> = observations.edges()
                .iter()
                .filter(|edge| edge.source_path == path && edge.source_name == caller && edge.kind == "calls")
                .map(|edge| edge.target_id.as_str())
                .collect();
            assert_eq!(
                targets, canonical_ids,
                "ordinary lexical/import/member authority precedes implicit receiver, active={active}: {path}"
            );
            let external = observations.coverage()
                .iter()
                .filter(|coverage| {
                    coverage.path == path
                        && matches!(coverage.expression.as_str(), "$signal" | "this.$signal")
                        && coverage.status == "external"
                })
                .count();
            assert_eq!(external, 0, "local receiver cannot become configured external: {path}");
        }
        for path in ["receiver-parameter.ts", "receiver-write.ts"] {
            let statuses: Vec<_> = observations.coverage()
                .iter()
                .filter(|coverage| coverage.path == path && coverage.expression == "$signal")
                .map(|coverage| coverage.status.as_str())
                .collect();
            assert!(
                !statuses.is_empty() && statuses.iter().all(|status| *status == "unresolved"),
                "unknown lexical receiver stays nonexternal, active={active}: {path}: {statuses:?}"
            );
            let guessed = observations.edges()
                .iter()
                .filter(|edge| edge.source_path == path && edge.kind == "calls")
                .count();
            assert_eq!(guessed, 0, "unknown lexical receiver has no guessed exact call: {path}");
        }
        let same_object = observations.edges()
            .iter()
            .filter(|edge| edge.kind == "calls" && edge.source_name == "start" && edge.target_name == "finish")
            .count();
        assert_eq!(
            same_object,
            usize::from(active),
            "configured object factory owns this member proof"
        );
        let cached = &observations.persisted_facts();
        let public_cached: Vec<_> = cached.iter().map(crate::common::assertions::projected_facts).collect();
        let origins = public_cached
            .iter()
            .flat_map(|facts| facts["nodes"].as_array().into_iter().flatten())
            .filter(|node| node["name"] == "start" || node["name"] == "finish")
            .filter(|node| {
                let ownership = &node["details"]["value_flow"]["object_ownership"];
                ownership["origin"]["pack"] == "atelier-components"
                    && ownership["origin"]["rule"] == "factory"
                    && ownership["guard"] == "Widget"
            })
            .count();
        assert_eq!(
            origins,
            if active { 2 } else { 0 },
            "factory origin survives the durable fact cache and disappears with its rule"
        );
        let configurations = cached
            .iter()
            .map(crate::common::assertions::projected_facts)
            .flat_map(|facts| facts["nodes"].as_array().cloned().unwrap_or_default())
            .flat_map(|node| {
                node["details"]["value_flow"]["component_configurations"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            })
            .filter(|fact| {
                fact["origin"]["pack"] == "atelier-components"
                    && fact["origin"]["rule"] == "components"
                    && fact["enabled"] == true
            })
            .count();
        assert_eq!(
            configurations,
            usize::from(active),
            "component configuration origin survives the durable cache"
        );
        let registrations=cached.iter().flat_map(|facts| &facts.references).filter(|reference| reference.kind=="references" && reference.expression=="Card").filter(|reference| matches!(&reference.receiver_hint,Some(contextunity_forge_mcp::core::models::ReceiverHint::ConfiguredReference { origin }) if origin.pack=="atelier-components" && origin.rule=="options")).count();
        assert_eq!(
            registrations,
            if active { 2 } else { 0 },
            "options references retain actual rule origin and canonical imported alias"
        );
        for path in ["registered.vue", "bare.vue"] {
            let references: Vec<_> = observations.edges()
                .iter()
                .filter(|edge| edge.owner_path == path && edge.kind == "references")
                .map(|edge| (edge.source_id.as_str(), edge.target_id.as_str(), edge.line))
                .collect();
            assert_eq!(
                references.len(),
                usize::from(active),
                "configured options own exact component reference: {path}"
            );
            if active {
                let target_ids: Vec<_> = observations.nodes()
                    .iter()
                    .filter(|node| node.path == "widgets/CardView.vue" && node.kind == "module")
                    .map(|node| node.id.as_str())
                    .collect();
                assert_eq!(target_ids.len(), 1, "unique imported component module");
                assert_eq!(
                    references[0].1, target_ids[0],
                    "registered alias targets its imported component module"
                );
            }
        }
    };
    assert_active(true);
    workspace.assert_cold_parity();
    for enabled in [true, false] {
        workspace.write(
            "atelier.config.ts",
            &format!("export default configureAtelier({{widgets:{enabled}, addons:[]}});\n"),
        );
        workspace.delta(&["atelier.config.ts"]);
        let frozen: Vec<_> = [manifest, "atelier.config.ts", "package.json", "page.vue"]
            .into_iter()
            .map(|path| (path, fs::read(workspace.root().join(path)).unwrap()))
            .collect();
        for content in [
            Some("<template><p>Added</p></template>\n"),
            Some("<template><p>Changed</p></template>\n"),
            None,
        ] {
            let provider = "widgets/LateView.vue";
            if let Some(content) = content {
                workspace.write(provider, content);
            } else {
                workspace.remove_file(provider).unwrap();
            }
            let report = workspace.delta(&[provider]);
            assert_eq!(
                report["reparsed_files"],
                usize::from(content.is_some()),
                "only the provider is reparsed: {report}"
            );
            for (path, bytes) in &frozen {
                assert_eq!(
                    fs::read(workspace.root().join(path)).unwrap(),
                    *bytes,
                    "unchanged cached input {path}"
                );
            }
            let edges = workspace
                .observe()
                .edges()
                .iter()
                .filter(|edge| {
                    edge.source_path == "page.vue"
                        && edge.target_path == provider
                        && edge.target_kind == "module"
                        && edge.kind == "references"
                })
                .count();
            assert_eq!(
                edges,
                usize::from(enabled && content.is_some()),
                "cached configuration admits the canonical provider, enabled={enabled}, content={content:?}"
            );
        }
    }
    workspace.write(
        "atelier.config.ts",
        "export default configureAtelier({widgets:true, addons:[]});\n",
    );
    workspace.delta(&["atelier.config.ts"]);
    let mut empty = pack.clone();
    empty["replace"] = true.into();
    empty["syntax_rules"] = serde_json::json!([]);
    write(&empty);
    workspace.delta(&[manifest]);
    assert_active(false);
    write(&pack);
    workspace.delta(&[manifest]);
    assert_active(true);
    workspace.assert_cold_parity();
}
