use super::*;

#[test]
fn configured_di_origins_follow_effective_rules_and_delta() {
    let workspace = HtmlWorkspaceHarness::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname='di-origins'\ndependencies=['atelier']\n",
    );
    let source = |callee: &str| {
        format!(
            "from atelier import {callee}\ndef provider(): return 1\ndef endpoint(): return {callee}(provider)\ndef shadow({callee}): return {callee}(provider)\ndef helper(): return 1\ndef ordinary(): return helper()\n"
        )
    };
    workspace.write("app.py", &source("Wire"));
    let mut pack = serde_json::json!({"di_calls":[{"callee":"Wire","argument":0}],"semantic_rules":[{"id":"inject","namespace":"atelier","scope":"project","activation":[{"kind":"package","name":"atelier"}],"operation":{"kind":"call_to_file","language":"python","callee":{"kind":"imported","module":"atelier","export":"Wire"},"argument":0,"target":{"kind":"imported_symbol","language":"python"},"relation":"injects_dependency"}}]});
    pack["semantic_rules"][0]["operation"]["kind"] = "call_to_symbol".into();
    let first = ".forge/frameworks/di-alpha.toml";
    let second = ".forge/frameworks/di-beta.toml";
    let write = |path: &str, value: &serde_json::Value| {
        workspace.write(path, &toml::to_string(value).unwrap())
    };
    write(first, &pack);
    write(second, &pack);
    workspace.build();
    let assert_state = |packs: &[&str], active: bool| {
        let observations = workspace.observe();
        let validity = &observations.validity();
        validity.assert_valid();
        let endpoint_ids: Vec<_> = observations
            .nodes()
            .iter()
            .filter(|node| node.path == "app.py" && node.name == "endpoint")
            .map(|node| node.id.as_str())
            .collect();
        assert_eq!(endpoint_ids.len(), 1, "unique endpoint definition");
        let endpoint = endpoint_ids[0];
        let cached = &observations.persisted_facts();
        let origins: std::collections::BTreeSet<_> = cached
            .iter()
            .flat_map(|facts| &facts.references)
            .filter(|reference| reference.kind == "injects" && reference.source == endpoint)
            .map(|reference| match &reference.receiver_hint {
                Some(contextunity_forge_mcp::core::models::ReceiverHint::ConfiguredReference {
                    origin,
                }) => {
                    assert!(!origin.rule.is_empty());
                    origin.pack.as_str()
                }
                other => panic!("DI fact lost actual rule origin: {other:?}"),
            })
            .collect();
        assert_eq!(
            origins,
            packs.iter().copied().collect(),
            "same-callee packs retain independent durable identity"
        );
        for (owner, expected) in [("endpoint", usize::from(active)), ("shadow", 0)] {
            let edges = observations
                .edges()
                .iter()
                .filter(|edge| {
                    edge.kind == "injects_dependency"
                        && edge.source_name == owner
                        && edge.target_name == "provider"
                })
                .count();
            assert_eq!(edges, expected, "DI lexical proof: {owner}");
        }
        let helper_ids: Vec<_> = observations
            .nodes()
            .iter()
            .filter(|node| node.path == "app.py" && node.name == "helper")
            .map(|node| node.id.as_str())
            .collect();
        assert_eq!(helper_ids.len(), 1, "unique ordinary helper definition");
        assert_eq!(
            observations
                .edges()
                .iter()
                .filter(|edge| {
                    edge.owner_path == "app.py"
                        && edge.kind == "calls"
                        && edge.target_id == helper_ids[0]
                })
                .count(),
            1,
            "ordinary calls survive DI rule transitions"
        );
    };
    assert_state(&["di-alpha", "di-beta"], true);
    workspace.assert_cold_parity();
    let renamed = ".forge/frameworks/di-renamed.toml";
    let renamed_second = ".forge/frameworks/di-beta-current.toml";
    let observations = workspace.observe();
    let stale: std::collections::BTreeMap<_, _> = observations
        .persisted_facts()
        .iter()
        .cloned()
        .map(|facts| {
            let path = facts
                .nodes
                .iter()
                .find(|node| node.kind == "module")
                .unwrap()
                .path
                .clone();
            (path, facts)
        })
        .collect();
    drop(observations);
    workspace.rename_file(first, renamed).unwrap();
    workspace.rename_file(second, renamed_second).unwrap();
    let registry =
        contextunity_forge_mcp::engine::languages::dependency_registry::DependencyRegistry::collect(
            Some(workspace.root()),
        );
    let replay = contextunity_forge_mcp::engine::linker::link_with_registry(
        &stale,
        None,
        Some(workspace.root()),
        &registry,
    );
    assert!(
        !replay
            .edges
            .iter()
            .any(|edge| matches!(edge.kind.as_str(), "injects" | "injects_dependency")),
        "both current consumers reject persisted origins from removed packs despite identical active callees"
    );
    workspace.delta(&[first, renamed, second, renamed_second]);
    assert_state(&["di-renamed", "di-beta-current"], true);
    let second = renamed_second;
    workspace.remove_file(second).unwrap();
    workspace.delta(&[second]);
    assert_state(&["di-renamed"], true);
    let mut empty = pack.clone();
    empty["replace"] = true.into();
    empty["di_calls"] = serde_json::json!([]);
    empty["semantic_rules"] = serde_json::json!([]);
    write(renamed, &empty);
    workspace.delta(&[renamed]);
    assert_state(&[], false);
    pack["di_calls"][0]["callee"] = "Select".into();
    pack["semantic_rules"][0]["operation"]["callee"]["export"] = "Select".into();
    write(renamed, &pack);
    workspace.write("app.py", &source("Select"));
    workspace.delta(&[renamed, "app.py"]);
    assert_state(&["di-renamed"], true);
    pack["semantic_rules"][0]["activation"] =
        serde_json::json!([{"kind":"package","name":"missing-runtime"}]);
    write(renamed, &pack);
    workspace.delta(&[renamed]);
    assert_state(&["di-renamed"], false);
    workspace.assert_cold_parity();
}
