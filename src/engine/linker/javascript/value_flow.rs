use super::super::value_flow::{Binding, ReadTrace, SemanticResolver, TypeTarget, ValueFlowIndex};
use crate::core::{models::Node, semantic::SourcePosition};

pub(in crate::engine::linker) fn construct<'a>(
    owner: &Node,
    callee: &str,
) -> Option<TypeTarget<'a>> {
    if matches!(owner.language.as_str(), "javascript" | "typescript" | "vue") && callee == "[]" {
        Some(TypeTarget::Builtin("Array".to_owned()))
    } else {
        None
    }
}

pub(in crate::engine::linker) fn evaluate_call<'a>(
    index: &ValueFlowIndex<'a>,
    owner: &'a Node,
    callee: &str,
    at: SourcePosition,
    resolver: &dyn SemanticResolver<'a>,
    trace: Option<&ReadTrace<'a, '_>>,
) -> Option<TypeTarget<'a>> {
    let callee = callee.trim();
    if callee.ends_with(')') {
        if let Some(base) = strip_call_arguments(callee) {
            return evaluate_call(index, owner, base, at, resolver, trace);
        }
    }
    if let Some(target) = resolver.resolve_external_factory(owner, callee, at) {
        return Some(target);
    }
    if let Some((receiver_expr, member)) = callee.rsplit_once('.') {
        let receiver_type = if receiver_expr.ends_with(')') {
            evaluate_call(index, owner, receiver_expr, at, resolver, trace)
        } else {
            let (base_receiver, fields) =
                receiver_expr.split_once('.').unwrap_or((receiver_expr, ""));
            let mut target = index
                .lookup_tracked(owner, base_receiver, at, trace)
                .cloned()
                .or_else(|| {
                    match resolver.resolve_symbol(
                        owner,
                        base_receiver,
                        at,
                        super::super::value_flow::SymbolRole::Type,
                    ) {
                        super::super::value_flow::Symbol::Type(target) => Some(target),
                        _ => None,
                    }
                });
            for field in fields.split('.').filter(|field| !field.is_empty()) {
                let next = match target.as_ref() {
                    Some(TypeTarget::Local(class)) => {
                        index.field_type_tracked(class, field, trace).cloned()
                    }
                    Some(TypeTarget::Builtin(name)) => resolver
                        .property_type(owner, name, field)
                        .map(TypeTarget::Builtin),
                    _ => None,
                };
                target = next;
            }
            target
        };
        return receiver_type
            .as_ref()
            .and_then(|receiver| platform_member_return(owner, receiver, member));
    }
    None
}

pub(in crate::engine::linker) fn platform_member_return<'a>(
    owner: &Node,
    receiver: &TypeTarget<'a>,
    member: &str,
) -> Option<TypeTarget<'a>> {
    if !matches!(owner.language.as_str(), "javascript" | "typescript" | "vue") {
        return None;
    }
    if let TypeTarget::ConfiguredReceiver { fallback, .. } = receiver {
        return fallback
            .as_deref()
            .and_then(|fallback| platform_member_return(owner, fallback, member));
    }
    let TypeTarget::Builtin(receiver) = receiver else {
        return None;
    };
    let name = super::super::typescript::semantic_context::typed_dom_receiver(receiver)
        .unwrap_or(receiver);
    let returned = match (name, member) {
        (
            "Array",
            "filter" | "map" | "slice" | "concat" | "flat" | "flatMap" | "reverse" | "sort"
            | "splice",
        ) => "Array",
        ("Promise", "then" | "catch" | "finally") => "Promise",
        (
            name,
            "querySelector" | "closest" | "appendChild" | "removeChild" | "cloneNode"
            | "insertBefore" | "replaceChild",
        ) if matches!(name, "Element" | "HTMLElement" | "Node")
            || super::super::typescript::semantic_context::typed_dom_receiver(name)
                == Some("Element") =>
        {
            "Element"
        }
        ("ParentNode" | "DocumentFragment", "querySelector") => "Element",
        ("Document", "createElement" | "getElementById" | "querySelector") => "Element",
        _ => return None,
    };
    Some(TypeTarget::Builtin(returned.to_owned()))
}

pub(in crate::engine::linker) fn stable_immutable_capture(writes: &[Binding<'_>]) -> bool {
    let Some((initializer, sentinels)) = writes.split_last() else {
        return false;
    };
    (2..=3).contains(&writes.len())
        && initializer.immutable_initializer
        && !matches!(
            &initializer.target,
            TypeTarget::Unknown | TypeTarget::Ambiguous
        )
        && sentinels.iter().all(|binding| {
            !binding.immutable_initializer && matches!(&binding.target, TypeTarget::Unknown)
        })
}

fn strip_call_arguments(expression: &str) -> Option<&str> {
    if !expression.ends_with(')') {
        return None;
    }
    let mut depth = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    for (index, character) in expression.char_indices().rev() {
        match character {
            '\'' if !in_double_quote => in_single_quote = !in_single_quote,
            '"' if !in_single_quote => in_double_quote = !in_double_quote,
            ')' if !in_single_quote && !in_double_quote => depth += 1,
            '(' if !in_single_quote && !in_double_quote => {
                depth -= 1;
                if depth == 0 {
                    return Some(expression[..index].trim());
                }
            }
            _ => {}
        }
    }
    None
}
