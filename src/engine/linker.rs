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
mod compact;
pub(crate) mod contracts;
use compact::GraphPart;
mod framework_relations;
mod html;
#[cfg(feature = "lang-html")]
use html::templates as html_templates;
mod indexing;
pub mod interner;
mod javascript;
mod named_bindings;
mod projections;
mod python;
mod resolution;
mod routes;
mod rust;
mod semantic_context;
pub mod traits;
mod typescript;
/// Implements value flow support.
pub mod value_flow;
mod vue;
use javascript::commonjs_bindings as commonjs;
use python::receivers;
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
    dependencies: &languages::dependency_registry::DependencyRegistry,
    named_candidates: &std::collections::BTreeSet<String>,
    route_catalog: &[Node],
) -> std::collections::BTreeSet<String> {
    // Indexed old/new named selectors include providers for presently missing joins.
    // Hydrate them at the same convergence boundary as ordinary import providers.
    let mut required = named_candidates.clone();
    let route_nodes: Vec<_> = route_catalog.iter().collect();
    let routes = routes::RouteIndex::build(&route_nodes);
    let target_contexts = routes::HttpTargetContexts::build(
        facts
            .iter()
            .filter(|(_, file)| {
                file.references.iter().any(|reference| {
                    reference.kind == "calls_endpoint"
                        && matches!(
                            reference.receiver_hint,
                            Some(crate::core::models::ReceiverHint::ConfiguredReference { .. })
                        )
                })
            })
            .map(|(path, _)| path.as_str()),
        dependencies,
    );
    for (path, file) in facts {
        let rules = dependencies.language_context_for_path(path);
        for reference in file
            .references
            .iter()
            .filter(|reference| reference.kind == "calls_endpoint")
        {
            let Some(crate::core::models::ReceiverHint::ConfiguredReference { origin }) =
                &reference.receiver_hint
            else {
                continue;
            };
            if !rules.syntax_rules.iter().any(|rule| {
                rule.pack == origin.pack
                    && rule.id == origin.rule
                    && matches!(
                        rule.operation,
                        languages::manifests::syntax::SyntaxOperation::HttpClientCall { .. }
                    )
            }) {
                continue;
            }
            let Some(context) = target_contexts.get(path, origin) else {
                continue;
            };
            let (method, url) = reference
                .expression
                .split_once(' ')
                .unwrap_or(("ANY", &reference.expression));
            required.extend(
                routes
                    .matching_routes_in_context(method, url, Some(context), path)
                    .into_iter()
                    .map(|route| route.path.clone()),
            );
        }
    }
    let mut projection_owners = affected.clone();
    for (path, file) in facts {
        for node in &file.nodes {
            let Some(flow) = file
                .flows
                .get(node)
                .filter(|flow| !flow.fact_limit_exceeded)
            else {
                continue;
            };
            if flow.symbol_attachments().next().is_some()
                || flow.factory_results().next().is_some()
                || flow.named_bindings().next().is_some()
                || flow.endpoint_requests().next().is_some()
            {
                projection_owners.insert(path.clone());
            }
            for request in flow.endpoint_requests() {
                use crate::core::semantic::StaticString;
                if let (
                    StaticString::Literal { value: url },
                    StaticString::Literal { value: method },
                ) = (&request.url, &request.method)
                {
                    required.extend(
                        routes
                            .matching_routes(method, url)
                            .into_iter()
                            .map(|route| route.path.clone()),
                    );
                }
            }
            #[cfg(feature = "lang-html")]
            if node.details["url_status"].as_str() == Some("local_unverified")
                && html::relations::is_request(node, flow, path, dependencies)
            {
                let (method, url) = node.name.split_once(' ').unwrap_or(("GET", &node.name));
                required.extend(
                    routes
                        .matching_routes(method, url)
                        .into_iter()
                        .map(|route| route.path.clone()),
                );
            }
        }
    }
    let borrowed: BTreeMap<_, _> = facts
        .iter()
        .map(|(path, facts)| (path.clone(), &facts.facts))
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    for profile in languages::profiles() {
        if seen.insert(profile.id()) {
            required.extend(
                languages::linker_for(profile.id()).required_full_facts_borrowed(
                    &borrowed,
                    &projection_owners,
                    catalog,
                ),
            );
        }
    }
    #[cfg(feature = "lang-typescript")]
    required.extend(languages::typescript::linker::required_configuration_facts(
        facts,
        catalog,
        dependencies,
    ));
    #[cfg(not(feature = "lang-typescript"))]
    let _ = dependencies;
    #[cfg(feature = "lang-html")]
    required.extend(html::templates::required_full_facts(
        &borrowed,
        catalog,
        dependencies,
    ));
    required
}

/// Seeds shared-name invalidation from exports and configured factory results.
pub(crate) fn configured_callable_provider_paths<'a>(
    changed: &std::collections::BTreeSet<String>,
    catalog: impl IntoIterator<Item = (&'a str, &'a str)>,
    dependencies: &languages::dependency_registry::DependencyRegistry,
) -> std::collections::BTreeSet<String> {
    let mut directories =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<(String, String)>>::new();
    for path in changed {
        let Some(scope) = dependencies
            .nearest_manifest_scope_for_path(languages::LanguageFamily("javascript"), path)
        else {
            continue;
        };
        for rule in &dependencies.language_context_for_path(path).syntax_rules {
            let languages::manifests::syntax::SyntaxOperation::ComponentEnvironment {
                package,
                config_file,
                callable_imports: Some(descriptor),
                ..
            } = &rule.operation
            else {
                continue;
            };
            let config = if scope.is_empty() {
                config_file.clone()
            } else {
                format!("{scope}/{config_file}")
            };
            if path != &config
                || !rule
                    .activation_family()
                    .is_some_and(|family| dependencies.declares_for_path(family, path, package))
            {
                continue;
            }
            for selector in &descriptor.exports {
                let directory = if scope.is_empty() {
                    selector.directory.clone()
                } else {
                    format!("{scope}/{}", selector.directory)
                };
                directories
                    .entry(directory)
                    .or_default()
                    .insert((scope.to_owned(), selector.extension.clone()));
            }
        }
    }
    if directories.is_empty() {
        return std::collections::BTreeSet::new();
    }
    catalog
        .into_iter()
        .filter_map(|(path, language)| {
            if language != "typescript" {
                return None;
            }
            let (directory, _) = path.rsplit_once('/').unwrap_or(("", path));
            let selectors = directories.get(directory)?;
            selectors
                .iter()
                .any(|(scope, extension)| {
                    path.ends_with(extension)
                        && dependencies.nearest_manifest_scope_for_path(
                            languages::LanguageFamily("javascript"),
                            path,
                        ) == Some(scope.as_str())
                })
                .then(|| path.to_owned())
        })
        .collect()
}

/// Seeds shared-name invalidation from exports and configured factory results.
pub(crate) fn configured_result_names(
    facts: &crate::core::typed_facts::TypedFacts,
    dependencies: &languages::dependency_registry::DependencyRegistry,
) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    for node in &facts.facts.nodes {
        for export in node.details["exports"].as_array().into_iter().flatten() {
            for key in ["name", "local"] {
                if let Some(name) = export[key].as_str() {
                    names.insert(name.to_owned());
                }
            }
        }
        let Some(flow) = facts.flows.get(node) else {
            continue;
        };
        for factory in flow.factory_results() {
            names.insert(factory.binding.clone());
        }
        for rule in &dependencies
            .language_context_for_path(&node.path)
            .syntax_rules
        {
            if let languages::manifests::syntax::SyntaxOperation::ComponentEnvironment {
                modules,
                ..
            } = &rule.operation
            {
                for configuration in flow
                    .component_configurations()
                    .filter(|fact| fact.origin.pack == rule.pack && fact.origin.rule == rule.id)
                {
                    for module in &configuration.modules {
                        if let Some(descriptor) = modules.get(module) {
                            names.extend(descriptor.call_results.keys().cloned());
                        }
                    }
                }
            }
        }
    }
    names
}

pub(crate) fn named_binding_scope(
    path: &str,
    dependencies: &languages::dependency_registry::DependencyRegistry,
) -> String {
    named_bindings::project(path, dependencies).to_owned()
}
/// Performs link.
pub fn link(all: &BTreeMap<String, Facts>) -> PublicGraph {
    link_with_root(all, None, None)
}

/// Resolves the lean catalog with the same semantic rules as extracted facts.
pub fn link_catalog(catalog: &crate::core::semantic::SymbolCatalog) -> PublicGraph {
    let dependencies = languages::dependency_registry::DependencyRegistry::collect(None);
    link_compact_catalog_with_registry(catalog, None, &dependencies).into()
}

pub(crate) fn link_compact_catalog_with_registry(
    catalog: &crate::core::semantic::SymbolCatalog,
    root: Option<&Path>,
    dependencies: &languages::dependency_registry::DependencyRegistry,
) -> Graph {
    let mut interner = interner::LinkerInterner::from_catalog(&catalog.files);
    let flows: HashMap<_, _> = catalog
        .files
        .values()
        .flat_map(|file| {
            file.facts.nodes.iter().filter_map(|node| {
                file.flows.state(node).map(|state| {
                    (
                        interner
                            .lookup_symbol(&node.id)
                            .expect("catalog node id is interned"),
                        state,
                    )
                })
            })
        })
        .collect();
    let uncertainties: HashMap<_, _> = catalog
        .files
        .values()
        .flat_map(|file| file.flows.named_uncertainty_entries())
        .map(|(owner, markers)| {
            (
                interner
                    .lookup_symbol(owner)
                    .expect("catalog uncertainty owner is interned"),
                markers,
            )
        })
        .collect();
    let faults: HashSet<_> = catalog
        .files
        .values()
        .flat_map(|file| file.flows.named_uncertainty_faults())
        .map(|owner| {
            interner
                .lookup_symbol(owner)
                .expect("catalog fault owner is interned")
        })
        .collect();
    link_compact_impl(
        &catalog.files,
        None,
        None,
        root,
        dependencies,
        &mut interner,
        ResolutionInput {
            flows: Some(&flows),
            uncertainties: Some(&uncertainties),
            faults: Some(&faults),
        },
        Some(&catalog.occurrences),
    )
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
    let mut interner = interner::LinkerInterner::from_catalog(all);
    let mut flows = HashMap::new();
    let mut retained = HashMap::new();
    let mut faults = HashSet::new();
    for node in all.values().flat_map(|file| &file.nodes) {
        match crate::core::typed_facts::FlowStore::read_named_uncertainties(node) {
            Ok(markers) if !markers.is_empty() => {
                retained.insert(
                    interner
                        .lookup_symbol(&node.id)
                        .expect("node id is interned"),
                    markers,
                );
            }
            Err(()) => {
                faults.insert(
                    interner
                        .lookup_symbol(&node.id)
                        .expect("node id is interned"),
                );
            }
            _ => {}
        }
    }
    for file in all.values() {
        for node in &file.nodes {
            if let Some(state) = file.semantic.flows.state(node) {
                flows.insert(
                    interner
                        .lookup_symbol(&node.id)
                        .expect("node id is interned"),
                    state,
                );
            }
        }
        retained.extend(
            file.semantic
                .flows
                .named_uncertainty_entries()
                .map(|(owner, markers)| {
                    (
                        interner
                            .lookup_symbol(owner)
                            .expect("uncertainty owner is interned"),
                        markers.to_vec(),
                    )
                }),
        );
        faults.extend(file.semantic.flows.named_uncertainty_faults().map(|owner| {
            interner
                .lookup_symbol(owner)
                .expect("fault owner is interned")
        }));
    }
    let uncertainties: HashMap<_, _> = retained
        .iter()
        .map(|(owner, markers)| (*owner, markers.as_slice()))
        .collect();
    link_compact_impl(
        all,
        Some(all),
        owners,
        root,
        dependency_registry,
        &mut interner,
        ResolutionInput {
            flows: (!flows.is_empty()).then_some(&flows),
            uncertainties: Some(&uncertainties),
            faults: Some(&faults),
        },
        None,
    )
}

pub(crate) fn link_compact_with_typed_registry(
    all: &BTreeMap<String, crate::core::typed_facts::TypedFacts>,
    owners: Option<&std::collections::BTreeSet<String>>,
    root: Option<&Path>,
    dependency_registry: &languages::dependency_registry::DependencyRegistry,
) -> Graph {
    let mut interner = interner::LinkerInterner::from_catalog(all);
    let mut flows = HashMap::new();
    let mut uncertainties = HashMap::new();
    let mut faults = HashSet::new();
    for file in all.values() {
        for node in &file.facts.nodes {
            if let Some(state) = file.flows.state(node) {
                flows.insert(
                    interner
                        .lookup_symbol(&node.id)
                        .expect("typed node id is interned"),
                    state,
                );
            }
        }
        uncertainties.extend(
            file.flows
                .named_uncertainty_entries()
                .map(|(owner, markers)| {
                    (
                        interner
                            .lookup_symbol(owner)
                            .expect("typed uncertainty owner is interned"),
                        markers,
                    )
                }),
        );
        faults.extend(file.flows.named_uncertainty_faults().map(|owner| {
            interner
                .lookup_symbol(owner)
                .expect("typed fault owner is interned")
        }));
    }
    link_compact_impl(
        all,
        None,
        owners,
        root,
        dependency_registry,
        &mut interner,
        ResolutionInput {
            flows: Some(&flows),
            uncertainties: Some(&uncertainties),
            faults: Some(&faults),
        },
        None,
    )
}

struct ResolutionInput<'maps, 'facts, 'uncertainties> {
    flows: Option<&'maps HashMap<interner::SymbolId, &'facts crate::core::typed_facts::FlowState>>,
    uncertainties: Option<
        &'maps HashMap<
            interner::SymbolId,
            &'uncertainties [crate::core::semantic::NamedDefinitionUncertainty],
        >,
    >,
    faults: Option<&'maps HashSet<interner::SymbolId>>,
}

fn intern_call_site_symbols<'a>(
    interner: &mut interner::LinkerInterner<'a>,
    expression: &'a crate::core::semantic::ValueExpr,
) {
    use crate::core::semantic::ValueExpr;
    match expression {
        ValueExpr::Call { callee, site } => {
            interner.intern_symbol(callee);
            if let Some(site) = site {
                for argument in &site.arguments {
                    if let crate::core::semantic::StaticArgument::String { value } = argument {
                        interner.intern_symbol(value);
                    }
                }
                if let Some(receiver) = site.receiver.as_deref() {
                    intern_call_site_symbols(interner, receiver);
                }
            }
        }
        ValueExpr::Await { value } | ValueExpr::Scoped { value, .. } => {
            intern_call_site_symbols(interner, value)
        }
        ValueExpr::BranchJoin { alternatives } => {
            for alternative in alternatives {
                intern_call_site_symbols(interner, alternative);
            }
        }
        ValueExpr::Project { value, .. } => intern_call_site_symbols(interner, value),
        ValueExpr::Annotated { .. }
        | ValueExpr::Construct { .. }
        | ValueExpr::Alias { .. }
        | ValueExpr::LoopElement { .. }
        | ValueExpr::Field { .. }
        | ValueExpr::Object { .. }
        | ValueExpr::Unknown => {}
    }
}

fn language_context_key_hash(
    workspace: &str,
    families: &std::collections::BTreeSet<&'static str>,
    scopes: &[&str],
) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    workspace.hash(&mut hasher);
    for (family, scope) in families.iter().zip(scopes) {
        family.hash(&mut hasher);
        scope.hash(&mut hasher);
    }
    hasher.finish()
}

#[allow(clippy::too_many_arguments)]
fn link_compact_impl<'a, 'maps, 'uncertainties, F: AsRef<Facts> + Sync>(
    all: &'a BTreeMap<String, F>,
    public_all: Option<&'a BTreeMap<String, Facts>>,
    owners: Option<&std::collections::BTreeSet<String>>,
    root: Option<&Path>,
    dependency_registry: &'a languages::dependency_registry::DependencyRegistry,
    interner: &mut interner::LinkerInterner<'a>,
    input: ResolutionInput<'maps, 'a, 'uncertainties>,
    occurrences: Option<&BTreeMap<String, Vec<u8>>>,
) -> Graph {
    let ResolutionInput {
        flows,
        uncertainties,
        faults,
    } = input;
    let mut component_name_scopes = HashSet::new();
    let families: std::collections::BTreeSet<&'static str> = languages::profiles()
        .map(|profile| profile.family().0)
        .collect();
    let mut context_representatives = HashMap::<u64, Vec<(&str, Vec<&str>)>>::new();
    let mut context_scopes = Vec::with_capacity(families.len());
    for path in all.keys() {
        let (workspace, _) = languages::workspace_path(path);
        context_scopes.clear();
        for family in &families {
            context_scopes.push(
                dependency_registry
                    .nearest_manifest_scope_for_path(languages::LanguageFamily(family), path)
                    .unwrap_or(workspace),
            );
        }
        let context_hash = language_context_key_hash(workspace, &families, &context_scopes);
        let representatives = context_representatives.entry(context_hash).or_default();
        if representatives.iter().any(|(context_workspace, scopes)| {
            *context_workspace == workspace && scopes.as_slice() == context_scopes.as_slice()
        }) {
            continue;
        }
        representatives.push((workspace, context_scopes.clone()));

        let rules = dependency_registry.language_context_for_path(path);
        for rule in &rules.syntax_rules {
            interner.intern_owned_symbol_str(&rule.pack);
            interner.intern_owned_symbol_str(&rule.id);
            use languages::manifests::syntax::SyntaxOperation;
            match &rule.operation {
                SyntaxOperation::ComponentEnvironment {
                    directory,
                    extension,
                    autoimports,
                    builtins,
                    modules,
                    callable_imports,
                    ..
                } => {
                    if let Some(scope) = dependency_registry.nearest_manifest_scope_for_path(
                        languages::LanguageFamily("javascript"),
                        path,
                    ) {
                        let scope_key = (scope.to_owned(), rule.pack.clone(), rule.id.clone());
                        if component_name_scopes.insert(scope_key) {
                            for provider_path in all.keys() {
                                if let Some((pascal, kebab)) = semantic_context::component_names(
                                    provider_path,
                                    scope,
                                    directory,
                                    extension,
                                ) {
                                    interner.intern_owned_symbol(pascal);
                                    interner.intern_owned_symbol(kebab);
                                }
                            }
                        }
                    }
                    for name in autoimports.iter().chain(builtins) {
                        interner.intern_owned_symbol_str(name);
                    }
                    if let Some(imports) = callable_imports {
                        for alias in &imports.aliases {
                            interner.intern_owned_symbol_str(alias);
                        }
                    }
                    for (name, module) in modules {
                        interner.intern_owned_symbol_str(name);
                        interner.intern_owned_symbol_str(&module.package);
                        for component in &module.components {
                            interner.intern_owned_symbol_str(component);
                        }
                        for (callee, members) in &module.call_results {
                            interner.intern_owned_symbol_str(callee);
                            for member in members {
                                interner.intern_owned_symbol_str(member);
                            }
                        }
                    }
                }
                SyntaxOperation::ImplicitBuiltins { names } => {
                    for name in names {
                        interner.intern_owned_symbol_str(name);
                    }
                }
                SyntaxOperation::ReceiverType { receiver } => {
                    interner.intern_owned_symbol_str(&receiver.name);
                    for callback in &receiver.callback_callees {
                        interner.intern_owned_symbol_str(callback);
                    }
                    for factory in &receiver.argument_factories {
                        if let languages::manifests::projections::Api::GlobalMember {
                            receiver,
                            member,
                        } = &factory.callee
                        {
                            let callee = format!("{receiver}.{member}");
                            interner.intern_owned_symbol(callee);
                        }
                        for literal in &factory.literals {
                            interner.intern_owned_symbol_str(literal);
                        }
                    }
                }
                SyntaxOperation::NamedBinding {
                    binding:
                        languages::manifests::projections::NamedBindingForm::CallReference {
                            provider_scopes,
                            ..
                        },
                    ..
                } => {
                    for provider in provider_scopes {
                        if let Some(scope) = dependency_registry.named_provider_scope(provider) {
                            interner.intern_path(scope);
                        }
                    }
                }
                SyntaxOperation::NamedBinding {
                    binding:
                        languages::manifests::projections::NamedBindingForm::AssignmentDefinition {
                            template_root,
                            ..
                        },
                    ..
                } => {
                    interner.intern_owned_symbol_str(template_root);
                }
                _ => {}
            }
        }
        for (pack, base) in dependency_registry.hierarchy_return_targets_for_path(path) {
            interner.intern_owned_symbol_str(pack);
            interner.intern_owned_symbol_str(base);
        }
    }

    for path in all.keys() {
        interner.intern_path(languages::workspace_path(path).0);
        for family in [
            languages::LanguageFamily("javascript"),
            languages::LanguageFamily("python"),
        ] {
            if let Some(scope) = dependency_registry.nearest_manifest_scope_for_path(family, path) {
                interner.intern_path(scope);
            }
        }
    }
    if let Some(flows) = flows {
        for state in flows.values() {
            if let crate::core::typed_facts::FlowState::Valid { facts, .. } = state {
                for binding in &facts.bindings {
                    interner.intern_symbol(&binding.name);
                    intern_call_site_symbols(interner, &binding.value);
                }
                for join in &facts.branch_joins {
                    interner.intern_symbol(&join.name);
                    intern_call_site_symbols(interner, &join.value);
                }
                for field in &facts.fields {
                    interner.intern_symbol(&field.name);
                    intern_call_site_symbols(interner, &field.value);
                }
                for result in facts.factory_results() {
                    interner.intern_symbol(&result.binding);
                    interner.intern_symbol(&result.origin.pack);
                    interner.intern_symbol(&result.origin.rule);
                }
                for binding in facts.named_bindings() {
                    interner.intern_symbol(&binding.domain);
                    interner.intern_symbol(&binding.origin.pack);
                    interner.intern_symbol(&binding.origin.rule);
                    if let crate::core::semantic::StaticString::Literal { value } = &binding.key {
                        interner.intern_symbol(value);
                    }
                }
                for configuration in facts.component_configurations() {
                    interner.intern_symbol(&configuration.origin.pack);
                    interner.intern_symbol(&configuration.origin.rule);
                    for module in &configuration.modules {
                        interner.intern_symbol(module);
                    }
                }
            }
        }
    }
    if let Some(uncertainties) = uncertainties {
        for marker in uncertainties.values().flat_map(|markers| markers.iter()) {
            interner.intern_owned_symbol(std::sync::Arc::<str>::from(marker.domain.as_str()));
            interner.intern_owned_symbol(std::sync::Arc::<str>::from(marker.origin.pack.as_str()));
            interner.intern_owned_symbol(std::sync::Arc::<str>::from(marker.origin.rule.as_str()));
            if let crate::core::semantic::StaticString::Literal { value } = &marker.key {
                interner.intern_owned_symbol(std::sync::Arc::<str>::from(value.as_str()));
            }
        }
    }
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
    let (parts, sort_per_file) = {
        let nodes: Vec<&Node> = all.values().flat_map(|f| f.as_ref().nodes.iter()).collect();
        let sort_per_file = nodes.len() >= 8192 && all.len() >= 32;
        let mut by_name: HashMap<&str, Vec<&Node>> = HashMap::with_capacity(nodes.len());
        let mut by_qual: HashMap<&str, Vec<&Node>> = HashMap::with_capacity(nodes.len());
        let mut by_suffix: HashMap<&str, Vec<&Node>> = HashMap::new();
        let mut by_id: HashMap<interner::SymbolId, &Node> = HashMap::with_capacity(nodes.len());
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
            by_id.insert(
                interner.lookup_symbol(&n.id).expect("node id is interned"),
                n,
            );
            by_module.entry(&n.path).or_default().push(n);
            if needs_discovery {
                for (i, c) in n.qualname.char_indices() {
                    if c == '.' {
                        by_suffix.entry(&n.qualname[i + 1..]).or_default().push(n);
                    }
                }
            }
        }
        let mut node_bindings: HashMap<interner::SymbolId, hashbrown::HashSet<interner::SymbolId>> =
            HashMap::new();
        let mut node_rebindings: HashMap<
            interner::SymbolId,
            hashbrown::HashSet<interner::SymbolId>,
        > = HashMap::new();
        for n in &nodes {
            if n.binding_count() > 0 {
                let node_id = interner.lookup_symbol(&n.id).expect("node id is interned");
                node_bindings.insert(
                    node_id,
                    n.binding_names()
                        .map(|name| {
                            interner
                                .lookup_symbol(name)
                                .expect("binding name is interned")
                        })
                        .collect(),
                );
            }
            if n.rebinding_count() > 0 {
                let node_id = interner.lookup_symbol(&n.id).expect("node id is interned");
                node_rebindings.insert(
                    node_id,
                    n.rebinding_names()
                        .map(|name| {
                            interner
                                .lookup_symbol(name)
                                .expect("rebinding name is interned")
                        })
                        .collect(),
                );
            }
            if n.kind == "module" && n.language == "python" {
                let node_id = interner.lookup_symbol(&n.id).expect("node id is interned");
                if let Some(crate::core::typed_facts::FlowState::Valid { facts, .. }) =
                    flows.and_then(|states| states.get(&node_id))
                {
                    let rebindings = node_rebindings.entry(node_id).or_default();
                    rebindings.extend(facts.bindings.iter().map(|binding| {
                        interner
                            .lookup_symbol(&binding.name)
                            .expect("flow binding name is interned")
                    }));
                    rebindings.extend(facts.branch_joins.iter().map(|join| {
                        interner
                            .lookup_symbol(&join.name)
                            .expect("branch join name is interned")
                    }));
                }
            }
        }
        let mut children_by_parent: HashMap<interner::SymbolId, Vec<&Node>> = HashMap::new();
        for f in all.values() {
            let f = f.as_ref();
            for e in &f.edges {
                if e.kind == "contains" {
                    let Some(source_id) = interner.lookup_symbol(&e.src) else {
                        continue;
                    };
                    let Some(target_id) = interner.lookup_symbol(&e.dst) else {
                        continue;
                    };
                    if let Some(target_node) = by_id.get(&target_id) {
                        children_by_parent
                            .entry(source_id)
                            .or_default()
                            .push(target_node);
                    }
                }
            }
        }
        let namespace_indexes =
            indexing::NamespaceIndexes::build(all, &nodes, root, dependency_registry, interner);
        let borrowed_all = public_all.is_none().then(|| {
            all.iter()
                .map(|(path, facts)| (path.clone(), facts.as_ref()))
                .collect::<BTreeMap<_, _>>()
        });
        let package_exports = indexing::package_exports(
            &namespace_indexes.profiles,
            &namespace_indexes.modules_by_namespace,
            &by_module,
            &node_rebindings,
            public_all,
            borrowed_all.as_ref(),
            root,
            interner,
            #[cfg(feature = "lang-rust")]
            &namespace_indexes.rust_forwarding,
        );
        mark("namespace indexes and exports");
        let inheritance_indexes = indexing::InheritanceIndexes::build(
            all,
            &*interner,
            #[cfg(feature = "lang-rust")]
            flows,
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
        let normalized_imports = &namespace_indexes.normalized_imports;
        let modules_by_namespace = &namespace_indexes.modules_by_namespace;
        let python_receivers = inheritance_indexes.python_receivers;
        #[cfg(feature = "lang-rust")]
        let rust_members = inheritance_indexes.rust_members;
        mark("inheritance indexes");
        let semantic_context = semantic_context::Context::build(
            all,
            interner,
            modules_by_namespace,
            &by_module,
            &by_qual,
            &by_id,
            &node_bindings,
            &node_rebindings,
            &package_exports,
            normalized_imports,
            #[cfg(feature = "lang-rust")]
            &namespace_indexes.rust_forwarding,
            dependency_registry,
        )
        .with_python(&python_receivers)
        .with_flows(flows, uncertainties, faults, all, dependency_registry);
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
        let value_flow = value_flow::ValueFlowIndex::build_parallel(
            all,
            nodes.clone(),
            &*interner,
            &by_id,
            &semantic_context,
        );
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        unsafe {
            // SAFETY: initialization buffers have dropped; glibc synchronizes arenas.
            libc::malloc_trim(0);
        }
        mark("value flow");
        let prepared_semantic_facts =
            framework_relations::PreparedSemanticFacts::build(all, interner);
        let mut parts = resolution::resolve(resolution::ResolutionInput {
            all,
            interner,
            owners,
            dependency_registry,
            flows,
            sort_per_file,
            occurrences,
            by_name: &by_name,
            by_qual: &by_qual,
            by_suffix: &by_suffix,
            by_id: &by_id,
            by_module: &by_module,
            node_bindings: &node_bindings,
            node_rebindings: &node_rebindings,
            children_by_parent: &children_by_parent,
            profiles,
            normalized_imports,
            modules_by_namespace,
            #[cfg(feature = "lang-rust")]
            rust_forwarding: &namespace_indexes.rust_forwarding,
            package_exports: &package_exports,
            semantic_context: &semantic_context,
            prepared_semantic_facts: &prepared_semantic_facts,
            module_nodes: &module_nodes,
            value_flow: &value_flow,
        });
        drop(value_flow);
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        unsafe {
            // SAFETY: the reference pass no longer borrows the value-flow cache.
            libc::malloc_trim(0);
        }
        mark("reference linking");
        parts.extend(
            framework_relations::compose(
                all,
                owners,
                flows,
                dependency_registry,
                profiles,
                &semantic_context,
                framework_relations::CompositionInput {
                    nodes: &nodes,
                    by_id: &by_id,
                    semantic: &prepared_semantic_facts,
                },
            )
            .into_iter()
            .map(GraphPart::Resident),
        );
        mark("framework relation composition");
        (parts, sort_per_file)
    };
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        // SAFETY: glibc synchronizes arenas; all resolution caches have dropped.
        libc::malloc_trim(0);
    }
    let mut result = Graph {
        edges: Vec::with_capacity(parts.iter().map(|g| g.counts().0).sum()),
        coverage: Vec::with_capacity(parts.iter().map(|g| g.counts().1).sum()),
    };
    let mut remaining = parts.into_iter();
    loop {
        let mut batch = Vec::new();
        let mut rows = 0;
        for part in remaining.by_ref() {
            let (edges, coverage) = part.counts();
            rows += edges + coverage;
            batch.push(part);
            if rows >= 4096 || occurrences.is_none() {
                break;
            }
        }
        if batch.is_empty() {
            break;
        }
        let graphs: Vec<_> = if occurrences.is_some() {
            batch.into_par_iter().map(GraphPart::into_graph).collect()
        } else {
            batch.into_iter().map(GraphPart::into_graph).collect()
        };
        for graph in graphs {
            result.edges.extend(graph.edges);
            result.coverage.extend(graph.coverage);
        }
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
        semantic_context::Member::ConfiguredExternal { authority, origin, evidence } => {
            let authority = match authority {
                value_flow::ConfiguredReceiverAuthority::Hierarchy { pack, rule } => format!("pack {pack}; hierarchy provider {}; ordered bases {}", rule.provider, rule.bases.join(",")),
                value_flow::ConfiguredReceiverAuthority::Nominal { pack, rule_id, .. } => format!("pack {pack}; receiver rule {rule_id}"),
            };
            let origin = match origin {
                value_flow::TypeTarget::External { module, import_line } => format!("external import {module} at line {import_line}"),
                value_flow::TypeTarget::Local(node) | value_flow::TypeTarget::Callable(node) => format!("local receiver {} in {} at line {}", node.qualname, node.path, node.line),
                value_flow::TypeTarget::ConfiguredReceiver { evidence, .. } => format!("configured receiver {evidence}"),
                _ => "non-import receiver without an exact local declaration".to_owned(),
            };
            let language = languages::for_path(Path::new(path)).map_or("unknown", |profile| profile.id());
            ("external", format!("finite {language} configured member {}; {authority}; receiver origin {origin}; configuration {evidence}", reference.expression).into())
        }
        semantic_context::Member::External(module, line, Some(provider)) => {
            let member = reference.expression.rsplit('.').next().unwrap_or_default();
            let diagnostic_marker = if languages::for_path(Path::new(path))
                .is_some_and(|profile| profile.id() == "python" && !profile.is_stdlib(module))
            {
                "; diagnostic_reason=framework_magic"
            } else {
                ""
            };
            (
                "external",
                format!("finite Python framework member from {provider} via external import {module} at line {line}; resolved member {provider}.{member}; receiver origin statically inferred{diagnostic_marker}").into(),
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
    graph.reference_coverage(path, reference, status, evidence);
    if let semantic_context::Member::Local(node) = result {
        graph.reference_edge(
            path,
            reference,
            node.id.clone(),
            reference.kind.as_str(),
            "inferred",
        );
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

#[cfg(all(test, feature = "lang-python"))]
mod local_field_reference_contract {
    use super::link;

    fn linked_graph(source: &str) -> crate::core::models::Graph {
        let facts = crate::engine::ast::extract("service.py", "python", source).unwrap();
        let all = std::collections::BTreeMap::from([("service.py".to_owned(), facts)]);
        link(&all)
    }

    #[test]
    fn same_class_field_references_retain_exact_resolution() {
        let graph = linked_graph(
            "class Host:\n    request = None\n    def current(self): return self.request\n",
        );
        let coverage = graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == "self.request")
            .unwrap();
        assert_eq!(coverage.status, "resolved", "{coverage:?}");
        let edge = graph
            .edges
            .iter()
            .find(|edge| edge.kind == "references" && edge.evidence == "self.request")
            .unwrap();
        assert_eq!(edge.confidence, "exact", "{edge:?}");
    }

    #[test]
    fn constructor_assigned_field_reads_keep_field_identity() {
        let source = "class Host:\n    def __init__(self, request):\n        self.request = request\n    def current(self):\n        return self.request\n";
        let graph = linked_graph(source);
        let coverage = graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == "self.request" && coverage.line == 5)
            .unwrap();
        assert_eq!(coverage.status, "resolved", "{coverage:?}");
        let edge = graph
            .edges
            .iter()
            .find(|edge| {
                edge.kind == "references" && edge.evidence == "self.request" && edge.line == 5
            })
            .unwrap();
        assert_eq!(
            edge.dst, "field:service.py:3:service.Host.request",
            "{edge:?}"
        );
    }

    #[test]
    fn field_values_do_not_become_callable_targets() {
        let graph = linked_graph(
            "class Host:\n    def __init__(self, request):\n        self.request = request\n    def current(self):\n        return self.request()\n",
        );
        let coverage = graph
            .coverage
            .iter()
            .find(|coverage| coverage.expression == "self.request" && coverage.line == 5)
            .unwrap();
        assert_eq!(coverage.status, "unresolved", "{coverage:?}");
        assert!(!graph
            .edges
            .iter()
            .any(|edge| { edge.kind == "calls" && edge.evidence == "self.request()" }));
    }
}
