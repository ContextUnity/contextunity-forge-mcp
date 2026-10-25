use super::*;
use crate::core::typed_facts::FlowState;
use crate::engine::languages::{
    ImportPath, LanguageProfile, dependency_registry::DependencyRegistry,
};
use crate::engine::linker::traits::{ModulesByNamespace, PackageExports, import_provider_modules};

type ExternalAliases<'a> = HashMap<&'a str, Option<(String, usize)>>;
mod documentation;
#[cfg(feature = "lang-html")]
mod framework_projections;
mod imports;
mod value_flow_phase;

fn chained_member_receiver<'a, 'idx, F>(
    receiver: &'idx value_flow::TypeTarget<'a>,
    fields: &str,
    owner: &'a Node,
    path: &str,
    at: crate::core::semantic::SourcePosition,
    input: &ResolutionInput<'a, F>,
    profile: Option<&dyn LanguageProfile>,
) -> Option<Cow<'idx, value_flow::TypeTarget<'a>>>
where
    F: AsRef<Facts> + Sync,
{
    let value_flow = input.value_flow;
    let semantic_context = input.semantic_context;
    let mut receiver = Cow::Borrowed(receiver);
    for field in fields.split('.').filter(|field| !field.is_empty()) {
        receiver = match receiver.as_ref() {
            value_flow::TypeTarget::Local(class) => {
                value_flow.field_type(class, field).map(Cow::Borrowed)
            }
            value_flow::TypeTarget::RustDeref { .. }
            | value_flow::TypeTarget::RustReference { .. } => {
                super::rust::value_flow::local_class(receiver.as_ref())
                    .and_then(|class| value_flow.field_type(class, field))
                    .map(Cow::Borrowed)
                    .or_else(|| {
                        match value_flow::SemanticResolver::resolve_member(
                            semantic_context,
                            receiver.as_ref(),
                            field,
                            owner,
                            at,
                        ) {
                            value_flow::Symbol::Type(target) => Some(Cow::Owned(target)),
                            _ => None,
                        }
                    })
            }
            value_flow::TypeTarget::ConfiguredReceiver { .. } => {
                match value_flow::SemanticResolver::resolve_member(
                    semantic_context,
                    receiver.as_ref(),
                    field,
                    owner,
                    at,
                ) {
                    value_flow::Symbol::Type(target) => Some(Cow::Owned(target)),
                    _ => None,
                }
            }
            value_flow::TypeTarget::Builtin(name)
                if profile
                    .is_some_and(|profile| matches!(profile.id(), "typescript" | "javascript")) =>
            {
                semantic_context
                    .property_type_for_path(path, name, field)
                    .map(|name| Cow::Owned(value_flow::TypeTarget::Builtin(name)))
            }
            _ => None,
        }?;
    }
    Some(receiver)
}

fn builtin_member_origin(
    receiver: &value_flow::TypeTarget<'_>,
    profile: Option<&dyn LanguageProfile>,
    default_origin: Option<&'static str>,
) -> Option<&'static str> {
    match receiver {
        value_flow::TypeTarget::Builtin(name)
            if profile.is_some_and(|profile| profile.id() == "python") =>
        {
            match name.as_str() {
                "logging.Logger" => Some("builtin:logging.Logger"),
                "logging.LoggerAdapter" => Some("builtin:logging.LoggerAdapter"),
                "sqlite3.Connection" | "sqlite3.Cursor" => Some("builtin:sqlite3"),
                _ => default_origin,
            }
        }
        value_flow::TypeTarget::Builtin(name)
            if profile
                .is_some_and(|profile| matches!(profile.id(), "javascript" | "typescript"))
                && (name == "Web.Response"
                    || super::typescript::semantic_context::typed_dom_receiver(name).is_some()) =>
        {
            Some("builtin:web_api")
        }
        _ => default_origin,
    }
}

pub(in crate::engine::linker) fn reference_scope<'a>(owner: &'a Node, path: &str) -> &'a str {
    if owner.details["included_from"].is_string()
        && owner.details["source_path"].as_str() == Some(path)
    {
        owner.details["source_qualname"]
            .as_str()
            .filter(|scope| !scope.is_empty())
            .unwrap_or(owner.qualname.as_str())
    } else {
        owner.qualname.as_str()
    }
}

fn document_symbol_candidates<'a>(
    normalized_qualname: &str,
    declared_name: &str,
    source_path: &str,
    target_path: Option<&str>,
    by_qual: &HashMap<&'a str, Vec<&'a Node>>,
    by_name: &HashMap<&'a str, Vec<&'a Node>>,
    by_suffix: &HashMap<&'a str, Vec<&'a Node>>,
) -> Option<Vec<&'a Node>> {
    for tier in [
        by_qual.get(normalized_qualname),
        by_name.get(declared_name),
        by_suffix.get(normalized_qualname),
    ]
    .into_iter()
    .flatten()
    {
        let mut candidates: Option<Vec<&'a Node>> = None;
        for node in tier.iter().copied().filter(|node| {
            !matches!(node.kind.as_str(), "document" | "invariant" | "component")
                && target_path.map_or_else(
                    || {
                        crate::core::config::workspace_path(&node.path).0
                            == crate::core::config::workspace_path(source_path).0
                    },
                    |path| node.path == path,
                )
        }) {
            candidates.get_or_insert_with(Vec::new).push(node);
        }
        let Some(mut candidates) = candidates else {
            continue;
        };
        candidates.sort_by(|left, right| left.id.cmp(&right.id));
        candidates.dedup_by(|left, right| left.id == right.id);
        return Some(candidates);
    }
    None
}

pub(super) struct ResolutionInput<'a, F> {
    pub(super) all: &'a BTreeMap<String, F>,
    pub(super) interner: &'a interner::LinkerInterner<'a>,
    pub(super) owners: Option<&'a std::collections::BTreeSet<String>>,
    pub(super) dependency_registry: &'a DependencyRegistry,
    pub(super) flows: Option<&'a HashMap<super::interner::SymbolId, &'a FlowState>>,
    pub(super) sort_per_file: bool,
    pub(super) occurrences: Option<&'a BTreeMap<String, Vec<u8>>>,
    pub(super) by_name: &'a HashMap<&'a str, Vec<&'a Node>>,
    pub(super) by_qual: &'a HashMap<&'a str, Vec<&'a Node>>,
    pub(super) by_suffix: &'a HashMap<&'a str, Vec<&'a Node>>,
    pub(super) by_id: &'a HashMap<super::interner::SymbolId, &'a Node>,
    pub(super) by_module: &'a HashMap<&'a str, Vec<&'a Node>>,
    pub(super) node_bindings:
        &'a HashMap<super::interner::SymbolId, HashSet<super::interner::SymbolId>>,
    pub(super) node_rebindings:
        &'a HashMap<super::interner::SymbolId, HashSet<super::interner::SymbolId>>,
    pub(super) children_by_parent: &'a HashMap<super::interner::SymbolId, Vec<&'a Node>>,
    pub(super) profiles: &'a HashMap<&'a str, &'static dyn LanguageProfile>,
    pub(super) normalized_imports: &'a HashMap<(&'a str, &'a str), Option<ImportPath>>,
    pub(super) modules_by_namespace: &'a ModulesByNamespace<'a>,
    #[cfg(feature = "lang-rust")]
    pub(super) rust_forwarding: &'a languages::rust::linker::RustForwarding<'a>,
    pub(super) package_exports: &'a HashMap<&'static str, PackageExports<'a>>,
    pub(super) semantic_context: &'a semantic_context::Context<'a>,
    pub(super) prepared_semantic_facts: &'a framework_relations::PreparedSemanticFacts<'a>,
    pub(super) module_nodes: &'a [&'a Node],
    pub(super) value_flow: &'a value_flow::ValueFlowIndex<'a>,
}

struct LinkerContext<'a> {
    member_lookup: semantic_context::LanguageMemberLookup<'a, 'a>,
    #[cfg(feature = "lang-html")]
    html_templates: html::templates::Registry<'a>,
    #[cfg(feature = "lang-html")]
    template_roots: html::templates::TemplateRootRegistry<'a>,
    #[cfg(feature = "lang-html")]
    html_scripts: html::scripts::Registry<'a>,
}

pub(super) struct FileResolver<'a, 'file, F> {
    input: &'file ResolutionInput<'a, F>,
    linker: &'file LinkerContext<'a>,
    path: &'file str,
    facts: &'a Facts,
    references: &'file [Reference],
    file_module: &'file str,
    language_context: &'file languages::LanguageContext,
    graph: Graph,
    aliases: HashMap<&'file str, HashMap<&'a str, Vec<&'a Node>>>,
    external_aliases: HashMap<&'file str, ExternalAliases<'a>>,
    type_only_aliases: HashMap<&'file str, HashSet<&'a str>>,
    lookup_key: String,
    #[cfg(feature = "lang-html")]
    template_loads: Vec<&'file Reference>,
    #[cfg(feature = "lang-html")]
    template_registry_rules: Vec<&'file languages::manifests::syntax::SyntaxRule>,
    #[cfg(feature = "lang-html")]
    template_rules: Vec<&'file languages::manifests::syntax::SyntaxRule>,
}

impl<'a, 'file, F> FileResolver<'a, 'file, F>
where
    'a: 'file,
    F: AsRef<Facts> + Sync,
{
    fn new(
        input: &'file ResolutionInput<'a, F>,
        path: &'file str,
        facts: &'a Facts,
        references: &'file [Reference],
        file_module: &'file str,
        language_context: &'file languages::LanguageContext,
        linker: &'file LinkerContext<'a>,
    ) -> Self {
        Self {
            input,
            linker,
            path,
            facts,
            references,
            file_module,
            language_context,
            graph: Graph {
                edges: facts.edges.iter().map(Edge::from).collect(),
                coverage: Vec::with_capacity(references.len()),
            },
            aliases: HashMap::new(),
            external_aliases: HashMap::new(),
            type_only_aliases: HashMap::new(),
            lookup_key: String::new(),
            #[cfg(feature = "lang-html")]
            template_loads: Vec::new(),
            #[cfg(feature = "lang-html")]
            template_registry_rules: Vec::new(),
            #[cfg(feature = "lang-html")]
            template_rules: Vec::new(),
        }
    }
}

impl<'a, F> Copy for ResolutionInput<'a, F> {}

impl<'a, F> Clone for ResolutionInput<'a, F> {
    fn clone(&self) -> Self {
        *self
    }
}

pub(super) fn resolve<'a, F: AsRef<Facts> + Sync>(input: ResolutionInput<'a, F>) -> Vec<GraphPart> {
    let ResolutionInput {
        all,
        owners,
        dependency_registry,
        flows,
        sort_per_file,
        occurrences,
        by_id,
        by_module,
        profiles,
        normalized_imports,
        modules_by_namespace,
        semantic_context,
        prepared_semantic_facts,
        ..
    } = input;
    let member_lookup = semantic_context::LanguageMemberLookup::new(semantic_context);

    #[cfg(feature = "lang-html")]
    let html_templates =
        html::templates::Registry::build(all, flows, input.interner, dependency_registry);
    #[cfg(feature = "lang-html")]
    let template_roots = html::templates::TemplateRootRegistry::build(
        all,
        dependency_registry,
        flows,
        input.interner,
    );
    #[cfg(feature = "lang-html")]
    let html_scripts = html::scripts::Registry::build(
        all,
        normalized_imports,
        modules_by_namespace,
        dependency_registry,
    );
    let linker_context = LinkerContext {
        member_lookup,
        #[cfg(feature = "lang-html")]
        html_templates,
        #[cfg(feature = "lang-html")]
        template_roots,
        #[cfg(feature = "lang-html")]
        html_scripts,
    };

    let prepared_files: Vec<_> = all
        .iter()
        .filter(|(path, _)| owners.is_none_or(|owner_paths| owner_paths.contains(*path)))
        .map(|(path, file)| {
            (
                path.as_str(),
                file.as_ref(),
                dependency_registry.language_context_for_path(path),
            )
        })
        .collect();

    let parts: Vec<GraphPart> = prepared_files
        .par_iter()
        .map(|(path, facts, file_language_context)| {
            let path = *path;
            let facts = *facts;
            let restored_references = occurrences.and_then(|files| files.get(path)).map(|bytes| {
                crate::core::semantic::decode_catalog_references(bytes, &facts.references)
            });
            let references = restored_references.as_deref().unwrap_or(&facts.references);
            let profile = profiles.get(path).copied();
            let file_module = facts
                .nodes
                .iter()
                .find(|node| node.kind == "module")
                .map(|node| node.qualname.clone())
                .unwrap_or_else(|| {
                    profile.map_or_else(|| module_name(path), |profile| profile.module_name(path))
                });
            let mut file_resolver = FileResolver::new(
                &input,
                path,
                facts,
                references,
                &file_module,
                file_language_context.as_ref(),
                &linker_context,
            );
            imports::phase_imports(&mut file_resolver);
            #[cfg(feature = "lang-html")]
            framework_projections::phase_framework_projections(&mut file_resolver);
            value_flow_phase::phase_value_flow(&mut file_resolver);
            documentation::phase_documentation(&mut file_resolver);
            if profile.is_some_and(|profile| profile.id() == "python") {
                python::semantic_relations::compose_symbol_relations(
                    path,
                    facts,
                    references,
                    dependency_registry,
                    semantic_context,
                    by_module,
                    by_id,
                    prepared_semantic_facts,
                    &mut file_resolver.graph,
                );
            }
            if sort_per_file {
                sort_graph(&mut file_resolver.graph);
            }
            std::mem::take(&mut file_resolver.graph)
        })
        .map(|graph| GraphPart::new(graph, occurrences.is_some()))
        .collect();
    parts
}
