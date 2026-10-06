use crate::core::{
    models::{Facts, Node, ReceiverHint, Reference},
    semantic::{SourcePosition, TypeExpr, ValueExpr, ValueFlowFacts},
};
use hashbrown::HashMap;
use rayon::prelude::*;
use serde::Deserialize;
use std::{borrow::Cow, cell::RefCell, collections::BTreeMap, sync::Arc};

type Definitions<'a> = HashMap<(&'a str, SourcePosition), Vec<&'a Node>>;
type NamedNodes<'a> = HashMap<(&'a str, &'a str), Vec<&'a Node>>;
type CollectionElements<'a> =
    HashMap<&'a str, HashMap<String, Vec<(SourcePosition, TypeTarget<'a>)>>>;

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
}
impl std::hash::Hash for ComputedKey<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&(self.source, self.position), state);
    }
}
impl hashbrown::Equivalent<(&str, SourcePosition)> for ComputedKey<'_> {
    fn equivalent(&self, key: &(&str, SourcePosition)) -> bool {
        self.source == key.0 && self.position == key.1
    }
}

#[derive(Clone, Debug)]
/// Enumerates the supported type target values.
pub enum TypeTarget<'a> {
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
    /// Represents the builtin case.
    Builtin(String),
    /// Represents the object case.
    Object(Arc<HashMap<String, &'a Node>>),
    /// Represents the ambiguous case.
    Ambiguous,
    /// Represents the unknown case.
    Unknown,
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

struct Binding<'a> {
    position: SourcePosition,
    target: TypeTarget<'a>,
    valid_until: Option<SourcePosition>,
    restores_previous: bool,
    immutable_initializer: bool,
}

struct Scope<'a> {
    parents: Vec<&'a str>,
    bindings: HashMap<String, Vec<Binding<'a>>>,
    declared_at: SourcePosition,
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
    vue_slots: HashMap<&'a str, HashMap<(String, String), TypeTarget<'a>>>,
    local_types: HashMap<&'a str, TypeTarget<'a>>,
    computed: HashMap<(&'a str, SourcePosition), TypeTarget<'a>>,
    collection_elements: CollectionElements<'a>,
    unknown: TypeTarget<'a>,
    ambiguous: TypeTarget<'a>,
    limits: SemanticLimits,
}

type InitializedScope<'a> = (&'a Node, Cow<'a, ValueFlowFacts>, Scope<'a>, TypeTarget<'a>);

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

struct ReadTrace<'a, 'b> {
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
        Ok(Cow::Owned(ValueFlowFacts::default()))
    } else if let Some(facts) = typed {
        Ok(Cow::Borrowed(facts))
    } else if resolver.has_typed_flows() || encoded.is_null() || oversized {
        Ok(Cow::Owned(ValueFlowFacts::default()))
    } else {
        ValueFlowFacts::deserialize_for_scope(
            encoded,
            node.kind == "template_scope"
                && node.language == "javascript"
                && node.details["classic_global"] == true,
        )
        .map(Cow::Owned)
    };
    let (facts, mut invalid) = match decoded {
        Ok(facts) => (facts, false),
        Err(_) => (Cow::Owned(ValueFlowFacts::default()), true),
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
        if let Some(names) = node.details["bindings"].as_array() {
            for name in names.iter().filter_map(serde_json::Value::as_str) {
                if name.len() <= limits.name_bytes {
                    bindings.insert(
                        name.to_owned(),
                        vec![Binding {
                            position: node_position(node),
                            target: TypeTarget::Unknown,
                            valid_until: None,
                            restores_previous: false,
                            immutable_initializer: false,
                        }],
                    );
                }
            }
        }
    }
    if let Some(params) = node.details["param_types"].as_object() {
        for (name, annotation) in params {
            let position = node_position(node);
            let target = if invalid || name.len() > limits.name_bytes {
                TypeTarget::Unknown
            } else {
                annotation
                    .as_str()
                    .filter(|name| !name.is_empty() && name.len() <= limits.name_bytes)
                    .map_or(TypeTarget::Unknown, |name| {
                        resolve_type(node, &stored_type_expr(name), position, resolver, limits)
                    })
            };
            bindings.insert(
                name.clone(),
                vec![Binding {
                    position,
                    target,
                    valid_until: None,
                    restores_previous: false,
                    immutable_initializer: false,
                }],
            );
        }
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
            declared_at: node_position(node),
            invalid,
            module: node.kind == "module",
        },
        returned,
    )
}

impl<'a> ValueFlowIndex<'a> {
    /// Reports a concrete lexical assignment, including one in a parent scope.
    pub fn has_local_write(&self, owner: &Node, name: &str) -> bool {
        let Some(scope) = self.scopes.get(owner.id.as_str()) else {
            return false;
        };
        std::iter::once(owner.id.as_str())
            .chain(scope.parents.iter().copied())
            .filter_map(|id| self.scopes.get(id))
            .any(|candidate| {
                candidate.bindings.get(name).is_some_and(|writes| {
                    writes
                        .last()
                        .is_some_and(|binding| binding.position > candidate.declared_at)
                })
            })
    }
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
        let by_id = nodes.iter().map(|node| (node.id.as_str(), *node)).collect();
        Self::build_initialized(all, nodes, &by_id, resolver, limits, |nodes, names| {
            nodes
                .iter()
                .map(|&node| initialize_scope(node, names, resolver, limits))
                .collect()
        })
    }

    pub(crate) fn build_parallel<F: AsRef<Facts>>(
        all: &'a BTreeMap<String, F>,
        nodes: Vec<&'a Node>,
        by_id: &HashMap<&str, &'a Node>,
        resolver: &(impl SemanticResolver<'a> + Sync),
    ) -> Self {
        let limits = SemanticLimits::default();
        Self::build_initialized(all, nodes, by_id, resolver, limits, |nodes, names| {
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
        })
    }

    fn build_initialized<F: AsRef<Facts>>(
        all: &'a BTreeMap<String, F>,
        nodes: Vec<&'a Node>,
        by_id: &HashMap<&str, &'a Node>,
        resolver: &impl SemanticResolver<'a>,
        limits: SemanticLimits,
        initialize: impl FnOnce(&[&'a Node], &NamedNodes<'a>) -> Vec<InitializedScope<'a>>,
    ) -> Self {
        let mut by_name: NamedNodes<'a> = HashMap::with_capacity(nodes.len());
        for node in &nodes {
            by_name
                .entry((&node.path, &node.qualname))
                .or_default()
                .push(node);
        }
        let mut raw = HashMap::with_capacity(nodes.len());
        let mut index = Self {
            scopes: HashMap::with_capacity(nodes.len()),
            returns: HashMap::with_capacity(nodes.len()),
            async_returns: HashMap::new(),
            return_collection_elements: HashMap::new(),
            async_return_collection_elements: HashMap::new(),
            fields: HashMap::new(),
            vue_slots: HashMap::new(),
            local_types: HashMap::with_capacity(nodes.len()),
            computed: HashMap::new(),
            collection_elements: HashMap::new(),
            unknown: TypeTarget::Unknown,
            ambiguous: TypeTarget::Ambiguous,
            limits,
        };
        let initialized = initialize(&nodes, &by_name);
        for (node, facts, scope, returned) in initialized {
            if node.language == "python" && !scope.invalid {
                for fact in &facts.collection_elements {
                    let element =
                        resolve_type(node, &fact.element_type, fact.position, resolver, limits);
                    if matches!(element, TypeTarget::Unknown | TypeTarget::Ambiguous) {
                        continue;
                    }
                    index
                        .collection_elements
                        .entry(node.id.as_str())
                        .or_default()
                        .entry(fact.name.clone())
                        .or_default()
                        .push((fact.position, element));
                }
                if matches!(node.kind.as_str(), "function" | "method") {
                    if let Some(async_type) = facts.async_return_type.as_ref() {
                        let returned =
                            resolve_type(node, async_type, node_position(node), resolver, limits);
                        if !matches!(returned, TypeTarget::Unknown | TypeTarget::Ambiguous) {
                            index.async_returns.insert(node.id.as_str(), returned);
                        }
                        if let TypeExpr::Applied { base, args } = async_type {
                            if matches!(base.as_str(), "list" | "List" | "Sequence" | "Iterable")
                                && args.len() == 1
                            {
                                let element = resolve_type(
                                    node,
                                    &args[0],
                                    node_position(node),
                                    resolver,
                                    limits,
                                );
                                if !matches!(element, TypeTarget::Unknown | TypeTarget::Ambiguous) {
                                    index
                                        .async_return_collection_elements
                                        .insert(node.id.as_str(), element);
                                }
                            }
                        }
                    }
                    if let Some(TypeExpr::Applied { base, args }) = facts.return_type.as_ref() {
                        if matches!(base.as_str(), "list" | "List" | "Sequence" | "Iterable")
                            && args.len() == 1
                        {
                            let element =
                                resolve_type(node, &args[0], node_position(node), resolver, limits);
                            if !matches!(element, TypeTarget::Unknown | TypeTarget::Ambiguous) {
                                index
                                    .return_collection_elements
                                    .insert(node.id.as_str(), element);
                            }
                        }
                    }
                }
            }
            index.returns.insert(node.id.as_str(), returned);
            index
                .local_types
                .insert(node.id.as_str(), TypeTarget::Local(node));
            index.scopes.insert(node.id.as_str(), scope);
            raw.insert(node.id.as_str(), facts);
        }
        let mut owners = nodes;
        owners.sort_by_cached_key(|node| {
            (
                node.path.as_str(),
                node.qualname.matches('.').count(),
                node.line,
                node.id.as_str(),
            )
        });
        let definitions: Definitions<'a> =
            owners
                .iter()
                .fold(HashMap::with_capacity(owners.len()), |mut map, node| {
                    map.entry((node.path.as_str(), node_position(node)))
                        .or_default()
                        .push(*node);
                    map
                });
        let mut ordered = HashMap::with_capacity(owners.len());
        for owner in &owners {
            if owner.language != "vue" || owner.kind != "module" {
                continue;
            }
            for slot in &raw[owner.id.as_str()].vue_slots {
                let target = if index.scopes[owner.id.as_str()].invalid {
                    TypeTarget::Unknown
                } else {
                    index.evaluate(
                        owner,
                        &ValueExpr::Annotated {
                            type_expr: slot.type_expr.clone(),
                        },
                        slot.position,
                        resolver,
                        Some(&definitions),
                    )
                };
                use hashbrown::hash_map::Entry;
                match index
                    .vue_slots
                    .entry(owner.id.as_str())
                    .or_default()
                    .entry((slot.slot.clone(), slot.binding.clone()))
                {
                    Entry::Vacant(entry) => {
                        entry.insert(target);
                    }
                    Entry::Occupied(mut entry) => {
                        entry.insert(TypeTarget::Ambiguous);
                    }
                }
            }
        }
        for owner in &owners {
            let mut bindings: Vec<_> = raw[owner.id.as_str()].bindings.iter().collect();
            bindings.sort_by_key(|binding| binding.position);
            let mut events = Vec::with_capacity(bindings.len());
            for binding in bindings {
                let loop_element = matches!(&binding.value, ValueExpr::LoopElement { .. });
                let guarded_python = owner.language == "python"
                    && matches!(&binding.value, ValueExpr::Scoped { .. });
                let target = if (binding.conditional && !loop_element && !guarded_python)
                    || index.scopes[owner.id.as_str()].invalid
                {
                    TypeTarget::Unknown
                } else {
                    index.evaluate(
                        owner,
                        &binding.value,
                        binding.position,
                        resolver,
                        Some(&definitions),
                    )
                };
                if owner.language == "python"
                    && matches!(
                        &target,
                        TypeTarget::Builtin(name)
                            if matches!(name.as_str(), "list" | "List" | "Sequence" | "Iterable")
                    )
                {
                    let call = match &binding.value {
                        ValueExpr::Call { callee } => Some((callee.as_str(), false)),
                        ValueExpr::Await { value } => match value.as_ref() {
                            ValueExpr::Call { callee } => Some((callee.as_str(), true)),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some((callee, awaited)) = call {
                        let callable = index.callable_expression_tracked(
                            owner,
                            callee,
                            binding.position,
                            resolver,
                            None,
                        );
                        let return_elements = if awaited {
                            &index.async_return_collection_elements
                        } else {
                            &index.return_collection_elements
                        };
                        if let Some(element) = callable
                            .and_then(|callable| return_elements.get(callable.id.as_str()))
                            .filter(|_| {
                                !index
                                    .collection_elements
                                    .get(owner.id.as_str())
                                    .and_then(|names| names.get(&binding.name))
                                    .is_some_and(|elements| {
                                        elements
                                            .iter()
                                            .any(|(position, _)| *position == binding.position)
                                    })
                            })
                            .cloned()
                        {
                            index
                                .collection_elements
                                .entry(owner.id.as_str())
                                .or_default()
                                .entry(binding.name.clone())
                                .or_default()
                                .push((binding.position, element));
                        }
                    }
                }
                let writes = index
                    .scopes
                    .get_mut(owner.id.as_str())
                    .expect("scope indexed")
                    .bindings
                    .entry(binding.name.clone())
                    .or_default();
                events.push((binding, writes.len()));
                writes.push(Binding {
                    position: binding.position,
                    target,
                    valid_until: match &binding.value {
                        ValueExpr::LoopElement { body_end, .. } => Some(*body_end),
                        ValueExpr::Scoped { body_end, .. } => Some(*body_end),
                        _ => None,
                    },
                    restores_previous: owner.language != "python"
                        && matches!(&binding.value, ValueExpr::Scoped { .. }),
                    immutable_initializer: raw[owner.id.as_str()]
                        .immutable_initializers
                        .binary_search(&binding.position)
                        .is_ok(),
                });
            }
            ordered.insert(owner.id.as_str(), events);
        }
        let mut field_events = Vec::new();
        for owner in &owners {
            if raw[owner.id.as_str()].fields.is_empty() {
                continue;
            }
            let Some(class) = field_owner(owner, &by_name, resolver) else {
                continue;
            };
            for field in &raw[owner.id.as_str()].fields {
                let target = if field.conditional || index.scopes[owner.id.as_str()].invalid {
                    TypeTarget::Unknown
                } else {
                    index.evaluate(
                        owner,
                        &field.value,
                        field.position,
                        resolver,
                        Some(&definitions),
                    )
                };
                let entry = index
                    .fields
                    .entry(class.id.as_str())
                    .or_default()
                    .entry(field.name.clone())
                    .or_insert_with(|| target.clone());
                *entry = merge(entry, &target);
                field_events.push((*owner, class, field, target));
            }
        }
        // Replay in source order so aliases of field-derived values see the summaries.
        for owner in &owners {
            for (binding, event_index) in &ordered[owner.id.as_str()] {
                let guarded_python = owner.language == "python"
                    && matches!(&binding.value, ValueExpr::Scoped { .. });
                if (!binding.conditional || guarded_python)
                    && !index.scopes[owner.id.as_str()].invalid
                {
                    let writes = &index.scopes[owner.id.as_str()].bindings[&binding.name];
                    if duplicate_position(writes, *event_index) {
                        index
                            .scopes
                            .get_mut(owner.id.as_str())
                            .expect("scope indexed")
                            .bindings
                            .get_mut(&binding.name)
                            .expect("binding indexed")[*event_index]
                            .target = TypeTarget::Unknown;
                    } else if flow_dependent(&binding.value) {
                        let target = index.evaluate(
                            owner,
                            &binding.value,
                            binding.position,
                            resolver,
                            Some(&definitions),
                        );
                        index
                            .scopes
                            .get_mut(owner.id.as_str())
                            .expect("scope indexed")
                            .bindings
                            .get_mut(&binding.name)
                            .expect("binding indexed")[*event_index]
                            .target = target;
                    }
                }
            }
        }
        let inferred: Vec<_> = owners
            .iter()
            .filter_map(|owner| {
                let facts = &raw[owner.id.as_str()];
                if facts.return_type.is_some() || index.scopes[owner.id.as_str()].invalid {
                    return None;
                }
                facts
                    .return_value
                    .as_ref()
                    .zip(facts.return_position)
                    .map(|(value, position)| (*owner, value, position))
            })
            .collect();
        let replay_events: Vec<_> = owners
            .iter()
            .filter(|owner| !index.scopes[owner.id.as_str()].invalid)
            .flat_map(|owner| {
                ordered[owner.id.as_str()]
                    .iter()
                    .filter(|(binding, _)| {
                        (!binding.conditional
                            || (owner.language == "python"
                                && matches!(&binding.value, ValueExpr::Scoped { .. })))
                            && flow_dependent(&binding.value)
                    })
                    .map(move |(binding, event_index)| (*owner, *binding, *event_index))
            })
            .collect();
        let mut epochs = Epochs::default();
        let mut inferred_reads: Vec<Option<ReadEpochs<'a>>> =
            (0..inferred.len()).map(|_| None).collect();
        let mut replay_reads: Vec<Option<ReadEpochs<'a>>> =
            (0..replay_events.len()).map(|_| None).collect();
        let mut field_reads: Vec<Option<ReadEpochs<'a>>> =
            (0..field_events.len()).map(|_| None).collect();
        for iteration in 0..limits.alias_depth.min(8) {
            let mut changed = false;
            for ((owner, value, position), reads) in inferred.iter().zip(&mut inferred_reads) {
                if iteration > 0 && !flow_dependent(value) {
                    continue;
                }
                if reads.as_ref().is_some_and(|reads| !reads.dirty(&epochs)) {
                    continue;
                }
                let target = if flow_dependent(value) {
                    let trace = ReadTrace::new(&epochs, reads.take().unwrap_or_default());
                    let target = index.evaluate_tracked(
                        owner,
                        value,
                        *position,
                        resolver,
                        Some(&definitions),
                        Some(&trace),
                    );
                    *reads = Some(trace.reads.into_inner());
                    target
                } else {
                    index.evaluate(owner, value, *position, resolver, Some(&definitions))
                };
                let previous = index
                    .returns
                    .get_mut(owner.id.as_str())
                    .expect("return indexed");
                if !same_target(previous, &target) {
                    *previous = target;
                    *epochs.returns.entry(owner.id.as_str()).or_default() += 1;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
            for ((owner, binding, event_index), reads) in
                replay_events.iter().zip(&mut replay_reads)
            {
                if reads.as_ref().is_some_and(|reads| !reads.dirty(&epochs)) {
                    continue;
                }
                let writes = &index.scopes[owner.id.as_str()].bindings[&binding.name];
                let trace = ReadTrace::new(&epochs, reads.take().unwrap_or_default());
                let target = if duplicate_position(writes, *event_index) {
                    TypeTarget::Unknown
                } else {
                    index.evaluate_tracked(
                        owner,
                        &binding.value,
                        binding.position,
                        resolver,
                        Some(&definitions),
                        Some(&trace),
                    )
                };
                *reads = Some(trace.reads.into_inner());
                let previous = &mut index
                    .scopes
                    .get_mut(owner.id.as_str())
                    .expect("scope indexed")
                    .bindings
                    .get_mut(&binding.name)
                    .expect("binding indexed")[*event_index]
                    .target;
                if !same_target(previous, &target) {
                    *previous = target;
                    *epochs.scopes.entry(owner.id.as_str()).or_default() += 1;
                }
            }
            let mut field_targets_changed = false;
            for ((owner, _, field, target), reads) in field_events.iter_mut().zip(&mut field_reads)
            {
                if !field.conditional
                    && !index.scopes[owner.id.as_str()].invalid
                    && flow_dependent(&field.value)
                    && reads.as_ref().is_none_or(|reads| reads.dirty(&epochs))
                {
                    let trace = ReadTrace::new(&epochs, reads.take().unwrap_or_default());
                    let updated = index.evaluate_tracked(
                        owner,
                        &field.value,
                        field.position,
                        resolver,
                        Some(&definitions),
                        Some(&trace),
                    );
                    *reads = Some(trace.reads.into_inner());
                    if !same_target(target, &updated) {
                        *target = updated;
                        field_targets_changed = true;
                    }
                }
            }
            if field_targets_changed {
                let mut fields: HashMap<&str, HashMap<String, TypeTarget<'a>>> = HashMap::new();
                for (_, class, field, target) in &field_events {
                    let entry = fields
                        .entry(class.id.as_str())
                        .or_default()
                        .entry(field.name.clone())
                        .or_insert_with(|| target.clone());
                    *entry = merge(entry, target);
                }
                if !same_fields(&index.fields, &fields) {
                    epochs.fields += 1;
                }
                index.fields = fields;
            }
        }
        for reference in all.values().flat_map(|facts| &facts.as_ref().references) {
            if !matches!(
                reference.receiver_hint,
                Some(ReceiverHint::ConstructorResult { .. } | ReceiverHint::CallResult { .. })
            ) {
                continue;
            }
            let Some(owner) = by_id.get(reference.source.as_str()).copied() else {
                continue;
            };
            let at = SourcePosition {
                line: reference.line,
                column: reference.column,
            };
            let target = match reference.receiver_hint.as_ref() {
                Some(ReceiverHint::ConstructorResult { callee, .. }) => {
                    if callee.len() > limits.name_bytes {
                        TypeTarget::Unknown
                    } else {
                        symbol_type(resolver.resolve_symbol(
                            owner,
                            callee,
                            at,
                            SymbolRole::Constructor,
                        ))
                    }
                }
                Some(ReceiverHint::CallResult { callee, .. }) => {
                    if callee.len() > limits.name_bytes
                        || callee.matches('.').count() > limits.alias_depth
                    {
                        TypeTarget::Unknown
                    } else if let Some(inner) = callee
                        .strip_suffix("()")
                        .filter(|_| owner.language == "python")
                    {
                        index
                            .type_of_expression(owner, inner, at, resolver)
                            .and_then(|target| match target {
                                TypeTarget::Callable(callable) => index.return_type(callable),
                                _ => None,
                            })
                            .cloned()
                            .unwrap_or(TypeTarget::Unknown)
                    } else {
                        index.evaluate(
                            owner,
                            &ValueExpr::Call {
                                callee: callee.clone(),
                            },
                            at,
                            resolver,
                            None,
                        )
                    }
                }
                _ => continue,
            };
            index
                .computed
                .entry((owner.id.as_str(), at))
                .and_modify(|existing| *existing = merge(existing, &target))
                .or_insert(target);
        }
        index
    }

    /// Performs lookup.
    pub fn lookup(&self, owner: &Node, name: &str, at: SourcePosition) -> Option<&TypeTarget<'a>> {
        self.lookup_tracked(owner, name, at, None)
    }

    fn lookup_tracked(
        &self,
        owner: &Node,
        name: &str,
        at: SourcePosition,
        trace: Option<&ReadTrace<'a, '_>>,
    ) -> Option<&TypeTarget<'a>> {
        let (id, scope) = self.scopes.get_key_value(owner.id.as_str())?;
        if scope.invalid {
            return Some(&self.unknown);
        }
        if let Some(writes) = scope.bindings.get(name) {
            if let Some(trace) = trace {
                trace.scope(id);
            }
            return Some(reaching(writes, at).unwrap_or(&self.unknown));
        }
        for id in &scope.parents {
            let parent = &self.scopes[id];
            if parent.invalid {
                return Some(&self.unknown);
            }
            if let Some(writes) = parent.bindings.get(name) {
                let setup_complete =
                    owner.kind == "template_scope" && owner.language == "vue" && parent.module;
                let immutable_capture =
                    matches!(owner.language.as_str(), "javascript" | "typescript")
                        && stable_immutable_capture(writes);
                if writes.len() > 1 && !setup_complete && !immutable_capture {
                    return Some(&self.unknown);
                }
                if let Some(trace) = trace {
                    trace.scope(id);
                }
                let position = if setup_complete {
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
            source: reference.source.as_str(),
            position: SourcePosition {
                line: reference.line,
                column: reference.column,
            },
        })
    }

    /// Performs field type.
    pub fn field_type(&self, class: &Node, name: &str) -> Option<&TypeTarget<'a>> {
        self.field_type_tracked(class, name, None)
    }

    /// Returns a slot prop type declared in the exact Vue provider module.
    pub fn vue_slot_type(
        &self,
        module: &Node,
        slot: &str,
        binding: &str,
    ) -> Option<&TypeTarget<'a>> {
        self.vue_slots
            .get(module.id.as_str())
            .and_then(|slots| slots.get(&(slot.to_owned(), binding.to_owned())))
    }

    fn field_type_tracked(
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

    fn type_of_expression_tracked(
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
        if owner.language == "python" {
            if let Some(target) = self.borrow_symbol(
                resolver.resolve_symbol(owner, expression, at, SymbolRole::Constructor),
                trace,
            ) {
                return Some(target);
            }
        }
        if let Some((receiver, member)) = expression.rsplit_once('.') {
            if self.lookup_tracked(owner, receiver, at, trace).is_none()
                && matches!(resolver.resolve_symbol(owner, receiver, at, SymbolRole::Type), Symbol::Type(TypeTarget::Local(node)) if matches!(node.kind.as_str(), "class" | "struct" | "interface" | "enum"))
            {
                return self.borrow_return_symbol(
                    resolver.resolve_symbol(owner, expression, at, SymbolRole::Callable),
                    trace,
                );
            }
            if let Some(target) = self.value_expression(owner, receiver, at, resolver, trace) {
                if let Some(method) = Self::object_member(target, member) {
                    return self.return_type_tracked(method, trace);
                }
                return self.borrow_return_symbol(
                    resolver.resolve_member(target, member, owner, at),
                    trace,
                );
            }
        }
        self.borrow_return_symbol(
            resolver.resolve_symbol(owner, expression, at, SymbolRole::Callable),
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
        if let Some(TypeTarget::Callable(callable)) =
            self.lookup_tracked(owner, expression, at, trace)
        {
            return Some(*callable);
        }
        if let Some((receiver, member)) = expression.rsplit_once('.') {
            if self.lookup_tracked(owner, receiver, at, trace).is_none()
                && matches!(resolver.resolve_symbol(owner, receiver, at, SymbolRole::Type), Symbol::Type(TypeTarget::Local(node)) if matches!(node.kind.as_str(), "class" | "struct" | "interface" | "enum"))
            {
                return match resolver.resolve_symbol(owner, expression, at, SymbolRole::Callable) {
                    Symbol::Callable(callable) => Some(callable),
                    _ => None,
                };
            }
            if let Some(target) = self.value_expression(owner, receiver, at, resolver, trace) {
                if let Some(method) = Self::object_member(target, member) {
                    return Some(method);
                }
                return match resolver.resolve_member(target, member, owner, at) {
                    Symbol::Callable(callable) => Some(callable),
                    _ => None,
                };
            }
        }
        match resolver.resolve_symbol(owner, expression, at, SymbolRole::Callable) {
            Symbol::Callable(callable) => Some(callable),
            _ => None,
        }
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
            if let Some(TypeTarget::Local(class)) =
                self.value_expression(owner, receiver, at, resolver, trace)
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
                if owner.language == "python" {
                    if let Some(builtin) = crate::core::semantic::python_literal_type(callee) {
                        return TypeTarget::Builtin(builtin.to_owned());
                    }
                }
                if matches!(owner.language.as_str(), "javascript" | "typescript" | "vue")
                    && callee == "[]"
                {
                    return TypeTarget::Builtin("Array".to_owned());
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
                let ValueExpr::Call { callee } = value.as_ref() else {
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
            ValueExpr::Call { callee } => {
                if owner.language == "python" {
                    let constructor =
                        resolver.resolve_symbol(owner, callee, at, SymbolRole::Constructor);
                    if let Symbol::Type(target @ (TypeTarget::Local(_) | TypeTarget::Builtin(_))) =
                        constructor
                    {
                        return target;
                    }
                    if let Some(target) = resolver.resolve_external_factory(owner, callee, at) {
                        return target;
                    }
                } else if matches!(owner.language.as_str(), "javascript" | "typescript" | "vue") {
                    if let Some(target) = self.evaluate_js_call(owner, callee, at, resolver, trace)
                    {
                        return target;
                    }
                } else if owner.language == "rust" {
                    let returned = self
                        .type_of_expression_tracked(owner, callee, at, resolver, trace)
                        .cloned()
                        .unwrap_or(TypeTarget::Unknown);
                    if !matches!(returned, TypeTarget::Unknown) {
                        return returned;
                    }
                    let callee_clean = callee.replace("::", ".");
                    if let Some((receiver, member)) = callee_clean.rsplit_once('.') {
                        if matches!(
                            member,
                            "new"
                                | "from"
                                | "with_capacity"
                                | "default"
                                | "open"
                                | "create"
                                | "build"
                                | "from_str"
                                | "parse"
                        ) {
                            let type_sym =
                                resolver.resolve_symbol(owner, receiver, at, SymbolRole::Type);
                            if let Symbol::Type(
                                target @ (TypeTarget::External { .. } | TypeTarget::Builtin(_)),
                            ) = type_sym
                            {
                                return target;
                            }
                        }
                    }
                    return TypeTarget::Unknown;
                }
                self.type_of_expression_tracked(owner, callee, at, resolver, trace)
                    .cloned()
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
                if let TypeTarget::Local(class) = &receiver_type {
                    if let Some(target) = self.field_type_tracked(class, member, trace) {
                        return target.clone();
                    }
                }
                if let TypeTarget::Builtin(name) = &receiver_type {
                    if matches!(owner.language.as_str(), "javascript" | "typescript") {
                        if let Some(prop_type) =
                            crate::engine::languages::typescript::typed_dom_property(name, member)
                        {
                            return TypeTarget::Builtin(prop_type.to_owned());
                        }
                    }
                }
                match resolver.resolve_member(&receiver_type, member, owner, at) {
                    Symbol::Callable(node) => TypeTarget::Callable(node),
                    _ => TypeTarget::Unknown,
                }
            }
            ValueExpr::Project { value, member } => {
                let source = self.evaluate_tracked(owner, value, at, resolver, definitions, trace);
                match source {
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

    fn evaluate_js_call(
        &self,
        owner: &'a Node,
        callee: &str,
        at: SourcePosition,
        resolver: &dyn SemanticResolver<'a>,
        trace: Option<&ReadTrace<'a, '_>>,
    ) -> Option<TypeTarget<'a>> {
        let callee = callee.trim();
        if callee.ends_with(')') {
            if let Some(base) = strip_call_arguments(callee) {
                return self.evaluate_js_call(owner, base, at, resolver, trace);
            }
        }
        if callee == "defineProps" && owner.language == "vue" && owner.kind == "module" {
            return Some(TypeTarget::Local(owner));
        }
        if let Some(target) = resolver.resolve_external_factory(owner, callee, at) {
            return Some(target);
        }
        if let Some((receiver_expr, member)) = callee.rsplit_once('.') {
            let receiver_type = if receiver_expr.ends_with(')') {
                self.evaluate_js_call(owner, receiver_expr, at, resolver, trace)
            } else {
                let (base_receiver, fields) =
                    receiver_expr.split_once('.').unwrap_or((receiver_expr, ""));
                let mut target = self
                    .lookup_tracked(owner, base_receiver, at, trace)
                    .cloned();
                if !fields.is_empty() {
                    for field in fields.split('.').filter(|f| !f.is_empty()) {
                        let next = match target.as_ref() {
                            Some(TypeTarget::Local(class)) => {
                                self.field_type_tracked(class, field, trace).cloned()
                            }
                            Some(TypeTarget::Builtin(name)) => {
                                crate::engine::languages::typescript::typed_dom_property(
                                    name, field,
                                )
                                .map(|n| TypeTarget::Builtin(n.to_owned()))
                            }
                            _ => None,
                        };
                        target = next;
                    }
                }
                target
            };
            let returned = match (receiver_type.as_ref(), member) {
                (
                    Some(TypeTarget::Builtin(name)),
                    "filter" | "map" | "slice" | "concat" | "flat" | "flatMap" | "reverse" | "sort"
                    | "splice",
                ) if name == "Array" => Some("Array"),
                (Some(TypeTarget::Builtin(name)), "then" | "catch" | "finally")
                    if name == "Promise" =>
                {
                    Some("Promise")
                }
                (
                    Some(TypeTarget::Builtin(name)),
                    "querySelector" | "closest" | "appendChild" | "removeChild" | "cloneNode"
                    | "insertBefore" | "replaceChild",
                ) if matches!(name.as_str(), "Element" | "HTMLElement" | "Node")
                    || crate::engine::languages::typescript::typed_dom_receiver(name)
                        == Some("Element") =>
                {
                    Some("Element")
                }
                (Some(TypeTarget::Builtin(name)), "querySelector")
                    if matches!(name.as_str(), "ParentNode" | "DocumentFragment") =>
                {
                    Some("Element")
                }
                (
                    Some(TypeTarget::Builtin(name)),
                    "createElement" | "getElementById" | "querySelector",
                ) if name == "Document" => Some("Element"),
                _ => None,
            };
            if let Some(returned) = returned {
                return Some(TypeTarget::Builtin(returned.to_owned()));
            }
        }
        None
    }
}

fn strip_call_arguments(s: &str) -> Option<&str> {
    if !s.ends_with(')') {
        return None;
    }
    let mut depth = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    for (idx, c) in s.char_indices().rev() {
        match c {
            '\'' if !in_double_quote => in_single_quote = !in_single_quote,
            '"' if !in_single_quote => in_double_quote = !in_double_quote,
            ')' if !in_single_quote && !in_double_quote => depth += 1,
            '(' if !in_single_quote && !in_double_quote => {
                depth -= 1;
                if depth == 0 {
                    return Some(s[..idx].trim());
                }
            }
            _ => {}
        }
    }
    None
}

fn node_position(node: &Node) -> SourcePosition {
    SourcePosition {
        line: node.line,
        column: node.details["column"].as_u64().unwrap_or(0) as usize,
    }
}

fn reaching<'a, 'b>(writes: &'b [Binding<'a>], at: SourcePosition) -> Option<&'b TypeTarget<'a>> {
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

fn stable_immutable_capture(writes: &[Binding<'_>]) -> bool {
    let Some((initializer, sentinels)) = writes.split_last() else {
        return false;
    };
    (2..=3).contains(&writes.len())
        && initializer.immutable_initializer
        && !matches!(
            &initializer.target,
            TypeTarget::Unknown | TypeTarget::Ambiguous
        )
        && sentinels.iter().all(|binding| {
            !binding.immutable_initializer && matches!(&binding.target, TypeTarget::Unknown)
        })
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
                resolver.resolve_applied(scope, base, args, at)
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
        if node.language == "python" && node.kind == "class" && typed_dict_class(node, resolver) {
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

fn typed_dict_class<'a>(node: &'a Node, resolver: &impl SemanticResolver<'a>) -> bool {
    let Some(bases) = node.details["bases"]
        .as_str()
        .and_then(|bases| bases.strip_prefix('('))
        .and_then(|bases| bases.strip_suffix(')'))
    else {
        return false;
    };
    let (base, options) = bases
        .split_once(',')
        .map_or((bases, ""), |(base, options)| (base, options));
    if !options.is_empty()
        && !matches!(
            options.trim().replace(' ', "").as_str(),
            "total=False" | "total=True"
        )
    {
        return false;
    }
    let base = base.trim();
    if !matches!(
        base,
        "TypedDict" | "typing.TypedDict" | "typing_extensions.TypedDict"
    ) {
        return false;
    }
    matches!(
        resolver.resolve_symbol(node, base, node_position(node), SymbolRole::Type),
        Symbol::Type(TypeTarget::External { module, .. })
            if matches!(module.as_str(), "typing" | "typing_extensions")
    )
}

fn stored_type_expr(annotation: &str) -> TypeExpr {
    let annotation = annotation.trim();
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

fn symbol_type(symbol: Symbol<'_>) -> TypeTarget<'_> {
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
    if owner.kind == "module" && owner.language == "vue" {
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
    fn value(expr: &ValueExpr, limits: SemanticLimits, depth: usize) -> bool {
        if depth >= 8 {
            return false;
        }
        let text = |name: &str| !name.is_empty() && name.len() <= limits.name_bytes;
        match expr {
            ValueExpr::Alias { name } => text(name),
            ValueExpr::Annotated { type_expr } => valid_type(type_expr, limits, 0),
            ValueExpr::Unknown => true,
            ValueExpr::Construct { callee } | ValueExpr::Call { callee } => text(callee),
            ValueExpr::Await { value: inner } => value(inner, limits, depth + 1),
            ValueExpr::LoopElement { iterable, body_end } => text(iterable) && body_end.line > 0,
            ValueExpr::Scoped {
                value: inner,
                body_end,
            } => body_end.line > 0 && value(inner, limits, depth + 1),
            ValueExpr::Field { receiver, member } => text(receiver) && text(member),
            ValueExpr::Project {
                value: inner,
                member,
            } => text(member) && value(inner, limits, depth + 1),
            ValueExpr::Object { members, .. } => {
                members.len() <= limits.facts_per_scope
                    && members
                        .iter()
                        .all(|member| text(&member.name) && member.position.line > 0)
            }
        }
    }
    valid_immutable_initializers(facts)
        && facts
            .bindings
            .len()
            .saturating_add(facts.fields.len())
            .saturating_add(facts.collection_elements.len())
            .saturating_add(facts.vue_slots.len())
            <= limits.facts_per_scope
        && facts.collection_elements.iter().all(|fact| {
            text(&fact.name) && fact.position.line > 0 && valid_type(&fact.element_type, limits, 0)
        })
        && facts.vue_slots.iter().all(|fact| {
            text(&fact.slot)
                && text(&fact.binding)
                && fact.position.line > 0
                && valid_type(&fact.type_expr, limits, 0)
        })
        && facts
            .bindings
            .iter()
            .all(|fact| text(&fact.name) && fact.position.line > 0 && value(&fact.value, limits, 0))
        && facts
            .fields
            .iter()
            .all(|fact| text(&fact.name) && fact.position.line > 0 && value(&fact.value, limits, 0))
        && facts
            .return_type
            .as_ref()
            .is_none_or(|ty| valid_type(ty, limits, 0))
        && facts
            .alias_type
            .as_ref()
            .is_none_or(|ty| valid_type(ty, limits, 0))
        && facts.return_value.as_ref().is_none_or(|returned| {
            value(returned, limits, 0)
                && facts
                    .return_position
                    .is_some_and(|position| position.line > 0)
        })
}

pub(crate) fn valid_scope_facts(
    node: &Node,
    facts: &ValueFlowFacts,
    limits: SemanticLimits,
) -> bool {
    if !valid_facts(facts, limits) {
        return false;
    }
    if !facts.vue_slots.is_empty() && (node.kind != "module" || node.language != "vue") {
        return false;
    }
    if matches!(
        node.kind.as_str(),
        "function" | "method" | "lambda" | "template_scope"
    ) {
        if let Some(names) = node.details["bindings"].as_array() {
            if names.len() > limits.facts_per_scope
                || names
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .any(|name| name.len() > limits.name_bytes)
            {
                return false;
            }
        }
    }
    node.details["param_types"]
        .as_object()
        .is_none_or(|params| {
            params
                .len()
                .saturating_add(crate::core::typed_facts::scope_fact_count(facts))
                <= limits.facts_per_scope
                && params.keys().all(|name| name.len() <= limits.name_bytes)
        })
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
mod tests {
    use super::*;
    use crate::core::semantic::BindingFact;

    #[test]
    fn reaching_binding_requires_strict_source_order() {
        let writes = vec![
            Binding {
                position: SourcePosition { line: 4, column: 8 },
                target: TypeTarget::Builtin("str".into()),
                valid_until: None,
                restores_previous: false,
                immutable_initializer: false,
            },
            Binding {
                position: SourcePosition {
                    line: 4,
                    column: 20,
                },
                target: TypeTarget::Unknown,
                valid_until: None,
                restores_previous: false,
                immutable_initializer: false,
            },
        ];
        assert!(reaching(&writes, SourcePosition { line: 4, column: 8 }).is_none());
        assert!(
            matches!(reaching(&writes, SourcePosition { line: 4, column: 19 }), Some(TypeTarget::Builtin(name)) if name == "str")
        );
        assert!(matches!(
            reaching(
                &writes,
                SourcePosition {
                    line: 4,
                    column: 21
                }
            ),
            Some(TypeTarget::Unknown)
        ));
    }

    #[test]
    fn semantic_limits_reject_excessive_or_invalid_facts() {
        let mut facts = ValueFlowFacts {
            bindings: vec![BindingFact {
                name: "value".into(),
                position: SourcePosition { line: 1, column: 0 },
                value: ValueExpr::Unknown,
                conditional: false,
            }],
            ..ValueFlowFacts::default()
        };
        assert!(valid_facts(&facts, SemanticLimits::default()));
        assert!(!valid_facts(
            &facts,
            SemanticLimits {
                facts_per_scope: 0,
                ..SemanticLimits::default()
            }
        ));
        facts.bindings[0].position.line = 0;
        assert!(!valid_facts(&facts, SemanticLimits::default()));
    }

    #[test]
    fn uncertain_field_writes_invalidate_declared_summary() {
        let known = TypeTarget::Builtin("str".into());
        assert!(matches!(
            merge(&known, &TypeTarget::Unknown),
            TypeTarget::Unknown
        ));
        assert!(matches!(
            merge(&known, &TypeTarget::Builtin("int".into())),
            TypeTarget::Ambiguous
        ));
    }

    #[test]
    fn named_type_aliases_resolve_with_cycle_and_depth_guards() {
        fn alias(name: &str, rhs: &str) -> Node {
            Node {
                id: name.into(),
                kind: "type_alias".into(),
                name: name.into(),
                qualname: name.into(),
                path: "test.py".into(),
                line: 1,
                end_line: 1,
                is_test: true,
                language: "python".into(),
                generated: false,
                details: serde_json::json!({"value_flow": {"alias_type": {"kind": "named", "name": rhs}}}),
            }
        }
        struct Resolver<'a> {
            nodes: Vec<&'a Node>,
        }
        impl<'a> SemanticResolver<'a> for Resolver<'a> {
            fn resolve_symbol(
                &self,
                _scope: &'a Node,
                name: &str,
                _at: SourcePosition,
                _role: SymbolRole,
            ) -> Symbol<'a> {
                if name == "str" {
                    return Symbol::Type(TypeTarget::Builtin("str".into()));
                }
                self.nodes
                    .iter()
                    .find(|node| node.name == name)
                    .map_or(Symbol::Unknown, |node| {
                        Symbol::Type(TypeTarget::Local(node))
                    })
            }
        }
        let first = alias("First", "Second");
        let second = alias("Second", "str");
        let looped = alias("Loop", "Loop");
        let resolver = Resolver {
            nodes: vec![&first, &second, &looped],
        };
        let at = SourcePosition { line: 3, column: 0 };
        let named = |name: &str| TypeExpr::Named { name: name.into() };
        assert!(
            matches!(resolve_type(&first, &named("First"), at, &resolver, SemanticLimits::default()), TypeTarget::Builtin(name) if name == "str")
        );
        assert!(matches!(
            resolve_type(
                &first,
                &named("Loop"),
                at,
                &resolver,
                SemanticLimits::default()
            ),
            TypeTarget::Unknown
        ));
        assert!(matches!(
            resolve_type(
                &first,
                &named("First"),
                at,
                &resolver,
                SemanticLimits {
                    alias_depth: 1,
                    ..SemanticLimits::default()
                }
            ),
            TypeTarget::Unknown
        ));
        let wrapper = TypeExpr::Applied {
            base: "Optional".into(),
            args: vec![named("str")],
        };
        assert!(matches!(
            resolve_type(&first, &wrapper, at, &resolver, SemanticLimits::default()),
            TypeTarget::Unknown
        ));
        assert!(!valid_type(
            &wrapper,
            SemanticLimits {
                alias_depth: 1,
                ..SemanticLimits::default()
            },
            0
        ));
    }

    #[test]
    fn computed_receiver_key_uses_borrowed_hash_lookup() {
        let source = String::from("owner");
        let position = SourcePosition { line: 7, column: 4 };
        let mut cache = HashMap::new();
        cache.insert((source.as_str(), position), TypeTarget::Unknown);
        let query = String::from("owner");
        assert!(cache
            .get(&ComputedKey {
                source: &query,
                position
            })
            .is_some());
        assert!(cache
            .get(&ComputedKey {
                source: &query,
                position: SourcePosition { line: 7, column: 5 }
            })
            .is_none());
    }

    #[test]
    fn replay_preserves_repeated_write_indices_and_rejects_duplicate_positions() {
        struct Resolver;
        impl<'a> SemanticResolver<'a> for Resolver {
            fn resolve_symbol(
                &self,
                _scope: &'a Node,
                name: &str,
                _at: SourcePosition,
                _role: SymbolRole,
            ) -> Symbol<'a> {
                Symbol::Type(TypeTarget::Builtin(name.into()))
            }
        }
        let position = |line| SourcePosition { line, column: 0 };
        let binding = |line, name: &str| BindingFact {
            name: "value".into(),
            position: position(line),
            value: ValueExpr::Annotated {
                type_expr: TypeExpr::Named { name: name.into() },
            },
            conditional: false,
        };
        let flow = ValueFlowFacts {
            bindings: vec![
                binding(2, "str"),
                binding(2, "int"),
                binding(4, "str"),
                binding(6, "int"),
            ],
            ..Default::default()
        };
        let owner = Node {
            id: "owner".into(),
            kind: "function".into(),
            name: "owner".into(),
            qualname: "owner".into(),
            path: "test.py".into(),
            line: 1,
            end_line: 8,
            is_test: true,
            language: "python".into(),
            generated: false,
            details: serde_json::json!({"value_flow": flow}),
        };
        let mut all = BTreeMap::new();
        all.insert(
            owner.path.clone(),
            Facts {
                nodes: vec![owner],
                ..Default::default()
            },
        );
        let owner = &all["test.py"].nodes[0];
        let index = ValueFlowIndex::build(&all, &Resolver);
        assert!(matches!(
            index.lookup(owner, "value", position(3)),
            Some(TypeTarget::Unknown)
        ));
        assert!(
            matches!(index.lookup(owner, "value", position(5)), Some(TypeTarget::Builtin(name)) if name == "str")
        );
        assert!(
            matches!(index.lookup(owner, "value", position(7)), Some(TypeTarget::Builtin(name)) if name == "int")
        );
    }

    #[test]
    fn inferred_factory_returns_relink_bindings_and_leave_cycles_unknown() {
        fn node(name: &str, kind: &str, line: usize, flow: ValueFlowFacts) -> Node {
            Node {
                id: name.into(),
                kind: kind.into(),
                name: name.into(),
                qualname: name.into(),
                path: "test.py".into(),
                line,
                end_line: line + 1,
                is_test: true,
                language: "python".into(),
                generated: false,
                details: serde_json::json!({"value_flow": flow}),
            }
        }
        struct Resolver<'a>(&'a [Node]);
        impl<'a> SemanticResolver<'a> for Resolver<'a> {
            fn resolve_symbol(
                &self,
                _scope: &'a Node,
                name: &str,
                _at: SourcePosition,
                role: SymbolRole,
            ) -> Symbol<'a> {
                let Some(node) = self.0.iter().find(|node| node.name == name) else {
                    return Symbol::Unknown;
                };
                match role {
                    SymbolRole::Type | SymbolRole::Constructor if node.kind == "class" => {
                        Symbol::Type(TypeTarget::Local(node))
                    }
                    SymbolRole::Callable if node.kind == "function" => Symbol::Callable(node),
                    _ => Symbol::Unknown,
                }
            }
        }
        let position = |line| SourcePosition { line, column: 0 };
        let returning = |name: &str, line| ValueFlowFacts {
            return_value: Some(ValueExpr::Call {
                callee: name.into(),
            }),
            return_position: Some(position(line)),
            ..Default::default()
        };
        let nodes = vec![
            node("wrapper", "function", 1, returning("maker", 2)),
            node("maker", "function", 3, returning("Worker", 4)),
            node("Worker", "class", 5, ValueFlowFacts::default()),
            node("cycle_a", "function", 7, returning("cycle_b", 8)),
            node("cycle_b", "function", 9, returning("cycle_a", 10)),
            node(
                "use",
                "function",
                11,
                ValueFlowFacts {
                    bindings: vec![BindingFact {
                        name: "client".into(),
                        position: position(12),
                        value: ValueExpr::Call {
                            callee: "wrapper".into(),
                        },
                        conditional: false,
                    }],
                    ..Default::default()
                },
            ),
            node(
                "explicit_unknown",
                "function",
                14,
                ValueFlowFacts {
                    return_type: Some(TypeExpr::Unknown),
                    ..returning("Worker", 15)
                },
            ),
        ];
        let mut all = BTreeMap::new();
        all.insert(
            "test.py".into(),
            Facts {
                nodes,
                ..Default::default()
            },
        );
        let nodes = &all["test.py"].nodes;
        let index = ValueFlowIndex::build(&all, &Resolver(nodes));
        assert!(
            matches!(index.return_type(&nodes[0]), Some(TypeTarget::Local(node)) if node.name == "Worker")
        );
        assert!(
            matches!(index.lookup(&nodes[5], "client", position(13)), Some(TypeTarget::Local(node)) if node.name == "Worker")
        );
        assert!(matches!(
            index.return_type(&nodes[3]),
            Some(TypeTarget::Unknown)
        ));
        assert!(matches!(
            index.return_type(&nodes[4]),
            Some(TypeTarget::Unknown)
        ));
        assert!(matches!(
            index.return_type(&nodes[6]),
            Some(TypeTarget::Unknown)
        ));
    }

    #[test]
    fn ordinary_typescript_class_call_is_not_a_constructor() {
        struct Resolver<'a>(&'a Node);
        impl<'a> SemanticResolver<'a> for Resolver<'a> {
            fn resolve_symbol(
                &self,
                _scope: &'a Node,
                _name: &str,
                _at: SourcePosition,
                _role: SymbolRole,
            ) -> Symbol<'a> {
                Symbol::Type(TypeTarget::Local(self.0))
            }
        }
        let class = Node {
            id: "Worker".into(),
            kind: "class".into(),
            name: "Worker".into(),
            qualname: "Worker".into(),
            path: "test.ts".into(),
            line: 1,
            end_line: 2,
            is_test: true,
            language: "typescript".into(),
            generated: false,
            details: serde_json::json!({}),
        };
        let mut all = BTreeMap::new();
        all.insert(
            class.path.clone(),
            Facts {
                nodes: vec![class],
                ..Default::default()
            },
        );
        let class = &all["test.ts"].nodes[0];
        let resolver = Resolver(class);
        let index = ValueFlowIndex::build(&all, &resolver);
        let at = SourcePosition { line: 3, column: 0 };
        assert!(index
            .type_of_expression(class, "Worker", at, &resolver)
            .is_none());
        assert!(matches!(
            index.resolve_value(
                class,
                &ValueExpr::Call {
                    callee: "Worker".into()
                },
                at,
                &resolver
            ),
            TypeTarget::Unknown
        ));
        assert!(
            matches!(index.resolve_value(class, &ValueExpr::Construct { callee: "Worker".into() }, at, &resolver), TypeTarget::Local(node) if node.id == "Worker")
        );
    }
}
