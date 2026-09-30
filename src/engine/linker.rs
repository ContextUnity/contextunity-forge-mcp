use crate::core::models::*;
use crate::engine::languages::{self, module_name, ImportPath, LanguageFamily, LanguageProfile};
use crate::engine::linker::traits::{
    ImportContext, ImportResolution, LanguageLinker, PackageExports, GENERIC_LINKER,
};
use hashbrown::{HashMap, HashSet};
use rayon::prelude::*;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::Path;
mod receivers;
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
    let dependency_registry = languages::manifests::DependencyRegistry::collect(root);
    link_with_registry(all, owners, root, &dependency_registry)
}

pub fn link_with_registry(
    all: &BTreeMap<String, Facts>,
    owners: Option<&std::collections::BTreeSet<String>>,
    root: Option<&Path>,
    dependency_registry: &languages::manifests::DependencyRegistry,
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
                    children_by_parent
                        .entry(e.src.as_str())
                        .or_default()
                        .push(target_node);
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
    let mut normalized_imports: HashMap<(&str, &str), Option<ImportPath>> = HashMap::new();
    for (path, facts) in all.iter() {
        let profile = profiles.get(path.as_str()).copied();
        if owners.is_some_and(|owners| !owners.contains(path))
            && !profile.is_some_and(|profile| profile.id() == "python")
        {
            continue;
        }
        for reference in facts
            .references
            .iter()
            .filter(|reference| reference.kind == "imports")
        {
            let module = reference.module.as_deref().unwrap_or(&reference.expression);
            normalized_imports
                .entry((path.as_str(), module))
                .or_insert_with(|| {
                    profile
                        .and_then(|profile| profile.normalize_import_with_root(root, path, module))
                });
        }
    }
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
    let python_receivers = receivers::PythonReceivers::build(all, |class, reference| {
        let facts = &all[&class.path];
        let expression = reference.expression.as_str();
        let (head, tail) = expression.split_once('.').unwrap_or((expression, ""));
        let mut scope = class
            .qualname
            .rsplit_once('.')
            .map_or("", |(parent, _)| parent);
        let mut key = String::new();
        while !scope.is_empty() {
            if by_qual.get(scope).is_some_and(|nodes| {
                nodes.iter().any(|node| {
                    node.path == class.path
                        && node_rebindings
                            .get(node.id.as_str())
                            .is_some_and(|bindings| bindings.contains(head))
                })
            }) {
                return receivers::Base::Unknown;
            }
            let local: Vec<_> = lookup(qualified(&mut key, scope, expression))
                .iter()
                .copied()
                .filter(|node| node.path == class.path && node.kind == "class")
                .collect();
            if local.len() == 1 {
                if facts.references.iter().any(|import| {
                    import.kind == "imports"
                        && import.alias.as_deref() == Some(head)
                        && import.line > local[0].line
                        && import.line <= class.line
                        && by_id
                            .get(import.source.as_str())
                            .is_some_and(|node| node.qualname == scope)
                }) {
                    return receivers::Base::Unknown;
                }
                return receivers::Base::Local(&local[0].id);
            }
            if local.len() > 1 {
                return receivers::Base::Unknown;
            }
            let imports: Vec<_> = facts
                .references
                .iter()
                .filter(|import| {
                    import.kind == "imports"
                        && import.alias.as_deref() == Some(head)
                        && by_id
                            .get(import.source.as_str())
                            .is_some_and(|node| node.qualname == scope)
                })
                .collect();
            if !imports.is_empty() {
                if imports.len() != 1 {
                    return receivers::Base::Unknown;
                }
                let import = imports[0];
                if import.line > class.line
                    || by_qual.get(scope).is_some_and(|nodes| {
                        nodes.iter().any(|node| {
                            node.path == class.path
                                && node_rebindings
                                    .get(node.id.as_str())
                                    .is_some_and(|bindings| bindings.contains(head))
                        })
                    })
                {
                    return receivers::Base::Unknown;
                }
                let Some(profile) = profiles.get(class.path.as_str()).copied() else {
                    return receivers::Base::Unknown;
                };
                let normalized = normalized_imports
                    .get(&(
                        class.path.as_str(),
                        import.module.as_deref().unwrap_or(&import.expression),
                    ))
                    .and_then(Option::as_ref);
                let Some(normalized) = normalized else {
                    return receivers::Base::Unknown;
                };
                let mut modules: Vec<_> = modules_by_namespace
                    .get(&profile.family())
                    .and_then(|namespaces| namespaces.get(&normalized.namespace))
                    .into_iter()
                    .flatten()
                    .copied()
                    .collect();
                let workspace = languages::workspace_path(&class.path).0;
                if modules
                    .iter()
                    .any(|module| languages::workspace_path(&module.path).0 == workspace)
                {
                    modules.retain(|module| languages::workspace_path(&module.path).0 == workspace);
                } else if normalized.relative {
                    modules.clear();
                }
                let mut targets = Vec::new();
                languages::linker_for(profile.id()).resolve_import(&mut ImportContext {
                    path: &class.path,
                    reference: import,
                    normalized: Some(normalized),
                    selected_namespace: &normalized.namespace,
                    family: profile.family(),
                    modules: &mut modules,
                    candidates: &mut targets,
                    modules_by_namespace: &modules_by_namespace,
                    by_module: &by_module,
                    by_qual: &by_qual,
                    package_exports: &package_exports[profile.id()],
                    lookup_key: &mut key,
                });
                if !tail.is_empty() {
                    targets = targets
                        .iter()
                        .flat_map(|target| {
                            lookup(&format!("{}.{}", target.qualname, tail))
                                .iter()
                                .copied()
                                .filter(move |node| node.path == target.path)
                        })
                        .collect();
                }
                targets.retain(|target| target.kind == "class");
                targets.sort_by_key(|target| &target.id);
                targets.dedup_by_key(|target| &target.id);
                if targets.len() == 1 {
                    return receivers::Base::Local(&targets[0].id);
                }
                if targets.is_empty() && modules.is_empty() && !normalized.relative {
                    let module = import.module.as_deref().unwrap_or(&import.expression);
                    let mut canonical = module.to_owned();
                    if import.expression != module {
                        canonical.push('.');
                        canonical.push_str(&import.expression);
                    }
                    if !tail.is_empty() {
                        canonical.push('.');
                        canonical.push_str(tail);
                    }
                    return receivers::Base::External(canonical, module, import.line);
                }
                return receivers::Base::Unknown;
            }
            if by_qual.get(scope).is_some_and(|nodes| {
                nodes.iter().any(|node| {
                    node.path == class.path
                        && node_bindings
                            .get(node.id.as_str())
                            .is_some_and(|bindings| bindings.contains(head))
                })
            }) {
                return receivers::Base::Unknown;
            }
            scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
        }
        if expression == "object" {
            receivers::Base::External("builtins.object".into(), "builtins", 0)
        } else {
            receivers::Base::Unknown
        }
    });
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
                let normalized = normalized_imports.get(&(path.as_str(), r.module.as_deref().unwrap_or(&r.expression))).expect("imports are normalized before parallel resolution");
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
                if candidates.len() > 1 {
                    let non_stubs: Vec<&Node> = candidates
                        .iter()
                        .copied()
                        .filter(|n| {
                            let is_stub = n.details.get("is_stub").and_then(|v| v.as_bool()).unwrap_or(false);
                            let is_overload = n.details.get("is_overload").and_then(|v| v.as_bool()).unwrap_or(false);
                            !is_stub && !is_overload
                        })
                        .collect();
                    if !non_stubs.is_empty() && non_stubs.len() < candidates.len() {
                        candidates = non_stubs;
                    }
                }
                let external = if modules.is_empty() && normalized.as_ref().is_some_and(|n| !n.relative) {
                    profile.and_then(|p| dependency_registry.classification(p, r.module.as_deref().unwrap_or(&r.expression)))
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
                    } else if external.is_some() && !local_declaration {
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
            let external_for = |name: &str, owner: Option<&Node>, definition_scope: bool| {
                let mut scope = owner.map(|n| n.qualname.as_str()).unwrap_or("");
                if definition_scope { scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent); }
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

            let mut alias_import_lines: HashMap<(&str, &str), usize> = HashMap::new();
            for reference in facts.references.iter().filter(|reference| reference.kind == "imports") {
                if let Some(alias) = reference.alias.as_deref() {
                    let scope = by_id.get(reference.source.as_str()).map_or(file_module.as_str(), |node| node.qualname.as_str());
                    let line = alias_import_lines.entry((scope, alias)).or_default();
                    *line = (*line).max(reference.line);
                }
            }
            let path_constructor_aliases: HashMap<(&str, &str), &Reference> = facts.references.iter().filter(|reference| reference.kind == "imports" && reference.module.as_deref() == Some("pathlib")).filter_map(|reference| {
                let alias = reference.alias.as_deref()?;
                let scope = by_id.get(reference.source.as_str()).map_or(file_module.as_str(), |node| node.qualname.as_str());
                Some(((scope, alias), reference))
            }).collect();
            let mut dynamic_type_aliases: HashMap<&str, HashSet<&str>> = HashMap::new();
            for reference in facts.references.iter().filter(|reference| reference.kind == "imports" && reference.expression == "Any" && matches!(reference.module.as_deref(), Some("typing" | "typing_extensions"))) {
                let scope = by_id.get(reference.source.as_str()).map_or(file_module.as_str(), |node| node.qualname.as_str());
                dynamic_type_aliases.entry(scope).or_default().insert(reference.alias.as_deref().unwrap_or("Any"));
            }
            let dynamic_type_for = |name: &str, owner: &Node| {
                let mut scope = owner.qualname.rsplit_once('.').map_or("", |(parent, _)| parent);
                loop {
                    if aliases.get(scope).is_some_and(|entries| entries.contains_key(name)) {
                        return dynamic_type_aliases.get(scope).is_some_and(|names| names.contains(name));
                    }
                    if scope.is_empty() { return false; }
                    scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
                }
            };
            let mut typed_parameters: HashMap<&str, HashMap<&str, ReceiverType<'_>>> = HashMap::new();
            let mut type_candidates = Vec::new();
            for node in &facts.nodes {
                let Some(parameters) = node.details["param_types"].as_object() else { continue; };
                for (parameter, annotation) in parameters {
                    if node_rebindings.get(node.id.as_str()).is_some_and(|bindings| bindings.contains(parameter.as_str())) {
                        continue;
                    }
                    let Some(annotation) = annotation.as_str().and_then(receiver_type_name) else { continue; };
                    let (head, member) = annotation.split_once('.').unwrap_or((&annotation, ""));
                    type_candidates.clear();
                    let mut scope = node.qualname.rsplit_once('.').map_or("", |(parent, _)| parent);
                    let mut annotation_shadowed = false;
                    while !scope.is_empty() {
                        if by_qual.get(scope).is_some_and(|scope_nodes| scope_nodes.iter().any(|scope_node| scope_node.path == *path && node_rebindings.get(scope_node.id.as_str()).is_some_and(|bindings| bindings.contains(head)))) {
                            annotation_shadowed = true; break;
                        }
                        type_candidates.extend(lookup(qualified(&mut lookup_key, scope, &annotation)).iter().copied().filter(|candidate| candidate.path == *path));
                        if !type_candidates.is_empty() && alias_import_lines.get(&(scope, head)).is_some_and(|line| *line <= node.line && type_candidates.iter().any(|candidate| candidate.line < *line)) { type_candidates.clear(); annotation_shadowed = true; break; }
                        if !type_candidates.is_empty() { break; }
                        if by_qual.get(scope).is_some_and(|scope_nodes| scope_nodes.iter().filter(|scope_node| scope_node.path == *path).any(|scope_node| node_bindings.get(scope_node.id.as_str()).is_some_and(|bindings| bindings.contains(head)))) {
                            annotation_shadowed = true;
                            break;
                        }
                        if aliases.get(scope).is_some_and(|entries| entries.contains_key(head)) { break; }
                        scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
                    }
                    let imported_type = if annotation_shadowed || !type_candidates.is_empty() { None } else { alias_for(head, Some(node), true) };
                    if let Some(imported) = imported_type {
                        if member.is_empty() {
                            type_candidates.extend_from_slice(imported);
                        } else {
                            for target in imported {
                                type_candidates.extend(lookup(qualified(&mut lookup_key, &target.qualname, member)).iter().copied().filter(|candidate| candidate.path == target.path));
                            }
                        }
                    }
                    type_candidates.retain(|candidate| matches!(candidate.kind.as_str(), "class" | "struct" | "enum" | "type" | "interface"));
                    type_candidates.sort_by(|a, b| a.id.cmp(&b.id));
                    type_candidates.dedup_by_key(|candidate| candidate.id.as_str());
                    let receiver = match type_candidates.as_slice() {
                        [target] => ReceiverType::Local(target),
                        [] => {
                            if imported_type.is_some_and(|targets| targets.is_empty()) {
                                external_for(head, Some(node), true)
                                    .filter(|(module, import_line)| *import_line <= node.line && !(matches!(module.as_str(), "typing" | "typing_extensions") && (member == "Any" || dynamic_type_for(head, node))))
                                    .map_or(ReceiverType::Unknown, |(module, line)| ReceiverType::External(module, *line))
                            } else {
                                ReceiverType::Unknown
                            }
                        }
                        _ => ReceiverType::Ambiguous,
                    };
                    if !matches!(receiver, ReceiverType::Unknown) {
                        typed_parameters.entry(node.id.as_str()).or_default().insert(parameter.as_str(), receiver);
                    }
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
                    let owner = by_id.get(r.source.as_str()).copied();
                    let mut computed = receivers::Member::Unknown;
                    let mut literal_builtin = false;
                    if profile.is_some_and(|profile| profile.id() == "python") {
                        match r.receiver_hint.as_ref() {
                            Some(ReceiverHint::StringLiteral { member }) => {
                                literal_builtin = python_string_method(member);
                            }
                            Some(ReceiverHint::Super { member }) => {
                                let shadowed_super = owner.is_some_and(|owner| {
                                    let mut scope = owner.qualname.as_str();
                                    while !scope.is_empty() {
                                        if by_qual.get(scope).is_some_and(|nodes| nodes.iter().any(|node| node.path == *path && node_bindings.get(node.id.as_str()).is_some_and(|bindings| bindings.contains("super")))) || aliases.get(scope).is_some_and(|bindings| bindings.contains_key("super")) { return true; }
                                        scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
                                    }
                                    false
                                });
                                if !shadowed_super {
                                    if let Some(owner) = owner.filter(|owner| owner.details["receiver_name"].as_str().is_some_and(|name| matches!(name, "self" | "cls") && !node_rebindings.get(owner.id.as_str()).is_some_and(|bindings| bindings.contains(name)))) {
                                        if let Some((scope, _)) = owner.qualname.rsplit_once('.') {
                                            if let Some([class]) = by_qual.get(scope).map(Vec::as_slice) {
                                                if class.kind == "class" && class.path == *path { computed = python_receivers.lookup(class, member, true); }
                                            }
                                        }
                                    }
                                }
                            }
                            Some(ReceiverHint::CallResult { callee, member }) => {
                                let (head, tail) = callee.split_once('.').unwrap_or((callee, ""));
                                let mut scope = owner.map_or(file_module.as_str(), |owner| owner.qualname.as_str());
                                let mut constructor_shadowed = false;
                                candidates.clear();
                                while !scope.is_empty() {
                                    if by_qual.get(scope).is_some_and(|nodes| nodes.iter().any(|node| node.path == *path && node_bindings.get(node.id.as_str()).is_some_and(|bindings| bindings.contains(head)))) { constructor_shadowed = true; break; }
                                    candidates.extend(lookup(qualified(&mut lookup_key, scope, callee)).iter().copied().filter(|node| node.path == *path));
                                    if !candidates.is_empty() && alias_import_lines.get(&(scope, head)).is_some_and(|line| *line <= r.line && candidates.iter().any(|candidate| candidate.line < *line)) { candidates.clear(); constructor_shadowed = true; break; }
                                    if !candidates.is_empty() || aliases.get(scope).is_some_and(|entries| entries.contains_key(head)) { break; }
                                    scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
                                }
                                if candidates.is_empty() && !constructor_shadowed && aliases.get(scope).is_some_and(|entries| entries.contains_key(head)) {
                                    if let Some(imported) = alias_for(head, owner, false) {
                                        if tail.is_empty() { candidates.extend_from_slice(imported); }
                                        else { for target in imported { candidates.extend(lookup(qualified(&mut lookup_key, &target.qualname, tail)).iter().copied().filter(|node| node.path == target.path)); } }
                                    }
                                }
                                candidates.sort_by_key(|node| &node.id); candidates.dedup_by_key(|node| &node.id);
                                if let [class] = candidates.as_slice() {
                                    if class.kind == "class" { computed = python_receivers.lookup(class, member, false); }
                                }
                                if candidates.is_empty() && !constructor_shadowed {
                                    if let Some((origin, import_line)) = external_for(head, owner, false).filter(|(_, line)| *line <= r.line) {
                                        if let Some(import) = path_constructor_aliases.get(&(scope, head)).filter(|import| import.line == *import_line && origin == "pathlib") {
                                            let class = if import.expression == "pathlib" { tail } else if tail.is_empty() { import.expression.as_str() } else { "" };
                                            let canonical = match class {
                                                "Path" => "pathlib.Path", "PosixPath" => "pathlib.PosixPath", "WindowsPath" => "pathlib.WindowsPath", "PurePath" => "pathlib.PurePath", "PurePosixPath" => "pathlib.PurePosixPath", "PureWindowsPath" => "pathlib.PureWindowsPath", _ => "",
                                            };
                                            if python_constructor_member(canonical, member) { computed = receivers::Member::External(origin, *import_line); }
                                        }
                                    }
                                }
                            }
                            None => {}
                        }
                    }
                    let (status, evidence) = match computed {
                        receivers::Member::Local(method) if matches!(method.kind.as_str(), "method" | "function") => {
                            graph.edges.push(Edge { src: r.source.clone(), dst: method.id.clone(), kind: r.kind.clone(), path: path.clone(), line: r.line, evidence: r.expression.clone(), confidence: "inferred".into() });
                            ("resolved", format!("computed receiver has statically proven member {}", method.qualname))
                        }
                        receivers::Member::External(module, _) => ("external", format!("call through external import {module}; callable target unverified")),
                        _ if literal_builtin => ("resolved", format!("standard library or built-in callee: {}", r.expression)),
                        _ => ("unresolved", "computed receiver or dynamic callee; callable identity is unknown".into()),
                    };
                    graph.coverage.push(Coverage {
                        path: path.clone(), line: r.line, expression: r.expression.clone(),
                        status: status.into(), evidence,
                    });
                    continue;
                }
                let expression = if r.expression.contains("::") { Cow::Owned(r.expression.replace("::", ".")) } else { Cow::Borrowed(r.expression.as_str()) };
                let (first, tail) = expression.split_once('.').unwrap_or((&expression, ""));
                let owner = by_id.get(r.source.as_str()).copied();
                let mut scope = owner.map(|n| n.qualname.as_str()).unwrap_or("");
                let definition_scope = matches!(r.kind.as_str(), "decorates" | "inherits" | "implements" | "bases");
                if definition_scope { scope = scope.rsplit_once('.').map_or("", |(p, _)| p); }
                let mut shadowed = false;
                let mut typed_receiver = None;
                let imported = alias_for(first, owner, definition_scope);
                candidates.clear();
                while !scope.is_empty() {
                    if let Some(scope_nodes) = by_qual.get(scope) {
                        if !profile.is_some_and(|p|p.class_scope()) && !definition_scope && owner.is_some_and(|n| matches!(n.kind.as_str(), "function" | "method"))
                            && scope_nodes.iter().any(|n| n.path==*path && matches!(n.kind.as_str(), "class" | "impl")) {
                            scope = scope.rsplit_once('.').map_or("", |(p, _)| p);
                            continue;
                        }
                        if profile.is_none_or(|p|p.value_binding_applies(r)) {
                            let mut binding_owners = scope_nodes.iter().filter(|node| node.path == *path && node_bindings.get(node.id.as_str()).is_some_and(|bindings| bindings.contains(first)));
                            if let Some(binding_owner) = binding_owners.next() {
                                shadowed = true;
                                if !tail.is_empty() && binding_owners.next().is_none() {
                                    typed_receiver = typed_parameters.get(binding_owner.id.as_str()).and_then(|parameters| parameters.get(first));
                                }
                                break;
                            }
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
                let receiver_class = if profile.is_some_and(|profile| profile.id() == "python") && !tail.is_empty() {
                    if let Some(ReceiverType::Local(class)) = typed_receiver { Some(*class) }
                    else if known_receiver { owner.and_then(|owner| owner.qualname.rsplit_once('.')).and_then(|(scope, _)| by_qual.get(scope)).and_then(|nodes| {
                        let mut classes = nodes.iter().copied().filter(|node| node.path == *path && node.kind == "class");
                        let class = classes.next()?; classes.next().is_none().then_some(class)
                    }) }
                    else if !shadowed {
                        let classes = imported.map(Vec::as_slice).unwrap_or_else(|| lookup(qualified(&mut lookup_key, &file_module, first)));
                        match classes { [class] if class.kind == "class" => Some(*class), _ => None }
                    } else { None }
                } else { None };
                let receiver_member = receiver_class.filter(|_| matches!(r.kind.as_str(), "calls" | "handles")).map(|class| python_receivers.lookup(class, tail, false));
                if shadowed
                    && !known_receiver
                    && !typed_receiver.is_some_and(|receiver| !matches!(receiver, ReceiverType::Unknown))
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
                if let Some(ReceiverType::Local(receiver)) = typed_receiver {
                    candidates.extend(lookup(qualified(&mut lookup_key, &receiver.qualname, tail)).iter().copied().filter(|candidate| candidate.path == receiver.path));
                }
                if let Some(receivers::Member::Local(method)) = receiver_member {
                    candidates.clear(); candidates.push(method);
                }
                if matches!(receiver_member, Some(receivers::Member::Unknown | receivers::Member::External(..))) { candidates.clear(); }
                if candidates.is_empty() && typed_receiver.is_none() && receiver_member.is_none() {
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
                                            language_package_exports,
                                            &mut lookup_key,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
                if candidates.is_empty() && imported.is_none() && typed_receiver.is_none() && receiver_member.is_none() {
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
                if imported.is_none() && typed_receiver.is_none() && !matches!(receiver_member, Some(receivers::Member::Local(_))) {
                    candidates.retain(|n| {
                        n.path == *path
                            || (profile.is_some_and(|p| p.sibling_accessible(path, &n.path))
                                && by_module
                                    .get(n.path.as_str())
                                    .into_iter()
                                    .flatten()
                                    .any(|m| m.kind == "module" && m.qualname == file_module))
                            || (r.kind == "references" && n.qualname == expression.as_ref())
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
                if candidates.len() > 1 {
                    let non_stubs: Vec<&Node> = candidates
                        .iter()
                        .copied()
                        .filter(|n| {
                            let is_stub = n.details.get("is_stub").and_then(|v| v.as_bool()).unwrap_or(false);
                            let is_overload = n.details.get("is_overload").and_then(|v| v.as_bool()).unwrap_or(false);
                            !is_stub && !is_overload
                        })
                        .collect();
                    if !non_stubs.is_empty() && non_stubs.len() < candidates.len() {
                        candidates = non_stubs;
                    }
                }
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
                let status = if matches!(typed_receiver, Some(ReceiverType::Ambiguous)) {
                    "ambiguous"
                } else if (candidates.len() == 1 || is_builtin) && source.is_some() && !source_heuristic {
                    "resolved"
                } else if candidates.is_empty() || source.is_none() || source_heuristic {
                    "unresolved"
                } else {
                    "ambiguous"
                };
                let external_origin = if let Some(receivers::Member::External(module, import_line)) = receiver_member {
                    Some((module, import_line))
                } else if let Some(ReceiverType::External(module, import_line)) = typed_receiver {
                    (*import_line <= r.line).then_some((*module, *import_line))
                } else if status == "unresolved" && !shadowed
                    && candidates.is_empty() && imported.is_some_and(|targets| targets.is_empty()) {
                    external_for(first, owner, false).filter(|(_, import_line)| *import_line <= r.line).map(|(module, line)| (module.as_str(), *line))
                } else {
                    None
                };
                let status = if external_origin.is_some() { "external" } else { status };
                let stub_target = match candidates.as_slice() {
                    [candidate] if language_linker.is_declaration_only(&candidate.path) => {
                        Some(candidate.path.as_str())
                    }
                    _ => None,
                };
                let evidence = if let Some(stub_path) = stub_target {
                    format!("type-stub declaration at {stub_path}; runtime implementation not statically indexed")
                } else if let Some((module, _)) = external_origin {
                    if r.kind == "calls" {
                        format!("call through external import {module}; callable target unverified")
                    } else {
                        format!("reference through external import {module}; target unverified")
                    }
                } else if let Some(ReceiverType::Local(receiver)) = typed_receiver {
                    format!("parameter receiver type {}; {} statically inferred candidates", receiver.qualname, candidates.len())
                } else if matches!(typed_receiver, Some(ReceiverType::Ambiguous)) {
                    "parameter receiver type has ambiguous declarations".into()
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
                        confidence: if source_heuristic { "heuristic" } else if typed_receiver.is_some() || receiver_member.is_some() { "inferred" } else { "exact" }.into(),
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

fn python_string_method(member: &str) -> bool {
    matches!(
        member,
        "capitalize"
            | "casefold"
            | "center"
            | "count"
            | "encode"
            | "endswith"
            | "expandtabs"
            | "find"
            | "format"
            | "format_map"
            | "index"
            | "isalnum"
            | "isalpha"
            | "isascii"
            | "isdecimal"
            | "isdigit"
            | "isidentifier"
            | "islower"
            | "isnumeric"
            | "isprintable"
            | "isspace"
            | "istitle"
            | "isupper"
            | "join"
            | "ljust"
            | "lower"
            | "lstrip"
            | "maketrans"
            | "partition"
            | "removeprefix"
            | "removesuffix"
            | "replace"
            | "rfind"
            | "rindex"
            | "rjust"
            | "rpartition"
            | "rsplit"
            | "rstrip"
            | "split"
            | "splitlines"
            | "startswith"
            | "strip"
            | "swapcase"
            | "title"
            | "translate"
            | "upper"
            | "zfill"
    )
}

fn python_constructor_member(callee: &str, member: &str) -> bool {
    match callee {
        "pathlib.PurePath" | "pathlib.PurePosixPath" | "pathlib.PureWindowsPath" => matches!(
            member,
            "as_posix"
                | "as_uri"
                | "is_absolute"
                | "is_relative_to"
                | "is_reserved"
                | "joinpath"
                | "match"
                | "relative_to"
                | "with_name"
                | "with_stem"
                | "with_suffix"
        ),
        "pathlib.Path" | "pathlib.PosixPath" | "pathlib.WindowsPath" => {
            python_constructor_member("pathlib.PurePath", member)
                || matches!(
                    member,
                    "absolute"
                        | "chmod"
                        | "exists"
                        | "expanduser"
                        | "glob"
                        | "hardlink_to"
                        | "is_block_device"
                        | "is_char_device"
                        | "is_dir"
                        | "is_fifo"
                        | "is_file"
                        | "is_mount"
                        | "is_socket"
                        | "is_symlink"
                        | "iterdir"
                        | "lstat"
                        | "mkdir"
                        | "open"
                        | "read_bytes"
                        | "read_text"
                        | "readlink"
                        | "rename"
                        | "replace"
                        | "resolve"
                        | "rglob"
                        | "rmdir"
                        | "samefile"
                        | "stat"
                        | "symlink_to"
                        | "touch"
                        | "unlink"
                        | "write_bytes"
                        | "write_text"
                )
        }
        _ => false,
    }
}

fn qualified<'a>(buffer: &'a mut String, namespace: &str, name: &str) -> &'a str {
    buffer.clear();
    buffer.push_str(namespace);
    buffer.push('.');
    buffer.push_str(name);
    buffer
}

#[derive(Clone, Copy)]
enum ReceiverType<'a> {
    Local(&'a Node),
    External(&'a str, usize),
    Ambiguous,
    Unknown,
}

fn receiver_type_name(annotation: &str) -> Option<Cow<'_, str>> {
    let mut name = annotation.trim().trim_matches(['\'', '"']);
    while let Some(rest) = name.strip_prefix('&') {
        name = rest.trim_start();
        if name.starts_with('\'') {
            name = name.split_once(char::is_whitespace)?.1.trim_start();
        }
        name = name.strip_prefix("mut ").unwrap_or(name).trim_start();
    }
    if name.is_empty()
        || !name.chars().all(|character| {
            character.is_alphanumeric() || matches!(character, '_' | '.' | ':' | '$')
        })
    {
        return None;
    }
    Some(if name.contains("::") {
        Cow::Owned(name.replace("::", "."))
    } else {
        Cow::Borrowed(name)
    })
}
