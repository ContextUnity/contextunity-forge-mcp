#![cfg(feature = "lang-proto")]

use contextunity_forge_mcp::engine::ast;

#[test]
fn test_proto_ast_extractor() {
    let source = r#"
syntax = "proto3";

package contextunity.core;

import "google/protobuf/struct.proto";

enum Modality {
    TEXT = 0;
    AUDIO = 1;
}

message ContextUnit {
    string unit_id = 1;
    Modality modality = 4;
}

service RouterService {
    rpc ExecuteAgent(ContextUnit) returns (ContextUnit);
}
"#;

    let facts = ast::extract("core/protos/test.proto", "proto", source).expect("proto extract");

    // Check nodes extracted
    let node_names: Vec<&str> = facts.nodes.iter().map(|n| n.name.as_str()).collect();
    assert!(
        node_names.contains(&"Modality"),
        "missing Modality enum: {:?}",
        node_names
    );
    assert!(
        node_names.contains(&"ContextUnit"),
        "missing ContextUnit message: {:?}",
        node_names
    );
    assert!(
        node_names.contains(&"RouterService"),
        "missing RouterService: {:?}",
        node_names
    );
    assert!(
        node_names.contains(&"ExecuteAgent"),
        "missing ExecuteAgent rpc: {:?}",
        node_names
    );

    // Check kinds
    let cu = facts
        .nodes
        .iter()
        .find(|n| n.name == "ContextUnit")
        .unwrap();
    assert_eq!(cu.kind, "class");

    let svc = facts
        .nodes
        .iter()
        .find(|n| n.name == "RouterService")
        .unwrap();
    assert_eq!(svc.kind, "service");

    let rpc = facts
        .nodes
        .iter()
        .find(|n| n.name == "ExecuteAgent")
        .unwrap();
    assert_eq!(rpc.kind, "function");

    // Check imports
    let imports: Vec<&str> = facts
        .references
        .iter()
        .filter(|r| r.kind == "imports")
        .map(|r| r.expression.as_str())
        .collect();
    assert!(
        imports.contains(&"google/protobuf/struct.proto"),
        "missing import reference: {:?}",
        imports
    );

    // Check rpc references to ContextUnit
    let rpc_refs: Vec<&str> = facts
        .references
        .iter()
        .filter(|r| r.kind == "references")
        .map(|r| r.expression.as_str())
        .collect();
    assert!(
        rpc_refs.contains(&"ContextUnit"),
        "missing rpc type references: {:?}",
        rpc_refs
    );

    // Check 0 errors
    assert!(
        facts.errors.is_empty(),
        "unexpected parse errors: {:?}",
        facts.errors
    );
}
