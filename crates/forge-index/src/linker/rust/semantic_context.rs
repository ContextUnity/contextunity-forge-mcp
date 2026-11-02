use crate::languages;
#[cfg(feature = "lang-rust")]
use crate::languages::ImportPath;
#[cfg(feature = "lang-rust")]
use crate::languages::{dependency_registry::DependencyRegistry, LanguageProfile};
#[cfg(feature = "lang-rust")]
use crate::linker::traits::ModulesByNamespace;
use crate::linker::value_flow::{Symbol, TypeTarget};
#[cfg(feature = "lang-rust")]
use forge_core::models::Facts;
use forge_core::models::Node;
#[cfg(feature = "lang-rust")]
use hashbrown::HashMap;
#[cfg(feature = "lang-rust")]
use std::borrow::Cow;
#[cfg(feature = "lang-rust")]
use std::collections::BTreeMap;

use super::super::semantic_context::LanguageMemberLookup;
#[cfg(feature = "lang-rust")]
use super::super::semantic_context::{Context, Imported};

pub(in crate::linker) fn deref_target<'a>(
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

pub(in crate::linker) fn receiver_symbol<'a>(
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
            let clean = super::super::receiver_type_name(receiver_type)
                .unwrap_or(std::borrow::Cow::Borrowed(receiver_type));
            return Some(resolve_type(clean.as_ref()));
        }
        return None;
    }
    #[cfg(feature = "lang-rust")]
    {
        let clean = name.replace("::", ".");
        if let Some((first, _)) = clean.split_once('.') {
            if super::resolution::is_registered_cargo_crate(first)
                && crate::languages::rust::is_registered_rust_type(name)
                && matches!(resolve_type(first), Symbol::Unknown)
            {
                return Some(Symbol::Type(TypeTarget::External {
                    module: name.replace('.', "::"),
                    import_line: 0,
                }));
            }
        }
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
pub(in crate::linker) fn qualified_generic_base(annotation: &str) -> Option<&str> {
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
pub(in crate::linker) fn qualified_annotation_type<'a>(
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
pub(in crate::linker) fn qualified_external_annotation<'a>(
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
pub(in crate::linker) fn external_annotation_type<'a>(
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
pub(in crate::linker) fn import_globs<'a, F: AsRef<Facts>>(
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

    for (facts, module) in rust_modules.clone() {
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
                |namespace| rust_forwarding.module_path_visible(namespace, &module.qualname),
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

    // Build alias counts per owner across all import references to detect conflicting aliases
    let mut alias_counts: hashbrown::HashMap<(&str, &str), usize> = hashbrown::HashMap::new();
    for facts in all.values() {
        let facts = facts.as_ref();
        for reference in facts.references.iter().filter(|r| r.kind == "imports") {
            let bound_alias = reference
                .alias
                .as_deref()
                .or_else(|| reference.expression.rsplit("::").next());
            if let Some(alias) = bound_alias {
                if alias != "*" {
                    *alias_counts
                        .entry((reference.source.as_str(), alias))
                        .or_default() += 1;
                }
            }
        }
    }

    for facts in all.values() {
        let facts = facts.as_ref();
        for reference in facts.references.iter().filter(|r| r.kind == "imports") {
            let expr = reference.module.as_deref().unwrap_or(&reference.expression);
            let first = expr.split("::").next().unwrap_or(expr);
            // Guard against local module shadowing: if facts declare a local module named `first`, do not route as external
            let has_local_module = facts.nodes.iter().any(|node| {
                (node.kind == "module" || node.kind == "module_declaration")
                    && (node.name == first
                        || node
                            .qualname
                            .rsplit_once('.')
                            .is_some_and(|(_, n)| n == first))
            });
            if has_local_module {
                continue;
            }

            if super::resolution::is_registered_cargo_crate(first) {
                let Some(owner_id) = context.symbol_id(&reference.source) else {
                    continue;
                };
                let bound_alias = reference
                    .alias
                    .as_deref()
                    .or_else(|| reference.expression.rsplit("::").next());
                let Some(alias) = bound_alias else {
                    continue;
                };
                if alias == "*" {
                    if expr.trim_end_matches("::*") == "rayon::prelude" {
                        if let Some(namespace) = context.symbol_id(expr) {
                            context.imports.entry(owner_id).or_default().insert(
                                namespace,
                                Imported::External(
                                    expr.to_owned(),
                                    reference.position(),
                                    reference,
                                    true,
                                ),
                            );
                        }
                    }
                    continue;
                }
                if alias_counts
                    .get(&(reference.source.as_str(), alias))
                    .copied()
                    .unwrap_or(0)
                    > 1
                {
                    // Conflicting duplicate alias: preserve Imported::Unknown
                    continue;
                }
                let Some(alias_id) = context.symbol_id(alias) else {
                    continue;
                };
                let aliases = context.imports.entry(owner_id).or_default();
                if matches!(aliases.get(&alias_id), None | Some(Imported::Unknown)) {
                    aliases.insert(
                        alias_id,
                        Imported::External(expr.to_owned(), reference.position(), reference, true),
                    );
                }
            }
        }
    }

    // Carry external provenance through ancestor imports independently of file order.
    // Only ancestor imports can expose private bindings; unrelated glob providers
    // continue to use the public forwarding projection above.
    let mut conflicts = hashbrown::HashSet::new();
    for _ in 0..64 {
        let mut additions = Vec::new();
        for facts in all.values() {
            let facts = facts.as_ref();
            for reference in facts.references.iter().filter(|reference| {
                reference.kind == "imports"
                    && reference.alias.as_deref() == Some("*")
                    && reference
                        .module
                        .as_deref()
                        .unwrap_or(&reference.expression)
                        .trim_end_matches("::*")
                        == "super"
            }) {
                let Some(owner_id) = context.symbol_id(&reference.source) else {
                    continue;
                };
                let Some(owner) = by_id.get(&owner_id) else {
                    continue;
                };
                let mut scope = owner.qualname.as_str();
                let mut module = None;
                loop {
                    if let Some(found) = context
                        .nodes_in_path(scope, &owner.path)
                        .find(|node| matches!(node.kind.as_str(), "module" | "module_declaration"))
                    {
                        module = Some(found);
                        break;
                    }
                    let Some((parent, _)) = scope.rsplit_once('.') else {
                        break;
                    };
                    scope = parent;
                }
                let Some(module) = module else {
                    continue;
                };
                let namespace = if module.kind == "module" {
                    module
                        .qualname
                        .strip_suffix(".lib")
                        .or_else(|| module.qualname.strip_suffix(".main"))
                        .unwrap_or(&module.qualname)
                } else {
                    &module.qualname
                };
                let Some((parent, _)) = namespace.rsplit_once('.') else {
                    continue;
                };
                let parent_lib = format!("{parent}.lib");
                let parent_main = format!("{parent}.main");
                let mut providers: Vec<_> = [parent, parent_lib.as_str(), parent_main.as_str()]
                    .into_iter()
                    .flat_map(|scope| {
                        context.nodes_in_path(scope, &owner.path).chain(
                            modules
                                .get(&languages::LanguageFamily("rust"))
                                .and_then(|modules| modules.get(scope))
                                .into_iter()
                                .flatten()
                                .copied(),
                        )
                    })
                    .filter(|node| matches!(node.kind.as_str(), "module" | "module_declaration"))
                    .collect();
                providers.sort_by_key(|node| node.id.as_str());
                providers.dedup_by_key(|node| node.id.as_str());
                let [provider] = providers.as_slice() else {
                    continue;
                };
                let Some(provider_id) = context.symbol_id(&provider.id) else {
                    continue;
                };
                for (&name, imported) in context.imports.get(&provider_id).into_iter().flatten() {
                    let Imported::External(origin, _, original, _) = imported else {
                        continue;
                    };
                    if original.alias.as_deref() == Some("*")
                        && origin.trim_end_matches("::*") == "rayon::prelude"
                        && !context
                            .imports
                            .get(&owner_id)
                            .is_some_and(|aliases| aliases.contains_key(&name))
                    {
                        additions.push((
                            owner_id,
                            name,
                            origin.clone(),
                            reference.position(),
                            *original,
                        ));
                    }
                }
            }
        }
        for (facts, module) in rust_modules.clone() {
            for reference in facts
                .references
                .iter()
                .filter(|reference| reference.kind == "imports" && reference.alias.is_some())
            {
                let glob = reference.alias.as_deref() == Some("*");
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
                    |namespace| rust_forwarding.module_path_visible(namespace, &module.qualname),
                );
                let [(provider, tail)] = providers.as_slice() else {
                    continue;
                };
                let namespace = provider
                    .qualname
                    .strip_suffix(".lib")
                    .or_else(|| provider.qualname.strip_suffix(".main"))
                    .unwrap_or(&provider.qualname);
                if (glob && !matches!(tail.as_str(), "" | "*"))
                    || (!glob && (tail.is_empty() || tail.contains('.') || tail == "*"))
                    || !module
                        .qualname
                        .strip_prefix(namespace)
                        .is_some_and(|suffix| suffix.starts_with('.'))
                {
                    continue;
                }
                let Some(owner_id) = context.symbol_id(&reference.source) else {
                    continue;
                };
                let Some(provider_id) = context.symbol_id(&provider.id) else {
                    continue;
                };
                for (&provider_name_id, imported) in
                    context.imports.get(&provider_id).into_iter().flatten()
                {
                    let Imported::External(origin, _, original, _) = imported else {
                        continue;
                    };
                    if original.alias.as_deref() == Some("*") {
                        continue;
                    }
                    let provider_name = context.symbol_name(provider_name_id);
                    if !glob && provider_name != tail {
                        continue;
                    }
                    let name = if glob {
                        provider_name
                    } else {
                        reference.alias.as_deref().unwrap()
                    };
                    let Some(name_id) = context.symbol_id(name) else {
                        continue;
                    };
                    if conflicts.contains(&(owner_id, name_id)) {
                        continue;
                    }
                    if facts.references.iter().any(|candidate| {
                        !std::ptr::eq(candidate, reference)
                            && candidate.kind == "imports"
                            && candidate.source == reference.source
                            && candidate.alias.as_deref() == Some(name)
                    }) || facts.nodes.iter().any(|node| {
                        node.kind != "module_declaration"
                            && node.kind != "impl"
                            && node
                                .qualname
                                .rsplit_once('.')
                                .is_some_and(|(scope, candidate)| {
                                    scope == module.qualname && candidate == name
                                })
                    }) {
                        continue;
                    }
                    let existing = context
                        .imports
                        .get(&owner_id)
                        .and_then(|aliases| aliases.get(&name_id));
                    if matches!(existing, None | Some(Imported::Unknown))
                        || matches!(existing, Some(Imported::External(existing, ..)) if existing != origin)
                    {
                        additions.push((
                            owner_id,
                            name_id,
                            origin.clone(),
                            reference.position(),
                            if glob { *original } else { reference },
                        ));
                    }
                }
            }
        }
        if additions.is_empty() {
            break;
        }
        for (owner, name, origin, position, reference) in additions {
            let aliases = context.imports.entry(owner).or_default();
            if matches!(aliases.get(&name), Some(Imported::External(existing, ..)) if existing != &origin)
            {
                conflicts.insert((owner, name));
            }
            if conflicts.contains(&(owner, name)) {
                aliases.insert(name, Imported::Unknown);
            } else {
                aliases.insert(name, Imported::External(origin, position, reference, true));
            }
        }
        for &(owner, name) in &conflicts {
            context
                .imports
                .entry(owner)
                .or_default()
                .insert(name, Imported::Unknown);
        }
    }
}

pub(in crate::linker) fn visible(owner: &Node, candidate: &Node) -> Option<bool> {
    if candidate.language == "rust" && candidate.kind == "impl" {
        Some(false)
    } else if owner.language == "rust" {
        Some(true)
    } else {
        None
    }
}

pub(in crate::linker) fn nominal_application<'a>(
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

pub(in crate::linker) fn admits_payload(
    owner: &Node,
    wrapper: &TypeTarget<'_>,
    argument_count: usize,
) -> bool {
    owner.language == "rust"
        && matches!(wrapper, TypeTarget::Builtin(name)
            if (name == "Option" && argument_count == 1)
                || (name == "Result" && argument_count <= 2))
}

pub(in crate::linker) fn reference_target<'target, 'node>(
    target: &'target TypeTarget<'node>,
) -> Option<&'target TypeTarget<'node>> {
    match target {
        TypeTarget::RustReference { target, .. } => Some(target),
        _ => None,
    }
}

pub(in crate::linker) fn standard_member(module: &str, member: &str) -> bool {
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

pub(in crate::linker) fn wrapper_value_member(wrapper: &str, member: &str) -> bool {
    match wrapper {
        "Box" => matches!(member, "as_ref" | "as_mut"),
        "Arc" | "Rc" => matches!(member, "clone" | "as_ref"),
        _ => false,
    }
}
