use super::*;

pub(in crate::languages::html::manifest_relations) fn configured_di_origins_follow_effective_rules_and_delta(
) {
    let workspace = Workspace::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname='di-origins'\ndependencies=['atelier']\n",
    );
    let source = |callee: &str| {
        format!("from atelier import {callee}\ndef provider(): return 1\ndef endpoint(): return {callee}(provider)\ndef shadow({callee}): return {callee}(provider)\ndef helper(): return 1\ndef ordinary(): return helper()\n")
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
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let assert_state = |packs: &[&str], active: bool| {
        let conn = workspace.open();
        assert_valid(&conn);
        let endpoint = node_id(&conn, "app.py", "endpoint");
        let cached = persisted_facts(&conn);
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
            let edges:usize=conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='injects_dependency' AND s.name=?1 AND d.name='provider'", [owner], |row|row.get(0)).unwrap();
            assert_eq!(edges, expected, "DI lexical proof: {owner}");
        }
        assert_eq!(
            edge_occurrence_rows(&conn, "app.py", "calls")
                .iter()
                .filter(|(_, target, _)| *target == node_id(&conn, "app.py", "helper"))
                .count(),
            1,
            "ordinary calls survive DI rule transitions"
        );
    };
    assert_state(&["di-alpha", "di-beta"], true);
    complete_parity(&workspace);
    let renamed = ".forge/frameworks/di-renamed.toml";
    let renamed_second = ".forge/frameworks/di-beta-current.toml";
    let stale: std::collections::BTreeMap<_, _> = persisted_facts(&workspace.open())
        .into_iter()
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
    fs::rename(workspace.0.join(first), workspace.0.join(renamed)).unwrap();
    fs::rename(workspace.0.join(second), workspace.0.join(renamed_second)).unwrap();
    let registry =
        contextunity_forge_mcp::engine::languages::dependency_registry::DependencyRegistry::collect(
            Some(&workspace.0),
        );
    let replay = contextunity_forge_mcp::engine::linker::link_with_registry(
        &stale,
        None,
        Some(&workspace.0),
        &registry,
    );
    assert!(!replay.edges.iter().any(|edge| matches!(edge.kind.as_str(), "injects" | "injects_dependency")), "both current consumers reject persisted origins from removed packs despite identical active callees");
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[
            PathBuf::from(first),
            PathBuf::from(renamed),
            PathBuf::from(second),
            PathBuf::from(renamed_second),
        ],
    )
    .unwrap();
    assert_state(&["di-renamed", "di-beta-current"], true);
    complete_parity(&workspace);
    let second = renamed_second;
    fs::remove_file(workspace.0.join(second)).unwrap();
    workspace.delta(second);
    assert_state(&["di-renamed"], true);
    complete_parity(&workspace);
    let mut empty = pack.clone();
    empty["replace"] = true.into();
    empty["di_calls"] = serde_json::json!([]);
    empty["semantic_rules"] = serde_json::json!([]);
    write(renamed, &empty);
    workspace.delta(renamed);
    assert_state(&[], false);
    complete_parity(&workspace);
    pack["di_calls"][0]["callee"] = "Select".into();
    pack["semantic_rules"][0]["operation"]["callee"]["export"] = "Select".into();
    write(renamed, &pack);
    workspace.write("app.py", &source("Select"));
    writer::delta(
        &workspace.0,
        &workspace.db(),
        &[PathBuf::from(renamed), PathBuf::from("app.py")],
    )
    .unwrap();
    assert_state(&["di-renamed"], true);
    complete_parity(&workspace);
    pack["semantic_rules"][0]["activation"] =
        serde_json::json!([{"kind":"package","name":"missing-runtime"}]);
    write(renamed, &pack);
    workspace.delta(renamed);
    assert_state(&["di-renamed"], false);
    complete_parity(&workspace);
}
