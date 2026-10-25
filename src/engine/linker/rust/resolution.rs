use super::super::semantic_context::LanguageMemberLookup;
use crate::core::models::{
    Node, Reference,
    compact_graph::{Coverage, Graph},
};
use crate::engine::languages::LanguageProfile;
use hashbrown::HashMap;

#[cfg(feature = "lang-rust")]
pub(in crate::engine::linker) fn rust_explicit_module_target(
    profile: Option<&dyn LanguageProfile>,
    path: &str,
    file_module: &str,
    expression: &str,
) -> Option<String> {
    let profile = profile.filter(|profile| profile.id() == "rust")?;
    if let Some(mut tail) = expression.strip_prefix("super::") {
        let mut module = file_module.rsplit_once('.')?.0;
        while let Some(rest) = tail.strip_prefix("super::") {
            module = module.rsplit_once('.')?.0;
            tail = rest;
        }
        Some(format!("{module}.{}", tail.replace("::", ".")))
    } else if let Some(tail) = expression.strip_prefix("self::") {
        Some(format!("{file_module}.{}", tail.replace("::", ".")))
    } else if expression.starts_with("crate::") {
        profile
            .normalize_import(path, expression)
            .map(|module| module.namespace)
    } else {
        None
    }
}

pub(in crate::engine::linker) fn resolve_self_reference(
    profile: Option<&dyn LanguageProfile>,
    reference: &Reference,
    owner: Option<&Node>,
    path: &str,
    member_lookup: &LanguageMemberLookup<'_, '_>,
    graph: &mut Graph,
) -> bool {
    if !profile.is_some_and(|profile| profile.id() == "rust")
        || !(reference.expression == "Self"
            || reference.expression.starts_with("Self::")
            || reference.expression.starts_with("Self."))
    {
        return false;
    }
    let Some(owner) = owner else {
        graph.reference_coverage(
            path,
            reference,
            "unresolved",
            "Self has no indexed impl receiver".into(),
        );
        return true;
    };
    let Some(receiver) = member_lookup.rust_receiver(owner) else {
        graph.reference_coverage(
            path,
            reference,
            "unresolved",
            "Self has no indexed impl receiver".into(),
        );
        return true;
    };
    let member = reference
        .expression
        .strip_prefix("Self::")
        .or_else(|| reference.expression.strip_prefix("Self."));
    if let Some(member) = member {
        let targets = member_lookup.rust(receiver, member, true, owner);
        match targets {
            [target] => {
                graph.reference_edge(
                    path,
                    reference,
                    target.id.clone(),
                    reference.kind.as_str(),
                    "inferred",
                );
                graph.reference_coverage(
                    path,
                    reference,
                    "resolved",
                    format!(
                        "Self::{member} resolves to declared associated member on {}",
                        receiver.qualname
                    )
                    .into(),
                );
            }
            [] => graph.reference_coverage(
                path,
                reference,
                "unresolved",
                format!(
                    "Self::{member} has no declared associated member on {}",
                    receiver.qualname
                )
                .into(),
            ),
            _ => graph.reference_coverage(
                path,
                reference,
                "ambiguous",
                format!(
                    "Self::{member} has multiple visible associated members on {}",
                    receiver.qualname
                )
                .into(),
            ),
        }
        return true;
    }
    if reference.kind == "references" {
        graph.reference_edge(
            path,
            reference,
            receiver.id.clone(),
            "references",
            "inferred",
        );
        graph.reference_coverage(
            path,
            reference,
            "resolved",
            format!("Self refers to indexed receiver type {}", receiver.qualname).into(),
        );
        return true;
    }
    false
}

pub(in crate::engine::linker) fn trait_object_call<'a>(
    profile: Option<&dyn LanguageProfile>,
    reference: &Reference,
    owner: Option<&Node>,
    path: &str,
    value_flow: &crate::engine::linker::value_flow::ValueFlowIndex<'a>,
    semantic_context: &super::super::semantic_context::Context<'a>,
    graph: &mut Graph,
) -> bool {
    if reference.kind != "calls" || !profile.is_some_and(|profile| profile.id() == "rust") {
        return false;
    }
    let Some(owner) = owner else {
        return false;
    };
    let Some((receiver, member)) = reference.expression.split_once('.') else {
        return false;
    };
    let flow_trait_object = value_flow
        .lookup(owner, receiver, reference.position())
        .and_then(super::value_flow::trait_object);
    let is_trait_object = owner
        .parameter_type(receiver)
        .is_some_and(|ty| ty.split_whitespace().any(|part| part == "dyn"))
        || flow_trait_object.is_some();
    if !is_trait_object {
        return false;
    }

    graph.coverage.push(Coverage {
        path: path.to_owned(),
        line: reference.line,
        expression: "trait_object_dispatch".into(),
        status: "unresolved".into(),
        evidence: format!(
            "analysis gap: Rust trait-object call through {} may dispatch to multiple implementations; concrete runtime target is unknown",
            receiver
        )
        .into(),
    });
    if let Some((declaration, implementors)) = flow_trait_object
        .and_then(|trait_node| semantic_context.dynamic_dispatch(trait_node, member))
    {
        graph.reference_edge(path, reference, declaration.id.clone(), "calls", "inferred");
        for target in implementors {
            graph.reference_edge(
                path,
                reference,
                target.id.clone(),
                "possible_calls",
                "possible",
            );
        }
        return true;
    }
    false
}

pub(in crate::engine::linker) fn macro_analysis_gap(
    profile: Option<&dyn LanguageProfile>,
    reference: &Reference,
    candidates: &[&Node],
    path: &str,
    graph: &mut Graph,
) {
    if reference.kind != "calls"
        || !profile.is_some_and(|profile| profile.id() == "rust")
        || !candidates.iter().any(|candidate| candidate.kind == "macro")
    {
        return;
    }
    graph.coverage.push(Coverage {
        path: path.to_owned(),
        line: reference.line,
        expression: "macro_expansion_not_analyzed".into(),
        status: "unresolved".into(),
        evidence: format!(
            "analysis gap: Rust macro invocation {} is linked to its declaration, but its expansion is not analyzed",
            reference.expression
        )
        .into(),
    });
}

pub(in crate::engine::linker) fn macro_reference_candidate(
    profile: Option<&dyn LanguageProfile>,
    owner: Option<&Node>,
    candidate: &Node,
) -> bool {
    profile.is_some_and(|profile| profile.id() == "rust")
        && owner.is_some_and(|owner| owner.kind == "macro")
        && matches!(candidate.kind.as_str(), "function" | "method")
}

pub(in crate::engine::linker) fn impl_source<'a>(
    implementation: &Node,
    owner_scope: &str,
    file_module: &str,
    by_qual: &HashMap<&'a str, Vec<&'a Node>>,
    by_name: &HashMap<&'a str, Vec<&'a Node>>,
    aliases: &HashMap<&str, HashMap<&'a str, Vec<&'a Node>>>,
    lookup_key: &mut String,
) -> (Option<String>, bool) {
    let raw = implementation
        .name
        .split('<')
        .next()
        .unwrap_or(&implementation.name)
        .trim();
    let clean: std::borrow::Cow<'_, str> = if raw.contains("::") {
        std::borrow::Cow::Owned(raw.replace("::", "."))
    } else {
        std::borrow::Cow::Borrowed(raw)
    };
    let (head, rest) = clean.split_once('.').unwrap_or((clean.as_ref(), ""));
    let mut scope = owner_scope
        .rsplit_once('.')
        .map_or("", |(parent, _)| parent);
    let imported = loop {
        if let Some(targets) = aliases.get(scope).and_then(|entries| entries.get(head)) {
            break Some(targets);
        }
        if scope.is_empty() {
            break None;
        }
        scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
    };
    let is_nominal = |node: &&Node| matches!(node.kind.as_str(), "struct" | "enum");
    let mut types: Vec<&Node> = if let Some(imported) = imported {
        if rest.is_empty() {
            imported.iter().copied().filter(is_nominal).collect()
        } else {
            imported
                .iter()
                .flat_map(|node| {
                    by_qual
                        .get(super::super::qualified(lookup_key, &node.qualname, rest))
                        .into_iter()
                        .flatten()
                        .copied()
                        .filter(move |target| target.path == node.path)
                })
                .filter(is_nominal)
                .collect()
        }
    } else {
        by_qual
            .get(super::super::qualified(lookup_key, file_module, &clean))
            .into_iter()
            .flatten()
            .copied()
            .filter(is_nominal)
            .collect()
    };
    let mut source_heuristic = false;
    if types.is_empty() && imported.is_none() {
        types = by_name
            .get(clean.as_ref())
            .into_iter()
            .flatten()
            .copied()
            .filter(is_nominal)
            .collect();
        source_heuristic = !types.is_empty();
    }
    (
        (types.len() == 1).then(|| types[0].id.clone()),
        source_heuristic,
    )
}
