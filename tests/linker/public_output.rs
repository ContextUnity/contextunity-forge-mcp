use contextunity_forge_mcp::{
    core::models::{Edge, Facts},
    engine::linker,
};
use std::collections::BTreeMap;

#[test]
fn public_linker_preserves_arbitrary_edge_tags_and_orders_edges_stably() {
    let unusual = Edge {
        src: "source-α".into(),
        dst: "target-β".into(),
        kind: "custom-kind-雪".into(),
        path: "z/文件.rs".into(),
        line: 17,
        evidence: "evidence-дані".into(),
        confidence: "confidence-🧭".into(),
    };
    let earlier = Edge {
        path: "a/файл.rs".into(),
        ..unusual.clone()
    };
    let facts = BTreeMap::from([(
        "input.rs".to_owned(),
        Facts {
            edges: vec![unusual.clone(), earlier.clone()],
            ..Facts::default()
        },
    )]);

    let linked = linker::link(&facts);
    assert_eq!(linked.edges.len(), 2);
    assert_eq!(linked.edges[0].path, earlier.path);
    assert_eq!(linked.edges[1].path, unusual.path);
    assert_eq!(
        serde_json::to_vec(&linked.edges[0]).unwrap(),
        serde_json::to_vec(&earlier).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(&linked.edges[1]).unwrap(),
        serde_json::to_vec(&unusual).unwrap()
    );
}
