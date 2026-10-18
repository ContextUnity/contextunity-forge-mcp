/// Typed framework rule data loaded from `.forge/frameworks/*.toml`, `.yaml`, or `.yml`.
pub mod framework;

pub use framework::{
    ClassHierarchyReceiverRule, DiCallRule, FrameworkManifest, FrameworkManifestValue,
    RouteCallRule, RouteDecoratorRule, RuleCoverageCode, RuleCoverageDiagnostic, RuleDomain,
    RuleIdentity,
};

pub(crate) mod semantic;
pub(crate) use semantic::{Activation, SemanticCallee, SemanticOperation, SemanticRule};

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

/// Resolves a visible named kind without treating an unknown name as kind zero.
pub(crate) fn named_kind(grammar: &tree_sitter::Language, name: &str) -> Option<u16> {
    let kind = grammar.id_for_node_kind(name, true);
    (grammar.node_kind_for_id(kind) == Some(name) && grammar.node_kind_is_named(kind))
        .then_some(kind)
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct RuleIRKey {
    grammar: tree_sitter::Language,
    workspace: Arc<str>,
}

#[derive(Debug, Default)]
struct CompiledRuleIR {
    route_decorators: HashMap<u16, Vec<RouteDecoratorRule>>,
    route_calls: HashMap<u16, Vec<RouteCallRule>>,
    di_calls: HashMap<u16, Vec<DiCallRule>>,
}

impl CompiledRuleIR {
    fn compile(grammar: &tree_sitter::Language, context: &super::LanguageContext) -> Self {
        let mut compiled = Self::default();
        for kind in ["call", "call_expression"]
            .into_iter()
            .filter_map(|name| named_kind(grammar, name))
        {
            compiled
                .route_calls
                .insert(kind, context.route_calls.clone());
            compiled.di_calls.insert(kind, context.di_calls.clone());
            if named_kind(grammar, "decorator").is_some() {
                compiled
                    .route_decorators
                    .insert(kind, context.route_decorators.clone());
            }
        }
        compiled
    }
}

/// Registry-generation cache plus an optional grammar-bound typed rule dispatch.
#[doc(hidden)]
#[derive(Clone, Debug, Default)]
pub struct RuleIRHandle {
    cache: Arc<RwLock<HashMap<RuleIRKey, Arc<CompiledRuleIR>>>>,
    workspace: Arc<str>,
    bound: Option<Arc<CompiledRuleIR>>,
}

impl RuleIRHandle {
    pub(crate) fn for_workspace(&self, workspace: &str) -> Self {
        Self {
            cache: self.cache.clone(),
            workspace: workspace.into(),
            bound: None,
        }
    }

    pub(crate) fn bind(
        &self,
        grammar: &tree_sitter::Language,
        context: &super::LanguageContext,
    ) -> Self {
        // Manifest route/DI rules are fixed for this registry generation and workspace.
        // Semantic rules remain path-scoped, so callers dispatch them from LanguageContext.
        let key = RuleIRKey {
            grammar: grammar.clone(),
            workspace: self.workspace.clone(),
        };
        {
            let cache = self.cache.read().expect("framework rule cache lock");
            if let Some(compiled) = cache.get(&key) {
                return Self {
                    cache: self.cache.clone(),
                    workspace: self.workspace.clone(),
                    bound: Some(compiled.clone()),
                };
            }
        }
        let mut cache = self.cache.write().expect("framework rule cache lock");
        let bound = if let Some(compiled) = cache.get(&key) {
            compiled.clone()
        } else {
            let compiled = Arc::new(CompiledRuleIR::compile(grammar, context));
            // Bound cache growth to this registry generation. Existing handles retain
            // their immutable dispatch even when the reusable cache is cleared.
            if cache.len() >= 256 {
                cache.clear();
            }
            cache.insert(key, compiled.clone());
            compiled
        };
        Self {
            cache: self.cache.clone(),
            workspace: self.workspace.clone(),
            bound: Some(bound),
        }
    }

    pub(crate) fn route_decorators_for_kind(&self, kind: u16) -> &[RouteDecoratorRule] {
        self.bound
            .as_ref()
            .and_then(|compiled| compiled.route_decorators.get(&kind))
            .map_or(&[], Vec::as_slice)
    }

    pub(crate) fn route_calls_for_kind(&self, kind: u16) -> &[RouteCallRule] {
        self.bound
            .as_ref()
            .and_then(|compiled| compiled.route_calls.get(&kind))
            .map_or(&[], Vec::as_slice)
    }

    pub(crate) fn di_calls_for_kind(&self, kind: u16) -> &[DiCallRule] {
        self.bound
            .as_ref()
            .and_then(|compiled| compiled.di_calls.get(&kind))
            .map_or(&[], Vec::as_slice)
    }
}
