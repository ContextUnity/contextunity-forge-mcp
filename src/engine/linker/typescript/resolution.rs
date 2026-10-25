use super::super::interner::{LinkerInterner, SymbolId};
use super::super::resolution::ResolutionInput;
use super::super::semantic_context::{Context, LanguageMemberLookup, Member};
use super::super::value_flow::{self, Symbol, SymbolRole, TypeTarget};
use crate::core::models::compact_graph::Graph;
use crate::core::models::{Facts, Node, Reference};
use crate::core::semantic::ObjectOwnershipFact;
use crate::core::typed_facts::FlowState;
use crate::engine::languages::LanguageProfile;
use crate::engine::languages::manifests::syntax::{SyntaxOperation, SyntaxRule};
use hashbrown::{HashMap, HashSet};

pub(in crate::engine::linker) fn builtin_property_chain(
    expression: &str,
    profile: &dyn LanguageProfile,
    path: &str,
    semantic_context: &Context<'_>,
) -> bool {
    if !matches!(profile.id(), "typescript" | "javascript") {
        return false;
    }
    let (head, tail) = expression.split_once('.').unwrap_or(("", ""));
    if !profile.builtin(head) {
        return false;
    }

    let mut current = std::borrow::Cow::Borrowed(head);
    let mut valid = false;
    let mut parts = tail.split('.').peekable();
    while let Some(part) = parts.next() {
        if let Some(next_type) = semantic_context.property_type_for_path(path, &current, part) {
            current = std::borrow::Cow::Owned(next_type);
            if parts.peek().is_none() {
                valid = true;
            }
        } else if parts.peek().is_none() && profile.builtin_member(&current, part) {
            valid = true;
            break;
        } else {
            valid = false;
            break;
        }
    }
    valid
}

pub(in crate::engine::linker) fn admits_static_receiver_candidate(
    candidate: &Node,
    expression: &str,
) -> bool {
    if !super::super::receiver_language_requires_static(candidate)
        || candidate.kind != "method"
        || candidate.details["is_static"] == true
    {
        return true;
    }
    let Some((class_qual, _)) = candidate.qualname.rsplit_once('.') else {
        return true;
    };
    let class_name = class_qual.rsplit('.').next().unwrap_or(class_qual);
    let receiver_name = expression
        .rsplit_once('.')
        .map(|(receiver, _)| receiver.rsplit('.').next().unwrap_or(receiver));
    receiver_name != Some(class_name)
}

pub(in crate::engine::linker) fn object_ownership<'flow>(
    node: &Node,
    flows: Option<&HashMap<SymbolId, &'flow FlowState>>,
    interner: &LinkerInterner<'_>,
    syntax_rules: &[SyntaxRule],
) -> Option<&'flow ObjectOwnershipFact> {
    let id = interner.lookup_symbol(&node.id)?;
    let FlowState::Valid { facts, .. } = flows?.get(&id)? else {
        return None;
    };
    let fact = facts.object_ownership.as_ref()?;
    syntax_rules
        .iter()
        .any(|rule| {
            rule.pack == fact.origin.pack
                && rule.id == fact.origin.rule
                && matches!(rule.operation, SyntaxOperation::ObjectFactory { .. })
        })
        .then_some(fact)
}

pub(in crate::engine::linker) fn lexical_this_calls<'facts, 'references>(
    facts: &'facts Facts,
    references: &'references [Reference],
    object_ownership: impl Fn(&Node) -> Option<&'facts ObjectOwnershipFact>,
) -> HashSet<(&'references str, usize, usize, &'references str)> {
    if !facts.nodes.iter().any(|node| {
        (node.kind == "function"
            && node.details["receiver_name"] == "this"
            && node.details["receiver_type"].is_string())
            || object_ownership(node).is_some()
    }) {
        return HashSet::new();
    }

    references
        .iter()
        .filter(|reference| reference.kind == "calls" && reference.expression.starts_with("this."))
        .map(|reference| {
            (
                reference.source.as_str(),
                reference.line,
                reference.column,
                reference.expression.as_str(),
            )
        })
        .collect()
}

pub(in crate::engine::linker) fn property_reference<'a, F>(
    input: &ResolutionInput<'a, F>,
    reference: &Reference,
    ordinary_member_receivers: &HashSet<&str>,
    active_profile: Option<&dyn LanguageProfile>,
) -> Option<(Member<'a>, bool)> {
    if reference.kind != "references" || reference.receiver_hint.is_some() {
        return None;
    }
    let (owner_id, (receiver, member)) = input
        .interner
        .lookup_symbol(reference.source.as_str())
        .zip(reference.expression.split_once('.'))?;
    let owner = *input.by_id.get(&owner_id)?;
    let prior_grammar = ordinary_member_receivers.contains(receiver)
        || active_profile.is_some_and(|profile| profile.builtin(&reference.expression));
    let candidate = !matches!(receiver, "this" | "self" | "cls")
        && !member.contains('.')
        && matches!(owner.language.as_str(), "typescript" | "javascript" | "vue")
        && owner.kind != "template_scope"
        && !prior_grammar
        && input
            .interner
            .lookup_symbol(receiver)
            .is_some_and(|receiver_id| {
                has_indexed_symbol_id(input.node_bindings, owner_id, receiver_id)
            });
    if !candidate {
        return None;
    }

    let at = reference.position();
    let target = input.value_flow.lookup(owner, receiver, at);
    let object_factory = input
        .semantic_context
        .factory_member_receiver(owner, receiver, at);
    let property_read = matches!(owner.language.as_str(), "typescript" | "javascript");
    if !object_factory && !property_read {
        return Some((Member::Unknown, false));
    }
    let result = target.map_or(Member::Unknown, |target| {
        if object_factory {
            input
                .semantic_context
                .lookup_member_mode(target, member, owner, false)
        } else {
            input
                .semantic_context
                .lookup_property(target, member, owner)
        }
    });
    if !object_factory && matches!(result, Member::Unknown) {
        return Some((Member::Unknown, false));
    }
    Some((result, true))
}

#[cfg(feature = "lang-typescript")]
pub(in crate::engine::linker) fn lexical_this_member<'a, F>(
    input: &ResolutionInput<'a, F>,
    path: &str,
    reference: &Reference,
    expression: &str,
    lexical_this_calls: &HashSet<(&str, usize, usize, &str)>,
    lookup_key: &mut String,
    graph: &mut Graph,
) -> bool {
    if !matches!(reference.kind.as_str(), "calls" | "references" | "mutates") {
        return false;
    }
    let Some(("this", member_name)) = expression.split_once('.') else {
        return false;
    };
    if member_name.is_empty() || member_name.contains('.') {
        return false;
    }
    let Some(owner) = input
        .interner
        .lookup_node(input.by_id, reference.source.as_str())
        .filter(|owner| {
            (owner.kind == "function"
                && matches!(owner.language.as_str(), "typescript" | "javascript")
                || owner.kind == "method"
                    && owner.language == "javascript"
                    && owner.details["prototype_valid"] == true)
                && owner.details["receiver_name"] == "this"
        })
    else {
        return false;
    };
    let Some(receiver_type) = owner.details["receiver_type"].as_str() else {
        return false;
    };

    let mut scope = owner
        .qualname
        .rsplit_once('.')
        .map_or("", |(parent, _)| parent);
    let mut class = None;
    while !scope.is_empty() {
        if let Some(candidates) = input.by_qual.get(scope) {
            let mut local = candidates.iter().copied().filter(|candidate| {
                candidate.path == *path
                    && candidate.kind == "class"
                    && candidate.name == receiver_type
            });
            if let Some(candidate) = local.next().filter(|_| local.next().is_none()) {
                class = Some(candidate);
                break;
            }
        }
        scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
    }
    if class.is_none()
        && owner.language == "javascript"
        && (owner.details["prototype_valid"] == true
            || owner.details["prototype_lexical_this"] == true)
    {
        let at = reference.position();
        if let Symbol::Type(TypeTarget::Local(candidate)) =
            value_flow::SemanticResolver::resolve_symbol(
                input.semantic_context,
                owner,
                receiver_type,
                at,
                SymbolRole::Constructor,
            )
        {
            if candidate.path == *path
                && candidate.name == receiver_type
                && (candidate.kind == "class"
                    || candidate.kind == "function"
                        && candidate.details["prototype_constructor"] == true)
            {
                class = Some(candidate);
            }
        }
    }
    let Some(class) = class else {
        return false;
    };

    let associated = owner.details["is_static"] == true;
    let member_lookup = LanguageMemberLookup::new(input.semantic_context);
    let methods = member_lookup.typescript(class, member_name, associated);
    let method = match methods {
        [method]
            if super::super::typescript::semantic_context::prototype_visible_at(
                method,
                path,
                reference.line,
                reference.column,
            ) =>
        {
            Some(*method)
        }
        _ => None,
    };
    let mut fields = input
        .by_qual
        .get(super::super::qualified(
            lookup_key,
            &class.qualname,
            member_name,
        ))
        .into_iter()
        .flatten()
        .copied()
        .filter(|field| {
            field.path == *path
                && field.kind == "field"
                && (field.details["is_static"] == true) == associated
        });
    let field = fields.next();
    let ambiguous =
        methods.len() > 1 || fields.next().is_some() || (field.is_some() && method.is_some());
    if ambiguous {
        graph.reference_coverage(
            path,
            reference,
            "unresolved",
            "lexical this has ambiguous class members in this file".into(),
        );
        return true;
    }
    let target = if reference.kind == "calls" {
        method
    } else {
        field
    };
    if let Some(target) = target {
        graph.reference_edge(
            path,
            reference,
            target.id.clone(),
            reference.kind.as_str(),
            "exact",
        );
        graph.reference_coverage(
            path,
            reference,
            "resolved",
            format!("lexically captured this member {}", target.qualname).into(),
        );
        return true;
    }
    if let ("references", Some(method)) = (reference.kind.as_str(), method) {
        if !lexical_this_calls.contains(&(
            reference.source.as_str(),
            reference.line,
            reference.column,
            reference.expression.as_str(),
        )) {
            graph.reference_edge(path, reference, method.id.clone(), "references", "exact");
        }
        graph.reference_coverage(
            path,
            reference,
            "resolved",
            format!("lexically captured this method {}", method.qualname).into(),
        );
        return true;
    }
    graph.reference_coverage(
        path,
        reference,
        "unresolved",
        "lexical this has no matching class member in this file and static mode".into(),
    );
    true
}

pub(in crate::engine::linker) fn has_indexed_name(
    interner: &LinkerInterner<'_>,
    index: &super::super::NodeBindingIndex,
    node: &Node,
    name: &str,
) -> bool {
    interner
        .lookup_symbol(name)
        .is_some_and(|name| has_indexed_symbol(interner, index, node, name))
}

pub(in crate::engine::linker) fn has_indexed_symbol(
    interner: &LinkerInterner<'_>,
    index: &super::super::NodeBindingIndex,
    node: &Node,
    name: SymbolId,
) -> bool {
    interner
        .lookup_symbol(&node.id)
        .is_some_and(|node| has_indexed_symbol_id(index, node, name))
}

pub(in crate::engine::linker) fn has_indexed_symbol_id(
    index: &super::super::NodeBindingIndex,
    node: SymbolId,
    name: SymbolId,
) -> bool {
    index
        .get(&node)
        .is_some_and(|names| names.binary_search(&name).is_ok())
}
