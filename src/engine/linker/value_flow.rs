use super::interner::{LinkerInterner, SymbolId};
use crate::core::{
    models::{Facts, Node, ReceiverHint, Reference},
    semantic::{SourcePosition, TypeExpr, ValueExpr, ValueFlowFacts},
};
use hashbrown::HashMap;
use rayon::prelude::*;
use serde::Deserialize;
use std::{borrow::Cow, cell::RefCell, collections::BTreeMap, sync::Arc};

mod initialize;

type Definitions<'a> = HashMap<(&'a str, SourcePosition), Vec<&'a Node>>;
type NamedNodes<'a> = HashMap<(&'a str, &'a str), Vec<&'a Node>>;
type CollectionElements<'a> =
    HashMap<&'a str, HashMap<String, Vec<(SourcePosition, TypeTarget<'a>)>>>;

#[derive(Clone, Debug, PartialEq, Eq)]
/// Prepared, scoped authority for a finite external call result.
pub struct ConfiguredCallResult {
    /// Persisted configuration and rule provenance.
    pub evidence: String,
    /// Exact configured callee.
    pub callee: SymbolId,
    /// Finite callable members.
    pub members: Vec<SymbolId>,
}

/// Opaque nominal authority payload; manifest implementation types stay internal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfiguredNominalReceiver {
    /// Validated effective nominal rule, visible only to the canonical shared consumer.
    pub(crate) rule: crate::engine::languages::manifests::syntax::ReceiverTypeRule,
}

/// Effective finite receiver authority, retaining the originating manifest row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfiguredReceiverAuthority {
    /// Python hierarchy row, selected after lexical and C3 proof.
    Hierarchy {
        /// Originating effective framework pack.
        pack: String,
        /// Exact surviving ordered-base hierarchy row.
        rule: Arc<crate::engine::languages::manifests::ClassHierarchyReceiverRule>,
    },
    /// Exact nominal receiver operation.
    Nominal {
        /// Originating effective framework pack.
        pack: String,
        /// Exact surviving syntax operation identity.
        rule_id: String,
        /// Opaque prepared nominal rule payload.
        receiver: Arc<ConfiguredNominalReceiver>,
    },
}

pub(crate) const MAX_CANDIDATE_BRANCH_JOINS: usize = 8;

fn normalize_dotted_expression(expression: &str) -> Cow<'_, str> {
    let expression = expression.trim();
    if !expression.contains('.') {
        return Cow::Borrowed(expression);
    }
    if expression.split('.').all(|part| part.trim() == part) {
        return Cow::Borrowed(expression);
    }
    let mut normalized = String::with_capacity(expression.len());
    for (index, part) in expression.split('.').enumerate() {
        if index > 0 {
            normalized.push('.');
        }
        normalized.push_str(part.trim());
    }
    Cow::Owned(normalized)
}

struct ComputedKey<'a> {
    source: &'a str,
    position: SourcePosition,
    expression: &'a str,
    kind: &'a str,
}
impl std::hash::Hash for ComputedKey<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(
            &(self.source, self.position, self.expression, self.kind),
            state,
        );
    }
}
impl hashbrown::Equivalent<(&str, SourcePosition, &str, &str)> for ComputedKey<'_> {
    fn equivalent(&self, key: &(&str, SourcePosition, &str, &str)) -> bool {
        self.source == key.0
            && self.position == key.1
            && self.expression == key.2
            && self.kind == key.3
    }
}

#[derive(Clone, Debug)]
/// Enumerates the supported type target values.
pub enum TypeTarget<'a> {
    /// Finite rule result; missing members never acquire unrestricted external authority.
    ConfiguredReceiver {
        /// Effective row identity.
        authority: Arc<ConfiguredReceiverAuthority>,
        /// Original proven receiver/import provenance.
        origin: Box<TypeTarget<'a>>,
        /// Independently established platform fallback only.
        fallback: Option<Box<TypeTarget<'a>>>,
        /// Effective configuration identity.
        evidence: String,
    },
    /// Represents the local case.
    Local(&'a Node),
    /// Represents the callable case.
    Callable(&'a Node),
    /// Type supplied by a dependency outside the indexed workspace.
    External {
        /// Import module that supplies the type.
        module: String,
        /// Source line of the import declaration.
        import_line: usize,
    },
    /// Keep the Rust wrapper identity and bounded pointee result for member lookup.
    RustDeref {
        /// Name of the verified standard wrapper type.
        wrapper: &'static str,
        /// Type of the value reached through Rust's Deref contract.
        target: Box<TypeTarget<'a>>,
    },
    /// Standard Rust Option or Result retaining its generic arguments.
    RustWrapper {
        /// Standard wrapper constructor.
        wrapper: RustWrapperKind,
        /// Successful payload type.
        ok: Box<TypeTarget<'a>>,
        /// Error payload for Result; absent for Option.
        err: Option<Box<TypeTarget<'a>>>,
    },
    /// Rust reference retaining mutability for adapter and iterator resolution.
    RustReference {
        /// Whether this reference is mutable.
        mutable: bool,
        /// Referenced type.
        target: Box<TypeTarget<'a>>,
    },
    /// A proven standard iterator with its yielded item type.
    RustIterator {
        /// Iterator item type.
        item: Box<TypeTarget<'a>>,
    },
    /// Standard Rust vector retaining its element type.
    RustCollection {
        /// Element type.
        item: Box<TypeTarget<'a>>,
    },
    /// Rust slice retaining its element type.
    RustSlice {
        /// Element type.
        item: Box<TypeTarget<'a>>,
    },
    /// Represents the builtin case.
    Builtin(String),
    /// Represents the object case.
    Object(Arc<HashMap<String, &'a Node>>),
    /// A finite external result object, never a local declaration.
    ResultObject(Arc<ConfiguredCallResult>),
    /// A callable projected from a configured result, retaining its establishment.
    ExternalCallable {
        /// Immutable configuration authority.
        result: Arc<ConfiguredCallResult>,
        /// Lexical scope where the projection was established.
        owner: String,
        /// Completed lexical establishment position.
        position: SourcePosition,
        /// Exact public member.
        member: String,
    },
    /// Finite local alternatives from a complete or partially unknown branch join.
    Candidates {
        /// Stable, declaration-identity-deduplicated local type declarations.
        targets: Vec<&'a Node>,
        /// Whether every local control-flow alternative was proven.
        complete: bool,
        /// Whether more than the supported candidate bound was observed.
        exhausted: bool,
    },
    /// Represents the ambiguous case.
    Ambiguous,
    /// Represents the unknown case.
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Enumerates standard Rust result wrapper constructors.
pub enum RustWrapperKind {
    /// The `Option<T>` wrapper.
    Option,
    /// The `Result<T, E>` wrapper.
    Result,
}

#[derive(Clone, Copy, Debug)]
/// Enumerates the supported symbol role values.
pub enum SymbolRole {
    /// Represents the type case.
    Type,
    /// Represents the constructor case.
    Constructor,
    /// Represents the callable case.
    Callable,
}

/// Enumerates the supported symbol values.
pub enum Symbol<'a> {
    /// Represents the type case.
    Type(TypeTarget<'a>),
    /// Represents the callable case.
    Callable(&'a Node),
    /// Represents the ambiguous case.
    Ambiguous,
    /// Represents the unknown case.
    Unknown,
}

/// Defines semantic resolver behavior.
pub trait SemanticResolver<'a> {
    /// Returns the statically proven nominal target of a Rust dereference implementation.
    fn deref_target(&self, _receiver: &'a Node) -> Option<&'a Node> {
        None
    }
    /// Result policy of an already-proven external member.
    fn resolve_member_result(
        &self,
        _receiver: &TypeTarget<'a>,
        _member: &str,
        _scope: &'a Node,
        _at: SourcePosition,
    ) -> Option<TypeTarget<'a>> {
        None
    }
    /// Finite literal-factory selection after generic result proof.
    fn resolve_call_result(
        &self,
        _scope: &'a Node,
        _callee: &str,
        _site: Option<&crate::core::semantic::CallSiteEvidence>,
        _at: SourcePosition,
        _fallback: Option<TypeTarget<'a>>,
    ) -> Option<TypeTarget<'a>> {
        None
    }
    /// Resolves a configured callable result through its canonical binding provider.
    fn resolve_factory_result(
        &self,
        _scope: &'a Node,
        _callee: &str,
        _at: SourcePosition,
    ) -> Option<TypeTarget<'a>> {
        None
    }
    /// Checks a configured result member using the resolver's canonical symbol IDs.
    fn configured_result_member(&self, _result: &ConfiguredCallResult, _member: &str) -> bool {
        false
    }
    /// Resolves a bounded property projection from effective receiver data or platform semantics.
    fn property_type(&self, _scope: &'a Node, receiver: &str, member: &str) -> Option<String> {
        super::typescript::semantic_context::typed_dom_property(receiver, member).map(str::to_owned)
    }
    /// Performs value flow.
    fn value_flow(&self, _node: &'a Node) -> Option<&'a ValueFlowFacts> {
        None
    }
    /// Performs invalid value flow.
    fn invalid_value_flow(&self, _node: &'a Node) -> bool {
        false
    }
    /// Performs raw value flow.
    fn raw_value_flow(&self, _node: &'a Node) -> Option<&'a serde_json::Value> {
        None
    }
    /// Reports whether typed flows applies.
    fn has_typed_flows(&self) -> bool {
        false
    }
    /// Resolves symbol.
    fn resolve_symbol(
        &self,
        scope: &'a Node,
        name: &str,
        at: SourcePosition,
        role: SymbolRole,
    ) -> Symbol<'a>;
    /// Resolves external factory.
    fn resolve_external_factory(
        &self,
        _scope: &'a Node,
        _callee: &str,
        _at: SourcePosition,
    ) -> Option<TypeTarget<'a>> {
        None
    }
    /// Resolves member.
    fn resolve_member(
        &self,
        _receiver: &TypeTarget<'a>,
        _member: &str,
        _scope: &'a Node,
        _at: SourcePosition,
    ) -> Symbol<'a> {
        Symbol::Unknown
    }
    /// Resolves applied.
    fn resolve_applied(
        &self,
        _scope: &'a Node,
        _base: &str,
        _args: &[TypeExpr],
        _at: SourcePosition,
    ) -> TypeTarget<'a> {
        TypeTarget::Unknown
    }
    /// Resolves a payload only for an explicitly projected standard wrapper.
    fn resolve_payload(
        &self,
        _scope: &'a Node,
        _expression: &TypeExpr,
        _at: SourcePosition,
    ) -> TypeTarget<'a> {
        TypeTarget::Unknown
    }
    /// Resolves imported module.
    fn resolve_import_module(
        &self,
        _scope: &'a Node,
        _name: &str,
        _at: SourcePosition,
    ) -> Option<&'a Node> {
        None
    }
}

#[derive(Clone, Copy, Debug)]
/// Represents semantic limits data.
pub struct SemanticLimits {
    /// The facts per scope value.
    pub facts_per_scope: usize,
    /// The name bytes value.
    pub name_bytes: usize,
    /// The alias depth value.
    pub alias_depth: usize,
}

impl Default for SemanticLimits {
    fn default() -> Self {
        Self {
            facts_per_scope: crate::core::semantic::DEFAULT_FACTS_PER_SCOPE,
            name_bytes: 4_096,
            alias_depth: 64,
        }
    }
}

pub(in crate::engine::linker) struct Binding<'a> {
    position: SourcePosition,
    pub(in crate::engine::linker) target: TypeTarget<'a>,
    valid_until: Option<SourcePosition>,
    restores_previous: bool,
    pub(in crate::engine::linker) immutable_initializer: bool,
}

enum BindingList<'a> {
    One(Binding<'a>),
    Many(Vec<Binding<'a>>),
}

impl<'a> BindingList<'a> {
    fn push(&mut self, binding: Binding<'a>) {
        match self {
            Self::Many(bindings) => bindings.push(binding),
            Self::One(_) => {
                let Self::One(first) = std::mem::replace(self, Self::Many(Vec::new())) else {
                    unreachable!("single binding was replaced");
                };
                *self = Self::Many(vec![first, binding]);
            }
        }
    }
}

impl<'a> std::ops::Deref for BindingList<'a> {
    type Target = [Binding<'a>];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::One(binding) => std::slice::from_ref(binding),
            Self::Many(bindings) => bindings,
        }
    }
}

impl std::ops::DerefMut for BindingList<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::One(binding) => std::slice::from_mut(binding),
            Self::Many(bindings) => bindings,
        }
    }
}

#[derive(Clone, Copy)]
struct BindingEvent<'facts, 'arena> {
    // Decoded fallback values are temporary; names stay in the original facts arena.
    name: &'arena str,
    position: SourcePosition,
    value: &'facts ValueExpr,
    conditional: bool,
    immutable_initializer: bool,
}

struct Scope<'a> {
    parents: Vec<&'a str>,
    bindings: HashMap<&'a str, BindingList<'a>>,
    invalid: bool,
    module: bool,
}

/// Represents value flow index data.
pub struct ValueFlowIndex<'a> {
    scopes: HashMap<&'a str, Scope<'a>>,
    returns: HashMap<&'a str, TypeTarget<'a>>,
    async_returns: HashMap<&'a str, TypeTarget<'a>>,
    return_collection_elements: HashMap<&'a str, TypeTarget<'a>>,
    async_return_collection_elements: HashMap<&'a str, TypeTarget<'a>>,
    fields: HashMap<&'a str, HashMap<String, TypeTarget<'a>>>,
    component_slots: HashMap<&'a str, HashMap<(String, String), TypeTarget<'a>>>,
    local_types: HashMap<&'a str, TypeTarget<'a>>,
    computed: HashMap<(&'a str, SourcePosition, &'a str, &'a str), TypeTarget<'a>>,
    collection_elements: CollectionElements<'a>,
    unknown: TypeTarget<'a>,
    ambiguous: TypeTarget<'a>,
    limits: SemanticLimits,
}

enum ScopeFacts<'a> {
    Borrowed(&'a ValueFlowFacts),
    Decoded(Box<ValueFlowFacts>),
}

impl std::ops::Deref for ScopeFacts<'_> {
    type Target = ValueFlowFacts;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(facts) => facts,
            Self::Decoded(facts) => facts,
        }
    }
}

static EMPTY_SCOPE_FACTS: std::sync::LazyLock<ValueFlowFacts> =
    std::sync::LazyLock::new(ValueFlowFacts::default);

type InitializedScope<'a> = (&'a Node, ScopeFacts<'a>, Scope<'a>, TypeTarget<'a>);

#[derive(Default)]
struct Epochs<'a> {
    returns: HashMap<&'a str, u64>,
    scopes: HashMap<&'a str, u64>,
    fields: u64,
}

#[derive(Default)]
struct ReadEpochs<'a> {
    returns: Vec<(&'a str, u64)>,
    scopes: Vec<(&'a str, u64)>,
    fields: Option<u64>,
}

impl ReadEpochs<'_> {
    fn dirty(&self, epochs: &Epochs<'_>) -> bool {
        self.returns
            .iter()
            .any(|(id, epoch)| epochs.returns.get(id).copied().unwrap_or(0) != *epoch)
            || self
                .scopes
                .iter()
                .any(|(id, epoch)| epochs.scopes.get(id).copied().unwrap_or(0) != *epoch)
            || self.fields.is_some_and(|epoch| epoch != epochs.fields)
    }
}

pub(in crate::engine::linker) struct ReadTrace<'a, 'b> {
    epochs: &'b Epochs<'a>,
    reads: RefCell<ReadEpochs<'a>>,
}

impl<'a, 'b> ReadTrace<'a, 'b> {
    fn new(epochs: &'b Epochs<'a>, mut reads: ReadEpochs<'a>) -> Self {
        reads.returns.clear();
        reads.scopes.clear();
        reads.fields = None;
        Self {
            epochs,
            reads: RefCell::new(reads),
        }
    }

    fn returned(&self, id: &'a str) {
        let mut reads = self.reads.borrow_mut();
        if !reads.returns.iter().any(|(read, _)| *read == id) {
            reads
                .returns
                .push((id, self.epochs.returns.get(id).copied().unwrap_or(0)));
        }
    }

    fn scope(&self, id: &'a str) {
        let mut reads = self.reads.borrow_mut();
        if !reads.scopes.iter().any(|(read, _)| *read == id) {
            reads
                .scopes
                .push((id, self.epochs.scopes.get(id).copied().unwrap_or(0)));
        }
    }

    fn fields(&self) {
        self.reads.borrow_mut().fields = Some(self.epochs.fields);
    }
}

fn initialize_scope<'a>(
    node: &'a Node,
    by_name: &NamedNodes<'a>,
    resolver: &impl SemanticResolver<'a>,
    limits: SemanticLimits,
) -> InitializedScope<'a> {
    let encoded = &node.details["value_flow"];
    let typed = resolver.value_flow(node);
    let oversized = typed.is_some_and(|facts| {
        crate::core::typed_facts::scope_fact_count(facts) > limits.facts_per_scope
    }) || crate::core::typed_facts::encoded_scope_fact_count(encoded)
        > limits.facts_per_scope;
    let decoded = if oversized {
        Ok(ScopeFacts::Borrowed(&EMPTY_SCOPE_FACTS))
    } else if let Some(facts) = typed {
        Ok(ScopeFacts::Borrowed(facts))
    } else if resolver.has_typed_flows() || encoded.is_null() || oversized {
        Ok(ScopeFacts::Borrowed(&EMPTY_SCOPE_FACTS))
    } else {
        ValueFlowFacts::deserialize_for_scope(
            encoded,
            node.kind == "template_scope"
                && node.language == "javascript"
                && node.details["classic_global"] == true,
        )
        .map(|facts| ScopeFacts::Decoded(Box::new(facts)))
    };
    let (facts, mut invalid) = match decoded {
        Ok(facts) => (facts, false),
        Err(_) => (ScopeFacts::Borrowed(&EMPTY_SCOPE_FACTS), true),
    };
    invalid |=
        oversized || resolver.invalid_value_flow(node) || !valid_scope_facts(node, &facts, limits);
    let mut parents = Vec::new();
    let mut parent = node
        .qualname
        .rsplit_once('.')
        .map_or("", |(parent, _)| parent);
    while !parent.is_empty() {
        if let Some([owner]) = by_name
            .get(&(node.path.as_str(), parent))
            .map(Vec::as_slice)
        {
            if !(node.language == "python"
                && matches!(node.kind.as_str(), "function" | "method")
                && owner.kind == "class")
            {
                parents.push(owner.id.as_str());
            }
        }
        parent = parent.rsplit_once('.').map_or("", |(parent, _)| parent);
    }
    invalid |= parents.len() > limits.alias_depth;
    let mut bindings = HashMap::new();
    if matches!(
        node.kind.as_str(),
        "function" | "method" | "lambda" | "template_scope"
    ) {
        for name in node.binding_names() {
            if name.len() <= limits.name_bytes {
                bindings.insert(
                    name,
                    BindingList::One(Binding {
                        position: node_position(node),
                        target: TypeTarget::Unknown,
                        valid_until: None,
                        restores_previous: false,
                        immutable_initializer: false,
                    }),
                );
            }
        }
    }
    for (name, annotation) in node.parameter_types() {
        let position = node_position(node);
        let target = if invalid || name.len() > limits.name_bytes {
            TypeTarget::Unknown
        } else {
            annotation
                .filter(|name| !name.is_empty() && name.len() <= limits.name_bytes)
                .map_or(TypeTarget::Unknown, |name| {
                    resolve_type(
                        node,
                        &stored_type_expr(name, &node.language),
                        position,
                        resolver,
                        limits,
                    )
                })
        };
        bindings.insert(
            name,
            BindingList::One(Binding {
                position,
                target,
                valid_until: None,
                restores_previous: false,
                immutable_initializer: false,
            }),
        );
    }
    let returned = if invalid {
        TypeTarget::Unknown
    } else {
        facts
            .return_type
            .as_ref()
            .map_or(TypeTarget::Unknown, |ty| {
                resolve_type(node, ty, node_position(node), resolver, limits)
            })
    };
    (
        node,
        facts,
        Scope {
            parents,
            bindings,
            invalid,
            module: node.kind == "module",
        },
        returned,
    )
}

impl<'a> ValueFlowIndex<'a> {
    /// Performs build.
    pub fn build(all: &'a BTreeMap<String, Facts>, resolver: &impl SemanticResolver<'a>) -> Self {
        Self::build_with_limits(all, resolver, SemanticLimits::default())
    }

    /// Builds with limits.
    pub fn build_with_limits(
        all: &'a BTreeMap<String, Facts>,
        resolver: &impl SemanticResolver<'a>,
        limits: SemanticLimits,
    ) -> Self {
        let nodes: Vec<&Node> = all.values().flat_map(|facts| &facts.nodes).collect();
        let interner = LinkerInterner::from_catalog(all);
        let by_id: HashMap<SymbolId, &Node> = nodes
            .iter()
            .map(|node| {
                (
                    interner
                        .lookup_symbol(&node.id)
                        .expect("catalog node id is interned"),
                    *node,
                )
            })
            .collect();
        Self::build_initialized(
            all,
            nodes,
            &interner,
            &by_id,
            resolver,
            limits,
            |nodes, names| {
                nodes
                    .iter()
                    .map(|&node| initialize_scope(node, names, resolver, limits))
                    .collect()
            },
        )
    }

    pub(crate) fn build_parallel<F: AsRef<Facts>>(
        all: &'a BTreeMap<String, F>,
        nodes: Vec<&'a Node>,
        interner: &LinkerInterner<'a>,
        by_id: &HashMap<SymbolId, &'a Node>,
        resolver: &(impl SemanticResolver<'a> + Sync),
    ) -> Self {
        let limits = SemanticLimits::default();
        Self::build_initialized(
            all,
            nodes,
            interner,
            by_id,
            resolver,
            limits,
            |nodes, names| {
                if nodes.len() >= 8192 {
                    nodes
                        .par_iter()
                        .map(|&node| initialize_scope(node, names, resolver, limits))
                        .collect()
                } else {
                    nodes
                        .iter()
                        .map(|&node| initialize_scope(node, names, resolver, limits))
                        .collect()
                }
            },
        )
    }

    /// Performs lookup.
    pub fn lookup(&self, owner: &Node, name: &str, at: SourcePosition) -> Option<&TypeTarget<'a>> {
        self.lookup_tracked(owner, name, at, None)
    }

    pub(in crate::engine::linker) fn lookup_tracked(
        &self,
        owner: &Node,
        name: &str,
        at: SourcePosition,
        trace: Option<&ReadTrace<'a, '_>>,
    ) -> Option<&TypeTarget<'a>> {
        let (id, scope) = self.scopes.get_key_value(owner.id.as_str())?;
        for (is_parent, id) in
            std::iter::once((false, *id)).chain(scope.parents.iter().map(|id| (true, *id)))
        {
            let scope = &self.scopes[id];
            if scope.invalid {
                return Some(&self.unknown);
            }
            if let Some(writes) = scope.bindings.get(name) {
                let setup_complete =
                    is_parent && super::vue::value_flow::setup_scope_complete(owner, scope.module);
                let immutable_capture = is_parent
                    && matches!(owner.language.as_str(), "javascript" | "typescript")
                    && super::javascript::value_flow::stable_immutable_capture(writes);
                if is_parent && writes.len() > 1 && !setup_complete && !immutable_capture {
                    return Some(&self.unknown);
                }
                if let Some(trace) = trace {
                    trace.scope(id);
                }
                let position = if is_parent && setup_complete {
                    SourcePosition {
                        line: usize::MAX,
                        column: usize::MAX,
                    }
                } else {
                    at
                };
                return Some(reaching(writes, position).unwrap_or(&self.unknown));
            }
        }
        None
    }

    fn collection_element(
        &self,
        owner: &Node,
        iterable: &str,
        at: SourcePosition,
    ) -> TypeTarget<'a> {
        let Some(scope) = self.scopes.get(owner.id.as_str()) else {
            return TypeTarget::Unknown;
        };
        for (depth, id) in std::iter::once(&owner.id.as_str())
            .chain(scope.parents.iter())
            .enumerate()
        {
            let Some(candidate_scope) = self.scopes.get(*id) else {
                return TypeTarget::Unknown;
            };
            if candidate_scope.invalid {
                return TypeTarget::Unknown;
            }
            let Some(writes) = candidate_scope.bindings.get(iterable) else {
                continue;
            };
            if depth > 0 && writes.len() > 1 {
                return TypeTarget::Unknown;
            }
            let Some(last) = writes.iter().rev().find(|write| write.position < at) else {
                return TypeTarget::Unknown;
            };
            if !matches!(&last.target, TypeTarget::Builtin(name) if matches!(name.as_str(), "list" | "List" | "Sequence" | "Iterable"))
            {
                return TypeTarget::Unknown;
            }
            if let Some(element) = self
                .collection_elements
                .get(*id)
                .and_then(|names| names.get(iterable))
                .and_then(|elements| {
                    elements
                        .iter()
                        .rfind(|(position, _)| *position == last.position)
                })
                .map(|(_, element)| element.clone())
            {
                return element;
            }
        }
        TypeTarget::Unknown
    }

    /// Performs return type.
    pub fn return_type(&self, callable: &Node) -> Option<&TypeTarget<'a>> {
        self.return_type_tracked(callable, None)
    }

    fn return_type_tracked(
        &self,
        callable: &Node,
        trace: Option<&ReadTrace<'a, '_>>,
    ) -> Option<&TypeTarget<'a>> {
        self.returns
            .get_key_value(callable.id.as_str())
            .map(|(id, target)| {
                if let Some(trace) = trace {
                    trace.returned(id);
                }
                target
            })
    }

    /// Performs computed receiver.
    pub fn computed_receiver(&self, reference: &Reference) -> Option<&TypeTarget<'a>> {
        self.computed.get(&ComputedKey {
            expression: reference.expression.as_str(),
            kind: reference.kind.as_str(),
            source: reference.source.as_str(),
            position: reference.position(),
        })
    }

    /// Performs field type.
    pub fn field_type(&self, class: &Node, name: &str) -> Option<&TypeTarget<'a>> {
        self.field_type_tracked(class, name, None)
    }

    /// Returns a component slot prop type declared in the exact provider module.
    pub fn component_slot_type(
        &self,
        module: &Node,
        slot: &str,
        binding: &str,
    ) -> Option<&TypeTarget<'a>> {
        self.component_slots
            .get(module.id.as_str())
            .and_then(|slots| slots.get(&(slot.to_owned(), binding.to_owned())))
    }

    pub(in crate::engine::linker) fn field_type_tracked(
        &self,
        class: &Node,
        name: &str,
        trace: Option<&ReadTrace<'a, '_>>,
    ) -> Option<&TypeTarget<'a>> {
        if let Some(trace) = trace {
            trace.fields();
        }
        self.fields
            .get(class.id.as_str())
            .and_then(|fields| fields.get(name))
    }

    /// Performs object member.
    pub fn object_member(target: &TypeTarget<'a>, name: &str) -> Option<&'a Node> {
        match target {
            TypeTarget::Object(members) => members.get(name).copied(),
            _ => None,
        }
    }

    /// Reads only an already reduced, lexically established external callable.
    pub fn configured_callable(
        &self,
        owner: &'a Node,
        expression: &str,
        at: SourcePosition,
    ) -> Option<String> {
        let TypeTarget::ExternalCallable {
            result,
            owner: established,
            position,
            member,
        } = self.lookup(owner, expression, at)?
        else {
            return None;
        };
        Some(format!(
            "{}; callable:{}; binding:{}:{}:{}",
            result.evidence, member, established, position.line, position.column
        ))
    }

    /// Resolves value.
    pub fn resolve_value(
        &self,
        owner: &'a Node,
        value: &ValueExpr,
        at: SourcePosition,
        resolver: &impl SemanticResolver<'a>,
    ) -> TypeTarget<'a> {
        self.evaluate(owner, value, at, resolver, None)
    }

    /// Performs type of expression.
    pub fn type_of_expression(
        &self,
        owner: &'a Node,
        expression: &str,
        at: SourcePosition,
        resolver: &impl SemanticResolver<'a>,
    ) -> Option<&TypeTarget<'a>> {
        self.type_of_expression_tracked(owner, expression, at, resolver, None)
    }

    pub(in crate::engine::linker) fn type_of_expression_tracked(
        &self,
        owner: &'a Node,
        expression: &str,
        at: SourcePosition,
        resolver: &impl SemanticResolver<'a>,
        trace: Option<&ReadTrace<'a, '_>>,
    ) -> Option<&TypeTarget<'a>> {
        let expression = normalize_dotted_expression(expression);
        let expression = expression.as_ref();
        if expression.len() > self.limits.name_bytes
            || expression.matches('.').count() > self.limits.alias_depth
        {
            return Some(&self.unknown);
        }
        if let Some(TypeTarget::Callable(callable)) =
            self.lookup_tracked(owner, expression, at, trace)
        {
            return self.return_type_tracked(callable, trace);
        }
        if let Some(constructor) =
            super::python::value_flow::constructor_symbol(owner, expression, at, resolver)
        {
            if let Some(target) = self.borrow_symbol(constructor, trace) {
                return Some(target);
            }
        }
        self.borrow_return_symbol(
            self.resolve_callable_symbol_tracked(owner, expression, at, resolver, trace),
            trace,
        )
    }

    fn callable_expression_tracked(
        &self,
        owner: &'a Node,
        expression: &str,
        at: SourcePosition,
        resolver: &impl SemanticResolver<'a>,
        trace: Option<&ReadTrace<'a, '_>>,
    ) -> Option<&'a Node> {
        let expression = normalize_dotted_expression(expression);
        let expression = expression.as_ref();
        if expression.len() > self.limits.name_bytes
            || expression.matches('.').count() > self.limits.alias_depth
        {
            return None;
        }
        match self.resolve_callable_symbol_tracked(owner, expression, at, resolver, trace) {
            Symbol::Callable(callable) => Some(callable),
            _ => None,
        }
    }

    fn resolve_callable_symbol_tracked(
        &self,
        owner: &'a Node,
        expression: &str,
        at: SourcePosition,
        resolver: &impl SemanticResolver<'a>,
        trace: Option<&ReadTrace<'a, '_>>,
    ) -> Symbol<'a> {
        if let Some(TypeTarget::Callable(callable)) =
            self.lookup_tracked(owner, expression, at, trace)
        {
            return Symbol::Callable(callable);
        }
        if let Some((receiver, member)) = expression.rsplit_once('.') {
            if self.lookup_tracked(owner, receiver, at, trace).is_none()
                && matches!(
                    resolver.resolve_symbol(owner, receiver, at, SymbolRole::Type),
                    Symbol::Type(TypeTarget::Local(node))
                        if matches!(node.kind.as_str(), "class" | "struct" | "interface" | "enum")
                )
            {
                return resolver.resolve_symbol(owner, expression, at, SymbolRole::Callable);
            }
            if let Some(target) = self.value_expression(owner, receiver, at, resolver, trace) {
                if let Some(method) = Self::object_member(target, member) {
                    return Symbol::Callable(method);
                }
                return resolver.resolve_member(target, member, owner, at);
            }
        }
        resolver.resolve_symbol(owner, expression, at, SymbolRole::Callable)
    }

    /// Performs value type.
    pub fn value_type(
        &self,
        owner: &'a Node,
        expression: &str,
        at: SourcePosition,
        resolver: &impl SemanticResolver<'a>,
    ) -> Option<&TypeTarget<'a>> {
        if expression.len() > self.limits.name_bytes
            || expression.matches('.').count() > self.limits.alias_depth
        {
            return Some(&self.unknown);
        }
        self.value_expression(owner, expression, at, resolver, None)
    }

    fn value_expression(
        &self,
        owner: &'a Node,
        expression: &str,
        at: SourcePosition,
        resolver: &impl SemanticResolver<'a>,
        trace: Option<&ReadTrace<'a, '_>>,
    ) -> Option<&TypeTarget<'a>> {
        if let Some(target) = self.lookup_tracked(owner, expression, at, trace) {
            return Some(target);
        }
        if let Some((receiver, member)) = expression.rsplit_once('.') {
            if let Some(class) = self
                .value_expression(owner, receiver, at, resolver, trace)
                .and_then(super::rust::value_flow::local_class)
            {
                return self.field_type_tracked(class, member, trace);
            }
        }
        self.borrow_symbol(
            resolver.resolve_symbol(owner, expression, at, SymbolRole::Type),
            trace,
        )
    }

    fn borrow_symbol(
        &self,
        symbol: Symbol<'a>,
        trace: Option<&ReadTrace<'a, '_>>,
    ) -> Option<&TypeTarget<'a>> {
        match symbol {
            Symbol::Callable(node) => self.return_type_tracked(node, trace),
            Symbol::Type(TypeTarget::Local(node)) => self.local_types.get(node.id.as_str()),
            Symbol::Ambiguous => Some(&self.ambiguous),
            Symbol::Type(TypeTarget::Ambiguous) => Some(&self.ambiguous),
            _ => None,
        }
    }

    fn borrow_return_symbol(
        &self,
        symbol: Symbol<'a>,
        trace: Option<&ReadTrace<'a, '_>>,
    ) -> Option<&TypeTarget<'a>> {
        match symbol {
            Symbol::Callable(node) => self.return_type_tracked(node, trace),
            Symbol::Ambiguous => Some(&self.ambiguous),
            _ => None,
        }
    }

    fn evaluate(
        &self,
        owner: &'a Node,
        value: &ValueExpr,
        at: SourcePosition,
        resolver: &impl SemanticResolver<'a>,
        definitions: Option<&Definitions<'a>>,
    ) -> TypeTarget<'a> {
        self.evaluate_tracked(owner, value, at, resolver, definitions, None)
    }

    fn evaluate_tracked(
        &self,
        owner: &'a Node,
        value: &ValueExpr,
        at: SourcePosition,
        resolver: &impl SemanticResolver<'a>,
        definitions: Option<&Definitions<'a>>,
        trace: Option<&ReadTrace<'a, '_>>,
    ) -> TypeTarget<'a> {
        match value {
            ValueExpr::Annotated { type_expr } => {
                resolve_type(owner, type_expr, at, resolver, self.limits)
            }
            ValueExpr::Construct { callee } => {
                if let Some(target) = super::python::value_flow::literal_type(owner, callee) {
                    return target;
                }
                if let Some(target) = super::javascript::value_flow::construct(owner, callee) {
                    return target;
                }
                symbol_type(resolver.resolve_symbol(owner, callee, at, SymbolRole::Constructor))
            }
            ValueExpr::Alias { name } => self
                .lookup_tracked(owner, name, at, trace)
                .cloned()
                .unwrap_or_else(|| {
                    match resolver.resolve_symbol(owner, name, at, SymbolRole::Callable) {
                        Symbol::Callable(node) => TypeTarget::Callable(node),
                        _ => TypeTarget::Unknown,
                    }
                }),
            ValueExpr::LoopElement { iterable, .. } => self.collection_element(owner, iterable, at),
            ValueExpr::Await { value } => {
                let ValueExpr::Call { callee, .. } = value.as_ref() else {
                    if matches!(owner.language.as_str(), "typescript" | "javascript" | "vue") {
                        return self.explicit_payload(owner, value, at, resolver, trace, 0);
                    }
                    return TypeTarget::Unknown;
                };
                let callable = self.callable_expression_tracked(owner, callee, at, resolver, trace);
                callable.map_or(TypeTarget::Unknown, |callable| {
                    self.async_returns
                        .get(callable.id.as_str())
                        .cloned()
                        .unwrap_or(TypeTarget::Unknown)
                })
            }
            ValueExpr::Scoped { value, .. } => {
                self.evaluate_tracked(owner, value, at, resolver, definitions, trace)
            }
            ValueExpr::BranchJoin { alternatives } => {
                let mut targets = BTreeMap::<String, &'a Node>::new();
                let mut complete = true;
                let mut exhausted = false;
                for alternative in alternatives {
                    let result =
                        self.evaluate_tracked(owner, alternative, at, resolver, definitions, trace);
                    match result {
                        TypeTarget::Local(node)
                            if matches!(
                                node.kind.as_str(),
                                "class" | "struct" | "interface" | "enum"
                            ) =>
                        {
                            targets.insert(node.id.clone(), node);
                        }
                        TypeTarget::Candidates {
                            targets: nested,
                            complete: nested_complete,
                            exhausted: nested_exhausted,
                        } => {
                            complete &= nested_complete;
                            exhausted |= nested_exhausted;
                            for node in nested {
                                targets.insert(node.id.clone(), node);
                            }
                        }
                        TypeTarget::Unknown | TypeTarget::Ambiguous => {
                            complete = false;
                        }
                        _ => {
                            complete = false;
                        }
                    }
                    if targets.len() > MAX_CANDIDATE_BRANCH_JOINS {
                        exhausted = true;
                        complete = false;
                        targets.clear();
                    }
                }
                if exhausted {
                    return TypeTarget::Candidates {
                        targets: Vec::new(),
                        complete: false,
                        exhausted: true,
                    };
                }
                if targets.is_empty() {
                    TypeTarget::Candidates {
                        targets: Vec::new(),
                        complete: false,
                        exhausted: false,
                    }
                } else {
                    TypeTarget::Candidates {
                        targets: targets.into_values().collect(),
                        complete,
                        exhausted: false,
                    }
                }
            }
            ValueExpr::Call { callee, site } => {
                let at = site.as_ref().map_or(at, |site| site.position);
                if let Some(receiver) = site.as_ref().and_then(|site| site.receiver.as_deref()) {
                    let receiver =
                        self.evaluate_tracked(owner, receiver, at, resolver, definitions, trace);
                    if let Symbol::Callable(callable) =
                        resolver.resolve_member(&receiver, callee, owner, at)
                    {
                        if let Some(result) = self.return_type(callable) {
                            return result.clone();
                        }
                    }
                    if let Some(target) =
                        resolver.resolve_member_result(&receiver, callee, owner, at)
                    {
                        return target;
                    }
                    if let Some(target) = super::javascript::value_flow::platform_member_return(
                        owner, &receiver, callee,
                    ) {
                        return target;
                    }
                    return TypeTarget::Unknown;
                }
                if let Some(target) = resolver.resolve_factory_result(owner, callee, at) {
                    return target;
                }
                if let Some(target) =
                    super::python::value_flow::constructor_result(owner, callee, at, resolver)
                {
                    return target;
                } else if matches!(owner.language.as_str(), "javascript" | "typescript" | "vue") {
                    if self
                        .callable_expression_tracked(owner, callee, at, resolver, trace)
                        .is_some_and(|callable| {
                            self.async_returns.contains_key(callable.id.as_str())
                        })
                    {
                        // Retain the wrapper so aliases can await it without exposing the payload ordinarily.
                        return TypeTarget::Builtin("Promise".to_owned());
                    }
                    if let Some(target) = super::javascript::value_flow::evaluate_call(
                        self, owner, callee, at, resolver, trace,
                    ) {
                        return resolver
                            .resolve_call_result(
                                owner,
                                callee,
                                site.as_ref(),
                                at,
                                Some(target.clone()),
                            )
                            .unwrap_or(target);
                    }
                } else if let Some(target) = super::rust::value_flow::constructor_result(
                    self, owner, callee, at, resolver, trace,
                ) {
                    return target;
                }
                let existing = self
                    .type_of_expression_tracked(owner, callee, at, resolver, trace)
                    .cloned();
                if existing
                    .as_ref()
                    .is_some_and(|target| !matches!(target, TypeTarget::Unknown))
                {
                    return existing.unwrap();
                }
                if let Some(target) = super::python::value_flow::dotted_member_result(
                    self, owner, callee, at, resolver, trace,
                ) {
                    return target;
                }
                resolver
                    .resolve_call_result(owner, callee, site.as_ref(), at, None)
                    .unwrap_or(TypeTarget::Unknown)
            }
            ValueExpr::Field { receiver, member } => {
                let binding = self.lookup_tracked(owner, receiver, at, trace);
                let receiver_type = binding.cloned().unwrap_or_else(|| {
                    symbol_type(resolver.resolve_symbol(owner, receiver, at, SymbolRole::Type))
                });
                if binding.is_none()
                    && matches!(&receiver_type, TypeTarget::Local(node) if matches!(node.kind.as_str(), "class" | "struct" | "interface" | "enum"))
                {
                    return match resolver.resolve_symbol(
                        owner,
                        &format!("{receiver}.{member}"),
                        at,
                        SymbolRole::Callable,
                    ) {
                        Symbol::Callable(node) => TypeTarget::Callable(node),
                        _ => TypeTarget::Unknown,
                    };
                }
                if let Some(class) = super::rust::value_flow::local_class(&receiver_type) {
                    if let Some(target) = self.field_type_tracked(class, member, trace) {
                        return target.clone();
                    }
                }
                if let TypeTarget::Builtin(name) = &receiver_type {
                    if matches!(owner.language.as_str(), "javascript" | "typescript") {
                        if let Some(prop_type) = resolver.property_type(owner, name, member) {
                            return TypeTarget::Builtin(prop_type);
                        }
                    }
                }
                match resolver.resolve_member(&receiver_type, member, owner, at) {
                    Symbol::Type(target) => target,
                    Symbol::Callable(node) => TypeTarget::Callable(node),
                    _ => TypeTarget::Unknown,
                }
            }
            ValueExpr::Project { value, member } => {
                if owner.language == "rust" && super::rust::value_flow::is_adapter(member) {
                    let receiver =
                        self.evaluate_tracked(owner, value, at, resolver, definitions, trace);
                    if let Some(projected) =
                        super::rust::value_flow::project(&receiver, member, resolver)
                    {
                        if !matches!(projected, TypeTarget::Unknown) {
                            return projected;
                        }
                        if matches!(member.as_str(), "?" | "unwrap" | "expect")
                            && (matches!(&receiver, TypeTarget::Unknown)
                                || matches!(&receiver, TypeTarget::Builtin(name)
                                    if name == "Option" || name == "Result"))
                        {
                            return self.explicit_payload(owner, value, at, resolver, trace, 0);
                        }
                        return TypeTarget::Unknown;
                    }

                    // Prefer known Rust adapter semantics over a generic method signature:
                    // library signatures often erase the borrowed payload type.
                    if member != "?" {
                        if let Symbol::Callable(callable) =
                            resolver.resolve_member(&receiver, member, owner, at)
                        {
                            if let Some(trace) = trace {
                                trace.returned(callable.id.as_str());
                            }
                            return self
                                .returns
                                .get(callable.id.as_str())
                                .cloned()
                                .unwrap_or(TypeTarget::Unknown);
                        }
                    }
                    if matches!(member.as_str(), "?" | "unwrap" | "expect")
                        && (matches!(&receiver, TypeTarget::Unknown)
                            || matches!(&receiver, TypeTarget::Builtin(name)
                                if name == "Option" || name == "Result"))
                    {
                        return self.explicit_payload(owner, value, at, resolver, trace, 0);
                    }
                    return TypeTarget::Unknown;
                }
                let source = self.evaluate_tracked(owner, value, at, resolver, definitions, trace);
                match source {
                    TypeTarget::Builtin(ref name)
                        if matches!(owner.language.as_str(), "javascript" | "typescript") =>
                    {
                        resolver
                            .property_type(owner, name, member)
                            .map_or(TypeTarget::Unknown, TypeTarget::Builtin)
                    }
                    TypeTarget::ConfiguredReceiver { .. } => {
                        match resolver.resolve_member(&source, member, owner, at) {
                            Symbol::Type(target) => target,
                            _ => TypeTarget::Unknown,
                        }
                    }
                    TypeTarget::ResultObject(result)
                        if resolver.configured_result_member(&result, member) =>
                    {
                        TypeTarget::ExternalCallable {
                            result,
                            owner: owner.id.clone(),
                            position: at,
                            member: member.clone(),
                        }
                    }
                    TypeTarget::RustReference { .. } | TypeTarget::RustDeref { .. } => {
                        match resolver.resolve_member(&source, member, owner, at) {
                            Symbol::Type(target) => target,
                            Symbol::Callable(node) => TypeTarget::Callable(node),
                            Symbol::Ambiguous => TypeTarget::Ambiguous,
                            _ => TypeTarget::Unknown,
                        }
                    }
                    TypeTarget::Object(ref members) => {
                        members.get(member).map_or(TypeTarget::Unknown, |node| {
                            if matches!(node.kind.as_str(), "function" | "method") {
                                TypeTarget::Callable(node)
                            } else {
                                TypeTarget::Unknown
                            }
                        })
                    }
                    TypeTarget::Local(class) => self
                        .field_type_tracked(class, member, trace)
                        .cloned()
                        .unwrap_or_else(|| {
                            match resolver.resolve_member(
                                &TypeTarget::Local(class),
                                member,
                                owner,
                                at,
                            ) {
                                Symbol::Callable(node) => TypeTarget::Callable(node),
                                Symbol::Ambiguous => TypeTarget::Ambiguous,
                                _ => TypeTarget::Unknown,
                            }
                        }),
                    TypeTarget::Ambiguous => TypeTarget::Ambiguous,
                    _ => TypeTarget::Unknown,
                }
            }
            ValueExpr::Object {
                members,
                builtin_guard,
            } => {
                if *builtin_guard
                    && !matches!(
                        resolver.resolve_symbol(owner, "Object", at, SymbolRole::Type),
                        Symbol::Type(TypeTarget::Builtin(name)) if name == "Object"
                    )
                {
                    return TypeTarget::Unknown;
                }
                let Some(definitions) = definitions else {
                    return TypeTarget::Unknown;
                };
                let mut resolved = HashMap::new();
                for member in members {
                    let Some([node]) = definitions
                        .get(&(owner.path.as_str(), member.position))
                        .map(Vec::as_slice)
                    else {
                        return TypeTarget::Unknown;
                    };
                    if !matches!(node.kind.as_str(), "function" | "method" | "field")
                        || resolved.contains_key(&member.name)
                    {
                        return TypeTarget::Unknown;
                    }
                    resolved.insert(member.name.clone(), *node);
                }
                TypeTarget::Object(Arc::new(resolved))
            }
            ValueExpr::Unknown => TypeTarget::Unknown,
        }
    }

    fn explicit_payload(
        &self,
        owner: &'a Node,
        value: &ValueExpr,
        at: SourcePosition,
        resolver: &impl SemanticResolver<'a>,
        trace: Option<&ReadTrace<'a, '_>>,
        depth: usize,
    ) -> TypeTarget<'a> {
        if depth >= self.limits.alias_depth.min(8) {
            return TypeTarget::Unknown;
        }
        let flow = |node: &'a Node| {
            if self
                .scopes
                .get(node.id.as_str())
                .is_none_or(|scope| scope.invalid)
            {
                return None;
            }
            if let Some(facts) = resolver.value_flow(node) {
                Some(Cow::Borrowed(facts))
            } else if !resolver.has_typed_flows() {
                ValueFlowFacts::deserialize_for_scope(&node.details["value_flow"], false)
                    .ok()
                    .map(Cow::Owned)
            } else {
                None
            }
        };
        match value {
            ValueExpr::Annotated { type_expr } => resolver.resolve_payload(owner, type_expr, at),
            ValueExpr::Call { callee, .. } => {
                let Some(callable) =
                    self.callable_expression_tracked(owner, callee, at, resolver, trace)
                else {
                    return TypeTarget::Unknown;
                };
                if let Some(trace) = trace {
                    trace.returned(callable.id.as_str());
                }
                if matches!(owner.language.as_str(), "typescript" | "javascript" | "vue") {
                    return self
                        .async_returns
                        .get(callable.id.as_str())
                        .cloned()
                        .unwrap_or(TypeTarget::Unknown);
                }
                flow(callable)
                    .and_then(|facts| {
                        facts.return_type.as_ref().map(|ty| {
                            resolver.resolve_payload(callable, ty, node_position(callable))
                        })
                    })
                    .unwrap_or(TypeTarget::Unknown)
            }
            ValueExpr::Alias { name } => {
                // Reaching-value uncertainty still fences projection; a write cannot revive an old annotation.
                let reaching_wrapper = match self.lookup_tracked(owner, name, at, trace) {
                    Some(target) if owner.language == "rust" => {
                        matches!(target, TypeTarget::RustWrapper { .. })
                            || matches!(target, TypeTarget::Builtin(wrapper)
                                if matches!(wrapper.as_str(), "Option" | "Result"))
                    }
                    Some(TypeTarget::Builtin(wrapper))
                        if matches!(
                            owner.language.as_str(),
                            "typescript" | "javascript" | "vue"
                        ) =>
                    {
                        wrapper == "Promise"
                    }
                    _ => false,
                };
                if !reaching_wrapper {
                    return TypeTarget::Unknown;
                }
                let Some(scope) = self.scopes.get(owner.id.as_str()) else {
                    return TypeTarget::Unknown;
                };
                for id in std::iter::once(owner.id.as_str()).chain(scope.parents.iter().copied()) {
                    let Some(TypeTarget::Local(declaration)) = self.local_types.get(id) else {
                        continue;
                    };
                    let Some(facts) = flow(declaration) else {
                        return TypeTarget::Unknown;
                    };
                    if let Some(binding) = facts
                        .bindings
                        .iter()
                        .filter(|binding| binding.name == *name && binding.position < at)
                        .max_by_key(|binding| binding.position)
                    {
                        if binding.conditional {
                            return TypeTarget::Unknown;
                        }
                        return self.explicit_payload(
                            declaration,
                            &binding.value,
                            binding.position,
                            resolver,
                            trace,
                            depth + 1,
                        );
                    }
                    if self.scopes[id].bindings.contains_key(name.as_str()) {
                        return TypeTarget::Unknown;
                    }
                }
                TypeTarget::Unknown
            }
            ValueExpr::Project { value, member }
                if owner.language == "rust" && super::rust::value_flow::is_adapter(member) =>
            {
                let receiver = self.evaluate_tracked(owner, value, at, resolver, None, trace);
                let projected = super::rust::value_flow::project(&receiver, member, resolver)
                    .unwrap_or(TypeTarget::Unknown);
                super::rust::value_flow::unwrap_payload(&projected).unwrap_or(projected)
            }
            ValueExpr::Scoped { value, .. } => {
                self.explicit_payload(owner, value, at, resolver, trace, depth + 1)
            }
            _ => TypeTarget::Unknown,
        }
    }
}

pub(in crate::engine::linker) fn node_position(node: &Node) -> SourcePosition {
    SourcePosition {
        line: node.line,
        column: node.details["column"].as_u64().unwrap_or(0) as usize,
    }
}

fn reaching<'a, 'b>(writes: &'b [Binding<'a>], at: SourcePosition) -> Option<&'b TypeTarget<'a>> {
    if let [binding] = writes {
        return (binding.position < at && binding.valid_until.is_none_or(|until| at < until))
            .then_some(&binding.target);
    }
    let end = writes.partition_point(|binding| binding.position < at);
    for binding in writes[..end].iter().rev().take(32) {
        if binding.valid_until.is_none_or(|until| at < until) {
            return Some(&binding.target);
        }
        if !binding.restores_previous {
            return None;
        }
    }
    None
}

fn duplicate_position(writes: &[Binding<'_>], index: usize) -> bool {
    let position = writes[index].position;
    index
        .checked_sub(1)
        .is_some_and(|previous| writes[previous].position == position)
        || writes
            .get(index + 1)
            .is_some_and(|next| next.position == position)
}

pub(super) fn resolve_type<'a>(
    scope: &'a Node,
    expression: &TypeExpr,
    at: SourcePosition,
    resolver: &impl SemanticResolver<'a>,
    limits: SemanticLimits,
) -> TypeTarget<'a> {
    let mut scope = scope;
    let mut expression = Cow::Borrowed(expression);
    let mut at = at;
    let mut visited = Vec::new();
    for _ in 0..limits.alias_depth {
        if let TypeExpr::Applied { base, args } = &*expression {
            return if valid_type(&expression, limits, 0) {
                super::rust::value_flow::resolve_reference_expr(
                    scope, base, args, at, resolver, limits,
                )
                .unwrap_or_else(|| resolver.resolve_applied(scope, base, args, at))
            } else {
                TypeTarget::Unknown
            };
        }
        let TypeExpr::Named { name } = &*expression else {
            return TypeTarget::Unknown;
        };
        if name.is_empty() || name.len() > limits.name_bytes {
            return TypeTarget::Unknown;
        }
        let target = symbol_type(resolver.resolve_symbol(scope, name, at, SymbolRole::Type));
        let TypeTarget::Local(node) = &target else {
            return target;
        };
        if node.language == "python"
            && node.kind == "class"
            && super::python::value_flow::typed_dict_class(node, resolver)
        {
            return TypeTarget::Builtin("Mapping".to_owned());
        }
        let alias = if let Some(alias) = resolver
            .value_flow(node)
            .and_then(|facts| facts.alias_type.as_ref())
        {
            Cow::Borrowed(alias)
        } else {
            let encoded = match resolver.raw_value_flow(node) {
                Some(raw) => &raw["alias_type"],
                None if resolver.has_typed_flows() => return target,
                None => &node.details["value_flow"]["alias_type"],
            };
            if encoded.is_null() {
                return target;
            }
            let Ok(alias) = TypeExpr::deserialize(encoded) else {
                return TypeTarget::Unknown;
            };
            Cow::Owned(alias)
        };
        if visited.contains(&node.id.as_str()) {
            return TypeTarget::Unknown;
        }
        visited.push(node.id.as_str());
        expression = alias;
        scope = node;
        at = node_position(node);
    }
    TypeTarget::Unknown
}

fn stored_type_expr(annotation: &str, language: &str) -> TypeExpr {
    let annotation = annotation.trim();
    if language == "python" {
        if let Some(expression) = super::python::value_flow::nullable_type_expr(annotation) {
            return expression;
        }
    }
    if let Some((base, rest)) = annotation.split_once('[') {
        if let Some(argument) = rest.strip_suffix(']') {
            let base = base.trim();
            let argument = argument.trim();
            if matches!(base, "list" | "List" | "Sequence" | "Iterable")
                && !argument.is_empty()
                && !argument.contains(['[', ',', ']'])
            {
                return TypeExpr::Applied {
                    base: base.to_owned(),
                    args: vec![TypeExpr::Named {
                        name: argument.to_owned(),
                    }],
                };
            }
        }
    }
    TypeExpr::Named {
        name: annotation.to_owned(),
    }
}

pub(in crate::engine::linker) fn symbol_type(symbol: Symbol<'_>) -> TypeTarget<'_> {
    match symbol {
        Symbol::Type(target) => target,
        Symbol::Ambiguous => TypeTarget::Ambiguous,
        _ => TypeTarget::Unknown,
    }
}

fn field_owner<'a>(
    owner: &'a Node,
    names: &NamedNodes<'a>,
    resolver: &impl SemanticResolver<'a>,
) -> Option<&'a Node> {
    if matches!(owner.kind.as_str(), "class" | "struct" | "interface") {
        return Some(owner);
    }
    if super::vue::value_flow::owns_module_fields(owner) {
        return Some(owner);
    }
    let mut parent = owner
        .qualname
        .rsplit_once('.')
        .map_or("", |(parent, _)| parent);
    while !parent.is_empty() {
        if let Some([node]) = names.get(&(owner.path.as_str(), parent)).map(Vec::as_slice) {
            if matches!(node.kind.as_str(), "class" | "struct" | "interface") {
                return Some(*node);
            }
        }
        parent = parent.rsplit_once('.').map_or("", |(parent, _)| parent);
    }
    owner.details["receiver_type"].as_str().and_then(|name| {
        match resolver.resolve_symbol(owner, name, node_position(owner), SymbolRole::Type) {
            Symbol::Type(TypeTarget::Local(node)) => Some(node),
            _ => None,
        }
    })
}

fn merge<'a>(left: &TypeTarget<'a>, right: &TypeTarget<'a>) -> TypeTarget<'a> {
    if matches!(
        left,
        TypeTarget::ResultObject(_)
            | TypeTarget::ExternalCallable { .. }
            | TypeTarget::ConfiguredReceiver { .. }
            | TypeTarget::RustDeref { .. }
            | TypeTarget::RustReference { .. }
            | TypeTarget::RustWrapper { .. }
            | TypeTarget::RustIterator { .. }
            | TypeTarget::RustCollection { .. }
            | TypeTarget::RustSlice { .. }
    ) && same_target(left, right)
    {
        return left.clone();
    }
    match (left, right) {
        (TypeTarget::Local(a), TypeTarget::Local(b)) if a.id == b.id => left.clone(),
        (TypeTarget::Callable(a), TypeTarget::Callable(b)) if a.id == b.id => left.clone(),
        (
            TypeTarget::External {
                module: a,
                import_line: x,
            },
            TypeTarget::External {
                module: b,
                import_line: y,
            },
        ) if a == b && x == y => left.clone(),
        (TypeTarget::Builtin(a), TypeTarget::Builtin(b)) if a == b => left.clone(),
        (
            TypeTarget::Candidates {
                targets: a,
                complete: ac,
                exhausted: ae,
            },
            TypeTarget::Candidates {
                targets: b,
                complete: bc,
                exhausted: be,
            },
        ) if ac == bc
            && ae == be
            && a.len() == b.len()
            && a.iter().zip(b).all(|(left, right)| left.id == right.id) =>
        {
            left.clone()
        }
        (TypeTarget::Unknown, _) | (_, TypeTarget::Unknown) => TypeTarget::Unknown,
        _ => TypeTarget::Ambiguous,
    }
}

fn same_fields(
    left: &HashMap<&str, HashMap<String, TypeTarget<'_>>>,
    right: &HashMap<&str, HashMap<String, TypeTarget<'_>>>,
) -> bool {
    left.len() == right.len()
        && left.iter().all(|(id, fields)| {
            right.get(id).is_some_and(|other| {
                fields.len() == other.len()
                    && fields.iter().all(|(name, value)| {
                        other
                            .get(name)
                            .is_some_and(|other| same_target(value, other))
                    })
            })
        })
}

fn same_target(left: &TypeTarget<'_>, right: &TypeTarget<'_>) -> bool {
    match (left, right) {
        (
            TypeTarget::ConfiguredReceiver {
                evidence: a,
                authority: ar,
                origin: ao,
                fallback: af,
            },
            TypeTarget::ConfiguredReceiver {
                evidence: b,
                authority: br,
                origin: bo,
                fallback: bf,
            },
        ) => {
            a == b
                && ar == br
                && same_target(ao, bo)
                && match (af, bf) {
                    (None, None) => true,
                    (Some(a), Some(b)) => same_target(a, b),
                    _ => false,
                }
        }
        (TypeTarget::ResultObject(a), TypeTarget::ResultObject(b)) => a == b,
        (
            TypeTarget::ExternalCallable {
                result: a,
                owner: ao,
                position: ap,
                member: am,
            },
            TypeTarget::ExternalCallable {
                result: b,
                owner: bo,
                position: bp,
                member: bm,
            },
        ) => a == b && ao == bo && ap == bp && am == bm,
        (TypeTarget::Local(a), TypeTarget::Local(b))
        | (TypeTarget::Callable(a), TypeTarget::Callable(b)) => a.id == b.id,
        (
            TypeTarget::External {
                module: a,
                import_line: x,
            },
            TypeTarget::External {
                module: b,
                import_line: y,
            },
        ) => a == b && x == y,
        (TypeTarget::Builtin(a), TypeTarget::Builtin(b)) => a == b,
        (
            TypeTarget::RustDeref {
                wrapper: aw,
                target: at,
            },
            TypeTarget::RustDeref {
                wrapper: bw,
                target: bt,
            },
        ) => aw == bw && same_target(at, bt),
        (
            TypeTarget::RustReference {
                mutable: am,
                target: at,
            },
            TypeTarget::RustReference {
                mutable: bm,
                target: bt,
            },
        ) => am == bm && same_target(at, bt),
        (
            TypeTarget::RustWrapper {
                wrapper: aw,
                ok: ao,
                err: ae,
            },
            TypeTarget::RustWrapper {
                wrapper: bw,
                ok: bo,
                err: be,
            },
        ) => {
            aw == bw
                && same_target(ao, bo)
                && match (ae, be) {
                    (None, None) => true,
                    (Some(a), Some(b)) => same_target(a, b),
                    _ => false,
                }
        }
        (TypeTarget::RustIterator { item: a }, TypeTarget::RustIterator { item: b }) => {
            same_target(a, b)
        }
        (TypeTarget::RustCollection { item: a }, TypeTarget::RustCollection { item: b })
        | (TypeTarget::RustSlice { item: a }, TypeTarget::RustSlice { item: b }) => {
            same_target(a, b)
        }
        (
            TypeTarget::Candidates {
                targets: a,
                complete: ac,
                exhausted: ae,
            },
            TypeTarget::Candidates {
                targets: b,
                complete: bc,
                exhausted: be,
            },
        ) => {
            ac == bc
                && ae == be
                && a.len() == b.len()
                && a.iter().zip(b).all(|(left, right)| left.id == right.id)
        }
        (TypeTarget::Object(a), TypeTarget::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(name, node)| b.get(name).is_some_and(|other| other.id == node.id))
        }
        (TypeTarget::Unknown, TypeTarget::Unknown)
        | (TypeTarget::Ambiguous, TypeTarget::Ambiguous) => true,
        _ => false,
    }
}

fn flow_dependent(value: &ValueExpr) -> bool {
    if let ValueExpr::Scoped { value, .. } | ValueExpr::Await { value } = value {
        return flow_dependent(value);
    }
    if matches!(value, ValueExpr::BranchJoin { .. }) {
        return true;
    }
    matches!(
        value,
        ValueExpr::Call { .. }
            | ValueExpr::Alias { .. }
            | ValueExpr::Field { .. }
            | ValueExpr::Project { .. }
            | ValueExpr::LoopElement { .. }
    )
}

fn valid_immutable_initializers(facts: &ValueFlowFacts) -> bool {
    if facts.immutable_initializers.is_empty() {
        return true;
    }
    if facts
        .immutable_initializers
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
    {
        return false;
    }
    let mut next = 0;
    for binding in &facts.bindings {
        let Some(&position) = facts.immutable_initializers.get(next) else {
            break;
        };
        if binding.position > position {
            return false;
        }
        if binding.position == position {
            if binding.conditional || matches!(&binding.value, ValueExpr::Unknown) {
                return false;
            }
            next += 1;
        }
    }
    next == facts.immutable_initializers.len()
}

fn valid_facts(facts: &ValueFlowFacts, limits: SemanticLimits) -> bool {
    let text = |name: &str| !name.is_empty() && name.len() <= limits.name_bytes;
    valid_immutable_initializers(facts)
        && facts
            .bindings
            .len()
            .saturating_add(facts.branch_joins.len())
            .saturating_add(facts.fields.len())
            .saturating_add(facts.collection_elements().count())
            .saturating_add(facts.vue_slots().count())
            <= limits.facts_per_scope
        && facts.collection_elements().all(|fact| {
            text(&fact.name) && fact.position.line > 0 && valid_type(&fact.element_type, limits, 0)
        })
        && facts.vue_slots().all(|fact| {
            text(&fact.slot)
                && text(&fact.binding)
                && fact.position.line > 0
                && valid_type(&fact.type_expr, limits, 0)
        })
        && facts.bindings.iter().all(|fact| {
            text(&fact.name) && fact.position.line > 0 && valid_value(&fact.value, limits, 0)
        })
        && facts.branch_joins.len() <= limits.facts_per_scope
        && facts.branch_joins.iter().all(|fact| {
            text(&fact.name) && fact.position.line > 0 && valid_value(&fact.value, limits, 0)
        })
        && facts.fields.iter().all(|fact| {
            text(&fact.name) && fact.position.line > 0 && valid_value(&fact.value, limits, 0)
        })
        && facts
            .return_type
            .as_ref()
            .is_none_or(|ty| valid_type(ty, limits, 0))
        && facts
            .alias_type
            .as_ref()
            .is_none_or(|ty| valid_type(ty, limits, 0))
        && facts.return_value.as_ref().is_none_or(|returned| {
            valid_value(returned, limits, 0)
                && facts
                    .return_position
                    .is_some_and(|position| position.line > 0)
        })
}

fn valid_value(expr: &ValueExpr, limits: SemanticLimits, depth: usize) -> bool {
    if depth >= 8 {
        return false;
    }
    let text = |name: &str| !name.is_empty() && name.len() <= limits.name_bytes;
    match expr {
        ValueExpr::Alias { name } => text(name),
        ValueExpr::Annotated { type_expr } => valid_type(type_expr, limits, 0),
        ValueExpr::Unknown => true,
        ValueExpr::Construct { callee } => text(callee),
        ValueExpr::Call { callee, site } => {
            text(callee)
                && expr.valid_call_sites(depth)
                && site
                    .as_ref()
                    .and_then(|site| site.receiver.as_deref())
                    .is_none_or(|receiver| valid_value(receiver, limits, depth + 1))
        }
        ValueExpr::Await { value: inner } => valid_value(inner, limits, depth + 1),
        ValueExpr::LoopElement { iterable, body_end } => text(iterable) && body_end.line > 0,
        ValueExpr::Scoped {
            value: inner,
            body_end,
        } => body_end.line > 0 && valid_value(inner, limits, depth + 1),
        ValueExpr::BranchJoin { alternatives } => {
            !alternatives.is_empty()
                && alternatives.len() <= limits.facts_per_scope
                && alternatives
                    .iter()
                    .all(|alternative| valid_value(alternative, limits, depth + 1))
        }
        ValueExpr::Field { receiver, member } => text(receiver) && text(member),
        ValueExpr::Project {
            value: inner,
            member,
        } => text(member) && valid_value(inner, limits, depth + 1),
        ValueExpr::Object { members, .. } => {
            members.len() <= limits.facts_per_scope
                && members.iter().all(|member| {
                    text(&member.name)
                        && text(member.declaration_name())
                        && member.position.line > 0
                })
        }
    }
}

pub(crate) fn valid_scope_facts(
    node: &Node,
    facts: &ValueFlowFacts,
    limits: SemanticLimits,
) -> bool {
    if !valid_facts(facts, limits) {
        return false;
    }
    if facts.vue_slots().next().is_some()
        && (node.kind != "module"
            || !facts
                .component_macros()
                .any(|fact| fact.role == crate::core::semantic::ComponentMacroRole::Slots))
    {
        return false;
    }
    if matches!(
        node.kind.as_str(),
        "function" | "method" | "lambda" | "template_scope"
    ) && (node.binding_count() > limits.facts_per_scope
        || node
            .binding_names()
            .any(|name| name.len() > limits.name_bytes))
    {
        return false;
    }
    node.parameter_count()
        .saturating_add(crate::core::typed_facts::scope_fact_count(facts))
        <= limits.facts_per_scope
        && node
            .parameter_types()
            .all(|(name, _)| name.len() <= limits.name_bytes)
}

fn valid_type(expression: &TypeExpr, limits: SemanticLimits, depth: usize) -> bool {
    if depth >= limits.alias_depth {
        return false;
    }
    let name = |name: &str| !name.is_empty() && name.len() <= limits.name_bytes;
    match expression {
        TypeExpr::Named { name: text } => name(text),
        TypeExpr::Applied { base, args } => {
            name(base)
                && args.len() <= limits.facts_per_scope
                && args.iter().all(|arg| valid_type(arg, limits, depth + 1))
        }
        TypeExpr::Unknown => true,
    }
}

#[cfg(test)]
mod tests;
