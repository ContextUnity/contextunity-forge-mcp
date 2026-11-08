use super::*;
use contextunity_forge_mcp::core::{
    models::{Facts, Node},
    semantic::{SourcePosition, TypeExpr, ValueExpr, ValueFlowFacts},
};
use forge_index::languages::manifests::{ClassHierarchyReceiverRule, ReturnPolicy};
use forge_index::linker::value_flow::{
    ConfiguredReceiverAuthority, SemanticResolver, Symbol, SymbolRole, TypeTarget, ValueFlowIndex,
};

struct SeamValueFlowResolver;

impl<'a> forge_index::linker::value_flow::SemanticResolver<'a> for SeamValueFlowResolver {
    fn resolve_symbol(
        &self,
        _scope: &'a contextunity_forge_mcp::core::models::Node,
        _name: &str,
        _at: contextunity_forge_mcp::core::semantic::SourcePosition,
        _role: forge_index::linker::value_flow::SymbolRole,
    ) -> forge_index::linker::value_flow::Symbol<'a> {
        forge_index::linker::value_flow::Symbol::Unknown
    }
}

const SOURCE: &str = r#"
use std::collections::HashMap;

pub struct Config {
    pub host: String,
    pub port: u16,
}

pub enum Status {
    Active,
    Inactive,
}

impl Config {
    pub fn new(host: String, port: u16) -> Self {
        Self { host, port }
    }
}
"#;

fn facts() -> Facts {
    ExtractorHarness::for_lang("rust")
        .at_path("test.rs")
        .extract(SOURCE)
}

fn declaration<'a>(facts: &'a Facts, kind: &str, name: &str) -> &'a Node {
    facts
        .nodes
        .iter()
        .find(|node| node.kind == kind && node.name == name)
        .unwrap()
}

fn call(callee: &str) -> ValueExpr {
    ValueExpr::Call {
        callee: callee.to_owned(),
        site: None,
    }
}

fn supplied_projection(member: &str) -> ValueExpr {
    ValueExpr::Project {
        value: Box::new(ValueExpr::Annotated {
            type_expr: TypeExpr::Named {
                name: "Supplied".to_owned(),
            },
        }),
        member: member.to_owned(),
    }
}

fn after(owner: &Node) -> SourcePosition {
    SourcePosition {
        line: owner.line + 1,
        column: 0,
    }
}

struct DotConstructorResolver<'a> {
    class: &'a Node,
    constructor: &'a Node,
    flow: &'a ValueFlowFacts,
}
impl<'a> SemanticResolver<'a> for DotConstructorResolver<'a> {
    fn resolve_symbol(
        &self,
        _: &'a Node,
        name: &str,
        _: SourcePosition,
        role: SymbolRole,
    ) -> Symbol<'a> {
        match (name, role) {
            ("Config", SymbolRole::Type) => Symbol::Type(TypeTarget::Local(self.class)),
            _ => Symbol::Unknown,
        }
    }
    fn resolve_member(
        &self,
        receiver: &TypeTarget<'a>,
        member: &str,
        _: &'a Node,
        _: SourcePosition,
    ) -> Symbol<'a> {
        if matches!(receiver, TypeTarget::Local(node) if node.id == self.class.id)
            && member == "new"
        {
            Symbol::Callable(self.constructor)
        } else {
            Symbol::Unknown
        }
    }
    fn value_flow(&self, node: &'a Node) -> Option<&'a ValueFlowFacts> {
        (node.id == self.constructor.id).then_some(self.flow)
    }
}

#[test]
fn constructor_spellings_normalize_and_missing_constructors_stay_unknown() {
    let facts = facts();
    let empty_catalog = std::collections::BTreeMap::new();
    let seam_resolver = SeamValueFlowResolver;
    let _seam_index =
        ValueFlowIndex::build_with_limits(&empty_catalog, &seam_resolver, Default::default());
    let class = declaration(&facts, "struct", "Config");
    let constructor = declaration(&facts, "method", "new");
    let flow = ValueFlowFacts {
        return_type: Some(TypeExpr::Named {
            name: "Self".to_owned(),
        }),
        ..Default::default()
    };
    let resolver = DotConstructorResolver {
        class,
        constructor,
        flow: &flow,
    };
    let index = ValueFlowIndex::build(&empty_catalog, &resolver);
    for callee in ["Config::new", "Config.new", "Config::create"] {
        let result = index.resolve_value(
            class,
            &call(callee),
            SourcePosition {
                line: 20,
                column: 0,
            },
            &resolver,
        );
        if callee == "Config::create" {
            assert!(
                matches!(result, TypeTarget::Unknown),
                "missing constructors must stay unknown: {result:?}"
            );
        } else {
            assert!(
                matches!(result, TypeTarget::Local(node) if node.id == class.id),
                "constructor spelling must normalize: {callee}: {result:?}"
            );
        }
    }
}

#[test]
fn hint_free_value_flow_resolves_only_proven_receiver_chains() {
    let source = r#"
struct LeafBatch9 {}
impl LeafBatch9 { fn finish(&self) {} }
struct HubBatch9 {}
impl HubBatch9 { fn leaf(&self) -> LeafBatch9 { LeafBatch9 {} } }
fn probe(name: &str, path: &std::path::PathBuf, hub: HubBatch9, items: Vec<LeafBatch9>) {
    name.trim().is_empty();
    path.parent().unwrap().exists();
    hub.leaf().finish();
    hub.missing().finish();
    hub.leaf(unknown).finish();
    for mut item in items { item.finish(); }
    let mut bound = LeafBatch9 {}; bound.finish();
}
"#;
    let mut facts = ExtractorHarness::for_lang("rust")
        .at_path("batch9.rs")
        .extract(source);
    for reference in &mut facts.references {
        reference.receiver_hint = None;
    }
    let graph = ExtractorHarness::link_graph(&std::collections::BTreeMap::from([(
        "batch9.rs".into(),
        facts,
    )]));
    for (expression, expected) in [
        ("name.trim().is_empty", true),
        ("path.parent().unwrap().exists", true),
        ("hub.leaf().finish", true),
        ("hub.missing().finish", false),
        ("hub.leaf(unknown).finish", false),
        ("item.finish", true),
        ("bound.finish", true),
    ] {
        let row = graph
            .coverage
            .iter()
            .find(|row| row.expression == expression)
            .unwrap();
        assert_eq!(
            matches!(row.status.as_str(), "resolved" | "external"),
            expected,
            "{expression}: {row:?}"
        );
    }
}

fn configured(base: &str, member: &str, returns: bool) -> TypeTarget<'static> {
    TypeTarget::ConfiguredReceiver {
        authority: std::sync::Arc::new(ConfiguredReceiverAuthority::Hierarchy {
            pack: "supplied-rust-policy".to_owned(),
            rule: std::sync::Arc::new(ClassHierarchyReceiverRule {
                bases: vec![base.to_owned()],
                provider: "supplied".to_owned(),
                members: vec![member.to_owned()],
                returns: if returns {
                    std::collections::BTreeMap::from([(
                        member.to_owned(),
                        ReturnPolicy::Receiver {
                            base: "Replacement".to_owned(),
                        },
                    )])
                } else {
                    Default::default()
                },
            }),
        }),
        origin: Box::new(TypeTarget::External {
            module: base.to_owned(),
            import_line: 1,
        }),
        fallback: None,
        evidence: "supplied-policy".to_owned(),
    }
}
struct SuppliedResolver {
    receiver: TypeTarget<'static>,
    result: Option<TypeTarget<'static>>,
}
impl<'a> SemanticResolver<'a> for SuppliedResolver {
    fn resolve_symbol(
        &self,
        _: &'a Node,
        name: &str,
        _: SourcePosition,
        _: SymbolRole,
    ) -> Symbol<'a> {
        if name == "Supplied" {
            Symbol::Type(self.receiver.clone())
        } else {
            Symbol::Unknown
        }
    }
    fn resolve_member_result(
        &self,
        receiver: &TypeTarget<'a>,
        member: &str,
        _: &'a Node,
        _: SourcePosition,
    ) -> Option<TypeTarget<'a>> {
        let TypeTarget::ConfiguredReceiver { authority, .. } = receiver else {
            return None;
        };
        let ConfiguredReceiverAuthority::Hierarchy { rule, .. } = authority.as_ref() else {
            return None;
        };
        rule.returns.get(member)?;
        self.result.clone()
    }
}

#[test]
fn supplied_return_authority_controls_nested_wrapper_projections() {
    let facts = facts();
    let empty_catalog = std::collections::BTreeMap::new();
    let owner = declaration(&facts, "method", "new");
    let at = after(owner);
    for (base, member, wrappers) in [
        ("rusqlite::Rows", "next", 2),
        ("rusqlite::Row", "get", 1),
        ("std::process::Command", "output", 1),
        ("walkdir::WalkDir", "into_iter", 1),
    ] {
        let expression = supplied_projection(member);
        for returns in [true, false] {
            let resolver = SuppliedResolver {
                receiver: configured(base, member, returns),
                result: Some(TypeTarget::Builtin("Replacement".to_owned())),
            };
            let index = ValueFlowIndex::build(&empty_catalog, &resolver);
            let mut result = index.resolve_value(owner, &expression, at, &resolver);
            if !returns {
                assert!(
                    matches!(result, TypeTarget::Unknown),
                    "{base}.{member}: {result:?}"
                );
                continue;
            }
            if member == "into_iter" {
                let TypeTarget::RustIterator { item } = result else {
                    panic!("{base}.{member}: {result:?}");
                };
                result = *item;
            }
            for depth in 0..wrappers {
                let TypeTarget::RustWrapper { wrapper, ok, .. } = result else {
                    panic!("{base}.{member}: {result:?}");
                };
                assert_eq!(
                    wrapper,
                    if depth == 1 {
                        forge_index::linker::value_flow::RustWrapperKind::Option
                    } else {
                        forge_index::linker::value_flow::RustWrapperKind::Result
                    }
                );
                result = *ok;
            }
            assert!(
                matches!(result, TypeTarget::Builtin(name) if name == "Replacement"),
                "{base}.{member}"
            );
        }
    }
}

#[test]
fn nested_configured_results_without_return_authority_remain_unknown() {
    let facts = facts();
    let empty_catalog = std::collections::BTreeMap::new();
    let owner = declaration(&facts, "method", "new");
    let at = after(owner);
    let resolver = SuppliedResolver {
        receiver: configured("rusqlite::Row", "get", true),
        result: Some(configured("std::result::Result", "unwrap", false)),
    };
    let index = ValueFlowIndex::build(&empty_catalog, &resolver);
    let projected = index.resolve_value(owner, &supplied_projection("get"), at, &resolver);
    assert!(
        matches!(projected, TypeTarget::RustWrapper { ok, .. } if matches!(*ok, TypeTarget::Unknown))
    );
}

#[test]
fn declarations_and_static_constructor_metadata_are_preserved() {
    let facts = facts();
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "struct" && n.name == "Config"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "enum" && n.name == "Status"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "method" && n.name == "new"));
    let constructor = facts
        .nodes
        .iter()
        .find(|n| n.name == "new")
        .expect("new method missing");
    assert_eq!(constructor.details["receiver_type"], "Config");
    assert!(constructor.details.get("is_method").is_none());
    assert_eq!(constructor.details["is_static"], true);
    assert_eq!(
        forge_index::languages::rust::kind("function_item"),
        Some("function")
    );
    let _ = contextunity_forge_mcp::engine::languages::rust::language();
}
