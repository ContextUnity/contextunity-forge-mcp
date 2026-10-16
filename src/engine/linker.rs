use crate::core::models::*;
use crate::core::models::{
    compact_graph::{Coverage, Edge, Graph},
    Graph as PublicGraph,
};
#[cfg(feature = "lang-html")]
use crate::engine::languages::ImportPath;
use crate::engine::languages::{self, module_name, LanguageFamily};
use crate::engine::linker::traits::{
    ImportContext, ImportResolution, LanguageLinker, GENERIC_LINKER,
};
use hashbrown::{HashMap, HashSet};
use rayon::prelude::*;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::Path;
pub(crate) mod contracts;
mod framework_relations;
#[cfg(feature = "lang-html")]
mod html_scripts;
#[cfg(feature = "lang-html")]
mod html_templates;
mod indexing;
mod resolution;
mod routes;
mod semantic_context;
pub mod traits;
/// Implements value flow support.
pub mod value_flow;
use languages::python_receivers as receivers;
use languages::typescript_bindings as commonjs;
#[cfg(feature = "lang-typescript")]
fn prototype_visible_at(node: &Node, path: &str, line: usize, column: usize) -> bool {
    if node.language != "javascript" || node.details["prototype"] != true || node.path != path {
        return true;
    }
    let assigned_line = node.details["prototype_assignment_line"]
        .as_u64()
        .unwrap_or(node.line as u64) as usize;
    let assigned_column = node.details["prototype_assignment_column"]
        .as_u64()
        .or_else(|| node.details["column"].as_u64())
        .unwrap_or(0) as usize;
    (assigned_line, assigned_column) <= (line, column)
}

#[cfg(not(feature = "lang-typescript"))]
fn prototype_visible_at(_node: &Node, _path: &str, _line: usize, _column: usize) -> bool {
    true
}

#[cfg(feature = "lang-typescript")]
fn typed_dom_receiver(name: &str) -> Option<&'static str> {
    languages::typescript::typed_dom_receiver(name)
}

#[cfg(not(feature = "lang-typescript"))]
fn typed_dom_receiver(_name: &str) -> Option<&'static str> {
    None
}

#[cfg(feature = "lang-typescript")]
fn typed_dom_property(receiver: &str, property: &str) -> Option<&'static str> {
    languages::typescript::typed_dom_property(receiver, property)
}

#[cfg(not(feature = "lang-typescript"))]
fn typed_dom_property(_receiver: &str, _property: &str) -> Option<&'static str> {
    None
}

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
    let dependency_registry = languages::dependency_registry::DependencyRegistry::collect(root);
    link_with_registry(all, owners, root, &dependency_registry)
}

/// Performs link with registry.
pub fn link_with_registry(
    all: &BTreeMap<String, Facts>,
    owners: Option<&std::collections::BTreeSet<String>>,
    root: Option<&Path>,
    dependency_registry: &languages::dependency_registry::DependencyRegistry,
) -> PublicGraph {
    link_compact_with_registry(all, owners, root, dependency_registry).into()
}

pub(crate) fn link_compact_with_registry(
    all: &BTreeMap<String, Facts>,
    owners: Option<&std::collections::BTreeSet<String>>,
    root: Option<&Path>,
    dependency_registry: &languages::dependency_registry::DependencyRegistry,
) -> Graph {
    link_compact_impl(all, Some(all), owners, root, dependency_registry, None)
}

pub(crate) fn link_compact_with_typed_registry(
    all: &BTreeMap<String, crate::core::typed_facts::TypedFacts>,
    owners: Option<&std::collections::BTreeSet<String>>,
    root: Option<&Path>,
    dependency_registry: &languages::dependency_registry::DependencyRegistry,
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
    dependency_registry: &languages::dependency_registry::DependencyRegistry,
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
    let namespace_indexes =
        indexing::NamespaceIndexes::build(all, &nodes, root, dependency_registry);
    let borrowed_all = public_all.is_none().then(|| {
        all.iter()
            .map(|(path, facts)| (path.clone(), facts.as_ref()))
            .collect::<BTreeMap<_, _>>()
    });
    let package_exports = indexing::package_exports(
        &namespace_indexes.profiles,
        &namespace_indexes.modules_by_namespace,
        &by_module,
        public_all,
        borrowed_all.as_ref(),
        root,
    );
    mark("namespace indexes and exports");
    let inheritance_indexes = indexing::InheritanceIndexes::build(
        all,
        &by_id,
        &by_qual,
        &node_bindings,
        &node_rebindings,
        &by_module,
        &namespace_indexes,
        &package_exports,
        dependency_registry,
    );
    let profiles = &namespace_indexes.profiles;
    let nuxt_config_scopes = &namespace_indexes.nuxt_config_scopes;
    let normalized_imports = &namespace_indexes.normalized_imports;
    let modules_by_namespace = &namespace_indexes.modules_by_namespace;
    let python_receivers = inheritance_indexes.python_receivers;
    #[cfg(feature = "lang-rust")]
    let rust_members = inheritance_indexes.rust_members;
    mark("inheritance indexes");
    let semantic_context = semantic_context::Context::build(
        all,
        modules_by_namespace,
        &by_module,
        &by_qual,
        &package_exports,
        normalized_imports,
        dependency_registry,
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
    // Preserve inventory order for path/suffix selection after value flow takes
    // the full inventory and sorts it by lexical ownership.
    let module_nodes: Vec<&Node> = nodes
        .iter()
        .copied()
        .filter(|node| node.kind == "module")
        .collect();
    let value_flow =
        value_flow::ValueFlowIndex::build_parallel(all, nodes.clone(), &by_id, &semantic_context);
    mark("value flow");
    let mut parts = resolution::resolve(resolution::ResolutionInput {
        all,
        public_all,
        owners,
        root,
        dependency_registry,
        flows,
        sort_per_file,
        by_name: &by_name,
        by_qual: &by_qual,
        by_suffix: &by_suffix,
        by_id: &by_id,
        by_module: &by_module,
        node_bindings: &node_bindings,
        node_rebindings: &node_rebindings,
        children_by_parent: &children_by_parent,
        profiles,
        nuxt_config_scopes,
        normalized_imports,
        modules_by_namespace,
        package_exports: &package_exports,
        semantic_context: &semantic_context,
        module_nodes: &module_nodes,
        value_flow: &value_flow,
    });
    mark("reference linking");
    parts.extend(framework_relations::compose(
        all,
        owners,
        flows,
        dependency_registry,
        profiles,
        &semantic_context,
        &nodes,
    ));
    mark("framework relation composition");
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
    (&a.path, a.line, &a.expression, &a.status, &a.evidence).cmp(&(
        &b.path,
        b.line,
        &b.expression,
        &b.status,
        &b.evidence,
    ))
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
    builtin_origin: Option<&str>,
) {
    let (status, evidence): (&str, Cow<'static, str>) = match &result {
        semantic_context::Member::Local(node) => ("resolved", format!("statically inferred value receiver: {}", node.qualname).into()),
        semantic_context::Member::External(module, line, Some(provider)) => {
            let member = reference.expression.rsplit('.').next().unwrap_or_default();
            (
                "external",
                format!("finite Python framework member from {provider} via external import {module} at line {line}; resolved member {provider}.{member}; receiver origin statically inferred").into(),
            )
        }
        semantic_context::Member::External(module, line, None) => ("external", format!("call through external import {module} at line {line}; receiver origin statically inferred").into()),
        semantic_context::Member::Builtin => match builtin_origin {
            Some(origin) => ("external", format!("{origin} built-in: {}", reference.expression).into()),
            None => ("resolved", format!("standard library or built-in callee: {}", reference.expression).into()),
        },
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

fn receiver_head(annotation: &str) -> &str {
    let mut name = annotation.trim().trim_matches(['\'', '"']);
    while let Some(rest) = name.strip_prefix('&') {
        name = rest.trim_start();
        if name.starts_with('\'') {
            if let Some((_, rest)) = name.split_once(char::is_whitespace) {
                name = rest.trim_start();
            }
        }
        name = name.strip_prefix("mut ").unwrap_or(name).trim_start();
    }
    let head = name
        .split_once("::")
        .or_else(|| name.split_once('.'))
        .map(|(h, _)| h)
        .unwrap_or(name);
    let head = head.split_once('<').map(|(h, _)| h).unwrap_or(head);
    head.split_once('[').map(|(h, _)| h).unwrap_or(head).trim()
}

fn rust_qualified_generic_base(annotation: &str) -> Option<&str> {
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

pub(crate) fn python_has_proven_type_checking(
    path: &str,
    facts: &crate::core::models::Facts,
    modules_by_namespace: &crate::engine::linker::traits::ModulesByNamespace<'_>,
    dependency_registry: &crate::engine::languages::dependency_registry::DependencyRegistry,
    family: crate::engine::languages::LanguageFamily,
) -> bool {
    let module_node = facts
        .nodes
        .iter()
        .find(|n| n.kind == "module" && n.path == path);
    if let Some(module) = module_node {
        if module.details["rebindings"]
            .as_array()
            .is_some_and(|names| names.iter().any(|n| n.as_str() == Some("TYPE_CHECKING")))
        {
            return false;
        }
    }
    let (p_ws, _) = crate::engine::languages::workspace_path(path);
    let is_shadowed = |module_name: &str| -> bool {
        modules_by_namespace
            .get(&family)
            .and_then(|namespaces| namespaces.get(module_name))
            .is_some_and(|mods| {
                mods.iter().any(|m| {
                    let (m_ws, _) = crate::engine::languages::workspace_path(&m.path);
                    m_ws == p_ws
                })
            })
    };
    for r in &facts.references {
        if r.kind == "imports" {
            let proves = |module: &str| -> bool {
                (module == "typing" && !is_shadowed("typing"))
                    || (module == "typing_extensions"
                        && !is_shadowed("typing_extensions")
                        && (dependency_registry.declares_for_path(
                            family,
                            path,
                            "typing-extensions",
                        ) || dependency_registry.declares_for_path(
                            family,
                            path,
                            "typing_extensions",
                        )))
            };
            // `from typing import TYPE_CHECKING as TC` keeps the symbol in expression.
            if r.expression == "TYPE_CHECKING" {
                if let Some(module) = r.module.as_deref() {
                    if proves(module) {
                        return true;
                    }
                }
            } else if let Some(module) = r.module.as_deref() {
                // `import typing` and `import typing as aliases` store the module path
                // in both fields. A local name spelled `typing` is not proof.
                if r.expression == module && proves(module) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod route_extraction_contract {
    #[test]
    fn route_normalization_preserves_dynamic_segment_rules() {
        assert_eq!(
            super::routes::normalize_route_segments("/Users/<id>/posts/:slug/{page}"),
            vec!["users", "{param}", "posts", "{param}", "{param}"]
        );
    }
}
