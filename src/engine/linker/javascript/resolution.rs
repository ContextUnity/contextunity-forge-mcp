use super::super::interner::LinkerInterner;
#[cfg(feature = "lang-typescript")]
use super::super::interner::SymbolId;
use super::super::semantic_context::Context;
use super::super::traits::{LanguageLinker, PackageExports};
#[cfg(feature = "lang-typescript")]
use crate::core::models::compact_graph::Graph;
use crate::core::models::{Facts, Node, Reference};
#[cfg(feature = "lang-typescript")]
use crate::core::semantic::ObjectOwnershipFact;
use crate::engine::languages::LanguageProfile;
#[cfg(feature = "lang-typescript")]
use crate::engine::linker::resolution::ResolutionInput;
#[cfg(feature = "lang-typescript")]
use crate::engine::linker::value_flow;
use hashbrown::HashMap;
#[cfg(feature = "lang-typescript")]
use hashbrown::HashSet;

#[cfg(feature = "lang-typescript")]
pub(in crate::engine::linker) fn returned_object_field_writes<'node, 'reference>(
    profile: Option<&dyn LanguageProfile>,
    references: &'reference [Reference],
    by_id: &HashMap<SymbolId, &'node Node>,
    interner: &LinkerInterner<'_>,
) -> HashSet<(&'node str, &'reference str)> {
    if !profile.is_some_and(|profile| matches!(profile.id(), "javascript" | "html")) {
        return HashSet::new();
    }

    references
        .iter()
        .filter(|reference| reference.kind == "mutates")
        .filter_map(|reference| {
            let field = reference.expression.strip_prefix("this.")?;
            if field.contains('.') {
                return None;
            }
            let owner = interner.lookup_node(by_id, reference.source.as_str())?;
            let marker = owner
                .language
                .eq("javascript")
                .then(|| owner.details["receiver"].as_str())
                .flatten()?
                .strip_prefix("object_return:")?;
            Some((marker, field))
        })
        .collect()
}

#[cfg(feature = "lang-typescript")]
pub(in crate::engine::linker) fn has_returned_object_builtin_this_member<F>(
    input: &ResolutionInput<'_, F>,
    path: &str,
    reference: &Reference,
    expression: &str,
    active_profile: Option<&dyn LanguageProfile>,
    field_writes: &HashSet<(&str, &str)>,
) -> bool {
    if !matches!(reference.kind.as_str(), "calls" | "references") {
        return false;
    }
    let Some(("this", tail)) = expression.split_once('.') else {
        return false;
    };
    let Some((field_name, member_name)) = tail
        .split_once('.')
        .filter(|(_, member)| !member.contains('.'))
    else {
        return false;
    };
    let Some(marker) = input
        .interner
        .lookup_node(input.by_id, reference.source.as_str())
        .filter(|owner| owner.language == "javascript")
        .and_then(|owner| owner.details["receiver"].as_str())
        .filter(|marker| marker.starts_with("object_return:"))
    else {
        return false;
    };
    if field_writes.contains(&(marker.trim_start_matches("object_return:"), field_name)) {
        return false;
    }

    let mut fields = input
        .by_name
        .get(field_name)
        .into_iter()
        .flatten()
        .copied()
        .filter(|node| {
            node.path == *path
                && node.kind == "field"
                && node.details["receiver"].as_str() == Some(marker)
        });
    let (Some(field), None) = (fields.next(), fields.next()) else {
        return false;
    };
    let Some(value_flow::TypeTarget::Builtin(receiver)) = input.value_flow.return_type(field)
    else {
        return false;
    };
    active_profile.is_some_and(|profile| profile.builtin_member(receiver, member_name))
}

#[cfg(feature = "lang-typescript")]
pub(in crate::engine::linker) fn object_factory_this_member<'a, F>(
    input: &ResolutionInput<'a, F>,
    path: &str,
    reference: &Reference,
    aliases: &HashMap<&str, HashMap<&str, Vec<&'a Node>>>,
    object_ownership: impl Fn(&Node) -> Option<&'a ObjectOwnershipFact>,
    lexical_this_calls: &HashSet<(&str, usize, usize, &str)>,
    graph: &mut Graph,
) -> bool {
    if !matches!(reference.kind.as_str(), "calls" | "references" | "mutates") {
        return false;
    }
    let expression = reference.expression.as_str();
    let profile = input.profiles.get(path).copied();
    let tail = if expression.contains("::") {
        expression
            .strip_prefix("this::")
            .or_else(|| expression.strip_prefix("this."))
    } else if profile
        .is_some_and(|profile| matches!(profile.id(), "javascript" | "typescript" | "vue" | "html"))
        && expression.contains("?.")
    {
        expression
            .strip_prefix("this?.")
            .or_else(|| expression.strip_prefix("this."))
    } else {
        expression.strip_prefix("this.")
    };
    let Some(tail) = tail.filter(|tail| !tail.contains('.') && !tail.contains("::")) else {
        return false;
    };
    if tail.is_empty() {
        return false;
    }
    let Some((owner, marker, ownership)) = input
        .interner
        .lookup_node(input.by_id, reference.source.as_str())
        .filter(|owner| {
            owner.language == "javascript" && matches!(owner.kind.as_str(), "method" | "function")
        })
        .and_then(|owner| {
            owner.details["receiver"]
                .as_str()
                .filter(|marker| {
                    marker.starts_with("object_return:") || object_ownership(owner).is_some()
                })
                .map(|marker| (owner, marker, object_ownership(owner)))
        })
    else {
        return false;
    };

    let mut shadowed = false;
    if let Some(binding) = ownership.as_ref().and_then(|fact| fact.guard.as_deref()) {
        let binding_id = input.interner.lookup_symbol(binding);
        let mut scope = owner.qualname.as_str();
        while !scope.is_empty() {
            let registration_child_scope = input.by_qual.get(scope).is_some_and(|nodes| {
                nodes.iter().any(|node| {
                    node.path == *path && node.details["receiver"].as_str() == Some(marker)
                })
            });
            if !registration_child_scope {
                shadowed |= binding_id.is_some_and(|binding_id| {
                    input.by_qual.get(scope).is_some_and(|nodes| {
                        nodes.iter().any(|node| {
                            node.path == *path
                                && input
                                    .interner
                                    .lookup_symbol(&node.id)
                                    .is_some_and(|node_id| {
                                        super::super::typescript::resolution::has_indexed_symbol_id(
                                            input.node_bindings,
                                            node_id,
                                            binding_id,
                                        ) || super::super::typescript::resolution::has_indexed_symbol_id(
                                            input.node_rebindings,
                                            node_id,
                                            binding_id,
                                        )
                                    })
                        })
                    })
                }) || aliases
                    .get(scope)
                    .is_some_and(|names| names.contains_key(binding));
            }
            scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
        }
    }
    if !shadowed {
        let mut members = input
            .by_name
            .get(tail)
            .into_iter()
            .flatten()
            .copied()
            .filter(|node| {
                node.path == *path
                    && matches!(node.kind.as_str(), "field" | "method")
                    && node.details["receiver"].as_str() == Some(marker)
                    && object_ownership(node) == ownership
            });
        let member = members.next();
        let unique = member.is_some() && members.next().is_none();
        if unique {
            if let Some(member) = member
                .filter(|member| reference.kind != "calls" || member.kind == "method")
                .filter(|member| reference.kind != "mutates" || member.kind == "field")
            {
                if reference.kind != "references"
                    || member.kind != "method"
                    || !lexical_this_calls.contains(&(
                        reference.source.as_str(),
                        reference.line,
                        reference.column,
                        reference.expression.as_str(),
                    ))
                {
                    graph.reference_edge(
                        path,
                        reference,
                        member.id.clone(),
                        reference.kind.as_str(),
                        "exact",
                    );
                }
                graph.reference_coverage(
                    path,
                    reference,
                    "resolved",
                    format!("same-file object member {}", member.qualname).into(),
                );
                return true;
            }
        }
    }
    graph.reference_coverage(
        path,
        reference,
        "unresolved",
        "object member has no unique unshadowed same-file provider".into(),
    );
    true
}

pub(in crate::engine::linker) fn builtin_origin<'a>(
    first: &str,
    profile: Option<&dyn LanguageProfile>,
    configured: Option<&'a str>,
) -> Option<&'a str> {
    let profile =
        profile.filter(|profile| matches!(profile.family().0, "typescript" | "javascript"))?;
    Some(configured.unwrap_or_else(|| match first {
        "Buffer" | "process" => "builtin:node",
        "document" | "window" | "localStorage" | "sessionStorage" | "fetch" | "Headers"
        | "Request" | "Response" | "FormData" | "HTMLElement" | "HTMLTableElement" | "Element"
        | "URL" | "URLSearchParams" | "Blob" | "File" | "DragEvent" | "ClipboardEvent"
        | "FocusEvent" | "MouseEvent" | "KeyboardEvent" | "Event" | "ParentNode" | "ChildNode"
        | "DocumentFragment" | "Storage" => "builtin:web_api",
        "console" | "CSS" if profile.id() == "javascript" => "builtin:web_api",
        "Object" | "Array" | "Math" | "JSON" | "Promise" | "Date"
            if profile.id() == "javascript" =>
        {
            "builtin:ecmascript"
        }
        _ => "JavaScript/Web platform",
    }))
}

#[allow(clippy::too_many_arguments)]
pub(in crate::engine::linker) fn commonjs_default_callable<'a>(
    reference: &Reference,
    profile: Option<&dyn LanguageProfile>,
    first: &str,
    owner: Option<&Node>,
    facts: &Facts,
    imported: &[&'a Node],
    context: &Context<'a>,
    interner: &LinkerInterner<'a>,
    linker: &dyn LanguageLinker,
    by_module: &HashMap<&'a str, Vec<&'a Node>>,
    by_qual: &HashMap<&'a str, Vec<&'a Node>>,
    package_exports: &PackageExports<'a>,
    lookup_key: &mut String,
) -> Option<&'a Node> {
    if !matches!(reference.kind.as_str(), "calls" | "handles")
        || !profile.is_some_and(|profile| profile.id() == "javascript")
    {
        return None;
    }
    let (Some(owner), [module]) = (owner, imported) else {
        return None;
    };
    let scope = facts.nodes.first()?;
    if module.kind != "module"
        || !context.admits_commonjs_binding(scope, first, owner, reference.position())
    {
        return None;
    }
    let defaults = linker.resolve_imported_member(
        interner,
        module,
        "default",
        by_module,
        by_qual,
        package_exports,
        lookup_key,
    );
    match defaults.as_slice() {
        [callable]
            if callable.path == module.path
                && callable.kind == "function"
                && callable.details["commonjs_default_callable"] == true =>
        {
            Some(*callable)
        }
        _ => None,
    }
}
