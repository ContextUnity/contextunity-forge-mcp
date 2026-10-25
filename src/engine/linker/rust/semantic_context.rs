#[cfg(feature = "lang-rust")]
use crate::core::models::Facts;
use crate::core::models::Node;
use crate::engine::languages;
#[cfg(feature = "lang-rust")]
use crate::engine::languages::ImportPath;
#[cfg(feature = "lang-rust")]
use crate::engine::languages::{LanguageProfile, dependency_registry::DependencyRegistry};
#[cfg(feature = "lang-rust")]
use crate::engine::linker::traits::ModulesByNamespace;
use crate::engine::linker::value_flow::{Symbol, TypeTarget};
#[cfg(feature = "lang-rust")]
use hashbrown::HashMap;
#[cfg(feature = "lang-rust")]
use std::borrow::Cow;
#[cfg(feature = "lang-rust")]
use std::collections::BTreeMap;

use super::super::semantic_context::LanguageMemberLookup;
#[cfg(feature = "lang-rust")]
use super::super::semantic_context::{Context, Imported};

pub(in crate::engine::linker) fn deref_target<'a>(
    lookup: &LanguageMemberLookup<'_, 'a>,
    receiver: &Node,
) -> Option<&'a Node> {
    #[cfg(feature = "lang-rust")]
    {
        lookup.rust_deref_target(receiver)
    }
    #[cfg(not(feature = "lang-rust"))]
    {
        let _ = (lookup, receiver);
        None
    }
}

pub(in crate::engine::linker) fn receiver_symbol<'a>(
    lookup: Option<&LanguageMemberLookup<'_, 'a>>,
    owner: &'a Node,
    name: &str,
    mut resolve_type: impl FnMut(&str) -> Symbol<'a>,
) -> Option<Symbol<'a>> {
    if name == "Self" {
        #[cfg(feature = "lang-rust")]
        if let Some(receiver) = lookup.and_then(|lookup| lookup.rust_receiver(owner)) {
            return Some(Symbol::Type(TypeTarget::Local(receiver)));
        }
        if let Some(receiver_type) = owner.details["receiver_type"]
            .as_str()
            .filter(|name| *name != "Self")
        {
            return Some(resolve_type(receiver_type));
        }
        return None;
    }
    #[cfg(feature = "lang-rust")]
    {
        let member = name
            .strip_prefix("Self::")
            .or_else(|| name.strip_prefix("Self."))?;
        let Some(lookup) = lookup else {
            return Some(Symbol::Unknown);
        };
        let Some(receiver) = lookup.rust_receiver(owner) else {
            return Some(Symbol::Unknown);
        };
        let methods = lookup.rust(receiver, member, true, owner);
        match methods {
            [target] => Some(Symbol::Callable(target)),
            [] => Some(Symbol::Unknown),
            _ => Some(Symbol::Ambiguous),
        }
    }
    #[cfg(not(feature = "lang-rust"))]
    {
        let _ = lookup;
        None
    }
}

#[cfg(feature = "lang-rust")]
pub(in crate::engine::linker) fn qualified_generic_base(annotation: &str) -> Option<&str> {
    let mut name = annotation.trim();
    while let Some(rest) = name.strip_prefix('&') {
        name = rest.trim_start();
        if name.starts_with('\'') {
            name = name.split_once(char::is_whitespace)?.1.trim_start();
        }
        name = name.strip_prefix("mut ").unwrap_or(name).trim_start();
    }
    let (base, generic) = name.split_once('<')?;
    if !base.contains("::") || !name.ends_with('>') {
        return None;
    }
    let mut depth = 1usize;
    let outer_generic_ends_type = generic.char_indices().all(|(index, character)| {
        match character {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 && index + character.len_utf8() != generic.len() {
                    return false;
                }
            }
            _ => {}
        }
        depth > 0 || index + character.len_utf8() == generic.len()
    }) && depth == 0;
    outer_generic_ends_type.then_some(base.trim_end())
}

#[cfg(feature = "lang-rust")]
pub(in crate::engine::linker) fn qualified_annotation_type<'a>(
    profile: Option<&dyn LanguageProfile>,
    annotation: &'a str,
    path: &str,
    dependencies: &DependencyRegistry,
) -> Option<Cow<'a, str>> {
    let profile = profile.filter(|profile| profile.id() == "rust")?;
    let base = qualified_generic_base(annotation)?;
    let head = base.split_once("::")?.0;
    dependencies
        .declares_for_path(profile.family(), path, head)
        .then(|| super::super::receiver_type_name(base))
        .flatten()
}

#[cfg(feature = "lang-rust")]
pub(in crate::engine::linker) fn qualified_external_annotation<'a>(
    profile: Option<&dyn LanguageProfile>,
    annotation: &'a str,
    head: &str,
    path: &str,
    dependencies: &DependencyRegistry,
) -> Option<super::super::ReceiverType<'a>> {
    let profile = profile.filter(|profile| profile.id() == "rust")?;
    if !dependencies.declares_for_path(profile.family(), path, head) {
        return None;
    }
    let original = annotation.trim().trim_start_matches('&').trim_start();
    let original = original.strip_prefix("mut ").unwrap_or(original);
    Some(match original.split_once("::") {
        Some((origin, _)) if origin == head => super::super::ReceiverType::External(origin, 0),
        _ => super::super::ReceiverType::Unknown,
    })
}

#[cfg(feature = "lang-rust")]
pub(in crate::engine::linker) fn external_annotation_type<'a>(
    profile: Option<&dyn LanguageProfile>,
    raw_head: &'a str,
    head: &str,
    path: &str,
    dependencies: &DependencyRegistry,
) -> Option<super::super::ReceiverType<'a>> {
    let profile = profile.filter(|profile| profile.id() == "rust")?;
    (profile.external_import(head).is_some()
        || dependencies.declares_for_path(profile.family(), path, head))
    .then_some(super::super::ReceiverType::External(raw_head, 0))
}

#[cfg(feature = "lang-rust")]
pub(in crate::engine::linker) fn import_globs<'a, F: AsRef<Facts>>(
    context: &mut Context<'a>,
    all: &'a BTreeMap<String, F>,
    by_id: &'a HashMap<super::super::interner::SymbolId, &'a Node>,
    normalized_imports: &'a HashMap<(&'a str, &'a str), Option<ImportPath>>,
    modules: &'a ModulesByNamespace<'a>,
    rust_forwarding: &'a languages::rust::linker::RustForwarding<'a>,
) {
    let rust_modules = all.values().filter_map(|facts| {
        let facts = facts.as_ref();
        facts
            .nodes
            .iter()
            .find(|node| node.kind == "module" && node.language == "rust")
            .map(|module| (facts, module))
    });

    for (facts, module) in rust_modules.clone() {
        let Some(reference) = facts.references.iter().find(|reference| {
            reference.kind == "imports"
                && reference.source == module.id
                && reference.alias.as_deref() == Some("*")
        }) else {
            continue;
        };
        let glob_position = reference.position();
        let module_id = context
            .symbol_id(&module.id)
            .expect("Rust module id is interned");
        for (name, targets) in rust_forwarding.visible_bindings(module, &module.qualname) {
            let Some(name_id) = context.symbol_id(name) else {
                continue;
            };
            let aliases = context.imports.entry(module_id).or_default();
            if facts.nodes.iter().any(|node| {
                node.kind != "module_declaration"
                    && node
                        .qualname
                        .rsplit_once('.')
                        .is_some_and(|(scope, candidate)| {
                            scope == module.qualname && candidate == name
                        })
            }) {
                aliases.remove(&name_id);
                continue;
            }
            if targets.is_empty() && matches!(aliases.get(&name_id), Some(Imported::External(..))) {
                continue;
            }
            let position = match aliases.get(&name_id) {
                Some(Imported::Nodes(_, position, _)) => *position,
                _ => glob_position,
            };
            aliases.insert(
                name_id,
                if targets.is_empty() {
                    Imported::Unknown
                } else {
                    Imported::Nodes(targets.to_vec(), position, false)
                },
            );
        }
    }

    for (facts, module) in rust_modules {
        for reference in facts.references.iter().filter(|reference| {
            reference.kind == "imports"
                && reference.source != module.id
                && reference.alias.as_deref() == Some("*")
        }) {
            let Some(owner) = context
                .symbol_id(&reference.source)
                .and_then(|id| by_id.get(&id).copied())
            else {
                continue;
            };
            let raw = reference.module.as_deref().unwrap_or(&reference.expression);
            let Some(normalized) = normalized_imports
                .get(&(module.path.as_str(), raw))
                .and_then(Option::as_ref)
            else {
                continue;
            };
            let providers = languages::rust::linker::modules_at_import(
                modules,
                languages::LanguageFamily("rust"),
                normalized,
                languages::workspace_path(&module.path).0,
            );
            let [(provider, tail)] = providers.as_slice() else {
                continue;
            };
            if !matches!(tail.as_str(), "" | "*") {
                continue;
            }
            let owner_id = context
                .symbol_id(&owner.id)
                .expect("Rust import owner id is interned");
            for (name, targets) in rust_forwarding.visible_bindings(provider, &module.qualname) {
                if facts.references.iter().any(|candidate| {
                    candidate.kind == "imports"
                        && candidate.source == owner.id
                        && candidate.alias.as_deref() == Some(name)
                }) {
                    continue;
                }
                let Some(name_id) = context.symbol_id(name) else {
                    continue;
                };
                let aliases = context.imports.entry(owner_id).or_default();
                if facts.nodes.iter().any(|node| {
                    node.qualname
                        .rsplit_once('.')
                        .is_some_and(|(scope, candidate)| {
                            scope == owner.qualname && candidate == name
                        })
                }) {
                    aliases.remove(&name_id);
                    continue;
                }
                match aliases.entry(name_id) {
                    hashbrown::hash_map::Entry::Vacant(entry) => {
                        entry.insert(if targets.is_empty() {
                            Imported::Unknown
                        } else {
                            Imported::Nodes(targets.to_vec(), reference.position(), false)
                        });
                    }
                    hashbrown::hash_map::Entry::Occupied(mut entry) => {
                        if let Imported::Nodes(nodes, _, _) = entry.get_mut() {
                            if targets.is_empty() {
                                entry.insert(Imported::Unknown);
                            } else {
                                nodes.extend_from_slice(targets);
                                nodes.sort_by_key(|node| node.id.as_str());
                                nodes.dedup_by_key(|node| node.id.as_str());
                            }
                        } else {
                            entry.insert(Imported::Unknown);
                        }
                    }
                }
            }
        }
    }
}

pub(in crate::engine::linker) fn visible(owner: &Node, candidate: &Node) -> Option<bool> {
    if candidate.language == "rust" && candidate.kind == "impl" {
        Some(false)
    } else if owner.language == "rust" {
        Some(true)
    } else {
        None
    }
}

pub(in crate::engine::linker) fn nominal_application<'a>(
    owner: &Node,
    nominal: &Symbol<'a>,
) -> Option<TypeTarget<'a>> {
    if owner.language != "rust" {
        return None;
    }
    match nominal {
        Symbol::Type(TypeTarget::Local(node))
            if matches!(node.kind.as_str(), "struct" | "enum") =>
        {
            Some(TypeTarget::Local(node))
        }
        _ => None,
    }
}

pub(in crate::engine::linker) fn admits_payload(
    owner: &Node,
    wrapper: &TypeTarget<'_>,
    argument_count: usize,
) -> bool {
    owner.language == "rust"
        && matches!(wrapper, TypeTarget::Builtin(name)
            if (name == "Option" && argument_count == 1)
                || (name == "Result" && argument_count <= 2))
}

pub(in crate::engine::linker) fn reference_target<'target, 'node>(
    target: &'target TypeTarget<'node>,
) -> Option<&'target TypeTarget<'node>> {
    match target {
        TypeTarget::RustReference { target, .. } => Some(target),
        _ => None,
    }
}

pub(in crate::engine::linker) fn standard_member(module: &str, member: &str) -> bool {
    if matches!(member, "new" | "from" | "with_capacity" | "default") {
        return false;
    }
    let module = super::value_flow::import_path(module);
    let nominal = match module.as_ref() {
        "std.collections.HashMap"
        | "std.collections.HashSet"
        | "std.collections.BTreeMap"
        | "std.collections.BTreeSet" => module.rsplit_once('.').map_or("", |(_, name)| name),
        "std.string.String" | "alloc.string.String" => "String",
        "std.vec.Vec" | "alloc.vec.Vec" => "Vec",
        "std.path.Path" => "Path",
        "std.path.PathBuf" => "PathBuf",
        "std.option.Option" | "core.option.Option" => "Option",
        "std.result.Result" | "core.result.Result" => "Result",
        _ => return false,
    };
    languages::by_id("rust").is_some_and(|profile| profile.builtin_member(nominal, member))
}

pub(in crate::engine::linker) fn wrapper_value_member(wrapper: &str, member: &str) -> bool {
    match wrapper {
        "Box" => matches!(member, "as_ref" | "as_mut"),
        "Arc" | "Rc" => matches!(member, "clone" | "as_ref"),
        _ => false,
    }
}
