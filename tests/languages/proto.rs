#![cfg(feature = "lang-proto")]

use super::support::Workspace;
use contextunity_forge_mcp::engine::ast;
use std::fs;

#[cfg(feature = "lang-proto")]
#[test]
fn proto_package_qualified_types_and_fields() {
    let w = Workspace::new();
    w.write(
        "protos/auth.proto",
        r#"syntax = "proto3";

package mycompany.auth;

message Credentials {
    string username = 1;
    string token = 2;
}

message Session {
    mycompany.auth.Credentials creds = 1;
}

service AuthService {
    rpc Login(mycompany.auth.Credentials) returns (Session);
}
"#,
    );

    let facts = ast::extract(
        "protos/auth.proto",
        "proto",
        &fs::read_to_string(w.0.join("protos/auth.proto")).unwrap(),
    )
    .unwrap();

    // Verify module qualname is package
    let module_node = facts.nodes.iter().find(|n| n.kind == "module").unwrap();
    assert_eq!(module_node.qualname, "mycompany.auth");

    // Verify field references was extracted
    let field_refs: Vec<&str> = facts
        .references
        .iter()
        .filter(|r| r.kind == "references")
        .map(|r| r.expression.as_str())
        .collect();
    assert!(
        field_refs.contains(&"mycompany.auth.Credentials"),
        "expected field reference to mycompany.auth.Credentials: {:?}",
        field_refs
    );

    // Verify build & link
    let conn = w.build();
    let ref_edges: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare("SELECT (SELECT id FROM nodes WHERE node_hash=src_hash), (SELECT id FROM nodes WHERE node_hash=dst_hash) FROM edges WHERE kind='references'")
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get(0).unwrap(), r.get(1).unwrap())))
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    assert!(
        ref_edges.iter().any(|(_, dst)| dst.contains("Credentials")),
        "expected reference edge to Credentials: {:?}",
        ref_edges
    );
}

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
