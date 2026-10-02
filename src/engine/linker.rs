use crate::core::models::*;
use crate::core::models::{
    compact_graph::{Coverage, Edge, Graph},
    Graph as PublicGraph,
};
use crate::engine::languages::{self, module_name, ImportPath, LanguageFamily, LanguageProfile};
use crate::engine::linker::traits::{
    ImportContext, ImportResolution, LanguageLinker, PackageExports, GENERIC_LINKER,
};
use hashbrown::{HashMap, HashSet};
use rayon::prelude::*;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::Path;
pub(crate) mod contracts;
mod semantic_context;
pub mod traits;
/// Implements value flow support.
pub mod value_flow;
use languages::python_receivers as receivers;
use languages::typescript_bindings as commonjs;
/// Performs needs reference identity.
pub fn needs_reference_identity(path: &str, facts: &Facts) -> bool {
    let language = facts
        .nodes
        .iter()
        .find(|node| node.kind == "module")
        .map_or("", |node| node.language.as_str());
    languages::linker_for(language).needs_reference_identity(path, facts)
}
/// Performs reference identity changed.
pub fn reference_identity_changed(path: &str, old: &Facts, new: &Facts) -> bool {
    let language = new
        .nodes
        .iter()
        .find(|node| node.kind == "module")
        .map_or("", |node| node.language.as_str());
    languages::linker_for(language).reference_identity_changed(path, old, new)
}
/// Performs required full facts.
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

pub(crate) fn required_full_typed_facts(
    facts: &BTreeMap<String, crate::core::typed_facts::TypedFacts>,
    affected: &std::collections::BTreeSet<String>,
    catalog: &[(&str, &str)],
) -> std::collections::BTreeSet<String> {
    let borrowed: BTreeMap<_, _> = facts
        .iter()
        .map(|(path, facts)| (path.clone(), &facts.facts))
        .collect();
    let mut required = std::collections::BTreeSet::new();
    let mut seen = std::collections::BTreeSet::new();
    for profile in languages::profiles() {
        if seen.insert(profile.id()) {
            required.extend(
                languages::linker_for(profile.id())
                    .required_full_facts_borrowed(&borrowed, affected, catalog),
            );
        }
    }
    required
}
/// Performs link.
pub fn link(all: &BTreeMap<String, Facts>) -> PublicGraph {
    link_with_root(all, None, None)
}
/// Performs link owners.
pub fn link_owners(
    all: &BTreeMap<String, Facts>,
    owners: Option<&std::collections::BTreeSet<String>>,
) -> PublicGraph {
    link_with_root(all, owners, None)
}
/// Performs link with root.
pub fn link_with_root(
    all: &BTreeMap<String, Facts>,
    owners: Option<&std::collections::BTreeSet<String>>,
    root: Option<&Path>,
) -> PublicGraph {
    let dependency_registry = languages::manifests::DependencyRegistry::collect(root);
    link_with_registry(all, owners, root, &dependency_registry)
}

/// Performs link with registry.
pub fn link_with_registry(
    all: &BTreeMap<String, Facts>,
    owners: Option<&std::collections::BTreeSet<String>>,
    root: Option<&Path>,
    dependency_registry: &languages::manifests::DependencyRegistry,
) -> PublicGraph {
    link_compact_with_registry(all, owners, root, dependency_registry).into()
}

pub(crate) fn link_compact_with_registry(
    all: &BTreeMap<String, Facts>,
    owners: Option<&std::collections::BTreeSet<String>>,
    root: Option<&Path>,
    dependency_registry: &languages::manifests::DependencyRegistry,
) -> Graph {
    link_compact_impl(all, Some(all), owners, root, dependency_registry, None)
}

pub(crate) fn link_compact_with_typed_registry(
    all: &BTreeMap<String, crate::core::typed_facts::TypedFacts>,
    owners: Option<&std::collections::BTreeSet<String>>,
    root: Option<&Path>,
    dependency_registry: &languages::manifests::DependencyRegistry,
) -> Graph {
    let flows: HashMap<_, _> = all
        .values()
        .flat_map(|file| {
            file.facts.nodes.iter().filter_map(|node| {
                file.flows
                    .state(node)
                    .map(|state| (node.id.as_str(), state))
            })
        })
        .collect();
    link_compact_impl(all, None, owners, root, dependency_registry, Some(&flows))
}

fn link_compact_impl<'a, F: AsRef<Facts> + Sync>(
    all: &'a BTreeMap<String, F>,
    public_all: Option<&'a BTreeMap<String, Facts>>,
    owners: Option<&std::collections::BTreeSet<String>>,
    root: Option<&Path>,
    dependency_registry: &languages::manifests::DependencyRegistry,
    flows: Option<&'a HashMap<&'a str, &'a crate::core::typed_facts::FlowState>>,
) -> Graph {
    let profiling = std::env::var_os("FORGE_PROFILE_LINKER").is_some();
    let mut checkpoint = std::time::Instant::now();
    let mut mark = |phase: &str| {
        if profiling {
            eprintln!(
                "forge linker {phase}: {:.3}ms",
                checkpoint.elapsed().as_secs_f64() * 1000.
            );
            checkpoint = std::time::Instant::now();
        }
    };
    let nodes: Vec<&Node> = all.values().flat_map(|f| f.as_ref().nodes.iter()).collect();
    let sort_per_file = nodes.len() >= 8192 && all.len() >= 32;
    let mut by_name: HashMap<&str, Vec<&Node>> = HashMap::with_capacity(nodes.len());
    let mut by_qual: HashMap<&str, Vec<&Node>> = HashMap::with_capacity(nodes.len());
    let mut by_suffix: HashMap<&str, Vec<&Node>> = HashMap::new();
    let mut by_id: HashMap<&str, &Node> = HashMap::with_capacity(nodes.len());
    let mut by_module: HashMap<&str, Vec<&Node>> = HashMap::with_capacity(all.len());
    let needs_discovery = all.iter().any(|(path, facts)| {
        let facts = facts.as_ref();
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
        let f = f.as_ref();
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
            let f = f.as_ref();
            f.nodes
                .iter()
                .find(|n| n.kind == "module")
                .and_then(|n| languages::by_id(&n.language))
                .map(|p| (path.as_str(), p))
        })
        .collect();
    let mut normalized_imports: HashMap<(&str, &str), Option<ImportPath>> = HashMap::new();
    for (path, facts) in all.iter() {
        let facts = facts.as_ref();
        let profile = profiles.get(path.as_str()).copied();
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
    let borrowed = public_all.is_none().then(|| {
        all.iter()
            .map(|(path, facts)| (path.clone(), facts.as_ref()))
            .collect::<BTreeMap<_, _>>()
    });
    let mut package_exports: HashMap<&str, PackageExports<'_>> = language_ids
        .into_iter()
        .map(|id| {
            (
                id,
                if let Some(public_all) = public_all {
                    languages::linker_for(id).package_exports(
                        public_all,
                        &modules_by_namespace,
                        &by_module,
                        root,
                    )
                } else {
                    languages::linker_for(id).package_exports_borrowed(
                        borrowed.as_ref().expect("typed facts view exists"),
                        &modules_by_namespace,
                        &by_module,
                        root,
                    )
                },
            )
        })
        .collect();
    package_exports.insert("", HashMap::new());
    mark("namespace indexes and exports");
    let python_receivers = receivers::PythonReceivers::build(all, |class, reference| {
        let facts = all[&class.path].as_ref();
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
    #[cfg(feature = "lang-rust")]
    let rust_members = languages::rust::linker::RustMembers::build(all);
    mark("inheritance indexes");
    let semantic_context = semantic_context::Context::build(
        all,
        &modules_by_namespace,
        &by_module,
        &by_qual,
        &package_exports,
        &normalized_imports,
    )
    .with_python(&python_receivers)
    .with_flows(flows);
    #[cfg(feature = "lang-rust")]
    let semantic_context = semantic_context.with_rust(&rust_members);
    #[cfg(feature = "lang-typescript")]
    let typescript_members =
        languages::typescript::linker::TypeScriptMembers::build(all, &semantic_context);
    #[cfg(feature = "lang-typescript")]
    let semantic_context = semantic_context.with_typescript(&typescript_members);
    mark("semantic admission and members");
    let value_flow = value_flow::ValueFlowIndex::build_parallel(all, &semantic_context);
    mark("value flow");
    let parts: Vec<Graph> = all
        .par_iter()
        .filter(|(path, _)| owners.is_none_or(|o| o.contains(*path)))
        .map(|(path, facts)| {
            let facts = facts.as_ref();
            let mut graph = Graph {
                edges: facts.edges.iter().map(Edge::from).collect(),
                coverage: Vec::with_capacity(facts.references.len()),
            };
            let profile = profiles.get(path.as_str()).copied();
            #[cfg(feature = "lang-vue")]
            let vue_module = profile.filter(|profile| profile.id() == "vue")
                .and_then(|_| facts.nodes.iter().find(|node| node.kind == "module"));
            let builtin_in_scope = |expression: &str, reference: &Reference| {
                #[cfg(feature = "lang-vue")]
                if vue_module.is_some() && languages::vue::compiler_macro(expression)
                {
                    return vue_module.is_some_and(|module| languages::vue::admits_macro(module, crate::core::semantic::SourcePosition { line: reference.line, column: reference.column }));
                }
                let _ = reference;
                profile.is_some_and(|profile| profile.builtin(expression))
            };
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
                let scope = by_id.get(r.source.as_str()).map(|n| n.qualname.clone()).unwrap_or_else(|| file_module.clone());
                let mut imported_targets = Vec::new();
                let is_wildcard = r.alias.as_deref() == Some("*")
                    || (r.expression == "*" && r.alias.is_none());
                if is_wildcard {
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
                    let local_declaration = by_qual.get(qualified(&mut lookup_key, &scope, alias)).is_some_and(|nodes| nodes.iter().any(|n| n.path == *path));
                    let mut targets = if local_declaration { Vec::new() } else { candidates.clone() };
                    let symbol_name = symbol.rsplit('.').next().unwrap_or(&symbol);
                    if targets.is_empty() && !local_declaration {
                        let mut inherited_ext = None;
                        for target_module in &modules {
                            let target_scope = target_module.qualname.as_str();
                            if let Some(target_alias) = aliases.get(target_scope).and_then(|entries| entries.get(symbol_name)) {
                                targets.extend(target_alias.iter().copied());
                            }
                            if inherited_ext.is_none() {
                                inherited_ext = external_aliases.get(target_scope).and_then(|entries| entries.get(symbol_name)).and_then(Option::as_ref).cloned();
                            }
                        }
                        if let Some(target_ext) = inherited_ext {
                            external_aliases.entry(scope.clone()).or_default().insert(alias.clone(), Some(target_ext));
                        }
                    }
                    let alias_taken = aliases.get(&scope).is_some_and(|entries| entries.contains_key(alias));
                    if alias_taken {
                        if let Some(provenance) = external_aliases.get_mut(&scope).and_then(|entries| entries.get_mut(alias)) {
                            *provenance = None;
                        }
                    } else if external.is_some() && !local_declaration {
                        external_aliases.entry(scope.clone()).or_default().insert(alias.clone(), Some((symbol.clone(), r.line)));
                    }
                    aliases.entry(scope.clone()).or_default().entry(alias.clone()).or_default().extend(targets.clone());
                    imported_targets = targets;
                }
                let resolved = modules.len() == 1 && (r.alias.is_none() || is_wildcard || candidates.len() == 1 || imported_targets.len() == 1);
                let is_external_alias = r.alias.as_ref().is_some_and(|a| external_aliases.get(&scope).and_then(|e| e.get(a)).is_some_and(Option::is_some));
                let status = if resolved { "resolved" } else if modules.len() > 1 || candidates.len() > 1 || imported_targets.len() > 1 { "ambiguous" } else if external.is_some() || is_external_alias { "external" } else { "unresolved" };
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
                let import_owner = by_id.get(r.source.as_str()).copied()
                    .filter(|owner| owner.language == "html" || (owner.kind == "template_scope" && owner.details["embedded_language"] == "javascript"))
                    .and_then(|owner| owner.details["html_import_owner"].as_str())
                    .filter(|id| by_id.get(*id).is_some_and(|node| node.kind == "module" && node.language == "html" && node.path == *path))
                    .unwrap_or(r.source.as_str());
                if modules.len() == 1 {
                    graph.edges.push(Edge {
                        src: import_owner.into(),
                        dst: modules[0].id.clone(),
                        kind: "imports".into(),
                        path: path.clone(),
                        line: r.line,
                        evidence: r.expression.clone(),
                        confidence: "exact".into(),
                    });
                    if child_module {
                        graph.edges.push(Edge {
                            src: import_owner.into(),
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
                                src: import_owner.into(),
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
                            src: import_owner.into(),
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
            #[cfg(feature = "lang-html")]
            let django_templates = profile.is_some_and(|profile| profile.id() == "html")
                && dependency_registry.declares_for_path(languages::LanguageFamily("python"), path, "django");
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
                        kind: r.kind.as_str().into(),
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
                    #[cfg(feature = "lang-html")]
                    let framework = languages::html::framework_template_origin(target_path).filter(|_| django_templates);
                    #[cfg(not(feature = "lang-html"))]
                    let framework: Option<&str> = None;
                    graph.coverage.push(Coverage {
                        path: path.clone(),
                        line: r.line,
                        expression: r.expression.clone(),
                        status: if framework.is_some() { "external" } else { "unresolved" }.into(),
                        evidence: match framework {
                            Some(package) => format!("framework built-in template from {package}: {}; target unindexed", r.expression),
                            None => format!("template '{}' not found", r.expression),
                        },
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
                                    .filter(|(_, import_line)| *import_line <= node.line && !(matches!(head, "typing" | "typing_extensions") && (member == "Any" || dynamic_type_for(head, node))))
                                    .map(|(module, line)| ReceiverType::External(module.as_str(), *line))
                                    .unwrap_or(ReceiverType::Unknown)
                            } else if matches!(head, "std" | "core" | "alloc") {
                                match head {
                                    "std" => ReceiverType::External("std", 0),
                                    "core" => ReceiverType::External("core", 0),
                                    "alloc" => ReceiverType::External("alloc", 0),
                                    _ => ReceiverType::Unknown,
                                }
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
                if matches!(r.kind.as_str(), "calls" | "handles") {
                    if let Some(owner) = by_id.get(r.source.as_str()).copied() {
                        if let Some(ReceiverHint::CallResult { member, .. } | ReceiverHint::ConstructorResult { member, .. }) = &r.receiver_hint {
                            if let Some(receiver) = value_flow.computed_receiver(r).filter(|receiver| !matches!(receiver, value_flow::TypeTarget::Unknown)) {
                                let result = semantic_context.lookup_member(receiver, member, owner);
                                emit_inferred(&mut graph, path, r, result);
                                continue;
                            }
                        }
                        if let Some(ReceiverHint::StringLiteral { member }) = &r.receiver_hint {
                            if profile.is_some_and(|profile| profile.builtin_member("string", member)) {
                                emit_inferred(&mut graph, path, r, semantic_context::Member::Builtin);
                                continue;
                            }
                        }
                    }
                }
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
                                            if python_constructor_member(canonical, member) { computed = receivers::Member::External(origin.as_str(), *import_line); }
                                        }
                                    }
                                }
                            }
                            Some(ReceiverHint::ConstructorResult { .. }) => {}
                            None => {}
                        }
                    }
                    let (status, evidence) = match computed {
                        receivers::Member::Local(method) if matches!(method.kind.as_str(), "method" | "function") => {
                            graph.edges.push(Edge { src: r.source.clone(), dst: method.id.clone(), kind: r.kind.as_str().into(), path: path.clone(), line: r.line, evidence: r.expression.clone(), confidence: "inferred".into() });
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
                let mut lexical_head_declared = false;
                let builtin_member_candidate = !tail.is_empty() && builtin_in_scope(&expression, r);
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
                            let mut binding_owners = scope_nodes.iter().filter(|node| node.path == *path && node_bindings.get(node.id.as_str()).is_some_and(|bindings| bindings.contains(first))
                                && !owner.is_some_and(|owner| semantic_context.admits_commonjs_binding(node, first, owner, crate::core::semantic::SourcePosition { line: r.line, column: r.column })));
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
                        lexical_head_declared |= !candidates.is_empty();
                        if !candidates.is_empty() {
                            break;
                        }
                    } else if builtin_member_candidate {
                        lexical_head_declared |= lookup(qualified(&mut lookup_key, scope, first)).iter().any(|n|n.path==*path);
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
                if matches!(r.kind.as_str(), "calls" | "handles") {
                    if let Some(owner) = owner {
                        let at = crate::core::semantic::SourcePosition { line: r.line, column: r.column };
                        let implicit_receiver = if known_receiver {
                            #[cfg(feature = "lang-rust")]
                            let rust_receiver = rust_members.receiver_for(owner);
                            #[cfg(not(feature = "lang-rust"))]
                            let rust_receiver: Option<&Node> = None;
                            rust_receiver.or_else(|| owner.qualname.rsplit_once('.').and_then(|(scope, _)| by_qual.get(scope)).and_then(|nodes| {
                                let mut classes = nodes.iter().copied().filter(|node| node.path == *path && matches!(node.kind.as_str(), "class" | "interface"));
                                let class = classes.next()?; classes.next().is_none().then_some(class)
                            })).map(value_flow::TypeTarget::Local)
                        } else { None };
                        let inferred = implicit_receiver.as_ref().or_else(|| value_flow.lookup(owner, first, at));
                        if let Some(mut receiver) = inferred.filter(|receiver| !matches!(receiver, value_flow::TypeTarget::Unknown)) {
                            if tail.is_empty() {
                                if let value_flow::TypeTarget::Callable(callable) = receiver {
                                    emit_inferred(&mut graph, path, r, semantic_context::Member::Local(callable));
                                    continue;
                                }
                            } else {
                                let (fields, member) = tail.rsplit_once('.').unwrap_or(("", tail));
                                let mut missing_field = false;
                                for field in fields.split('.').filter(|field| !field.is_empty()) {
                                    if let value_flow::TypeTarget::Local(class) = receiver {
                                        if let Some(field_type) = value_flow.field_type(class, field) { receiver = field_type; continue; }
                                    }
                                    missing_field = true; break;
                                }
                                if !missing_field {
                                    let associated = implicit_receiver.is_some() && fields.is_empty()
                                        && receiver_language_requires_static(owner) && owner.details["is_static"] == true;
                                    let result = semantic_context.lookup_member_mode(receiver, member, owner, associated);
                                    emit_inferred(&mut graph, path, r, result);
                                    continue;
                                }
                            }
                        }
                    }
                }
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
                            if let (Some(owner), [receiver]) = (owner, imported.as_slice()) {
                                if matches!(receiver.language.as_str(), "rust" | "typescript" | "javascript" | "vue")
                                    && matches!(receiver.kind.as_str(), "class" | "struct" | "enum" | "interface") {
                                    let receiver = value_flow::TypeTarget::Local(receiver);
                                    let member = semantic_context.lookup_member_mode(&receiver, tail, owner, true);
                                    if !matches!(member, semantic_context::Member::Unknown) || receiver_language_requires_static(imported[0]) {
                                        emit_inferred(&mut graph, path, r, member);
                                        continue;
                                    }
                                }
                            }
                            for n in imported {
                                if n.kind == "module" && matches!(n.language.as_str(), "typescript" | "javascript" | "vue") {
                                    candidates.extend(language_linker.resolve_imported_member(
                                        n, tail, &by_module, &by_qual, language_package_exports, &mut lookup_key,
                                    ));
                                    continue;
                                }
                                candidates.extend(lookup(qualified(&mut lookup_key, &n.qualname, tail)).iter().copied().filter(|target|target.path==n.path));
                            }
                            if candidates.is_empty() {
                                if let [module] = imported.as_slice() {
                                    if module.kind == "module" && !matches!(module.language.as_str(), "typescript" | "javascript" | "vue") {
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
                    "references" => matches!(n.kind.as_str(), "class" | "enum" | "type" | "struct" | "trait" | "interface" | "field")
                        || (n.kind == "module" && n.language == "vue" && n.details["default_export"] == true),
                    _ => matches!(
                        n.kind.as_str(),
                        "function" | "method" | "class" | "struct" | "enum" | "macro"
                    ),
                });
                if matches!(r.kind.as_str(), "calls" | "handles") && !tail.is_empty() {
                    candidates.retain(|candidate| {
                        if !receiver_language_requires_static(candidate) || candidate.kind != "method" || candidate.details["is_static"] == true { return true; }
                        let Some((class_qual, _)) = candidate.qualname.rsplit_once('.') else { return true; };
                        let class_name = class_qual.rsplit('.').next().unwrap_or(class_qual);
                        let receiver_name = expression.rsplit_once('.').map(|(receiver, _)| receiver.rsplit('.').next().unwrap_or(receiver));
                        receiver_name != Some(class_name)
                    });
                }
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
                    && !shadowed && imported.is_none()
                    && !lexical_head_declared
                    && builtin_in_scope(&expression, r);
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
                    && candidates.is_empty()
                    && (imported.is_some_and(|targets| targets.is_empty())
                        || profile.is_some_and(|p| p.id() == "rust" && (p.external_import(first).is_some() || dependency_registry.declares_for_path(p.family(), path, first)))
                        || external_for(first, owner, false).is_some()) {
                    if let Some(target) = external_for(first, owner, false).filter(|(_, import_line)| *import_line <= r.line) {
                        Some((target.0.as_str(), target.1))
                    } else if profile.is_some_and(|p| p.id() == "rust" && (p.external_import(first).is_some() || dependency_registry.declares_for_path(p.family(), path, first))) {
                        Some((first, 0))
                    } else {
                        None
                    }
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
            if sort_per_file {
                sort_graph(&mut graph);
            }
            graph
        })
        .collect();
    mark("reference linking");
    let mut result = Graph {
        edges: Vec::with_capacity(parts.iter().map(|g| g.edges.len()).sum()),
        coverage: Vec::with_capacity(parts.iter().map(|g| g.coverage.len()).sum()),
    };
    for g in parts {
        result.edges.extend(g.edges);
        result.coverage.extend(g.coverage);
    }
    if !sort_per_file
        || !result
            .edges
            .windows(2)
            .all(|rows| compare_edges(&rows[0], &rows[1]).is_le())
        || !result
            .coverage
            .windows(2)
            .all(|rows| compare_coverage(&rows[0], &rows[1]).is_le())
    {
        sort_graph(&mut result);
    }
    mark("graph merge and sort");
    result
}

fn compare_edges(a: &Edge, b: &Edge) -> std::cmp::Ordering {
    (&a.path, a.line, &a.src, &a.dst, &a.kind).cmp(&(&b.path, b.line, &b.src, &b.dst, &b.kind))
}

fn compare_coverage(a: &Coverage, b: &Coverage) -> std::cmp::Ordering {
    (&a.path, a.line, &a.expression).cmp(&(&b.path, b.line, &b.expression))
}

fn sort_graph(graph: &mut Graph) {
    graph.edges.sort_by(compare_edges);
    graph.coverage.sort_by(compare_coverage);
}

fn python_string_method(member: &str) -> bool {
    languages::by_id("python").is_some_and(|profile| profile.builtin_member("str", member))
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

fn receiver_language_requires_static(node: &Node) -> bool {
    matches!(node.language.as_str(), "typescript" | "javascript" | "vue")
}

fn emit_inferred(
    graph: &mut Graph,
    path: &str,
    reference: &Reference,
    result: semantic_context::Member<'_>,
) {
    let (status, evidence) = match &result {
        semantic_context::Member::Local(node) => ("resolved", format!("statically inferred value receiver: {}", node.qualname)),
        semantic_context::Member::External(module, line) => ("external", format!("call through external import {module} at line {line}; receiver origin statically inferred")),
        semantic_context::Member::Builtin => ("resolved", format!("standard library or built-in callee: {}", reference.expression)),
        semantic_context::Member::Ambiguous => ("ambiguous", "statically inferred receiver has multiple method candidates".into()),
        semantic_context::Member::Unknown => ("unresolved", "statically inferred receiver has no verified method candidate".into()),
    };
    graph.coverage.push(Coverage {
        path: path.into(),
        line: reference.line,
        expression: reference.expression.clone(),
        status: status.into(),
        evidence,
    });
    if let semantic_context::Member::Local(node) = result {
        graph.edges.push(Edge {
            src: reference.source.clone(),
            dst: node.id.clone(),
            kind: reference.kind.as_str().into(),
            path: path.into(),
            line: reference.line,
            evidence: reference.expression.clone(),
            confidence: "inferred".into(),
        });
    }
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
    if let Some((base, _)) = name.split_once('[') {
        name = base.trim_end();
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
