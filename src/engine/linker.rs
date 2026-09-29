use crate::core::models::*;
use crate::engine::languages::{self, module_name, LanguageFamily, LanguageProfile};
use crate::engine::linker::traits::{
    ImportContext, ImportResolution, LanguageLinker, PackageExports, GENERIC_LINKER,
};
use hashbrown::{HashMap, HashSet};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::path::Path;
pub mod traits;
pub fn needs_reference_identity(path: &str, facts: &Facts) -> bool {
    let language = facts
        .nodes
        .iter()
        .find(|node| node.kind == "module")
        .map_or("", |node| node.language.as_str());
    languages::linker_for(language).needs_reference_identity(path, facts)
}
pub fn reference_identity_changed(path: &str, old: &Facts, new: &Facts) -> bool {
    let language = new
        .nodes
        .iter()
        .find(|node| node.kind == "module")
        .map_or("", |node| node.language.as_str());
    languages::linker_for(language).reference_identity_changed(path, old, new)
}
pub fn required_full_facts(
    facts: &BTreeMap<String, Facts>,
    affected: &std::collections::BTreeSet<String>,
    catalog: &[(&str, &str)],
) -> std::collections::BTreeSet<String> {
    let mut required = std::collections::BTreeSet::new();
    let mut seen = std::collections::BTreeSet::new();
    for profile in languages::profiles() {
        if seen.insert(profile.id()) {
            required.extend(
                languages::linker_for(profile.id()).required_full_facts(facts, affected, catalog),
            );
        }
    }
    required
}
pub fn link(all: &BTreeMap<String, Facts>) -> Graph {
    link_with_root(all, None, None)
}
pub fn link_owners(
    all: &BTreeMap<String, Facts>,
    owners: Option<&std::collections::BTreeSet<String>>,
) -> Graph {
    link_with_root(all, owners, None)
}
pub fn link_with_root(
    all: &BTreeMap<String, Facts>,
    owners: Option<&std::collections::BTreeSet<String>>,
    root: Option<&Path>,
) -> Graph {
    let nodes: Vec<&Node> = all.values().flat_map(|f| f.nodes.iter()).collect();
    let mut by_name: HashMap<&str, Vec<&Node>> = HashMap::with_capacity(nodes.len());
    let mut by_qual: HashMap<&str, Vec<&Node>> = HashMap::with_capacity(nodes.len());
    let mut by_suffix: HashMap<&str, Vec<&Node>> = HashMap::new();
    let mut by_id: HashMap<&str, &Node> = HashMap::with_capacity(nodes.len());
    let mut by_module: HashMap<&str, Vec<&Node>> = HashMap::with_capacity(all.len());
    let needs_discovery = all.iter().any(|(path, facts)| {
        owners.is_none_or(|o| o.contains(path))
            && facts
                .docs
                .iter()
                .any(|doc| !doc.referenced_symbols.is_empty())
    });
    for n in &nodes {
        by_name.entry(&n.name).or_default().push(n);
        by_qual.entry(&n.qualname).or_default().push(n);
        by_id.insert(&n.id, n);
        by_module.entry(&n.path).or_default().push(n);
        if needs_discovery {
            for (i, c) in n.qualname.char_indices() {
                if c == '.' {
                    by_suffix.entry(&n.qualname[i + 1..]).or_default().push(n);
                }
            }
        }
    }
    let mut node_bindings: HashMap<&str, hashbrown::HashSet<&str>> = HashMap::new();
    let mut node_rebindings: HashMap<&str, hashbrown::HashSet<&str>> = HashMap::new();
    for n in &nodes {
        if let Some(arr) = n.details["bindings"].as_array() {
            if !arr.is_empty() {
                let set: hashbrown::HashSet<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                node_bindings.insert(&n.id, set);
            }
        }
        if let Some(arr) = n.details["rebindings"].as_array() {
            if !arr.is_empty() {
                let set: hashbrown::HashSet<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                node_rebindings.insert(&n.id, set);
            }
        }
    }
    let mut children_by_parent: HashMap<&str, Vec<&Node>> = HashMap::new();
    for f in all.values() {
        for e in &f.edges {
            if e.kind == "contains" {
                if let Some(target_node) = by_id.get(e.dst.as_str()) {
                    children_by_parent.entry(e.src.as_str()).or_default().push(target_node);
                }
            }
        }
    }
    let discovery_lookup = |name: &str| -> &[&Node] {
        by_qual
            .get(name)
            .or_else(|| by_suffix.get(name))
            .map(Vec::as_slice)
            .unwrap_or_default()
    };
    let lookup =
        |name: &str| -> &[&Node] { by_qual.get(name).map(Vec::as_slice).unwrap_or_default() };
    let profiles: HashMap<&str, &dyn LanguageProfile> = all
        .iter()
        .filter_map(|(path, f)| {
            f.nodes
                .iter()
                .find(|n| n.kind == "module")
                .and_then(|n| languages::by_id(&n.language))
                .map(|p| (path.as_str(), p))
        })
        .collect();
    let mut modules_by_namespace: HashMap<LanguageFamily, HashMap<String, Vec<&Node>>> =
        HashMap::new();
    for node in nodes.iter().filter(|n| n.kind == "module") {
        if let Some(profile) = profiles.get(node.path.as_str()) {
            let (ws, local_path) = languages::workspace_path(&node.path);
            let local_module = profile.module_name(local_path);
            let mut namespaces = vec![node.qualname.clone()];
            if !ws.is_empty() && node.qualname.starts_with(&format!("{ws}.")) {
                namespaces.push(node.qualname[ws.len() + 1..].to_string());
            }
            if !namespaces.contains(&local_module) {
                namespaces.push(local_module);
            }
            if let Some((_, src_rel)) = local_path.split_once("/src/") {
                let src_module = profile.module_name(src_rel);
                if !namespaces.contains(&src_module) {
                    namespaces.push(src_module);
                }
            } else if let Some(src_rel) = local_path.strip_prefix("src/") {
                let src_module = profile.module_name(src_rel);
                if !namespaces.contains(&src_module) {
                    namespaces.push(src_module);
                }
            }
            for ns in namespaces {
                let list = modules_by_namespace
                    .entry(profile.family())
                    .or_default()
                    .entry(ns)
                    .or_default();
                if !list.iter().any(|existing| existing.id == node.id) {
                    list.push(node);
                }
            }
        }
    }
    let language_ids: HashSet<&str> = profiles.values().map(|profile| profile.id()).collect();
    let mut package_exports: HashMap<&str, PackageExports<'_>> = language_ids
        .into_iter()
        .map(|id| {
            (
                id,
                languages::linker_for(id).package_exports(
                    all,
                    &modules_by_namespace,
                    &by_module,
                    root,
                ),
            )
        })
        .collect();
    package_exports.insert("", HashMap::new());
    let parts: Vec<Graph> = all
        .par_iter()
        .filter(|(path, _)| owners.is_none_or(|o| o.contains(*path)))
        .map(|(path, facts)| {
            let mut graph = Graph {
                edges: facts.edges.clone(),
                coverage: Vec::with_capacity(facts.references.len()),
            };
            let profile = profiles.get(path.as_str()).copied();
            let language_linker = profile.map_or(&GENERIC_LINKER as &dyn LanguageLinker, |profile| {
                languages::linker_for(profile.id())
            });
            let language_package_exports = package_exports
                .get(profile.map_or("", |profile| profile.id()))
                .expect("package export index exists for every linked language");
            let file_module = facts
                .nodes
                .iter()
                .find(|n| n.kind == "module")
                .map(|n| n.qualname.clone())
                .unwrap_or_else(|| profile.map_or_else(|| module_name(path), |p| p.module_name(path)));
            let mut aliases: HashMap<String, HashMap<String, Vec<&Node>>> = HashMap::new();
            let mut external_aliases: HashMap<String, HashMap<String, Option<(String, usize)>>> = HashMap::new();
            let mut lookup_key = String::new();
            for r in facts.references.iter().filter(|r| r.kind == "imports") {
                let normalized = profile.and_then(|p| {
                    p.normalize_import_with_root(
                        root,
                        path,
                        r.module.as_deref().unwrap_or(&r.expression),
                    )
                });
                let mut modules=Vec::new();
                let mut selected_namespace="";
                if let (Some(profile),Some(normalized))=(profile,&normalized) {
                    let workspace=languages::workspace_path(path).0;
                    for owner_workspace in [true,false] {
                        if !owner_workspace && normalized.relative { break; }
                        let mut namespace=normalized.namespace.as_str();
                        loop {
                            if owner_workspace || namespace.contains('.') {
                                modules=modules_by_namespace.get(&profile.family()).and_then(|namespaces|namespaces.get(namespace))
                                    .into_iter().flatten().copied()
                                    .filter(|n|(languages::workspace_path(&n.path).0==workspace)==owner_workspace).collect();
                            }
                            if !modules.is_empty(){selected_namespace=namespace;break;}
                            if !normalized.symbol_path {break;}
                            match namespace.rsplit_once('.') {Some((parent,_))=>namespace=parent,None=>break}
                        }
                        if !modules.is_empty(){break;}
                    }
                }
                let symbol=normalized.as_ref().map_or_else(||r.expression.clone(),|n|n.namespace.clone());
                let mut candidates=Vec::new();
                let resolution = language_linker.resolve_import(&mut ImportContext {
                    path,
                    reference: r,
                    normalized: normalized.as_ref(),
                    selected_namespace,
                    family: profile.map_or(LanguageFamily(""), |profile| profile.family()),
                    modules: &mut modules,
                    candidates: &mut candidates,
                    modules_by_namespace: &modules_by_namespace,
                    by_module: &by_module,
                    by_qual: &by_qual,
                    package_exports: language_package_exports,
                    lookup_key: &mut lookup_key,
                });
                let ImportResolution {
                    paired_stub,
                    child_module,
                    stub_symbol,
                    reexport_symbol,
                } = resolution;
                candidates.sort_by(|a,b|a.id.cmp(&b.id));
                candidates.dedup_by_key(|n|&n.id);
                let external = if modules.is_empty() && normalized.as_ref().is_some_and(|n| !n.relative) {
                    profile.and_then(|p| p.external_import(r.module.as_deref().unwrap_or(&r.expression)))
                } else {
                    None
                };
                let is_wildcard = r.alias.as_deref() == Some("*") || r.expression == "*";
                if is_wildcard {
                    let scope = by_id.get(r.source.as_str()).map(|n| n.qualname.clone()).unwrap_or_else(|| file_module.clone());
                    for target_module in &modules {
                        if let Some(module_nodes) = by_module.get(target_module.path.as_str()) {
                            for n in module_nodes {
                                if n.kind != "module" && !n.name.is_empty() {
                                    aliases.entry(scope.clone()).or_default().entry(n.name.clone()).or_default().push(n);
                                }
                            }
                        }
                    }
                } else if let Some(alias) = &r.alias {
                    let scope = by_id.get(r.source.as_str()).map(|n| n.qualname.clone()).unwrap_or_else(|| file_module.clone());
                    let local_declaration = by_qual.get(qualified(&mut lookup_key, &scope, alias)).is_some_and(|nodes| nodes.iter().any(|n| n.path == *path));
                    let targets = if local_declaration { Vec::new() } else { candidates.clone() };
                    let alias_taken = aliases.get(&scope).is_some_and(|entries| entries.contains_key(alias));
                    if alias_taken {
                        if let Some(provenance) = external_aliases.get_mut(&scope).and_then(|entries| entries.get_mut(alias)) {
                            *provenance = None;
                        }
                    } else if external.is_some() && !local_declaration && language_linker.tracks_external_aliases() {
                        external_aliases.entry(scope.clone()).or_default().insert(alias.clone(), Some((symbol.clone(), r.line)));
                    }
                    aliases.entry(scope).or_default().entry(alias.clone()).or_default().extend(targets);
                }
                let resolved = modules.len() == 1 && (r.alias.is_none() || is_wildcard || candidates.len() == 1);
                let status = if resolved { "resolved" } else if modules.len() > 1 || candidates.len() > 1 { "ambiguous" } else if external.is_some() { "external" } else { "unresolved" };
                let evidence = if is_wildcard && resolved {
                    format!("wildcard import from {symbol}: imported from {}", modules[0].path)
                } else if reexport_symbol {
                    format!("import {symbol}: explicit local package re-export")
                } else if stub_symbol {
                    format!("import {symbol}: declaration from paired type stub; runtime declaration not indexed")
                } else if let Some(kind)=external {
                    format!("{kind}; no indexed provider for {symbol}")
                } else if modules.is_empty() && normalized.as_ref().is_some_and(|n| !n.relative) {
                    format!("no indexed provider for absolute import {symbol}; external dependency or missing source remains unverified")
                } else {
                    format!("import {symbol}: {} modules, {} alias targets",modules.len(),candidates.len())
                };
                graph.coverage.push(Coverage {
                    path: path.clone(),
                    line: r.line,
                    expression: r.expression.clone(),
                    status: status.into(),
                    evidence,
                });
                if modules.len() == 1 {
                    graph.edges.push(Edge {
                        src: r.source.clone(),
                        dst: modules[0].id.clone(),
                        kind: "imports".into(),
                        path: path.clone(),
                        line: r.line,
                        evidence: r.expression.clone(),
                        confidence: "exact".into(),
                    });
                    if child_module {
                        graph.edges.push(Edge {
                            src: r.source.clone(),
                            dst: candidates[0].id.clone(),
                            kind: "imports".into(),
                            path: path.clone(),
                            line: r.line,
                            evidence: r.expression.clone(),
                            confidence: "exact".into(),
                        });
                    }
                    if stub_symbol {
                        if let Some(stub) = paired_stub {
                            graph.edges.push(Edge {
                                src: r.source.clone(),
                                dst: stub.id.clone(),
                                kind: "imports".into(),
                                path: path.clone(),
                                line: r.line,
                                evidence: r.expression.clone(),
                                confidence: "exact".into(),
                            });
                        }
                    }
                    if reexport_symbol {
                        graph.edges.push(Edge {
                            src: r.source.clone(),
                            dst: candidates[0].id.clone(),
                            kind: "imports".into(),
                            path: path.clone(),
                            line: r.line,
                            evidence: r.expression.clone(),
                            confidence: "exact".into(),
                        });
                    }
                }
            }
            let alias_for = |name: &str, owner: Option<&Node>, definition_scope: bool| {
                let mut scope = owner.map(|n| n.qualname.as_str()).unwrap_or("");
                if definition_scope { scope = scope.rsplit_once('.').map_or("", |(p, _)| p); }
                loop {
                    let class_scope = !profile.is_some_and(|p|p.class_scope()) && !definition_scope && owner.is_some_and(|n| matches!(n.kind.as_str(), "function" | "method"))
                        && by_qual.get(scope).is_some_and(|nodes| nodes.iter().any(|n| n.path==*path && matches!(n.kind.as_str(), "class" | "impl")));
                    if !class_scope {
                        if let Some(targets) = aliases.get(scope).and_then(|a| a.get(name)) { return Some(targets); }
                    }
                    if scope.is_empty() { return None; }
                    scope = scope.rsplit_once('.').map_or("", |(p, _)| p);
                }
            };
            let external_for = |name: &str, owner: Option<&Node>| {
                let mut scope = owner.map(|n| n.qualname.as_str()).unwrap_or("");
                loop {
                    let class_scope = !profile.is_some_and(|p|p.class_scope())
                        && owner.is_some_and(|n| matches!(n.kind.as_str(), "function" | "method"))
                        && by_qual.get(scope).is_some_and(|nodes| nodes.iter().any(|n| n.path==*path && matches!(n.kind.as_str(), "class" | "impl")));
                    if !class_scope && aliases.get(scope).is_some_and(|entries| entries.contains_key(name)) {
                        return external_aliases.get(scope).and_then(|entries| entries.get(name)).and_then(Option::as_ref);
                    }
                    if scope.is_empty() { return None; }
                    scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
                }
            };
            for r in facts
                .references
                .iter()
                .filter(|r| r.kind == "extends" || r.kind == "includes")
            {
                let target_path = r.expression.trim();
                let candidate = nodes
                    .iter()
                    .find(|n| n.kind == "module" && n.path == target_path)
                    .or_else(|| {
                        let suffix = format!("/{target_path}");
                        nodes
                            .iter()
                            .find(|n| n.kind == "module" && n.path.ends_with(&suffix))
                    });
                if let Some(target) = candidate {
                    graph.edges.push(Edge {
                        src: r.source.clone(),
                        dst: target.id.clone(),
                        kind: r.kind.clone(),
                        path: path.clone(),
                        line: r.line,
                        evidence: format!("{path}: {} {}", r.kind, target.path),
                        confidence: "exact".into(),
                    });
                    graph.coverage.push(Coverage {
                        path: path.clone(),
                        line: r.line,
                        expression: r.expression.clone(),
                        status: "resolved".into(),
                        evidence: format!("template target: {}", target.path),
                    });
                } else {
                    graph.coverage.push(Coverage {
                        path: path.clone(),
                        line: r.line,
                        expression: r.expression.clone(),
                        status: "unresolved".into(),
                        evidence: format!("template '{}' not found", r.expression),
                    });
                }
            }

            let mut candidates = Vec::new();
            for r in facts.references.iter().filter(|r| {
                matches!(
                    r.kind.as_str(),
                    "calls" | "inherits" | "implements" | "bases" | "decorates" | "mutates" | "handles" | "references"
                )
            }) {
                if r.dynamic {
                    graph.coverage.push(Coverage {
                        path: path.clone(), line: r.line, expression: r.expression.clone(),
                        status: "unresolved".into(),
                        evidence: "computed receiver or dynamic callee; callable identity is unknown".into(),
                    });
                    continue;
                }
                let expression = r.expression.replace("::", ".");
                let (first, tail) = expression.split_once('.').unwrap_or((&expression, ""));
                let owner = by_id.get(r.source.as_str()).copied();
                let mut scope = owner.map(|n| n.qualname.as_str()).unwrap_or("");
                let definition_scope = matches!(r.kind.as_str(), "decorates" | "inherits" | "implements" | "bases");
                if definition_scope { scope = scope.rsplit_once('.').map_or("", |(p, _)| p); }
                let mut shadowed = false;
                let imported = alias_for(first, owner, definition_scope);
                candidates.clear();
                while !scope.is_empty() {
                    if let Some(scope_nodes) = by_qual.get(scope) {
                        if !profile.is_some_and(|p|p.class_scope()) && !definition_scope && owner.is_some_and(|n| matches!(n.kind.as_str(), "function" | "method"))
                            && scope_nodes.iter().any(|n| n.path==*path && matches!(n.kind.as_str(), "class" | "impl")) {
                            scope = scope.rsplit_once('.').map_or("", |(p, _)| p);
                            continue;
                        }
                        if profile.is_none_or(|p|p.value_binding_applies(r)) && scope_nodes.iter().filter(|n|n.path==*path).any(|n| {
                            node_bindings.get(n.id.as_str()).is_some_and(|b| b.contains(first))
                        }) {
                            shadowed = true;
                            break;
                        }
                    }
                    if aliases.get(scope).is_some_and(|a| a.contains_key(first)) { break; }
                    if tail.is_empty() {
                        candidates.extend(lookup(qualified(&mut lookup_key, scope, &expression)).iter().copied().filter(|n|n.path==*path));
                        if !candidates.is_empty() {
                            break;
                        }
                    }
                    scope = scope.rsplit_once('.').map_or("", |(p, _)| p);
                }
                let known_receiver = !tail.is_empty()
                    && owner.is_some_and(|n|profile.is_some_and(|p|language_linker.resolve_receiver(p,first,n)))
                    && owner.is_some_and(|n| {
                        if node_rebindings.get(n.id.as_str()).is_some_and(|b| b.contains(first)) {
                            return false;
                        }
                        n.qualname.rsplit_once('.').is_some_and(|(scope, _)| {
                            by_qual.get(scope).is_some_and(|nodes| {
                                nodes.iter().any(|n| n.path==*path && matches!(n.kind.as_str(), "class" | "impl"))
                            })
                        })
                    });
                if shadowed
                    && !known_receiver
                    && matches!(r.kind.as_str(), "calls" | "handles" | "mutates")
                {
                    graph.coverage.push(Coverage {
                        path: path.clone(),
                        line: r.line,
                        expression: r.expression.clone(),
                        status: "unresolved".into(),
                        evidence:
                            "callee is shadowed by a parameter or local binding of unknown callable identity"
                                .into(),
                    });
                    continue;
                }
                if candidates.is_empty() {
                    if let Some(imported) = imported {
                        if tail.is_empty() {
                            candidates.extend_from_slice(imported);
                        } else {
                            for n in imported {
                                candidates.extend(lookup(qualified(&mut lookup_key, &n.qualname, tail)).iter().copied().filter(|target|target.path==n.path));
                            }
                            if candidates.is_empty() {
                                if let [module] = imported.as_slice() {
                                    if module.kind == "module" {
                                        candidates.extend(language_linker.resolve_imported_member(
                                            module,
                                            tail,
                                            &by_module,
                                            &by_qual,
                                            &mut lookup_key,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
                if candidates.is_empty() && imported.is_none() {
                    if !tail.is_empty() {
                        candidates.extend_from_slice(lookup(&expression));
                    }
                    if candidates.is_empty() {
                        if tail.is_empty() {
                            candidates.extend_from_slice(lookup(qualified(&mut lookup_key, &file_module, &expression)));
                        } else if known_receiver {
                            if let Some(owner) = owner.filter(|_| known_receiver) {
                                let scope = owner.qualname.rsplit_once('.').map_or("", |(p, _)| p);
                                candidates.extend_from_slice(lookup(qualified(&mut lookup_key, scope, tail)));
                            }
                        } else {
                            candidates.extend_from_slice(lookup(qualified(&mut lookup_key, &file_module, &expression)));
                        }
                    }
                }
                if imported.is_none() {
                    candidates.retain(|n| {
                        n.path == *path
                            || (profile.is_some_and(|p| p.sibling_accessible(path, &n.path))
                                && by_module
                                    .get(n.path.as_str())
                                    .into_iter()
                                    .flatten()
                                    .any(|m| m.kind == "module" && m.qualname == file_module))
                            || (r.kind == "references" && n.qualname == expression)
                    });
                }
                candidates.retain(|n| match r.kind.as_str() {
                    "inherits" | "bases" => matches!(n.kind.as_str(), "class" | "interface" | "struct"),
                    "implements" => matches!(n.kind.as_str(), "trait" | "interface"),
                    "mutates" => n.kind == "field",
                    "references" => matches!(n.kind.as_str(), "class" | "enum" | "type" | "struct" | "trait" | "interface" | "field"),
                    _ => matches!(
                        n.kind.as_str(),
                        "function" | "method" | "class" | "struct" | "enum" | "macro"
                    ),
                });
                candidates.sort_by(|a, b| a.id.cmp(&b.id));
                candidates.dedup_by_key(|n| &n.id);
                let mut source = Some(r.source.clone());
                let mut source_heuristic = false;
                if r.kind == "implements" {
                    if let Some(implementation) = owner.filter(|n| n.kind == "impl") {
                        let clean = implementation.name.split('<').next().unwrap_or(&implementation.name).trim().replace("::", ".");
                        let (head, rest) = clean.split_once('.').unwrap_or((&clean, ""));
                        let mut types: Vec<&Node> = if let Some(imported) = alias_for(head, owner, true) {
                            if rest.is_empty() { imported.clone() } else {
                                imported.iter().flat_map(|n| lookup(&format!("{}.{}", n.qualname, rest)).iter().copied().filter(move |target|target.path==n.path)).collect()
                            }
                        } else {
                            lookup(qualified(&mut lookup_key, &file_module, &clean)).to_vec()
                        };
                        types.retain(|n| matches!(n.kind.as_str(), "struct" | "enum"));
                        if types.is_empty() && alias_for(head, owner, true).is_none() {
                            types = by_name.get(clean.as_str()).into_iter().flatten().copied()
                                .filter(|n| matches!(n.kind.as_str(), "struct" | "enum")).collect();
                            source_heuristic = !types.is_empty();
                        }
                        source = (types.len() == 1).then(|| types[0].id.clone());
                    }
                }
                let is_builtin = candidates.is_empty()
                    && !shadowed && tail.is_empty() && imported.is_none()
                    && !by_module.get(path.as_str()).into_iter().flatten().any(|n| n.name == first)
                    && profile.is_some_and(|p|p.builtin(&expression));
                let status = if (candidates.len() == 1 || is_builtin) && source.is_some() && !source_heuristic {
                    "resolved"
                } else if candidates.is_empty() || source.is_none() || source_heuristic {
                    "unresolved"
                } else {
                    "ambiguous"
                };
                let external_origin = if status == "unresolved" && r.kind == "calls" && !shadowed
                    && candidates.is_empty() && imported.is_some_and(|targets| targets.is_empty()) {
                    external_for(first, owner).filter(|(_, import_line)| *import_line <= r.line)
                } else {
                    None
                };
                let stub_target = match candidates.as_slice() {
                    [candidate] if language_linker.is_declaration_only(&candidate.path) => {
                        Some(candidate.path.as_str())
                    }
                    _ => None,
                };
                let evidence = if let Some(stub_path) = stub_target {
                    format!("type-stub declaration at {stub_path}; runtime implementation not statically indexed")
                } else if let Some((module, _)) = external_origin {
                    format!("call through external import {module}; callable target unverified")
                } else if is_builtin {
                    format!("standard library or built-in callee: {expression}")
                } else {
                    format!("{} lexically justified candidates; implementor resolved={}, heuristic={source_heuristic}", candidates.len(), source.is_some())
                };
                graph.coverage.push(Coverage {
                    path: path.clone(),
                    line: r.line,
                    expression: r.expression.clone(),
                    status: status.into(),
                    evidence,
                });
                if let (Some(source), [candidate]) = (source, candidates.as_slice()) {
                    let target = candidate.id.clone();
                    let (src, dst) = if r.kind == "decorates" {
                        (target, source)
                    } else {
                        (source, target)
                    };
                    let kind_str = if r.kind == "bases" || r.kind == "inherits" {
                        let is_interface = candidate.kind == "interface"
                            || candidate.details.get("bases").and_then(|b| b.as_str()).is_some_and(|b| b.contains("Protocol") || b.contains("ABC"));
                        if is_interface && owner.is_some_and(|n| n.kind != "interface") {
                            "implements"
                        } else {
                            "inherits"
                        }
                    } else {
                        r.kind.as_str()
                    };
                    graph.edges.push(Edge {
                        src: src.clone(),
                        dst: dst.clone(),
                        kind: kind_str.into(),
                        path: path.clone(),
                        line: r.line,
                        evidence: r.expression.clone(),
                        confidence: if source_heuristic { "heuristic" } else { "exact" }.into(),
                    });
                    if (kind_str == "implements" || kind_str == "inherits") && r.kind != "decorates" {
                        if let Some(target_methods) = children_by_parent.get(candidate.id.as_str()) {
                            let source_container_id = if owner.is_some_and(|n| n.kind == "impl") {
                                r.source.as_str()
                            } else {
                                src.as_str()
                            };
                            if let Some(source_methods) = children_by_parent.get(source_container_id) {
                                let is_interface = candidate.kind == "interface"
                                    || candidate.kind == "trait"
                                    || candidate.details.get("bases").and_then(|b| b.as_str()).is_some_and(|b| b.contains("Protocol") || b.contains("ABC"));
                                let method_kind = if is_interface { "implements" } else { "overrides" };
                                for sm in source_methods {
                                    if matches!(sm.kind.as_str(), "function" | "method") {
                                        if let Some(tm) = target_methods.iter().find(|tm| tm.name == sm.name && matches!(tm.kind.as_str(), "function" | "method")) {
                                            graph.edges.push(Edge {
                                                src: sm.id.clone(),
                                                dst: tm.id.clone(),
                                                kind: method_kind.into(),
                                                path: path.clone(),
                                                line: sm.line,
                                                evidence: format!("{}::{}: {method_kind} {}::{}", sm.qualname, sm.name, tm.qualname, tm.name),
                                                confidence: "exact".into(),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            for doc in &facts.docs {
                for symbol in &doc.referenced_symbols {
                    let normalized = symbol.replace("::", ".");
                    let mut candidates = discovery_lookup(&normalized);
                    if candidates.is_empty() {
                        candidates = by_name.get(normalized.as_str()).map(Vec::as_slice).unwrap_or_default();
                    }
                    for node in candidates {
                        for (src, dst, kind) in [
                            (&doc.doc_id, &node.id, "documents"),
                            (&node.id, &doc.doc_id, "references_doc"),
                        ] {
                            graph.edges.push(Edge {
                                src: src.clone(),
                                dst: dst.clone(),
                                kind: kind.into(),
                                path: path.clone(),
                                line: 1,
                                evidence: symbol.clone(),
                                confidence: "exact".into(),
                            });
                        }
                    }
                }
            }
            graph
        })
        .collect();
    let mut result = Graph {
        edges: Vec::with_capacity(parts.iter().map(|g| g.edges.len()).sum()),
        coverage: Vec::with_capacity(parts.iter().map(|g| g.coverage.len()).sum()),
    };
    for g in parts {
        result.edges.extend(g.edges);
        result.coverage.extend(g.coverage);
    }
    result.edges.sort_by(|a, b| {
        (&a.path, a.line, &a.src, &a.dst, &a.kind).cmp(&(&b.path, b.line, &b.src, &b.dst, &b.kind))
    });
    result
        .coverage
        .sort_by(|a, b| (&a.path, a.line, &a.expression).cmp(&(&b.path, b.line, &b.expression)));
    result
}

fn qualified<'a>(buffer: &'a mut String, namespace: &str, name: &str) -> &'a str {
    buffer.clear();
    buffer.push_str(namespace);
    buffer.push('.');
    buffer.push_str(name);
    buffer
}
