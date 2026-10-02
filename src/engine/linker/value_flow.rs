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
pub enum TypeTarget<'a> {
    Local(&'a Node),
    Callable(&'a Node),
    External { module: String, import_line: usize },
    Builtin(String),
    Object(Arc<HashMap<String, &'a Node>>),
    Ambiguous,
    Unknown,
}

#[derive(Clone, Copy, Debug)]
pub enum SymbolRole {
    Type,
    Constructor,
    Callable,
}

pub enum Symbol<'a> {
    Type(TypeTarget<'a>),
    Callable(&'a Node),
    Ambiguous,
    Unknown,
}

pub trait SemanticResolver<'a> {
    fn value_flow(&self, _node: &'a Node) -> Option<&'a ValueFlowFacts> {
        None
    }
    fn invalid_value_flow(&self, _node: &'a Node) -> bool {
        false
    }
    fn raw_value_flow(&self, _node: &'a Node) -> Option<&'a serde_json::Value> {
        None
    }
    fn has_typed_flows(&self) -> bool {
        false
    }
    fn resolve_symbol(
        &self,
        scope: &'a Node,
        name: &str,
        at: SourcePosition,
        role: SymbolRole,
    ) -> Symbol<'a>;
    fn resolve_external_factory(
        &self,
        _scope: &'a Node,
        _callee: &str,
        _at: SourcePosition,
    ) -> Option<TypeTarget<'a>> {
        None
    }
    fn resolve_member(
        &self,
        _receiver: &TypeTarget<'a>,
        _member: &str,
        _scope: &'a Node,
        _at: SourcePosition,
    ) -> Symbol<'a> {
        Symbol::Unknown
    }
    fn resolve_applied(
        &self,
        _scope: &'a Node,
        _base: &str,
        _args: &[TypeExpr],
        _at: SourcePosition,
    ) -> TypeTarget<'a> {
        TypeTarget::Unknown
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SemanticLimits {
    pub facts_per_scope: usize,
    pub name_bytes: usize,
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
}

struct Scope<'a> {
    parents: Vec<&'a str>,
    bindings: HashMap<String, Vec<Binding<'a>>>,
    invalid: bool,
    module: bool,
}

pub struct ValueFlowIndex<'a> {
    scopes: HashMap<&'a str, Scope<'a>>,
    returns: HashMap<&'a str, TypeTarget<'a>>,
    fields: HashMap<&'a str, HashMap<String, TypeTarget<'a>>>,
    local_types: HashMap<&'a str, TypeTarget<'a>>,
    computed: HashMap<(&'a str, SourcePosition), TypeTarget<'a>>,
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
        facts.bindings.len().saturating_add(facts.fields.len()) > limits.facts_per_scope
    }) || encoded["bindings"]
        .as_array()
        .map_or(0, Vec::len)
        .saturating_add(encoded["fields"].as_array().map_or(0, Vec::len))
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
                        resolve_type(
                            node,
                            &TypeExpr::Named {
                                name: name.to_owned(),
                            },
                            position,
                            resolver,
                            limits,
                        )
                    })
            };
            bindings.insert(name.clone(), vec![Binding { position, target }]);
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
            invalid,
            module: node.kind == "module",
        },
        returned,
    )
}

impl<'a> ValueFlowIndex<'a> {
    pub fn build(all: &'a BTreeMap<String, Facts>, resolver: &impl SemanticResolver<'a>) -> Self {
        Self::build_with_limits(all, resolver, SemanticLimits::default())
    }

    pub fn build_with_limits(
        all: &'a BTreeMap<String, Facts>,
        resolver: &impl SemanticResolver<'a>,
        limits: SemanticLimits,
    ) -> Self {
        Self::build_initialized(all, resolver, limits, |nodes, names| {
            nodes
                .iter()
                .map(|&node| initialize_scope(node, names, resolver, limits))
                .collect()
        })
    }

    pub(crate) fn build_parallel<F: AsRef<Facts>>(
        all: &'a BTreeMap<String, F>,
        resolver: &(impl SemanticResolver<'a> + Sync),
    ) -> Self {
        let limits = SemanticLimits::default();
        Self::build_initialized(all, resolver, limits, |nodes, names| {
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
        resolver: &impl SemanticResolver<'a>,
        limits: SemanticLimits,
        initialize: impl FnOnce(&[&'a Node], &NamedNodes<'a>) -> Vec<InitializedScope<'a>>,
    ) -> Self {
        let nodes: Vec<&Node> = all
            .values()
            .flat_map(|facts| &facts.as_ref().nodes)
            .collect();
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
            fields: HashMap::new(),
            local_types: HashMap::with_capacity(nodes.len()),
            computed: HashMap::new(),
            unknown: TypeTarget::Unknown,
            ambiguous: TypeTarget::Ambiguous,
            limits,
        };
        let initialized = initialize(&nodes, &by_name);
        for (node, facts, scope, returned) in initialized {
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
            let mut bindings: Vec<_> = raw[owner.id.as_str()].bindings.iter().collect();
            bindings.sort_by_key(|binding| binding.position);
            let mut events = Vec::with_capacity(bindings.len());
            for binding in bindings {
                let target = if binding.conditional || index.scopes[owner.id.as_str()].invalid {
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
                if !binding.conditional && !index.scopes[owner.id.as_str()].invalid {
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
                    .filter(|(binding, _)| !binding.conditional && flow_dependent(&binding.value))
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
        let by_id: HashMap<&str, &Node> = owners
            .iter()
            .map(|node| (node.id.as_str(), *node))
            .collect();
        for reference in all.values().flat_map(|facts| &facts.as_ref().references) {
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
                if writes.len() > 1 && !setup_complete {
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

    pub fn computed_receiver(&self, reference: &Reference) -> Option<&TypeTarget<'a>> {
        self.computed.get(&ComputedKey {
            source: reference.source.as_str(),
            position: SourcePosition {
                line: reference.line,
                column: reference.column,
            },
        })
    }

    pub fn field_type(&self, class: &Node, name: &str) -> Option<&TypeTarget<'a>> {
        self.field_type_tracked(class, name, None)
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

    pub fn object_member(target: &TypeTarget<'a>, name: &str) -> Option<&'a Node> {
        match target {
            TypeTarget::Object(members) => members.get(name).copied(),
            _ => None,
        }
    }

    pub fn resolve_value(
        &self,
        owner: &'a Node,
        value: &ValueExpr,
        at: SourcePosition,
        resolver: &impl SemanticResolver<'a>,
    ) -> TypeTarget<'a> {
        self.evaluate(owner, value, at, resolver, None)
    }

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
                } else if owner.language == "rust" {
                    let callee_clean = callee.replace("::", ".");
                    if let Some((receiver, member)) = callee_clean.rsplit_once('.') {
                        if matches!(
                            member,
                            "new" | "default" | "open" | "create" | "build" | "from_str" | "parse"
                        ) {
                            let type_sym =
                                resolver.resolve_symbol(owner, receiver, at, SymbolRole::Type);
                            if let Symbol::Type(
                                target @ (TypeTarget::Local(_) | TypeTarget::External { .. }),
                            ) = type_sym
                            {
                                return target;
                            }
                        }
                    }
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
                match resolver.resolve_member(&receiver_type, member, owner, at) {
                    Symbol::Callable(node) => TypeTarget::Callable(node),
                    _ => TypeTarget::Unknown,
                }
            }
            ValueExpr::Object { members } => {
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
                    if !matches!(node.kind.as_str(), "function" | "method")
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
}

fn node_position(node: &Node) -> SourcePosition {
    SourcePosition {
        line: node.line,
        column: node.details["column"].as_u64().unwrap_or(0) as usize,
    }
}

fn reaching<'a, 'b>(writes: &'b [Binding<'a>], at: SourcePosition) -> Option<&'b TypeTarget<'a>> {
    let end = writes.partition_point(|binding| binding.position < at);
    end.checked_sub(1).map(|index| &writes[index].target)
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

fn resolve_type<'a>(
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
        let alias = if let Some(alias) = resolver
            .value_flow(node)
            .and_then(|facts| facts.alias_type.as_ref())
        {
            Cow::Borrowed(alias)
        } else if let Some(raw) = resolver.raw_value_flow(node) {
            let encoded = &raw["alias_type"];
            if encoded.is_null() {
                return target;
            }
            let Ok(alias) = TypeExpr::deserialize(encoded) else {
                return TypeTarget::Unknown;
            };
            Cow::Owned(alias)
        } else if resolver.has_typed_flows() {
            return target;
        } else {
            let encoded = &node.details["value_flow"]["alias_type"];
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
    matches!(
        value,
        ValueExpr::Call { .. } | ValueExpr::Alias { .. } | ValueExpr::Field { .. }
    )
}

fn valid_facts(facts: &ValueFlowFacts, limits: SemanticLimits) -> bool {
    let text = |name: &str| !name.is_empty() && name.len() <= limits.name_bytes;
    let value = |value: &ValueExpr| match value {
        ValueExpr::Alias { name } => text(name),
        ValueExpr::Annotated { type_expr } => valid_type(type_expr, limits, 0),
        ValueExpr::Unknown => true,
        ValueExpr::Construct { callee } | ValueExpr::Call { callee } => text(callee),
        ValueExpr::Field { receiver, member } => text(receiver) && text(member),
        ValueExpr::Object { members } => {
            members.len() <= limits.facts_per_scope
                && members
                    .iter()
                    .all(|member| text(&member.name) && member.position.line > 0)
        }
    };
    facts.bindings.len().saturating_add(facts.fields.len()) <= limits.facts_per_scope
        && facts
            .bindings
            .iter()
            .all(|fact| text(&fact.name) && fact.position.line > 0 && value(&fact.value))
        && facts
            .fields
            .iter()
            .all(|fact| text(&fact.name) && fact.position.line > 0 && value(&fact.value))
        && facts
            .return_type
            .as_ref()
            .is_none_or(|ty| valid_type(ty, limits, 0))
        && facts
            .alias_type
            .as_ref()
            .is_none_or(|ty| valid_type(ty, limits, 0))
        && facts.return_value.as_ref().is_none_or(|returned| {
            value(returned)
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
                .saturating_add(facts.bindings.len())
                .saturating_add(facts.fields.len())
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
            },
            Binding {
                position: SourcePosition {
                    line: 4,
                    column: 20,
                },
                target: TypeTarget::Unknown,
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
