/// Typed framework rule data loaded from `.forge/frameworks/*.toml`, `.yaml`, or `.yml`.
pub mod framework;

pub use framework::{
    ClassHierarchyReceiverRule, DiCallRule, FrameworkManifest, ReturnPolicy, RouteCallRule,
    RouteDecoratorRule, RuleCoverageCode, RuleCoverageDiagnostic, RuleDomain, RuleIdentity,
};

pub(crate) mod projections;
pub(crate) mod semantic;
pub(crate) mod syntax;
pub(crate) use semantic::{
    Activation, SemanticCallee, SemanticOperation, SemanticRule, SemanticTarget,
};

use std::{collections::HashMap, sync::Arc};

/// Resolves a visible named kind without treating an unknown name as kind zero.
pub(crate) fn named_kind(grammar: &tree_sitter::Language, name: &str) -> Option<u16> {
    let kind = grammar.id_for_node_kind(name, true);
    (grammar.node_kind_for_id(kind) == Some(name) && grammar.node_kind_is_named(kind))
        .then_some(kind)
}

#[derive(Debug, Default)]
struct CompiledRuleIR {
    call_kinds: [Option<u16>; 2],
    route_decorators: Vec<CompiledRouteRule<RouteDecoratorRule>>,
    route_calls: Vec<CompiledRouteRule<RouteCallRule>>,
    di_calls: Vec<CompiledRouteRule<DiCallRule>>,
}

#[derive(Clone, Debug)]
pub(crate) struct CompiledRouteRule<T> {
    rule: T,
    pub(crate) origin: Option<crate::core::semantic::ExtractionRuleOrigin>,
}
impl<T> std::ops::Deref for CompiledRouteRule<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.rule
    }
}

impl CompiledRuleIR {
    fn compile(grammar: &tree_sitter::Language, context: &super::LanguageContext) -> Self {
        let call_kinds = [
            named_kind(grammar, "call"),
            named_kind(grammar, "call_expression"),
        ];
        let mut compiled = Self {
            call_kinds,
            ..Self::default()
        };
        if call_kinds.iter().any(Option::is_some) {
            compiled.route_calls = context
                .route_calls
                .iter()
                .enumerate()
                .map(|(index, rule)| CompiledRouteRule {
                    rule: rule.clone(),
                    origin: context.route_rule_origins.get(&(false, index)).cloned(),
                })
                .collect();
            compiled.di_calls = context
                .di_calls
                .iter()
                .enumerate()
                .map(|(index, rule)| CompiledRouteRule {
                    rule: rule.clone(),
                    origin: context.di_rule_origins.get(&index).cloned(),
                })
                .collect();
            if named_kind(grammar, "decorator").is_some() {
                compiled.route_decorators = context
                    .route_decorators
                    .iter()
                    .enumerate()
                    .map(|(index, rule)| CompiledRouteRule {
                        rule: rule.clone(),
                        origin: context.route_rule_origins.get(&(true, index)).cloned(),
                    })
                    .collect();
            }
        }
        compiled
    }

    fn contains_call_kind(&self, kind: u16) -> bool {
        self.call_kinds.contains(&Some(kind))
    }
}

/// Immutable effective-context dispatch plus an optional grammar-bound view.
#[doc(hidden)]
#[derive(Clone, Debug, Default)]
pub struct RuleIRHandle {
    prepared: Arc<HashMap<tree_sitter::Language, Arc<CompiledRuleIR>>>,
    bound: Option<Arc<CompiledRuleIR>>,
}

impl RuleIRHandle {
    pub(crate) fn prepare(context: &super::LanguageContext) -> Self {
        let mut prepared = HashMap::new();
        for profile in super::profiles() {
            for extension in profile.extensions() {
                let grammar = profile.grammar(&format!("_.{extension}"));
                prepared
                    .entry(grammar.clone())
                    .or_insert_with(|| Arc::new(CompiledRuleIR::compile(&grammar, context)));
            }
        }
        Self {
            prepared: Arc::new(prepared),
            bound: None,
        }
    }

    pub(crate) fn bind(
        &self,
        grammar: &tree_sitter::Language,
        context: &super::LanguageContext,
    ) -> Self {
        // Registered grammars are compiled during effective-context preparation.
        // Direct legacy/custom contexts retain their original on-demand behavior.
        let bound = self
            .prepared
            .get(grammar)
            .cloned()
            .unwrap_or_else(|| Arc::new(CompiledRuleIR::compile(grammar, context)));
        Self {
            prepared: Arc::clone(&self.prepared),
            bound: Some(bound),
        }
    }

    pub(crate) fn route_decorators_for_kind(
        &self,
        kind: u16,
    ) -> &[CompiledRouteRule<RouteDecoratorRule>] {
        self.bound
            .as_ref()
            .filter(|compiled| compiled.contains_call_kind(kind))
            .map_or(&[], |compiled| compiled.route_decorators.as_slice())
    }

    pub(crate) fn route_calls_for_kind(&self, kind: u16) -> &[CompiledRouteRule<RouteCallRule>] {
        self.bound
            .as_ref()
            .filter(|compiled| compiled.contains_call_kind(kind))
            .map_or(&[], |compiled| compiled.route_calls.as_slice())
    }

    pub(crate) fn di_calls_for_kind(&self, kind: u16) -> &[CompiledRouteRule<DiCallRule>] {
        self.bound
            .as_ref()
            .filter(|compiled| compiled.contains_call_kind(kind))
            .map_or(&[], |compiled| compiled.di_calls.as_slice())
    }
}
