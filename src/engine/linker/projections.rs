use super::{
    named_bindings, semantic_context,
    value_flow::{SemanticResolver, Symbol, SymbolRole, TypeTarget},
};
use crate::core::{
    models::{
        Node,
        compact_graph::Graph,
    },
    semantic::{NamedBindingRole, StaticString, SymbolOperand, ValueFlowFacts},
};
use crate::engine::languages::dependency_registry::DependencyRegistry;
use hashbrown::HashMap;

fn declaration(symbol: Symbol<'_>) -> Result<Option<&Node>, ()> {
    match symbol {
        Symbol::Callable(node) | Symbol::Type(TypeTarget::Local(node)) => Ok(Some(node)),
        Symbol::Ambiguous | Symbol::Type(TypeTarget::Ambiguous) => Err(()),
        _ => Ok(None),
    }
}

fn operand<'a>(
    owner: &'a Node,
    value: &SymbolOperand,
    at: crate::core::semantic::SourcePosition,
    role: SymbolRole,
    context: &semantic_context::Context<'a>,
    nodes: &HashMap<super::interner::SymbolId, &'a Node>,
) -> Result<Option<&'a Node>, ()> {
    let target = match value {
        SymbolOperand::Node { id } => Ok(context
            .symbol_id(id)
            .and_then(|id| nodes.get(&id).copied())
            .filter(|node| node.path == owner.path)),
        SymbolOperand::Name { expression } => {
            declaration(context.resolve_symbol(owner, expression, at, role))
        }
        SymbolOperand::Unknown => Ok(None),
    }?;
    Ok(target.filter(|node| match role {
        SymbolRole::Callable => matches!(node.kind.as_str(), "function" | "method"),
        SymbolRole::Type | SymbolRole::Constructor => node.kind == "class",
    }))
}

fn emit(
    graph: &mut Graph,
    owner: &Node,
    target: Result<Option<&Node>, ()>,
    kind: &str,
    line: usize,
    expression: String,
) {
    let status = match target {
        Ok(Some(target)) => {
            graph.push_edge(
                owner.path.as_str(),
                owner.id.clone(),
                target.id.clone(),
                kind,
                line,
                expression.as_str(),
                "exact",
            );
            "resolved"
        }
        Ok(None) => "unresolved",
        Err(()) => "ambiguous",
    };
    graph.push_coverage(
        owner.path.as_str(),
        line,
        expression,
        status,
        "configured projection; unique canonical declaration required",
    );
}

pub(super) fn compose<'a>(
    graph: &mut Graph,
    owner: &'a Node,
    flow: &ValueFlowFacts,
    context: &semantic_context::Context<'a>,
    nodes: &HashMap<super::interner::SymbolId, &'a Node>,
    dependencies: &DependencyRegistry,
) {
    if flow.fact_limit_exceeded {
        return;
    }
    for fact in flow.symbol_attachments() {
        if !named_bindings::active(&fact.origin, &owner.path, dependencies) {
            continue;
        }
        let source = operand(
            owner,
            &fact.source,
            fact.position,
            SymbolRole::Callable,
            context,
            nodes,
        );
        let target = operand(
            owner,
            &fact.target,
            fact.position,
            SymbolRole::Type,
            context,
            nodes,
        );
        let expression = match &fact.target {
            SymbolOperand::Name { expression } => expression.clone(),
            _ => "configured attachment".into(),
        };
        let kind = match fact.relation {
            crate::core::semantic::AttachmentRelation::UsesModel => "uses_model",
            crate::core::semantic::AttachmentRelation::References => "references",
        };
        match (source, target) {
            (Ok(Some(source)), target) => {
                emit(graph, source, target, kind, fact.position.line, expression);
            }
            (Err(()), _) | (_, Err(())) => {
                emit(graph, owner, Err(()), kind, fact.position.line, expression);
            }
            _ => emit(graph, owner, Ok(None), kind, fact.position.line, expression),
        }
    }
    for fact in flow.named_bindings() {
        if !matches!(fact.role, NamedBindingRole::Reference)
            || !named_bindings::active(&fact.origin, &owner.path, dependencies)
        {
            continue;
        }
        let rules = dependencies.language_context_for_path(&owner.path);
        if rules.syntax_rules.iter().any(|rule| rule.pack == fact.origin.pack && rule.id == fact.origin.rule && matches!(&rule.operation, crate::engine::languages::manifests::syntax::SyntaxOperation::NamedBinding { binding: crate::engine::languages::manifests::projections::NamedBindingForm::MemberReference { .. }, .. })) { continue; }
        let (expression, target) = match &fact.key {
            StaticString::Literal { value } => (
                value.clone(),
                declaration(context.named_target(owner, &fact.domain, value, Some(&fact.origin))),
            ),
            StaticString::Unknown => ("dynamic named binding".into(), Ok(None)),
        };
        emit(
            graph,
            owner,
            target,
            "references",
            fact.position.line,
            expression,
        );
    }
    for fact in flow.endpoint_requests() {
        if !named_bindings::active(&fact.origin, &owner.path, dependencies) {
            continue;
        }
        if matches!(
            (&fact.url, &fact.method),
            (StaticString::Literal { .. }, StaticString::Literal { .. })
        ) {
            // Known requests use the ordinary indexed request/route composer.
            continue;
        }
        let expression = match &fact.url {
            StaticString::Literal { value } => value.clone(),
            _ => "dynamic endpoint".into(),
        };
        emit(
            graph,
            owner,
            Ok(None),
            "calls_endpoint",
            fact.position.line,
            expression,
        );
    }
}
