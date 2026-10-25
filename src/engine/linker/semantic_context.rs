//! Canonical symbol admission for the shared value-flow reducer.
//!
//! Import providers are selected once using the same language linker and
//! workspace namespaces as ordinary graph linking. No disk or SQL is consulted
//! while reducing values or resolving calls.

use super::interner::{PathId, SymbolId};
use super::traits::{ImportContext, ModulesByNamespace, PackageExports, import_provider_modules};
use super::value_flow::{SemanticResolver, Symbol, SymbolRole, TypeTarget};
use crate::core::{
    models::{Facts, Node, ReceiverHint, Reference},
    semantic::SourcePosition,
};
use crate::engine::languages::{self, ImportPath};
use hashbrown::{HashMap, HashSet};
use std::collections::BTreeMap;

mod components;
mod factories;
mod preparation;
mod type_resolution;

pub(super) fn component_names(
    path: &str,
    project: &str,
    root: &str,
    extension: &str,
) -> Option<(String, String)> {
    components::component_names(path, project, root, extension)
}

type ComponentScope = (PathId, SymbolId, SymbolId);
type ArgumentFactoryKey = (SymbolId, usize, SymbolId);
type ArgumentFactoryCandidates =
    Vec<std::sync::Arc<super::value_flow::ConfiguredReceiverAuthority>>;
type ArgumentFactories = HashMap<ArgumentFactoryKey, ArgumentFactoryCandidates>;
type HierarchyAuthorities =
    HashMap<(SymbolId, SymbolId), std::sync::Arc<super::value_flow::ConfiguredReceiverAuthority>>;
type ImplicitBuiltinMap = HashMap<SymbolId, String>;

struct ReceiverTypes {
    rules: std::sync::Arc<languages::LanguageContext>,
    indices: Vec<usize>,
}

impl ReceiverTypes {
    fn iter(&self) -> impl Iterator<Item = &languages::manifests::syntax::ReceiverTypeRule> {
        self.indices
            .iter()
            .filter_map(|&index| match &self.rules.syntax_rules[index].operation {
                languages::manifests::syntax::SyntaxOperation::ReceiverType { receiver } => {
                    Some(receiver)
                }
                _ => None,
            })
    }
}

struct ResultScope<'a> {
    config: &'a Node,
    module: SymbolId,
    exports: HashMap<SymbolId, Vec<(&'a Node, SymbolId)>>,
    calls: HashMap<SymbolId, std::sync::Arc<super::value_flow::ConfiguredCallResult>>,
}

#[derive(Default)]
struct CallableCandidates<'a> {
    nodes: Vec<&'a Node>,
    incomplete: bool,
}

struct CallableModule<'a> {
    owner: &'a Node,
    exports: HashMap<SymbolId, CallableCandidates<'a>>,
}

struct CallableScope<'a> {
    implicit: bool,
    aliases: Vec<SymbolId>,
    names: HashMap<SymbolId, CallableCandidates<'a>>,
    modules: HashMap<PathId, std::sync::Arc<CallableModule<'a>>>,
    incomplete: bool,
}

pub(in crate::engine::linker) enum Imported<'a> {
    Nodes(Vec<&'a Node>, SourcePosition, bool),
    External(String, SourcePosition, &'a Reference, bool),
    Unknown,
}

pub(super) enum Member<'a> {
    Local(&'a Node),
    External(&'a str, usize, Option<&'a str>),
    ConfiguredExternal {
        authority: &'a super::value_flow::ConfiguredReceiverAuthority,
        origin: &'a TypeTarget<'a>,
        evidence: &'a str,
    },
    Builtin,
    Ambiguous,
    Unknown,
}

fn name_rebound(owner: &Node, name: &str) -> bool {
    owner.rebinding_names().any(|candidate| candidate == name)
}

type FactoryImport<'a> = Option<(&'a Node, SymbolId, SourcePosition)>;

#[derive(Clone, Copy)]
enum FactoryBinding<'a> {
    Declaration(&'a crate::core::semantic::BindingFact),
    Initializer(&'a crate::core::semantic::BindingFact),
    Invalid,
}

enum FactoryDeclarations<'a> {
    Indexed {
        bindings: Option<&'a [SymbolId]>,
        rebindings: Option<&'a [SymbolId]>,
    },
    Raw(&'a Node),
}

pub(super) struct Context<'a> {
    interner: &'a super::interner::LinkerInterner<'a>,
    dependencies: &'a languages::dependency_registry::DependencyRegistry,
    named: super::named_bindings::NamedBindings<'a>,
    flows: Option<&'a HashMap<SymbolId, &'a crate::core::typed_facts::FlowState>>,
    by_qual: &'a HashMap<&'a str, Vec<&'a Node>>,
    by_id: &'a HashMap<SymbolId, &'a Node>,
    node_bindings: &'a super::NodeBindingIndex,
    node_rebindings: &'a super::NodeBindingIndex,
    exports: &'a HashMap<&'a str, PackageExports<'a>>,
    pub(in crate::engine::linker) imports: HashMap<SymbolId, HashMap<SymbolId, Imported<'a>>>,
    factory_imports: HashMap<(SymbolId, SymbolId), FactoryImport<'a>>,
    factory_bindings: HashMap<(SymbolId, SymbolId), FactoryBinding<'a>>,
    factory_declarations: HashMap<SymbolId, FactoryDeclarations<'a>>,
    factory_callees: HashSet<SymbolId>,
    receiver_types: HashMap<PathId, std::sync::Arc<ReceiverTypes>>,
    argument_factories: HashMap<PathId, std::sync::Arc<ArgumentFactories>>,
    hierarchy_results: HashMap<PathId, std::sync::Arc<HierarchyAuthorities>>,
    component_builtins: HashMap<PathId, HashMap<SymbolId, String>>,
    component_candidate_ids: HashMap<ComponentScope, usize>,
    component_candidates: Vec<HashMap<SymbolId, Vec<&'a Node>>>,
    component_consumers: HashMap<PathId, Vec<usize>>,
    implicit_builtins: HashMap<PathId, std::sync::Arc<ImplicitBuiltinMap>>,
    autoimports: HashMap<PathId, HashMap<SymbolId, String>>,
    result_scopes: HashMap<PathId, Vec<std::sync::Arc<ResultScope<'a>>>>,
    callable_scopes: HashMap<PathId, Vec<std::sync::Arc<CallableScope<'a>>>>,
    commonjs: super::commonjs::CommonJsBindings<'a>,
    members: HashMap<(SymbolId, SymbolId, bool), Vec<&'a Node>>,
    python: Option<&'a super::receivers::PythonReceivers<'a>>,
    #[cfg(feature = "lang-rust")]
    rust: Option<&'a languages::rust::linker::RustMembers<'a>>,
    #[cfg(feature = "lang-typescript")]
    typescript: Option<&'a languages::typescript::linker::TypeScriptMembers<'a>>,
}

/// Borrowed read-only access to the language-owned member indexes.
///
/// The shared linker depends on this lookup seam instead of carrying one
/// language-specific index field per profile. Each index remains built and
/// owned by its language resolver; lookups only borrow that precomputed state.
pub(super) struct LanguageMemberLookup<'ctx, 'a> {
    context: &'ctx Context<'a>,
}

impl<'ctx, 'a> LanguageMemberLookup<'ctx, 'a> {
    pub(super) fn new(context: &'ctx Context<'a>) -> Self {
        Self { context }
    }

    pub(super) fn python(
        &self,
        class: &Node,
        member: &str,
        associated: bool,
    ) -> super::receivers::Member<'a> {
        self.context
            .python
            .map_or(super::receivers::Member::Unknown, |index| {
                index.lookup(class, member, associated)
            })
    }

    #[cfg(feature = "lang-rust")]
    pub(super) fn rust_receiver(&self, owner: &Node) -> Option<&'a Node> {
        self.context
            .rust
            .and_then(|index| index.receiver_for(owner))
    }

    #[cfg(feature = "lang-rust")]
    pub(super) fn rust_deref_target(&self, receiver: &Node) -> Option<&'a Node> {
        self.context
            .rust
            .and_then(|index| index.deref_target(receiver))
    }

    #[cfg(feature = "lang-rust")]
    pub(super) fn rust<'lookup>(
        &'lookup self,
        receiver: &Node,
        member: &str,
        associated: bool,
        caller: &Node,
    ) -> &'lookup [&'a Node] {
        self.context.rust.map_or(&[], |index| {
            index.lookup(receiver, member, associated, caller)
        })
    }

    #[cfg(feature = "lang-typescript")]
    pub(super) fn typescript<'lookup>(
        &'lookup self,
        class: &Node,
        member: &str,
        associated: bool,
    ) -> &'lookup [&'a Node] {
        self.context
            .typescript
            .map_or(&[], |index| index.lookup(class, member, associated))
    }
}

impl<'a> Context<'a> {
    fn path_id(&self, path: &str) -> Option<PathId> {
        self.interner.lookup_path(path)
    }

    pub(in crate::engine::linker) fn symbol_id(&self, symbol: &str) -> Option<SymbolId> {
        self.interner.lookup_symbol(symbol)
    }

    fn factory_declares(&self, owner: &Node, name: &str) -> bool {
        let name_id = self.symbol_id(name);
        match self
            .symbol_id(&owner.id)
            .and_then(|owner| self.factory_declarations.get(&owner))
        {
            Some(FactoryDeclarations::Raw(declaration)) => declaration
                .binding_names()
                .chain(declaration.rebinding_names())
                .any(|binding| binding == name),
            Some(FactoryDeclarations::Indexed {
                bindings,
                rebindings,
            }) => name_id.is_some_and(|name_id| {
                bindings.is_some_and(|names| names.binary_search(&name_id).is_ok())
                    || rebindings.is_some_and(|names| names.binary_search(&name_id).is_ok())
            }),
            None => false,
        }
    }
    fn configured_unshadowed(&self, owner: &'a Node, name: &str) -> bool {
        let mut scope = owner.qualname.as_str();
        let mut key = String::new();
        for _ in 0..super::value_flow::SemanticLimits::default().alias_depth {
            if self
                .by_qual
                .get(scope)
                .into_iter()
                .flatten()
                .any(|lexical| {
                    lexical.path == owner.path
                        && (self.factory_declares(lexical, name)
                            || self
                                .symbol_id(&lexical.id)
                                .zip(self.symbol_id(name))
                                .is_some_and(|key| self.factory_bindings.contains_key(&key))
                            || self
                                .symbol_id(&lexical.id)
                                .and_then(|owner| self.imports.get(&owner))
                                .is_some_and(|imports| {
                                    self.symbol_id(name)
                                        .is_some_and(|name| imports.contains_key(&name))
                                }))
                })
                || self
                    .by_qual
                    .get(super::qualified(&mut key, scope, name))
                    .into_iter()
                    .flatten()
                    .any(|node| node.path == owner.path)
            {
                return false;
            }
            let Some((parent, _)) = scope.rsplit_once('.') else {
                return true;
            };
            scope = parent;
        }
        false
    }

    fn implicit_factory_result(&self, owner: &'a Node, callee: &str) -> Option<TypeTarget<'a>> {
        let callee_id = self.symbol_id(callee)?;
        let mut providers = self
            .result_scopes
            .get(&self.path_id(owner.path.as_str())?)
            .into_iter()
            .flatten()
            .filter_map(|scope| scope.exports.get(&callee_id))
            .flatten();
        let (provider, binding) = providers.next()?;
        if !self.configured_unshadowed(owner, callee) {
            return None;
        }
        if providers.next().is_some() {
            return Some(TypeTarget::Ambiguous);
        }
        Some(
            self.factory_value(provider, *binding, owner)
                .unwrap_or(TypeTarget::Unknown),
        )
    }

    fn factory_value(
        &self,
        provider: &'a Node,
        binding: SymbolId,
        consumer: &Node,
    ) -> Option<TypeTarget<'a>> {
        let definition = self
            .named
            .factories
            .get(&(self.interner.lookup_symbol(&provider.id)?, binding))?;
        let [fact] = definition.facts.as_slice() else {
            return Some(TypeTarget::Ambiguous);
        };
        if let Some(required) = &fact.configured_module {
            let required = self.symbol_id(required)?;
            let provider_path = self.path_id(provider.path.as_str())?;
            let consumer_path = self.path_id(consumer.path.as_str())?;
            let consumer_scopes = self.result_scopes.get(&consumer_path);
            let admitted = self
                .result_scopes
                .get(&provider_path)
                .into_iter()
                .flatten()
                .any(|scope| {
                    scope.module == required
                        && consumer_scopes.into_iter().flatten().any(|other| {
                            other.module == required && other.config.id == scope.config.id
                        })
                });
            if !admitted {
                return Some(TypeTarget::Unknown);
            }
        }
        Some(definition.value.clone())
    }

    pub(super) fn property_type_for_path(
        &self,
        path: &str,
        receiver: &str,
        member: &str,
    ) -> Option<String> {
        if let Some(receiver) = self.receiver_type(path, receiver) {
            return receiver.properties.get(member).cloned();
        }
        super::typescript::semantic_context::typed_dom_property(receiver, member).map(str::to_owned)
    }
    fn receiver_type(
        &self,
        path: &str,
        name: &str,
    ) -> Option<&languages::manifests::syntax::ReceiverTypeRule> {
        let path = self.path_id(path)?;
        let mut candidates = self
            .receiver_types
            .get(&path)?
            .iter()
            .filter(|receiver| receiver.name == name);
        let first = candidates.next()?;
        candidates.next().is_none().then_some(first)
    }

    pub(super) fn exact_external_import(
        &self,
        owner: &Node,
        alias: &str,
        module: &str,
        symbol: &str,
        at: SourcePosition,
    ) -> bool {
        self.symbol_id(&owner.id)
            .zip(self.symbol_id(alias))
            .is_some_and(|(owner_id, alias_id)| {
                matches!(
                    self.imports
                        .get(&owner_id)
                        .and_then(|imports| imports.get(&alias_id)),
                    Some(Imported::External(origin, position, reference, _))
                        if origin == module
                            && *position < at
                            && reference.source == owner.id
                            && reference.module.as_deref() == Some(module)
                            && reference.expression == symbol
                            && reference.alias.as_deref() == Some(alias)
                )
            })
    }

    pub(super) fn component_builtin(&self, path: &str, tag: &str) -> Option<&str> {
        let tag = self.symbol_id(tag)?;
        self.component_builtins
            .get(&self.path_id(path)?)?
            .get(&tag)
            .map(String::as_str)
    }

    pub(super) fn component_candidates(&self, path: &str, tag: &str) -> Vec<&'a Node> {
        let (Some(path), Some(tag)) = (self.path_id(path), self.symbol_id(tag)) else {
            return Vec::new();
        };
        let mut candidates: Vec<_> = self
            .component_consumers
            .get(&path)
            .into_iter()
            .flatten()
            .filter_map(|&index| self.component_candidates.get(index)?.get(&tag))
            .flatten()
            .copied()
            .collect();
        candidates.sort_unstable_by(|left, right| left.id.cmp(&right.id));
        candidates.dedup_by(|left, right| left.id == right.id);
        candidates
    }

    pub(super) fn autoimport(&self, path: &str, name: &str) -> bool {
        let Some(path) = self.path_id(path) else {
            return false;
        };
        let Some(name) = self.symbol_id(name) else {
            return false;
        };
        self.autoimports
            .get(&path)
            .is_some_and(|names| names.contains_key(&name))
            || self
                .implicit_builtins
                .get(&path)
                .is_some_and(|names| names.contains_key(&name))
    }

    /// Only configured, complete exported callables supplement ordinary lexical calls.
    pub(super) fn implicit_callables(&self, owner: &'a Node, name: &str) -> Vec<&'a Node> {
        let mut candidates = Vec::new();
        let Some(name_id) = self.symbol_id(name) else {
            return candidates;
        };
        let Some(path) = self.path_id(&owner.path) else {
            return candidates;
        };
        for scope in self
            .callable_scopes
            .get(&path)
            .into_iter()
            .flatten()
            .filter(|scope| scope.implicit)
        {
            if scope.incomplete {
                return Vec::new();
            }
            if let Some(found) = scope.names.get(&name_id) {
                if found.incomplete {
                    return Vec::new();
                }
                for node in &found.nodes {
                    if !candidates
                        .iter()
                        .any(|previous: &&Node| previous.id == node.id)
                    {
                        if candidates.len() == 8 {
                            return Vec::new();
                        }
                        candidates.push(*node);
                    }
                }
            }
        }
        if !candidates.is_empty() && !self.configured_unshadowed(owner, name) {
            candidates.clear();
        }
        candidates
    }

    /// Confined aliases normalize into the existing canonical import path.
    pub(super) fn callable_import_path(
        &self,
        path: &str,
        reference: &Reference,
    ) -> Option<ImportPath> {
        let (module, _) = self.callable_import_module(path, reference)?;
        Some(ImportPath {
            namespace: module.qualname.clone(),
            relative: true,
            symbol_path: false,
        })
    }

    fn callable_import_module(
        &self,
        path: &str,
        reference: &Reference,
    ) -> Option<(&'a Node, &CallableCandidates<'a>)> {
        let source = reference.module.as_deref()?;
        let path = self.path_id(path)?;
        let mut selected: Option<(&Node, &CallableCandidates<'a>)> = None;
        for scope in self.callable_scopes.get(&path).into_iter().flatten() {
            if scope.incomplete {
                return None;
            }
            for alias in &scope.aliases {
                let alias = self.interner.resolve_symbol(*alias);
                let Some(tail) = source.strip_prefix(alias) else {
                    continue;
                };
                if tail
                    .split('/')
                    .any(|part| part.is_empty() || matches!(part, "." | ".."))
                    || tail.contains('\\')
                {
                    return None;
                }
                let module = scope.modules.get(&self.path_id(tail)?)?;
                let target = module
                    .exports
                    .get(&self.symbol_id(reference.expression.as_str())?)?;
                if target.incomplete || target.nodes.is_empty() {
                    return None;
                }
                if selected.is_some_and(|(previous, _)| previous.id != module.owner.id) {
                    return None;
                }
                selected = Some((module.owner, target));
            }
        }
        let selected = selected?;
        let alias = reference.alias.as_deref()?;
        let owner = reference.source.as_str();
        let owner_id = self.symbol_id(owner)?;
        let alias_id = self.symbol_id(alias)?;
        let rebound = match self.factory_declarations.get(&owner_id) {
            Some(FactoryDeclarations::Indexed { rebindings, .. }) => {
                rebindings.is_some_and(|names| names.binary_search(&alias_id).is_ok())
            }
            Some(FactoryDeclarations::Raw(node)) => {
                node.rebinding_names().any(|name| name == alias)
            }
            None => false,
        };
        if rebound || self.factory_bindings.contains_key(&(owner_id, alias_id)) {
            return None;
        }
        Some(selected)
    }

    pub(super) fn autoimport_evidence(&self, path: &str, name: &str) -> Option<&str> {
        let path = self.path_id(path)?;
        let name = self.symbol_id(name)?;
        self.autoimports
            .get(&path)
            .and_then(|names| names.get(&name))
            .or_else(|| {
                self.implicit_builtins
                    .get(&path)
                    .and_then(|names| names.get(&name))
            })
            .map(String::as_str)
    }

    pub(in crate::engine::linker) fn unshadowed_global(
        &self,
        owner: &'a Node,
        name: &str,
        at: SourcePosition,
    ) -> bool {
        let mut scope = owner.qualname.as_str();
        let mut lookup_key = String::with_capacity(128);
        loop {
            if self.by_qual.get(scope).into_iter().flatten().any(|node| {
                node.path == owner.path
                    && (node.binding_names().any(|candidate| candidate == name)
                        || node.rebinding_names().any(|candidate| candidate == name)
                        || self.symbol_id(&node.id).is_some_and(|node_id| {
                            self.imports.get(&node_id).is_some_and(|imports| {
                                self.symbol_id(name)
                                    .is_some_and(|name| imports.contains_key(&name))
                            })
                        })
                        || self.value_flow(node).is_some_and(|flow| {
                            flow.bindings
                                .iter()
                                .any(|binding| binding.name == name && binding.position <= at)
                        }))
            }) || self
                .by_qual
                .get(super::qualified(&mut lookup_key, scope, name))
                .into_iter()
                .flatten()
                .any(|node| node.path == owner.path)
            {
                return false;
            }
            let Some((parent, _)) = scope.rsplit_once('.') else {
                break;
            };
            scope = parent;
        }
        true
    }

    pub(super) fn tracks_commonjs_binding(&self, scope: &Node, alias: &str) -> bool {
        self.commonjs.tracks(scope, alias)
    }

    pub(super) fn node_has_rebinding(&self, owner: &Node, alias: &str) -> bool {
        self.symbol_id(&owner.id)
            .zip(self.symbol_id(alias))
            .is_some_and(|(owner, alias)| {
                self.node_rebindings
                    .get(&owner)
                    .is_some_and(|bindings| bindings.binary_search(&alias).is_ok())
            })
    }

    pub(super) fn admits_commonjs_binding(
        &self,
        scope: &Node,
        alias: &str,
        owner: &Node,
        at: SourcePosition,
    ) -> bool {
        self.commonjs.admits(scope, alias, owner, at)
    }
    fn visible(owner: &Node, candidate: &Node, at: SourcePosition, role: SymbolRole) -> bool {
        if let Some(visible) = super::rust::semantic_context::visible(owner, candidate) {
            return visible;
        }
        if matches!(role, SymbolRole::Type) {
            return true;
        }
        if candidate
            .qualname
            .rsplit_once('.')
            .is_none_or(|(parent, _)| parent != owner.qualname)
        {
            return true;
        }
        if super::typescript::semantic_context::visible(owner, candidate) {
            return true;
        }
        SourcePosition {
            line: candidate.line,
            column: candidate.details["column"].as_u64().unwrap_or(0) as usize,
        } < at
    }
    pub(super) fn with_flows<F: AsRef<Facts>>(
        mut self,
        flows: Option<&'a HashMap<SymbolId, &'a crate::core::typed_facts::FlowState>>,
        uncertainties: Option<
            &'a HashMap<SymbolId, &'a [crate::core::semantic::NamedDefinitionUncertainty]>,
        >,
        faults: Option<&HashSet<SymbolId>>,
        all: &'a BTreeMap<String, F>,
        dependencies: &'a languages::dependency_registry::DependencyRegistry,
    ) -> Self {
        self.flows = flows;
        self.named = super::named_bindings::NamedBindings::build(
            all,
            self.interner,
            flows,
            uncertainties,
            faults,
            dependencies,
            self.by_id,
        );
        self.prepare_registry_bindings();
        self.prepare_factory_bindings(all);
        self.prepare_component_environments(all, dependencies);
        self.prepare_callable_aliases(all);
        let mut factory_callees = HashSet::new();
        factory_callees.extend(self.factory_imports.keys().map(|(_, callee_id)| *callee_id));
        factory_callees.extend(self.factory_bindings.keys().map(|(_, callee_id)| *callee_id));
        factory_callees.extend(self.named.factories.keys().map(|(_, binding)| *binding));
        for scopes in self.result_scopes.values() {
            for scope in scopes {
                factory_callees.extend(scope.calls.keys().copied());
            }
        }
        for map in self.implicit_builtins.values() {
            factory_callees.extend(map.keys().copied());
        }
        for map in self.component_builtins.values() {
            factory_callees.extend(map.keys().copied());
        }
        self.factory_callees = factory_callees;
        self
    }

    pub(super) fn named_target(
        &self,
        owner: &'a Node,
        domain: &str,
        key: &str,
        origin: Option<&crate::core::semantic::ExtractionRuleOrigin>,
    ) -> Symbol<'a> {
        use crate::core::semantic::{NamedBindingRole, NamedTargetKind, SymbolOperand};
        let scope = super::named_bindings::project(&owner.path, self.dependencies);
        let Some(scope_id) = self.interner.lookup_path(scope) else {
            return Symbol::Unknown;
        };
        let origin_scopes = origin.and_then(|origin| {
            self.symbol_id(&origin.pack)
                .zip(self.symbol_id(&origin.rule))
                .and_then(|(pack, rule)| self.named.scopes(scope_id, pack, rule))
        });
        let mut scoped_ids = Vec::new();
        if let Some(scopes) = origin_scopes {
            scoped_ids.extend_from_slice(scopes);
        } else {
            scoped_ids.push(scope_id);
        }
        let (Some(domain), Some(key)) = (self.symbol_id(domain), self.symbol_id(key)) else {
            return Symbol::Unknown;
        };
        let mut selected = None;
        let mut count = 0usize;
        let mut uncertain = false;
        for scope in scoped_ids {
            let (matching, unknown) = self.named.definitions(scope, domain, key);
            count += matching.len();
            selected = selected.or_else(|| matching.first());
            uncertain |= unknown;
        }
        if uncertain || count == 0 {
            return Symbol::Unknown;
        }
        if count != 1 {
            return Symbol::Ambiguous;
        }
        let Some(definition) = selected else {
            return Symbol::Unknown;
        };
        let NamedBindingRole::Definition {
            target,
            target_kind,
        } = &definition.fact.role
        else {
            return Symbol::Unknown;
        };
        match target {
            SymbolOperand::Node { id } => {
                let Some(target) = self.interner.lookup_node(self.by_id, id) else {
                    return Symbol::Unknown;
                };
                if target.path != definition.owner.path {
                    return Symbol::Unknown;
                }
                match target_kind {
                    NamedTargetKind::Object if target.kind == "object" => {
                        Symbol::Type(TypeTarget::Local(target))
                    }
                    NamedTargetKind::ListenerScope if target.kind == "template_scope" => {
                        Symbol::Type(TypeTarget::Local(target))
                    }
                    NamedTargetKind::Callable
                        if matches!(target.kind.as_str(), "function" | "method") =>
                    {
                        Symbol::Callable(target)
                    }
                    _ => Symbol::Unknown,
                }
            }
            SymbolOperand::Name { expression } if *target_kind == NamedTargetKind::Callable => {
                match self.resolve_symbol(
                    definition.owner,
                    expression,
                    definition.fact.position,
                    SymbolRole::Callable,
                ) {
                    symbol @ (Symbol::Callable(_) | Symbol::Ambiguous) => symbol,
                    _ => Symbol::Unknown,
                }
            }
            _ => Symbol::Unknown,
        }
    }

    /// None preserves ordinary template fallback; observed uncertainty suppresses it.
    pub(super) fn template_named_target(
        &self,
        owner: &'a Node,
        domain: &str,
        key: &str,
        kind: &str,
    ) -> Option<Symbol<'a>> {
        use crate::core::semantic::StaticString;
        if kind != "filter" {
            return None;
        }
        let scope = super::named_bindings::project(&owner.path, self.dependencies);
        let Some(scope_id) = self.interner.lookup_path(scope) else {
            return Some(Symbol::Unknown);
        };
        let (Some(domain), Some(key)) = (self.symbol_id(domain), self.symbol_id(key)) else {
            return Some(Symbol::Unknown);
        };
        let owner_path = self.path_id(&owner.path)?;
        if self
            .named
            .registry_uncertain(scope_id, domain, key, owner_path, self.interner)
        {
            return Some(Symbol::Unknown);
        }
        let mut result = None;
        for registry in self.named.registry_definitions(scope_id, domain) {
            if !registry.root.contains_path(owner_path, self.interner) {
                continue;
            }
            let definition = &registry.definition;
            match &definition.fact.key {
                StaticString::Literal { value } if self.symbol_id(value.as_str()) != Some(key) => {
                    continue;
                }
                StaticString::Unknown => return Some(Symbol::Unknown),
                _ => {}
            }
            let symbol = match registry.target {
                Ok(Some(node)) => Symbol::Callable(node),
                Ok(None) => Symbol::Unknown,
                Err(()) => Symbol::Ambiguous,
            };
            result = Some(match (result, symbol) {
                (_, Symbol::Unknown | Symbol::Type(_)) => return Some(Symbol::Unknown),
                (Some(Symbol::Unknown), _) => return Some(Symbol::Unknown),
                (Some(Symbol::Ambiguous), _) | (_, Symbol::Ambiguous) => Symbol::Ambiguous,
                (Some(Symbol::Callable(previous)), Symbol::Callable(next))
                    if previous.id != next.id =>
                {
                    Symbol::Ambiguous
                }
                (_, symbol) => symbol,
            });
        }
        result
    }

    fn prepare_registry_bindings(&mut self) {
        use crate::core::semantic::{NamedBindingRole, SymbolOperand, TypeExpr};
        let prepared: Vec<_> = self
            .named
            .registries
            .iter()
            .map(|(key, registries)| {
                let targets: Vec<_> = registries
                    .iter()
                    .map(|registry| {
                        let definition = &registry.definition;
                        let NamedBindingRole::RegistryDefinition {
                            receiver:
                                SymbolOperand::Name {
                                    expression: receiver,
                                },
                            target,
                        } = &definition.fact.role
                        else {
                            return Ok(None);
                        };
                        let Some(annotation) = definition.owner.parameter_type(receiver) else {
                            return Ok(None);
                        };
                        let receiver = super::value_flow::resolve_type(
                            definition.owner,
                            &TypeExpr::Named {
                                name: annotation.to_owned(),
                            },
                            definition.fact.position,
                            self,
                            super::value_flow::SemanticLimits::default(),
                        );
                        let TypeTarget::External {
                            module,
                            import_line,
                        } = receiver
                        else {
                            return Ok(None);
                        };
                        let expected = format!(
                            "{}.{}",
                            registry.receiver_type.module, registry.receiver_type.export
                        );
                        if (module != registry.receiver_type.module && module != expected)
                            || !self.registry_receiver_import(
                                definition.owner,
                                annotation,
                                &registry.receiver_type,
                                import_line,
                                definition.fact.position,
                            )
                        {
                            return Ok(None);
                        }
                        let symbol = match target {
                            SymbolOperand::Name { expression } => self.resolve_symbol(
                                definition.owner,
                                expression,
                                definition.fact.position,
                                SymbolRole::Callable,
                            ),
                            SymbolOperand::Node { id } => self
                                .interner
                                .lookup_node(self.by_id, id)
                                .filter(|node| {
                                    node.path == definition.owner.path
                                        && matches!(node.kind.as_str(), "function" | "method")
                                })
                                .map_or(Symbol::Unknown, Symbol::Callable),
                            SymbolOperand::Unknown => Symbol::Unknown,
                        };
                        match symbol {
                            Symbol::Callable(node) => Ok(Some(node)),
                            Symbol::Ambiguous => Err(()),
                            _ => Ok(None),
                        }
                    })
                    .collect();
                (*key, targets)
            })
            .collect();
        for (key, targets) in prepared {
            if let Some(registries) = self.named.registries.get_mut(&key) {
                for (registry, target) in registries.iter_mut().zip(targets) {
                    registry.target = target;
                }
            }
        }
    }

    fn registry_receiver_import(
        &self,
        owner: &'a Node,
        annotation: &str,
        expected: &languages::manifests::projections::ImportedReceiverType,
        import_line: usize,
        at: SourcePosition,
    ) -> bool {
        let (alias, member) = annotation.split_once('.').unwrap_or((annotation, ""));
        let mut scope = owner.qualname.as_str();
        loop {
            for node in self.nodes_in_path(scope, &owner.path) {
                let import = self
                    .symbol_id(&node.id)
                    .and_then(|node| self.imports.get(&node))
                    .and_then(|imports| {
                        self.symbol_id(alias).and_then(|alias| imports.get(&alias))
                    });
                if let Some(import) = import {
                    return matches!(import, Imported::External(module, position, reference, _)
                        if module == &expected.module
                            && position.line == import_line && *position < at
                            && reference.source == node.id
                            && reference.module.as_deref() == Some(expected.module.as_str())
                            && reference.alias.as_deref() == Some(alias)
                            && ((member.is_empty() && reference.expression == expected.export)
                                || (member == expected.export && reference.expression == expected.module)));
                }
            }
            let Some((parent, _)) = scope.rsplit_once('.') else {
                return false;
            };
            scope = parent;
        }
    }

    pub(super) fn named_member(
        &self,
        owner: &'a Node,
        name: &str,
        at: SourcePosition,
    ) -> Option<Symbol<'a>> {
        use crate::core::semantic::{NamedBindingRole, StaticString, ValueExpr};
        use languages::manifests::{projections::NamedBindingForm, syntax::SyntaxOperation};
        let (root, _) = name.split_once('.')?;
        let rules = self.dependencies.language_context_for_path(&owner.path);
        for &index in rules.named_member_rules(root) {
            let rule = &rules.syntax_rules[index];
            let SyntaxOperation::NamedBinding {
                domain,
                binding: NamedBindingForm::MemberReference { root },
            } = &rule.operation
            else {
                continue;
            };
            let Some(tail) = name
                .strip_prefix(root)
                .and_then(|tail| tail.strip_prefix('.'))
            else {
                continue;
            };
            let (key, member) = tail.split_once('.').unwrap_or((tail, ""));
            let proven = self.value_flow(owner).is_some_and(|flow| {
                flow.named_bindings().any(|fact| {
                    fact.origin.pack == rule.pack
                        && fact.origin.rule == rule.id
                        && fact.position.line == at.line
                        && matches!(fact.role, NamedBindingRole::Reference)
                        && matches!(&fact.key, StaticString::Literal { value } if value == key)
                })
            });
            if !proven {
                return Some(Symbol::Unknown);
            }
            let target = self.named_target(owner, domain, key, None);
            if member.is_empty() {
                return Some(target);
            }
            let Symbol::Type(TypeTarget::Local(object)) = target else {
                return Some(target);
            };
            let Some(ValueExpr::Object { members, .. }) = self
                .value_flow(object)
                .and_then(|flow| flow.return_value.as_ref())
            else {
                return Some(Symbol::Unknown);
            };
            let matching: Vec<_> = members.iter().filter(|item| item.name == member).collect();
            let [item] = matching.as_slice() else {
                return Some(Symbol::Unknown);
            };
            let nodes = self
                .interner
                .lookup_path(&object.path)
                .and_then(|path| self.named.positions.get(&(path, item.position)))
                .map_or(&[][..], Vec::as_slice);
            let matching: Vec<_> = nodes
                .iter()
                .copied()
                .filter(|node| node.name == member)
                .collect();
            return Some(match matching.as_slice() {
                [node] => Symbol::Callable(node),
                [] => Symbol::Unknown,
                _ => Symbol::Ambiguous,
            });
        }
        None
    }

    pub(super) fn with_python(
        mut self,
        members: &'a super::receivers::PythonReceivers<'a>,
    ) -> Self {
        self.python = Some(members);
        self
    }

    #[cfg(feature = "lang-rust")]
    pub(super) fn with_rust(
        mut self,
        members: &'a languages::rust::linker::RustMembers<'a>,
    ) -> Self {
        self.rust = Some(members);
        self
    }

    #[cfg(feature = "lang-typescript")]
    pub(super) fn with_typescript(
        mut self,
        members: &'a languages::typescript::linker::TypeScriptMembers<'a>,
    ) -> Self {
        self.typescript = Some(members);
        self
    }

    fn classify(&self, nodes: impl Iterator<Item = &'a Node>, role: SymbolRole) -> Symbol<'a> {
        let mut candidates: Vec<&'a Node> = nodes
            .filter(|node| {
                node.language != "javascript"
                    || node.details["prototype"] != true
                    || node.details["prototype_valid"] == true
            })
            .filter(|node| match role {
                SymbolRole::Type => matches!(
                    node.kind.as_str(),
                    "class" | "struct" | "enum" | "type" | "type_alias" | "interface" | "trait"
                ),
                SymbolRole::Constructor => {
                    matches!(node.kind.as_str(), "class" | "struct" | "enum")
                        || (node.language == "javascript"
                            && node.kind == "function"
                            && node.details["prototype_constructor"] == true)
                }
                SymbolRole::Callable => matches!(
                    node.kind.as_str(),
                    "function" | "method" | "class" | "struct" | "enum"
                ),
            })
            .collect();
        crate::engine::linker::traits::filter_stubs_and_overloads(&mut candidates);
        if candidates.is_empty() {
            return Symbol::Unknown;
        }
        if candidates.len() > 1 {
            return Symbol::Ambiguous;
        }
        let node = candidates[0];
        if matches!(node.kind.as_str(), "function" | "method")
            && !matches!(role, SymbolRole::Constructor)
        {
            Symbol::Callable(node)
        } else {
            Symbol::Type(TypeTarget::Local(node))
        }
    }

    pub(in crate::engine::linker) fn nodes_in_path<'s>(
        &'s self,
        qualified: &str,
        path: &'s str,
    ) -> impl Iterator<Item = &'a Node> + 's {
        self.by_qual
            .get(qualified)
            .into_iter()
            .flatten()
            .copied()
            .filter(move |node| node.path == path)
    }

    pub(in crate::engine::linker) fn imported_alias(
        &self,
        node: &Node,
        alias: &str,
    ) -> Option<&Imported<'a>> {
        self.symbol_id(&node.id)
            .and_then(|owner| self.imports.get(&owner))
            .and_then(|aliases| self.symbol_id(alias).and_then(|alias| aliases.get(&alias)))
    }

    fn member(
        &self,
        receiver: &'a Node,
        member: &str,
        associated: bool,
        caller: &'a Node,
    ) -> Symbol<'a> {
        #[cfg(feature = "lang-typescript")]
        if matches!(
            receiver.language.as_str(),
            "typescript" | "javascript" | "vue"
        ) {
            if let Some(index) = self.typescript {
                return self.classify(
                    index.lookup(receiver, member, associated).iter().copied(),
                    SymbolRole::Callable,
                );
            }
        }
        if receiver.language == "python" && receiver.kind == "class" {
            return match self
                .python
                .map(|index| index.lookup(receiver, member, false))
            {
                Some(super::receivers::Member::Local(node)) => {
                    self.classify(std::iter::once(node), SymbolRole::Callable)
                }
                Some(super::receivers::Member::Ambiguous) => Symbol::Ambiguous,
                _ => Symbol::Unknown,
            };
        }
        #[cfg(feature = "lang-rust")]
        if receiver.language == "rust" {
            return self.rust.map_or(Symbol::Unknown, |index| {
                self.classify(
                    index
                        .lookup(receiver, member, associated, caller)
                        .iter()
                        .copied(),
                    SymbolRole::Callable,
                )
            });
        }
        let _ = (associated, caller);
        let mut lookup_key = String::with_capacity(128);
        self.classify(
            self.by_qual
                .get(super::qualified(
                    &mut lookup_key,
                    &receiver.qualname,
                    member,
                ))
                .into_iter()
                .flatten()
                .copied()
                .filter(|node| node.path == receiver.path),
            SymbolRole::Callable,
        )
    }

    pub(super) fn lookup_member<'s>(
        &'s self,
        receiver: &'s TypeTarget<'a>,
        member: &str,
        caller: &'a Node,
    ) -> Member<'s> {
        self.lookup_member_mode(receiver, member, caller, false)
    }

    pub(super) fn lookup_property<'s>(
        &'s self,
        receiver: &'s TypeTarget<'a>,
        member: &str,
        caller: &'a Node,
    ) -> Member<'s> {
        match receiver {
            TypeTarget::ConfiguredReceiver {
                authority,
                origin,
                fallback,
                evidence,
            } => {
                if let super::value_flow::ConfiguredReceiverAuthority::Nominal {
                    receiver, ..
                } = authority.as_ref()
                {
                    if receiver.rule.properties.contains_key(member) {
                        return Member::ConfiguredExternal {
                            authority,
                            origin,
                            evidence,
                        };
                    }
                }
                fallback.as_deref().map_or(Member::Unknown, |fallback| {
                    self.lookup_property(fallback, member, caller)
                })
            }
            TypeTarget::Builtin(receiver)
                if self
                    .property_type_for_path(&caller.path, receiver, member)
                    .is_some() =>
            {
                Member::Builtin
            }
            TypeTarget::Local(_) => self.lookup_component_template_field(receiver, member),
            _ => Member::Unknown,
        }
    }

    #[cfg(feature = "lang-rust")]
    pub(super) fn dynamic_dispatch<'s>(
        &'s self,
        receiver: &'s Node,
        member: &'s str,
    ) -> Option<(&'a Node, &'s [&'a Node])> {
        self.rust?.dynamic_dispatch(receiver, member)
    }

    pub(super) fn lookup_member_mode<'s>(
        &'s self,
        receiver: &'s TypeTarget<'a>,
        member: &str,
        caller: &'a Node,
        associated: bool,
    ) -> Member<'s> {
        #[cfg(feature = "lang-rust")]
        if caller.language == "rust" {
            if let Some(target) = super::rust::semantic_context::reference_target(receiver) {
                return self.lookup_member_mode(target, member, caller, associated);
            }
        }
        match receiver {
            TypeTarget::ConfiguredReceiver {
                authority,
                origin,
                fallback,
                evidence,
            } => {
                let admitted = match authority.as_ref() {
                    super::value_flow::ConfiguredReceiverAuthority::Hierarchy { rule, .. } => {
                        rule.members.iter().any(|allowed| allowed == member)
                    }
                    super::value_flow::ConfiguredReceiverAuthority::Nominal {
                        receiver, ..
                    } => receiver
                        .rule
                        .members
                        .iter()
                        .any(|allowed| allowed == member),
                };
                if admitted {
                    Member::ConfiguredExternal {
                        authority,
                        origin,
                        evidence,
                    }
                } else if let Some(fallback) = fallback {
                    self.lookup_member_mode(fallback, member, caller, associated)
                } else {
                    Member::Unknown
                }
            }
            TypeTarget::Local(receiver)
                if receiver.language == "python" && receiver.kind == "class" =>
            {
                match self
                    .python
                    .map(|index| index.lookup(receiver, member, false))
                {
                    Some(super::receivers::Member::Local(node)) => Member::Local(node),
                    Some(super::receivers::Member::External(module, line, provider, _)) => {
                        Member::External(module, line, provider)
                    }
                    Some(super::receivers::Member::Ambiguous) => Member::Ambiguous,
                    _ => Member::Unknown,
                }
            }
            #[cfg(feature = "lang-rust")]
            TypeTarget::RustCollection { .. } if caller.language == "rust" => {
                if !associated
                    && languages::by_id("rust")
                        .is_some_and(|profile| profile.builtin_member("Vec", member))
                {
                    Member::Builtin
                } else {
                    Member::Unknown
                }
            }
            #[cfg(feature = "lang-rust")]
            TypeTarget::RustIterator { item } if caller.language == "rust" => {
                let standard = !associated
                    && languages::by_id("rust")
                        .is_some_and(|profile| profile.builtin_member("Iterator", member));
                let has_reference_item = matches!(
                    item.as_ref(),
                    TypeTarget::RustReference { mutable: false, .. }
                );
                if standard && (member == "next" || has_reference_item) {
                    Member::Builtin
                } else {
                    Member::Unknown
                }
            }
            #[cfg(feature = "lang-rust")]
            TypeTarget::RustSlice { .. } if caller.language == "rust" => {
                if !associated
                    && languages::by_id("rust")
                        .is_some_and(|profile| profile.builtin_member("Slice", member))
                {
                    Member::Builtin
                } else {
                    Member::Unknown
                }
            }
            #[cfg(feature = "lang-rust")]
            TypeTarget::RustWrapper { wrapper, ok, .. } if caller.language == "rust" => {
                let name = match wrapper {
                    super::value_flow::RustWrapperKind::Option => "Option",
                    super::value_flow::RustWrapperKind::Result => "Result",
                };
                let standard = languages::by_id("rust")
                    .is_some_and(|profile| profile.builtin_member(name, member));
                let adapter_shape = !matches!(member, "cloned" | "copied")
                    || matches!(
                        ok.as_ref(),
                        TypeTarget::RustReference { mutable: false, .. }
                    );
                if !associated && standard && adapter_shape {
                    Member::Builtin
                } else {
                    Member::Unknown
                }
            }
            #[cfg(feature = "lang-rust")]
            TypeTarget::RustDeref {
                wrapper, target, ..
            } if caller.language == "rust" => {
                if associated {
                    return Member::Unknown;
                }
                if super::rust::semantic_context::wrapper_value_member(wrapper, member) {
                    return Member::Builtin;
                }
                match target.as_ref() {
                    TypeTarget::External { module, .. } => {
                        if super::rust::semantic_context::standard_member(module, member) {
                            Member::Builtin
                        } else {
                            Member::Unknown
                        }
                    }
                    _ => self.lookup_member_mode(target, member, caller, associated),
                }
            }
            #[cfg(feature = "lang-rust")]
            TypeTarget::Local(receiver) if receiver.language == "rust" => {
                match self
                    .rust
                    .map(|index| index.lookup(receiver, member, associated, caller))
                {
                    Some([node]) => Member::Local(node),
                    Some(nodes) if nodes.len() > 1 => Member::Ambiguous,
                    _ => Member::Unknown,
                }
            }
            TypeTarget::Local(receiver) => {
                #[cfg(feature = "lang-typescript")]
                if matches!(
                    receiver.language.as_str(),
                    "typescript" | "javascript" | "vue"
                ) {
                    if let Some(index) = self.typescript {
                        return match index.lookup(receiver, member, associated) {
                            [node] => Member::Local(node),
                            nodes if nodes.len() > 1 => Member::Ambiguous,
                            _ => Member::Unknown,
                        };
                    }
                }
                match self
                    .interner
                    .lookup_symbol(&receiver.id)
                    .zip(self.interner.lookup_symbol(member))
                    .and_then(|(receiver, member)| {
                        self.members.get(&(receiver, member, associated))
                    })
                    .map(Vec::as_slice)
                {
                    Some([node]) => Member::Local(node),
                    Some(nodes) if nodes.len() > 1 => {
                        let non_stubs: Vec<&Node> = nodes
                            .iter()
                            .copied()
                            .filter(|n| {
                                let is_stub = n
                                    .details
                                    .get("is_stub")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                let is_overload = n
                                    .details
                                    .get("is_overload")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                !is_stub && !is_overload
                            })
                            .collect();
                        if non_stubs.len() == 1 {
                            Member::Local(non_stubs[0])
                        } else {
                            Member::Ambiguous
                        }
                    }
                    _ => Member::Unknown,
                }
            }
            TypeTarget::Object(members) => members
                .get(member)
                .map_or(Member::Unknown, |node| Member::Local(node)),
            TypeTarget::External {
                module,
                import_line,
            } => {
                if let Some(receiver) = self.receiver_type(&caller.path, module) {
                    if receiver.members.iter().any(|allowed| allowed == member) {
                        Member::External(module, *import_line, Some(module))
                    } else {
                        Member::Unknown
                    }
                } else if let Some(outcome) =
                    super::python::semantic_context::external_member(caller, module, member)
                {
                    match outcome {
                        super::python::semantic_context::ExternalMemberOutcome::Supported(
                            origin,
                        ) => Member::External(module, *import_line, Some(origin)),
                        super::python::semantic_context::ExternalMemberOutcome::Unsupported => {
                            Member::Unknown
                        }
                    }
                } else {
                    Member::External(module, *import_line, None)
                }
            }
            TypeTarget::Builtin(receiver) => {
                if self
                    .receiver_type(&caller.path, receiver)
                    .is_some_and(|receiver| {
                        receiver.members.iter().any(|allowed| allowed == member)
                    })
                    || languages::by_id(&caller.language)
                        .is_some_and(|profile| profile.builtin_member(receiver, member))
                {
                    Member::Builtin
                } else {
                    Member::Unknown
                }
            }
            TypeTarget::Ambiguous => Member::Ambiguous,
            _ => Member::Unknown,
        }
    }

    #[cfg(feature = "lang-typescript")]
    pub(super) fn lookup_component_template_field<'s>(
        &'s self,
        receiver: &'s TypeTarget<'a>,
        member: &str,
    ) -> Member<'s> {
        let TypeTarget::Local(receiver) = receiver else {
            return Member::Unknown;
        };
        if !matches!(
            receiver.language.as_str(),
            "typescript" | "javascript" | "vue"
        ) || !matches!(receiver.kind.as_str(), "class" | "interface")
        {
            return Member::Unknown;
        }
        match self
            .typescript
            .map(|index| index.lookup_field(receiver, member))
        {
            Some([field]) => Member::Local(field),
            Some(fields) if fields.len() > 1 => Member::Ambiguous,
            _ => Member::Unknown,
        }
    }

    #[cfg(not(feature = "lang-typescript"))]
    pub(super) fn lookup_component_template_field<'s>(
        &'s self,
        _receiver: &'s TypeTarget<'a>,
        _member: &str,
    ) -> Member<'s> {
        Member::Unknown
    }
}
