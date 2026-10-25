use super::super::resolution::ResolutionInput;
use super::super::semantic_context::Member;
use crate::core::models::{Facts, Node, ReceiverHint, Reference, compact_graph::Graph};
use crate::core::semantic::ComponentMacroRole;
use crate::engine::languages::LanguageContext;

pub(in crate::engine::linker) fn component_macro_callable_origin(
    reference: &Reference,
    context: &LanguageContext,
) -> Option<String> {
    let ReceiverHint::ConfiguredMacroCallable { origin, producer } =
        reference.receiver_hint.as_ref()?
    else {
        return None;
    };
    context
        .component_macro(producer)
        .filter(|(rule, role)| {
            rule.pack == origin.pack && rule.id == origin.rule && *role == ComponentMacroRole::Emits
        })
        .map(|(rule, _)| {
            format!(
                "{}; rule:{}:{}",
                rule.display
                    .builtin
                    .as_deref()
                    .unwrap_or("configured macro"),
                rule.pack,
                rule.id
            )
        })
}

pub(in crate::engine::linker) fn is_component_template_member(
    reference: &Reference,
    owner: Option<&Node>,
    tail: &str,
) -> bool {
    reference.kind == "references"
        && !tail.is_empty()
        && owner.is_some_and(|owner| {
            owner.kind == "template_scope" && owner.details["vue_slot"].is_object()
        })
}

pub(in crate::engine::linker) fn template_lexical_binding<'a, F>(
    input: &ResolutionInput<'a, F>,
    path: &str,
    reference: &Reference,
    owner: Option<&'a Node>,
    first: &str,
    component_module: Option<&'a Node>,
) -> Option<&'a Node>
where
    F: AsRef<Facts> + Sync,
{
    let owner = owner?;
    if let Some(first_id) = input.interner.lookup_symbol(first) {
        let mut lexical_scope = owner.qualname.as_str();
        while !lexical_scope.is_empty() {
            let binding = input.by_qual.get(lexical_scope).and_then(|nodes| {
                nodes.iter().copied().find(|node| {
                    node.path == path
                        && node.kind == "template_scope"
                        && super::super::typescript::resolution::has_indexed_symbol(
                            input.interner,
                            input.node_bindings,
                            node,
                            first_id,
                        )
                })
            });
            if binding.is_some() {
                return binding;
            }
            lexical_scope = lexical_scope
                .rsplit_once('.')
                .map_or("", |(parent, _)| parent);
        }
    }

    component_module.filter(|_| {
        matches!(
            reference.receiver_hint.as_ref(),
            Some(ReceiverHint::VueSetupBinding)
        )
    })
}

pub(in crate::engine::linker) fn component_slot_member<'a, 'aliases, F, A>(
    input: &ResolutionInput<'a, F>,
    path: &str,
    owner: &'a Node,
    first: &str,
    tail: &str,
    alias_for: A,
) -> Option<Member<'a>>
where
    F: AsRef<Facts> + Sync,
    'a: 'aliases,
    A: Fn(&str, Option<&'a Node>, bool) -> Option<&'aliases Vec<&'a Node>>,
{
    let mut scope = owner.qualname.as_str();
    while !scope.is_empty() {
        let mut scopes = input
            .by_qual
            .get(scope)
            .into_iter()
            .flatten()
            .copied()
            .filter(|node| node.path == path && node.kind == "template_scope");
        if let (Some(binding_scope), None) = (scopes.next(), scopes.next()) {
            let declares = binding_scope.binding_names().any(|name| name == first);
            if declares {
                let component = binding_scope.details["vue_slot"]["component"].as_str();
                let slot = binding_scope.details["vue_slot"]["slot"].as_str();
                if let (Some(component), Some(slot)) = (component, slot) {
                    if let Some([module]) = alias_for(component, Some(binding_scope), false)
                        .map(Vec::as_slice)
                        .filter(|targets| targets.len() == 1)
                    {
                        if module.kind == "module" {
                            if let Some(receiver) =
                                input.value_flow.component_slot_type(module, slot, first)
                            {
                                let result = input
                                    .semantic_context
                                    .lookup_component_template_field(receiver, tail);
                                if matches!(result, Member::Local(_)) {
                                    return Some(result);
                                }
                            }
                        }
                    }
                }
                break;
            }
        }
        scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
    }
    None
}

pub(in crate::engine::linker) fn resolve_component_tag<'a, F>(
    input: &ResolutionInput<'a, F>,
    path: &str,
    reference: &Reference,
    owner: Option<&Node>,
    graph: &mut Graph,
) -> bool {
    if reference.kind != "references"
        || !owner.is_some_and(|owner| owner.kind == "template_scope")
        || !matches!(
            reference.receiver_hint.as_ref(),
            Some(ReceiverHint::VueComponentTag)
        )
    {
        return false;
    }

    match input
        .semantic_context
        .component_candidates(path, &reference.expression)
        .as_slice()
    {
        [component] => {
            graph.reference_edge(path, reference, component.id.clone(), "references", "exact");
            graph.reference_coverage(
                path,
                reference,
                "resolved",
                format!("configured components directory: {}", component.path).into(),
            );
        }
        candidates if candidates.len() > 1 => {
            graph.reference_coverage(
                path,
                reference,
                "ambiguous",
                "multiple configured components share this registered name".into(),
            );
        }
        _ => {
            if let Some(origin) = input
                .semantic_context
                .component_builtin(path, &reference.expression)
            {
                graph.reference_coverage(
                    path,
                    reference,
                    "external",
                    format!("{origin} component registered for this project").into(),
                );
            } else {
                graph.reference_coverage(
                    path,
                    reference,
                    "unresolved",
                    "unregistered Vue template component tag".into(),
                );
            }
        }
    }
    true
}
