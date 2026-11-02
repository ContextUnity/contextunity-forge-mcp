use super::super::semantic_context::LanguageMemberLookup;
use crate::languages::LanguageProfile;
use forge_core::models::{compact_graph::Graph, Node, ReceiverHint, Reference};
use hashbrown::HashMap;

pub use crate::languages::rust::declarative_macro_classification;

pub(in crate::linker) fn dynamic_receiver_member<'a, F>(
    input: &super::super::resolution::ResolutionInput<'a, F>,
    path: &str,
    reference: &Reference,
    owner: Option<&'a Node>,
    graph: &mut Graph,
) -> bool
where
    F: AsRef<forge_core::models::Facts> + Sync,
{
    use super::super::{semantic_context::Member, value_flow::TypeTarget};

    let Some(owner) = owner.filter(|owner| owner.language == "rust") else {
        return false;
    };
    if reference.kind != "calls" {
        return false;
    }
    let (receiver, member) =
        if let Some((member, receiver)) = reference.receiver_hint.as_ref().and_then(|hint| {
            let ReceiverHint::CallResult { member, .. } = hint else { return None; };
            input.value_flow.computed_receiver(reference)
                .filter(|receiver| !matches!(receiver, TypeTarget::Unknown))
                .map(|receiver| (member, receiver))
        }) {
            (std::borrow::Cow::Borrowed(receiver), member.as_str())
        } else {
            let Some((receiver, member)) = reference.expression.rsplit_once('.') else {
                return false;
            };
            let Some(value) = fallback_receiver_value(receiver, reference.position()) else { return false; };
            (
                std::borrow::Cow::Owned(input.value_flow.resolve_value(
                    owner,
                    &value,
                    reference.position(),
                    input.semantic_context,
                )),
                member,
            )
        };
    if matches!(
        receiver.as_ref(),
        TypeTarget::Unknown | TypeTarget::Ambiguous | TypeTarget::Candidates { .. }
    ) {
        return false;
    }
    let result = input
        .semantic_context
        .lookup_member(receiver.as_ref(), member, owner);
    if matches!(result, Member::Unknown) {
        return false;
    }
    if let Member::Local(node) = result {
        if matches!(
            node.kind.as_str(),
            "method" | "function" | "function_item" | "function_signature_item"
        ) && !input.semantic_context.is_rust_inherent_member(node)
        {
            return false;
        }
    }
    super::super::emit_inferred(
        graph,
        path,
        reference,
        result,
        Some("Rust standard library"),
    );
    true
}

fn fallback_receiver_value(
    expression: &str,
    at: forge_core::semantic::SourcePosition,
) -> Option<forge_core::semantic::ValueExpr> {
    use forge_core::semantic::{CallSiteEvidence, ValueExpr};
    if expression.len() > 512 { return None; }
    if !expression.contains('.') && !expression.contains("::") && expression.ends_with("()") {
        return None;
    }
    let mut value = None;
    for (index, segment) in expression.split('.').enumerate() {
        if index > 6 { return None; }
        let (segment, try_result) = segment.strip_suffix('?').map_or((segment, false), |s| (s, true));
        let (name, called) = segment.strip_suffix("()").map_or((segment, false), |s| (s, true));
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || (index == 0 && c == ':')) {
            return None;
        }
        let next = match value {
            None if called => ValueExpr::Call { callee: name.to_owned(), site: None },
            None => ValueExpr::Alias { name: name.to_owned() },
            Some(inner) if called && super::value_flow::is_adapter(name) => ValueExpr::Project { value: Box::new(inner), member: name.to_owned() },
            Some(inner) if called => ValueExpr::Call {
                callee: name.to_owned(),
                site: Some(CallSiteEvidence { position: at, receiver: Some(Box::new(inner)), arguments: Vec::new() }),
            },
            Some(inner) => ValueExpr::Project { value: Box::new(inner), member: format!("__rust_field:{name}") },
        };
        value = Some(if try_result { ValueExpr::Project { value: Box::new(next), member: "?".to_owned() } } else { next });
    }
    value
}

/// Helper to verify registered Cargo ecosystem crates statically from ecosystem list
pub fn is_registered_cargo_crate(crate_name: &str) -> bool {
    matches!(
        crate_name,
        "rusqlite"
            | "tree-sitter"
            | "tree_sitter"
            | "serde"
            | "serde_json"
            | "serde_yaml"
            | "tokio"
            | "anyhow"
            | "clap"
            | "tracing"
            | "rayon"
            | "tempfile"
            | "regex"
            | "thiserror"
            | "parking_lot"
            | "crossbeam"
            | "log"
            | "git2"
            | "walkdir"
            | "ignore"
            | "sha2"
            | "hex"
            | "toml"
            | "uuid"
            | "chrono"
            | "hashbrown"
            | "indexmap"
            | "petgraph"
            | "libc"
            | "dirs"
            | "pulldown-cmark"
            | "pulldown_cmark"
            | "schemars"
            | "rmcp"
            | "zstd"
            | "cc"
    )
}

#[cfg(feature = "lang-rust")]
pub(in crate::linker) fn rust_explicit_module_target(
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

pub(in crate::linker) fn resolve_self_reference<'a>(
    profile: Option<&dyn LanguageProfile>,
    reference: &Reference,
    owner: Option<&'a Node>,
    path: &str,
    member_lookup: &LanguageMemberLookup<'_, 'a>,
    graph: &mut Graph,
) -> bool {
    if profile.is_some_and(|profile| profile.id() == "rust") && reference.kind == "calls" {
        if let Some(crate::linker::value_flow::Symbol::Callable(target)) = owner
            .and_then(|owner| member_lookup.rust_alias_member(owner, reference))
        {
            graph.reference_edge(path, reference, target.id.clone(), "calls", "exact");
            graph.reference_coverage(path, reference, "resolved", format!("Rust type alias associated member: {}", target.qualname).into());
            return true;
        }
    }
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

pub(in crate::linker) fn trait_object_call<'a>(
    profile: Option<&dyn LanguageProfile>,
    reference: &Reference,
    owner: Option<&Node>,
    path: &str,
    value_flow: &crate::linker::value_flow::ValueFlowIndex<'a>,
    semantic_context: &super::super::semantic_context::Context<'a>,
    graph: &mut Graph,
) -> bool {
    if reference.kind != "calls" || !profile.is_some_and(|profile| profile.id() == "rust") {
        return false;
    }
    if let Some(ReceiverHint::CallResult { callee, .. }) = &reference.receiver_hint {
        if callee == "macro_invocation" {
            let local_macro = {
                let expr = &reference.expression;
                let clean = expr.replace("::", ".");
                let (head, tail) = clean.split_once('.').unwrap_or((&clean, ""));

                let mut scopes = Vec::new();
                if let Some(o) = owner {
                    let mut cur = o.qualname.as_str();
                    loop {
                        scopes.push(cur);
                        if let Some((parent, _)) = cur.rsplit_once('.') {
                            cur = parent;
                        } else {
                            break;
                        }
                    }
                }
                scopes.push("");

                let mut matched_node: Option<&Node> = None;

                // 1. Check ancestor scopes for direct macro or module definitions
                for scope in &scopes {
                    let full_name = if scope.is_empty() {
                        clean.clone()
                    } else {
                        format!("{scope}.{clean}")
                    };
                    if let Some(node) = semantic_context
                        .nodes_in_path(&full_name, path)
                        .find(|n| n.kind == "macro")
                    {
                        matched_node = Some(node);
                        break;
                    }
                    if !tail.is_empty() {
                        let mod_name = if scope.is_empty() {
                            head.to_string()
                        } else {
                            format!("{scope}.{head}")
                        };
                        for mod_node in semantic_context.nodes_in_path(&mod_name, path) {
                            if matches!(mod_node.kind.as_str(), "module" | "module_declaration") {
                                if let Some(m) = semantic_context
                                    .nodes_in_path(&format!("{}.{tail}", mod_node.qualname), path)
                                    .find(|n| n.kind == "macro")
                                {
                                    matched_node = Some(m);
                                    break;
                                }
                                if let Some(re_imp) =
                                    semantic_context.imported_alias(mod_node, tail)
                                {
                                    match re_imp {
                                        super::super::semantic_context::Imported::Nodes(
                                            re_nodes,
                                            _,
                                            _,
                                        ) => {
                                            if let Some(m) =
                                                re_nodes.iter().copied().find(|n| n.kind == "macro")
                                            {
                                                matched_node = Some(m);
                                                break;
                                            }
                                        }
                                        super::super::semantic_context::Imported::External(
                                            target_path,
                                            _,
                                            _,
                                            _,
                                        ) => {
                                            let re_clean = target_path
                                                .strip_prefix("crate::")
                                                .unwrap_or(target_path)
                                                .replace("::", ".");
                                            let full_re =
                                                format!("{}.{re_clean}", mod_node.qualname);
                                            if let Some(m) = semantic_context
                                                .nodes_in_path(&full_re, path)
                                                .find(|n| n.kind == "macro")
                                            {
                                                matched_node = Some(m);
                                                break;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        if matched_node.is_some() {
                            break;
                        }
                    }
                }

                // 2. Check if head is imported via `use` in caller-visible scopes
                if matched_node.is_none() {
                    let mut visible_owners: Vec<&Node> = Vec::new();
                    if let Some(o) = owner {
                        visible_owners.push(o);
                    }
                    for scope in &scopes {
                        for n in semantic_context.nodes_in_path(scope, path) {
                            if matches!(n.kind.as_str(), "module" | "module_declaration")
                                && !visible_owners.iter().any(|v| v.id == n.id)
                            {
                                visible_owners.push(n);
                            }
                        }
                    }
                    for owner_node in &visible_owners {
                        if let Some(imported) = semantic_context.imported_alias(owner_node, head) {
                            match imported {
                                super::super::semantic_context::Imported::Nodes(nodes, _, _) => {
                                    if tail.is_empty() {
                                        if let Some(m) =
                                            nodes.iter().copied().find(|n| n.kind == "macro")
                                        {
                                            matched_node = Some(m);
                                            break;
                                        }
                                    } else {
                                        for mod_node in nodes {
                                            if matches!(
                                                mod_node.kind.as_str(),
                                                "module" | "module_declaration"
                                            ) {
                                                let full_tail =
                                                    format!("{}.{tail}", mod_node.qualname);
                                                if let Some(m) = semantic_context
                                                    .nodes_in_path(&full_tail, path)
                                                    .find(|n| n.kind == "macro")
                                                {
                                                    matched_node = Some(m);
                                                    break;
                                                }
                                                if let Some(re_imp) =
                                                    semantic_context.imported_alias(mod_node, tail)
                                                {
                                                    match re_imp {
                                                        super::super::semantic_context::Imported::Nodes(re_nodes, _, _) => {
                                                            if let Some(m) = re_nodes.iter().copied().find(|n| n.kind == "macro") {
                                                                matched_node = Some(m);
                                                                break;
                                                            }
                                                        }
                                                        super::super::semantic_context::Imported::External(target_path, _, _, _) => {
                                                            let re_clean = target_path.strip_prefix("crate::").unwrap_or(target_path).replace("::", ".");
                                                            let full_re = format!("{}.{re_clean}", mod_node.qualname);
                                                            if let Some(m) = semantic_context.nodes_in_path(&full_re, path).find(|n| n.kind == "macro") {
                                                                matched_node = Some(m);
                                                                break;
                                                            }
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                            }
                                        }
                                        if matched_node.is_some() {
                                            break;
                                        }
                                    }
                                }
                                super::super::semantic_context::Imported::External(
                                    target_path,
                                    _,
                                    _,
                                    _,
                                ) => {
                                    let target_clean = target_path
                                        .strip_prefix("crate::")
                                        .unwrap_or(target_path)
                                        .replace("::", ".");
                                    for scope in &scopes {
                                        let full_target = if scope.is_empty() {
                                            target_clean.clone()
                                        } else {
                                            format!("{scope}.{target_clean}")
                                        };
                                        if tail.is_empty() {
                                            if let Some(m) = semantic_context
                                                .nodes_in_path(&full_target, path)
                                                .find(|n| n.kind == "macro")
                                            {
                                                matched_node = Some(m);
                                                break;
                                            }
                                        } else {
                                            // target_clean is a module (e.g. `local`), check member `tail`
                                            let mod_tail = format!("{full_target}.{tail}");
                                            if let Some(m) = semantic_context
                                                .nodes_in_path(&mod_tail, path)
                                                .find(|n| n.kind == "macro")
                                            {
                                                matched_node = Some(m);
                                                break;
                                            }
                                            // Or re-export inside that module
                                            for mod_node in
                                                semantic_context.nodes_in_path(&full_target, path)
                                            {
                                                if let Some(re_imp) =
                                                    semantic_context.imported_alias(mod_node, tail)
                                                {
                                                    match re_imp {
                                                        super::super::semantic_context::Imported::Nodes(re_nodes, _, _) => {
                                                            if let Some(m) = re_nodes.iter().copied().find(|n| n.kind == "macro") {
                                                                matched_node = Some(m);
                                                                break;
                                                            }
                                                        }
                                                        super::super::semantic_context::Imported::External(re_target, _, _, _) => {
                                                            let re_clean = re_target.strip_prefix("crate::").unwrap_or(re_target).replace("::", ".");
                                                            let full_re = format!("{full_target}.{re_clean}");
                                                            if let Some(m) = semantic_context.nodes_in_path(&full_re, path).find(|n| n.kind == "macro") {
                                                                matched_node = Some(m);
                                                                break;
                                                            }
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                            }
                                            if matched_node.is_some() {
                                                break;
                                            }
                                        }
                                    }
                                    if matched_node.is_some() {
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        if matched_node.is_some() {
                            break;
                        }
                    }
                }

                matched_node
            };

            if let Some(target) = local_macro {
                graph.reference_edge(path, reference, target.id.clone(), "calls", "exact");
                graph.reference_coverage(
                    path,
                    reference,
                    "resolved",
                    "1 lexically justified candidates; implementor resolved=true, heuristic=false"
                        .into(),
                );
                macro_analysis_gap(profile, reference, &[target], path, graph);
                return true;
            }

            if let Some((provider, pack)) = declarative_macro_classification(&reference.expression)
            {
                graph.reference_coverage(
                    path,
                    reference,
                    "external",
                    format!(
                        "{pack}: declarative macro {provider}::{}",
                        reference.expression
                    )
                    .into(),
                );
                return true;
            }
        }
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

    graph.push_coverage(
        path,
        reference.line,
        "trait_object_dispatch",
        "unresolved",
        format!(
            "analysis gap: Rust trait-object call through {} may dispatch to multiple implementations; concrete runtime target is unknown",
            receiver
        ),
    );
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

pub(in crate::linker) fn macro_analysis_gap(
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
    graph.push_coverage(
        path,
        reference.line,
        "macro_expansion_not_analyzed",
        "unresolved",
        format!(
            "analysis gap: Rust macro invocation {} is linked to its declaration, but its expansion is not analyzed",
            reference.expression
        ),
    );
}

pub(in crate::linker) fn macro_reference_candidate(
    profile: Option<&dyn LanguageProfile>,
    owner: Option<&Node>,
    candidate: &Node,
) -> bool {
    profile.is_some_and(|profile| profile.id() == "rust")
        && owner.is_some_and(|owner| owner.kind == "macro")
        && matches!(candidate.kind.as_str(), "function" | "method")
}

pub(in crate::linker) fn impl_source<'a>(
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
