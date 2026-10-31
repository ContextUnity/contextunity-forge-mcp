use super::*;

pub(super) fn resolve_computed_call<F>(
    input: &ResolutionInput<'_, F>,
    path: &str,
    reference: &Reference,
    active_profile: Option<&dyn LanguageProfile>,
    builtin_origin: Option<&'static str>,
    graph: &mut Graph,
) -> bool
where
    F: AsRef<Facts> + Sync,
{
    if !(matches!(reference.kind.as_str(), "calls" | "handles")
        || (reference.kind == "references"
            && active_profile
                .is_some_and(|profile| matches!(profile.id(), "javascript" | "typescript"))
            && matches!(
                reference.receiver_hint,
                Some(ReceiverHint::TypedCallResult { .. })
            )))
    {
        return false;
    }
    let Some(owner) = input
        .interner
        .lookup_node(input.by_id, reference.source.as_str())
    else {
        return false;
    };
    if let Some(
        ReceiverHint::CallResult { member, .. }
        | ReceiverHint::ConstructorResult { member, .. }
        | ReceiverHint::TypedCallResult { member, .. },
    ) = &reference.receiver_hint
    {
        if let Some(receiver) = input
            .value_flow
            .computed_receiver(reference)
            .filter(|receiver| !matches!(receiver, value_flow::TypeTarget::Unknown))
        {
            let (fields, final_member) = member.rsplit_once('.').unwrap_or(("", member));
            if let Some(nested_receiver) = super::super::chained_member_receiver(
                receiver,
                fields,
                owner,
                path,
                reference.position(),
                input,
                active_profile,
            ) {
                let resolved_receiver = nested_receiver.as_ref();
                let result =
                    input
                        .semantic_context
                        .lookup_member(resolved_receiver, final_member, owner);
                let result = if reference.kind == "references"
                    && matches!(result, semantic_context::Member::Unknown)
                {
                    input
                        .semantic_context
                        .lookup_property(resolved_receiver, final_member, owner)
                } else {
                    result
                };
                let result = match result {
                    semantic_context::Member::Local(node)
                        if !super::super::super::typescript::semantic_context::prototype_visible_at(
                            node,
                            path,
                            reference.line,
                            reference.column,
                        ) =>
                    {
                        semantic_context::Member::Unknown
                    }
                    other => other,
                };
                let origin = super::super::builtin_member_origin(
                    resolved_receiver,
                    active_profile.filter(|profile| profile.id() != "python"),
                    builtin_origin,
                );
                emit_inferred(graph, path, reference, result, origin);
                return true;
            }
        }
    }
    if let Some(ReceiverHint::StringLiteral { member }) = &reference.receiver_hint {
        if active_profile.is_some_and(|profile| profile.builtin_member("string", member)) {
            emit_inferred(
                graph,
                path,
                reference,
                semantic_context::Member::Builtin,
                builtin_origin,
            );
            return true;
        }
    }
    false
}

pub(super) fn resolve_candidate_join(
    path: &str,
    reference: &Reference,
    tail: &str,
    owner: &Node,
    inferred: Option<&value_flow::TypeTarget<'_>>,
    semantic_context: &semantic_context::Context<'_>,
    graph: &mut Graph,
) -> bool {
    let Some(value_flow::TypeTarget::Candidates {
        targets,
        complete,
        exhausted,
    }) = inferred
    else {
        return false;
    };
    if reference.kind != "calls" {
        return false;
    }

    let (fields, member) = tail.rsplit_once('.').unwrap_or(("", tail));
    let mut candidates = Vec::new();
    let mut member_complete = *complete && !*exhausted && fields.is_empty();
    if fields.is_empty() && !member.is_empty() && !exhausted {
        for class in targets {
            match semantic_context.lookup_member_mode(
                &value_flow::TypeTarget::Local(class),
                member,
                owner,
                false,
            ) {
                semantic_context::Member::Local(method)
                    if method.kind == "method"
                        && super::super::super::typescript::semantic_context::prototype_visible_at(
                            method,
                            path,
                            reference.line,
                            reference.column,
                        ) =>
                {
                    candidates.push(method.id.clone());
                }
                _ => member_complete = false,
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    let candidate_names = targets
        .iter()
        .map(|target| target.qualname.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let (status, evidence): (&str, std::borrow::Cow<'static, str>) = if *exhausted {
        (
            "unresolved",
            format!(
                "candidate branch join overflow: more than {} distinct declarations; Unknown/Incomplete; no truncated target set emitted",
                value_flow::MAX_CANDIDATE_BRANCH_JOINS
            )
            .into(),
        )
    } else if member_complete && candidates.len() > 1 {
        (
            "ambiguous",
            format!(
                "complete local branch join (K={}) possible receiver types: {candidate_names}",
                value_flow::MAX_CANDIDATE_BRANCH_JOINS
            )
            .into(),
        )
    } else if member_complete {
        (
            "unresolved",
            format!(
                "complete local branch join (K={}) has one possible call target: {candidate_names}",
                value_flow::MAX_CANDIDATE_BRANCH_JOINS
            )
            .into(),
        )
    } else {
        (
            "unresolved",
            format!(
                "incomplete local branch join (K={}); unknown or unsupported alternatives retained; known receiver types: {candidate_names}",
                value_flow::MAX_CANDIDATE_BRANCH_JOINS
            )
            .into(),
        )
    };
    graph.reference_coverage(path, reference, status, evidence);
    for target in candidates {
        graph.reference_edge(path, reference, target, "possible_calls", "possible");
    }
    true
}
