use super::*;
use crate::core::semantic::ValueFlowFacts;
use serde_json::json;

#[test]
fn binary_bool_decoder_rejects_noncanonical_values() {
    for (encoded, expected) in [
        (0, Some(false)),
        (1, Some(true)),
        (2, None),
        (u8::MAX, None),
    ] {
        let mut cursor = 0;
        assert_eq!(bool::decode(&[encoded], &mut cursor), expected);
    }
}

#[test]
fn invalid_cache_magic_and_collection_lengths_are_misses() {
    assert!(decode_entry(b"invalid cache header", "key", 1024).is_none());
    let mut prefixes = Vec::new();
    for preceding_collections in 0..5 {
        prefixes.push(vec![0; preceding_collections * 4]);
    }
    let mut doc_prefix = vec![0; 3 * 4];
    1u32.encode(&mut doc_prefix);
    for _ in 0..5 {
        "".encode(&mut doc_prefix);
    }
    prefixes.push(doc_prefix.clone());
    0u32.encode(&mut doc_prefix);
    prefixes.push(doc_prefix);
    for tag in [6, 7] {
        let mut node_prefix = Vec::new();
        1u32.encode(&mut node_prefix);
        for _ in 0..5 {
            "".encode(&mut node_prefix);
        }
        0u64.encode(&mut node_prefix);
        0u64.encode(&mut node_prefix);
        0u8.encode(&mut node_prefix);
        "".encode(&mut node_prefix);
        0u8.encode(&mut node_prefix);
        (tag as u8).encode(&mut node_prefix);
        prefixes.push(node_prefix);
    }
    for mut payload in prefixes {
        u32::MAX.encode(&mut payload);
        let mut bytes = MAGIC_V4.to_vec();
        "key".encode(&mut bytes);
        (payload.len() as u64).encode(&mut bytes);
        bytes.extend_from_slice(&Sha256::digest(&payload));
        bytes.extend_from_slice(&payload);
        assert!(decode_entry(&bytes, "key", 1024).is_none());
    }
}

#[test]
fn cache_round_trip_preserves_flow_without_details_placeholder() {
    let mut facts = TypedFacts::default();
    let node = Node {
        native_semantic: None,
        id: "scope:src/main.js:1:1".into(),
        kind: "scope".into(),
        name: "main".into(),
        qualname: "main".into(),
        path: "src/main.js".into(),
        line: 1,
        end_line: 1,
        is_test: false,
        language: "javascript".into(),
        generated: false,
        details: json!({"scope": "module"}),
    };
    let node_id = node.id.clone();

    let fields_first = FlowState::Valid {
        facts: Box::new(ValueFlowFacts {
            fields: vec![crate::core::semantic::FieldFact {
                name: "field".into(),
                position: crate::core::semantic::SourcePosition { line: 1, column: 1 },
                value: crate::core::semantic::ValueExpr::Unknown,
                conditional: false,
            }],
            ..ValueFlowFacts::default()
        }),
        original: None,
        classic_fields_first: Some(true),
    };
    let fields_first_wire = serde_json::to_string(&fields_first.view()).unwrap();
    assert!(fields_first_wire.starts_with("{\"fields\":["));
    assert!(fields_first_wire.contains(",\"bindings\":null"));

    let original = json!({"preserved": true});
    let original_flow = FlowState::Valid {
        facts: Box::new(ValueFlowFacts::default()),
        original: Some(original.clone()),
        classic_fields_first: Some(true),
    };
    assert_eq!(
        serde_json::to_value(original_flow.view()).unwrap(),
        original
    );
    let invalid_flow = FlowState::Invalid(original.clone());
    assert_eq!(serde_json::to_value(invalid_flow.view()).unwrap(), original);
    let original_semantic = crate::core::typed_facts::SemanticState::Valid {
        facts: Vec::new(),
        original: Some(original.clone()),
    };
    assert_eq!(
        serde_json::to_value(original_semantic.view()).unwrap(),
        original
    );
    let invalid_semantic = crate::core::typed_facts::SemanticState::Invalid {
        original: original.clone(),
        error: "invalid original metadata".into(),
    };
    assert_eq!(
        serde_json::to_value(invalid_semantic.view()).unwrap(),
        original
    );

    for (details, native) in [
        (r#"{"bindings":[],"rebindings":[],"param_types":{}}"#, false),
        (
            r#"{"signature":"f(z,a)","param_types":{"z":"Z","a":"A"},"bindings":["z","a"],"visibility":"public","rebindings":[],"column":2}"#,
            true,
        ),
        (
            r#"{"bindings":["valid",7],"param_types":{"z":"Z","a":null},"rebindings":["changed"],"signature":"raw"}"#,
            true,
        ),
        (
            r#"{"bindings":null,"rebindings":false,"param_types":["raw"]}"#,
            false,
        ),
        (r#"{"signature":"absent metadata"}"#, false),
    ] {
        let mut public_node = node.clone();
        public_node.details = serde_json::from_str(details).unwrap();
        let public = TypedFacts {
            facts: crate::core::models::Facts {
                nodes: vec![public_node],
                ..Default::default()
            },
            flows: Default::default(),
        };
        let typed = TypedFacts::from_public(public.facts.clone());
        assert_eq!(
            typed.nodes[0].native_semantic.is_some(),
            native,
            "{details}"
        );
        assert_eq!(
            serde_json::to_value(&typed).unwrap(),
            serde_json::to_value(&public).unwrap(),
            "public metadata projection: {details}"
        );
        let key = "cd".repeat(32);
        let bytes = encode_entry(&key, &typed).unwrap();
        assert!(bytes.starts_with(MAGIC_V4));
        let public_bytes = encode_entry(&key, &public).unwrap();
        let mut corrupt = bytes.clone();
        *corrupt.last_mut().expect("encoded payload is present") ^= 1;
        assert!(decode_entry(&corrupt, &key, 1024 * 1024).is_none());
        let reopened = decode_entry(&bytes, &key, 1024 * 1024).unwrap();
        assert_eq!(
            serde_json::to_value(&reopened).unwrap(),
            serde_json::to_value(&public).unwrap(),
            "metadata cache round trip: {details}"
        );
        let public_reopened = decode_entry(&public_bytes, &key, 1024 * 1024).unwrap();
        assert_eq!(
            serde_json::to_value(&public_reopened).unwrap(),
            serde_json::to_value(&public).unwrap(),
            "public metadata cache round trip: {details}"
        );
    }
    facts.facts.nodes.push(node);
    let flow = ValueFlowFacts {
        framework: Some(Box::new(crate::core::semantic::FrameworkScopeFacts {
            fact_limit_exceeded: true,
            ..Default::default()
        })),
        ..Default::default()
    };
    facts.flows.insert(node_id.clone(), flow);

    let key = "ab".repeat(32);
    let encoded = encode_entry(&key, &facts).expect("cache entry encodes");
    let decoded = decode_entry(&encoded, &key, 1024 * 1024).expect("cache entry decodes");
    let decoded_node = decoded
        .facts
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .expect("node round-trips");

    assert!(matches!(
        decoded.flows.state(decoded_node),
        Some(FlowState::Invalid(value)) if value["fact_limit_exceeded"] == true
    ));

    use crate::core::semantic::{
        ExtractionRuleOrigin, FrameworkFact, NamedBindingFact, NamedBindingRole, NamedTargetKind,
        SourcePosition, StaticString, SymbolOperand,
    };
    let definition = NamedBindingFact {
        origin: ExtractionRuleOrigin {
            pack: "codec".into(),
            rule: "definitions".into(),
        },
        position: SourcePosition { line: 1, column: 0 },
        domain: "renderer".into(),
        key: StaticString::Literal {
            value: "over_limit".into(),
        },
        role: NamedBindingRole::Definition {
            target: SymbolOperand::Unknown,
            target_kind: NamedTargetKind::Callable,
        },
    };
    facts.flows.insert(
        node_id.clone(),
        ValueFlowFacts {
            framework: Some(Box::new(crate::core::semantic::FrameworkScopeFacts {
                facts: vec![FrameworkFact::NamedBinding(definition.clone()); 130],
                ..Default::default()
            })),
            ..Default::default()
        },
    );
    let original = match facts.flows.state(&facts.nodes[0]).unwrap() {
        FlowState::Invalid(raw) => raw.clone(),
        _ => panic!("scope must overflow"),
    };
    assert_eq!(
        facts.flows.named_uncertainties(&node_id).len(),
        1,
        "dedup before carrier bound"
    );
    let encoded = encode_entry(&key, &facts).unwrap();
    let decoded = decode_entry(&encoded, &key, 1024 * 1024).unwrap();
    assert!(
        matches!(decoded.flows.state(&decoded.nodes[0]),Some(FlowState::Invalid(raw)) if raw==&original)
    );
    assert_eq!(
        decoded.flows.named_uncertainties(&node_id),
        facts.flows.named_uncertainties(&node_id)
    );

    let valid_flow = ValueFlowFacts {
        bindings: vec![crate::core::semantic::BindingFact {
            name: "client".into(),
            position: crate::core::semantic::SourcePosition { line: 2, column: 3 },
            value: crate::core::semantic::ValueExpr::Construct {
                callee: "Client".into(),
            },
            conditional: false,
        }],
        return_type: Some(crate::core::semantic::TypeExpr::Named {
            name: "Client".into(),
        }),
        framework: Some(Box::new(crate::core::semantic::FrameworkScopeFacts {
            facts: vec![FrameworkFact::RuleOrigin(
                crate::core::semantic::ExtractionRuleOrigin {
                    pack: "codec".into(),
                    rule: "flow".into(),
                },
            )],
            ..Default::default()
        })),
        ..Default::default()
    };
    let mut flow_node = facts.nodes[0].clone();
    flow_node.details["value_flow"] = serde_json::to_value(valid_flow).unwrap();
    let flow_input = TypedFacts::from_public(crate::core::models::Facts {
        nodes: vec![flow_node],
        ..Default::default()
    });
    assert!(matches!(
        flow_input.flows.state(&flow_input.nodes[0]),
        Some(FlowState::Valid {
            original: Some(_),
            ..
        })
    ));
    let expected_flow = serde_json::to_value(&flow_input).unwrap();
    let flow_cache = encode_entry(&key, &flow_input).unwrap();
    let flow_reopened = decode_entry(&flow_cache, &key, 1024 * 1024).unwrap();
    assert!(matches!(
        flow_reopened.flows.state(&flow_reopened.nodes[0]),
        Some(FlowState::Valid { original: None, .. })
    ));
    assert_eq!(serde_json::to_value(flow_reopened).unwrap(), expected_flow);
    let local_flow_blob = crate::core::typed_facts::encode_facts(&flow_input).unwrap();
    let local_flow_reopened = crate::core::typed_facts::decode_facts(&local_flow_blob).unwrap();
    assert!(matches!(
        local_flow_reopened
            .flows
            .state(&local_flow_reopened.nodes[0]),
        Some(FlowState::Valid { original: None, .. })
    ));
    assert_eq!(
        serde_json::to_value(local_flow_reopened).unwrap(),
        expected_flow
    );
    let blob = crate::core::typed_facts::encode_facts(&facts).unwrap();
    assert!(blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"));
    assert!(crate::core::typed_facts::decode_facts(b"FORGE-LOCAL-FACTS\0\x01").is_err());
    assert!(crate::core::typed_facts::decode_facts(b"FORGE-LOCAL-FACTS\0\x02").is_err());
    let reopened = crate::core::typed_facts::decode_facts(&blob).unwrap();
    assert_eq!(
        reopened.flows.named_uncertainties(&node_id),
        facts.flows.named_uncertainties(&node_id)
    );
    assert!(serde_json::to_value(reopened.nodes[0].navigation_details())
        .unwrap()
        .get("named_definition_uncertainties")
        .is_none());

    let semantic = crate::core::semantic::SemanticFact {
        pack: "codec".into(),
        rule: "file".into(),
        identity: crate::core::semantic::SemanticIdentity {
            scope: ".".into(),
            namespace: "templates".into(),
            target: crate::core::semantic::SemanticTarget::ProjectPathSuffix {
                prefix: "templates/".into(),
                language: "html".into(),
            },
            path: "main.html".into(),
        },
        occurrence: crate::core::semantic::SemanticOccurrence {
            owner_id: node_id.clone(),
            line: 1,
            column: 0,
        },
        expression: "main.html".into(),
        role: crate::core::semantic::SemanticFactRole::FileProvider {
            target: node_id.clone(),
        },
    };
    assert!(facts
        .facts
        .semantic
        .push(&mut facts.facts.nodes, semantic.clone()));
    facts.facts.semantic.mark_complete();
    let public: crate::core::models::Facts =
        serde_json::from_value(serde_json::to_value(&facts).unwrap()).unwrap();
    let semantic_input = TypedFacts::from_public(public);
    assert!(matches!(
        semantic_input.facts.semantic.state(0),
        Some(crate::core::typed_facts::SemanticState::Valid {
            original: Some(_),
            ..
        })
    ));
    let native_public = serde_json::to_value(&semantic_input).unwrap();
    assert_eq!(
        native_public["nodes"][0]["details"]["semantic_facts"][0],
        serde_json::to_value(&semantic).unwrap()
    );
    let cached = encode_entry(&key, &semantic_input).expect("typed semantic cache encodes");
    let cached = decode_entry(&cached, &key, 1024 * 1024).expect("typed semantic cache decodes");
    assert_eq!(serde_json::to_value(&cached).unwrap(), native_public);
    assert!(matches!(
        cached.facts.semantic.state(0),
        Some(crate::core::typed_facts::SemanticState::Valid {
            facts, original: None
        })
            if facts.as_slice() == std::slice::from_ref(&semantic)
    ));
    let packed = crate::core::typed_facts::encode_facts(&semantic_input).unwrap();
    let native_reopened = crate::core::typed_facts::decode_facts(&packed).unwrap();
    assert!(native_reopened.facts.semantic.is_complete());
    assert!(matches!(
        native_reopened.facts.semantic.state(0),
        Some(crate::core::typed_facts::SemanticState::Valid { original: None, .. })
    ));
    assert_eq!(
        serde_json::to_value(&native_reopened).unwrap(),
        native_public
    );

    let mut bad_markers = vec![
        json!(true),
        json!([{"domain":"renderer","key":{"kind":"unknown"},"origin":{"pack":"codec","rule":"definitions"},"unknown_field":true}]),
    ];
    bad_markers.push(serde_json::Value::Array((0..129).map(|i|json!({"domain":format!("domain_{i}"),"key":{"kind":"unknown"},"origin":{"pack":"codec","rule":"definitions"}})).collect()));
    for bad in bad_markers {
        let mut public: crate::core::models::Facts =
            serde_json::from_value(serde_json::to_value(&decoded).unwrap()).unwrap();
        public.nodes[0].details["named_definition_uncertainties"] = bad;
        assert!(
            crate::core::typed_facts::FlowStore::read_named_uncertainties(&public.nodes[0])
                .is_err()
        );
        let invalid = TypedFacts::from_public(public);
        assert!(invalid.flows.has_invalid_named_uncertainty_carriers());
        assert!(invalid
            .flows
            .named_uncertainty_faults()
            .any(|owner| owner == node_id));
        assert!(encode_entry(&key, &invalid).is_err());
        assert!(crate::core::typed_facts::encode_facts(&invalid).is_err());
        assert!(
            matches!(invalid.flows.state(&invalid.nodes[0]),Some(FlowState::Invalid(raw)) if raw==&original)
        );
    }
    let mut different_keys: Vec<_> = (0..129)
        .map(|i| {
            let mut fact = definition.clone();
            fact.key = StaticString::Literal {
                value: format!("key_{i}"),
            };
            fact
        })
        .collect();
    let mut other_domain = definition.clone();
    other_domain.domain = "action".into();
    different_keys.push(other_domain.clone());
    facts.flows.insert(
        node_id.clone(),
        ValueFlowFacts {
            framework: Some(Box::new(crate::core::semantic::FrameworkScopeFacts {
                facts: different_keys
                    .into_iter()
                    .map(FrameworkFact::NamedBinding)
                    .collect(),
                ..Default::default()
            })),
            ..Default::default()
        },
    );
    assert_eq!(facts.flows.named_uncertainties(&node_id).len(), 2);
    assert!(facts
        .flows
        .named_uncertainties(&node_id)
        .iter()
        .any(|marker| marker.domain == "renderer" && marker.key == StaticString::Unknown));
    assert!(facts
        .flows
        .named_uncertainties(&node_id)
        .iter()
        .any(|marker| marker.domain == other_domain.domain && marker.key == other_domain.key));
    for distinct_origins in [false, true] {
        let different_groups = (0..129)
            .map(|i| {
                let mut fact = definition.clone();
                if distinct_origins {
                    fact.origin.rule = format!("definitions_{i}");
                } else {
                    fact.domain = format!("domain_{i}");
                }
                FrameworkFact::NamedBinding(fact)
            })
            .collect();
        facts.flows.insert(
            node_id.clone(),
            ValueFlowFacts {
                framework: Some(Box::new(crate::core::semantic::FrameworkScopeFacts {
                    facts: different_groups,
                    ..Default::default()
                })),
                ..Default::default()
            },
        );
        assert!(facts
            .flows
            .named_uncertainty_faults()
            .any(|owner| owner == node_id));
        let exhausted_raw = match facts.flows.state(&facts.nodes[0]).unwrap() {
            FlowState::Invalid(raw) => raw.clone(),
            _ => panic!("exhausted scope remains invalid"),
        };
        let cache_reopened =
            decode_entry(&encode_entry(&key, &facts).unwrap(), &key, 1024 * 1024).unwrap();
        let full_reopened = crate::core::typed_facts::decode_facts(
            &crate::core::typed_facts::encode_facts(&facts).unwrap(),
        )
        .unwrap();
        for reopened in [&cache_reopened, &full_reopened] {
            assert!(reopened.flows.named_uncertainty_fault(&node_id));
            assert!(!reopened.flows.has_invalid_named_uncertainty_carriers());
            assert!(reopened.flows.named_uncertainties(&node_id).is_empty());
            assert!(
                matches!(reopened.flows.state(&reopened.nodes[0]), Some(FlowState::Invalid(raw)) if raw == &exhausted_raw)
            );
            let public: crate::core::models::Facts =
                serde_json::from_value(serde_json::to_value(reopened).unwrap()).unwrap();
            assert!(
                crate::core::typed_facts::FlowStore::read_named_uncertainties(&public.nodes[0])
                    .is_err(),
                "infallible configured named authority remains unavailable"
            );
        }
    }
    let mut malformed: crate::core::models::Facts =
        serde_json::from_value(serde_json::to_value(&native_reopened).unwrap()).unwrap();
    let raw = json!([{"future_semantic_field":true}]);
    malformed.nodes[0].details["semantic_facts"] = raw.clone();
    let malformed = TypedFacts::from_public(malformed);
    let malformed =
        decode_entry(&encode_entry(&key, &malformed).unwrap(), &key, 1024 * 1024).unwrap();
    assert!(
        matches!(malformed.facts.semantic.state(0),Some(crate::core::typed_facts::SemanticState::Invalid{original,..}) if original==&raw)
    );
    assert_eq!(
        serde_json::to_value(&malformed).unwrap()["nodes"][0]["details"]["semantic_facts"],
        raw
    );
    let mut selected = decoded.clone();
    selected.project_named_selectors();
    assert!(selected
        .references
        .iter()
        .any(|reference| reference.kind == "semantic_named_definition"
            && reference.module.as_deref() == Some("renderer")
            && reference.expression == "over_limit"));
}

#[test]
fn test_classic_global_cache_roundtrip() {
    use crate::core::semantic::{BindingFact, SourcePosition, ValueExpr};
    use crate::core::typed_facts::encode_facts;

    let mut facts = TypedFacts::default();
    let node = Node {
        native_semantic: None,
        id: "html-js:change_form.html:classic".into(),
        kind: "template_scope".into(),
        name: "classic scripts".into(),
        qualname: "module.script".into(),
        path: "change_form.html".into(),
        line: 1,
        end_line: 10,
        is_test: false,
        language: "javascript".into(),
        generated: false,
        details: json!({
            "column": 43,
            "embedded_language": "javascript",
            "classic_global": true,
            "lexical_boundary": true,
            "html_import_owner": "module:change_form.html",
            "bindings": ["PRODUCT_PK"],
            "rebindings": ["PRODUCT_PK"],
            "value_flow": serde_json::Value::Null,
        }),
    };
    facts.facts.nodes.push(node);
    facts.flows.insert_classic(
        "html-js:change_form.html:classic".into(),
        ValueFlowFacts {
            bindings: vec![BindingFact {
                name: "PRODUCT_PK".into(),
                position: SourcePosition {
                    line: 244,
                    column: 6,
                },
                value: ValueExpr::Unknown,
                conditional: false,
            }],
            immutable_initializers: vec![SourcePosition {
                line: 256,
                column: 2,
            }],
            ..ValueFlowFacts::default()
        },
    );

    let key = "cc".repeat(32);
    let encoded_cache = encode_entry(&key, &facts).expect("cache entry encodes");
    let decoded = decode_entry(&encoded_cache, &key, 1024 * 1024).expect("cache entry decodes");

    let cold_encoded = encode_facts(&facts).expect("cold encode");
    let warm_encoded = encode_facts(&decoded).expect("warm encode");

    assert_eq!(
        cold_encoded, warm_encoded,
        "cold and warm facts must encode identically"
    );
}

#[test]
fn test_catalog_reference_and_prepared_storage_decoding() {
    let mut facts = TypedFacts::default();
    facts.facts.references.push(Reference {
        source: "owner".into(),
        expression: "callee".into(),
        kind: "calls".into(),
        line: 1,
        column: 0,
        alias: None,
        module: None,
        dynamic: false,
        receiver_hint: None,
    });
    let node = Node {
        native_semantic: None,
        id: "fn:src/lib.rs:hello".into(),
        kind: "function".into(),
        name: "hello".into(),
        qualname: "crate::hello".into(),
        path: "src/lib.rs".into(),
        line: 10,
        end_line: 20,
        is_test: false,
        language: "rust".into(),
        generated: false,
        details: json!({"async": true}),
    };
    facts.facts.nodes.push(node);

    let mut raw = Vec::new();
    encode_references(&facts.references, &mut raw);
    let encoded = zstd::stream::encode_all(raw.as_slice(), 1).unwrap();
    assert!(
        std::panic::catch_unwind(|| crate::core::semantic::decode_catalog_references(
            &encoded[..encoded.len() - 1],
            &[]
        ))
        .is_err()
    );
    for encoded in [
        encoded,
        zstd::stream::encode_all(serde_json::to_vec(&facts.references).unwrap().as_slice(), 1)
            .unwrap(),
        [
            zstd::stream::encode_all(&raw[..2], 1).unwrap(),
            zstd::stream::encode_all(&raw[2..], 1).unwrap(),
        ]
        .concat(),
    ] {
        assert_eq!(
            serde_json::to_value(crate::core::semantic::decode_catalog_references(
                &encoded,
                &[]
            ))
            .unwrap(),
            serde_json::to_value(&facts.references).unwrap()
        );
    }

    let prepared = super::super::PreparedCatalogStorage {
        facts,
        search_texts: vec!["hello".into()],
        reference_keys: vec!["callee".into()],
        leaves: Default::default(),
    };
    raw.clear();
    prepared.encode(&mut raw);
    let encoded = zstd::stream::encode_all(raw.as_slice(), -3).unwrap();
    assert!(super::super::decode_prepared_storage(&encoded[..encoded.len() - 1]).is_err());
    for encoded in [
        encoded,
        [
            zstd::stream::encode_all(&raw[..2], -3).unwrap(),
            zstd::stream::encode_all(&raw[2..], -3).unwrap(),
        ]
        .concat(),
    ] {
        let reopened = super::super::decode_prepared_storage(&encoded).unwrap();
        assert_eq!(
            serde_json::to_vec(&reopened.facts).unwrap(),
            serde_json::to_vec(&prepared.facts).unwrap()
        );
        assert_eq!(reopened.search_texts, prepared.search_texts);
        assert_eq!(reopened.reference_keys, prepared.reference_keys);
    }
}
