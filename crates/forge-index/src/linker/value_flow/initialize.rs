use super::*;

impl<'a> ValueFlowIndex<'a> {
    pub(super) fn build_initialized<F: AsRef<Facts>>(
        all: &'a BTreeMap<String, F>,
        nodes: Vec<&'a Node>,
        interner: &super::super::interner::LinkerInterner<'a>,
        by_id: &HashMap<super::super::interner::SymbolId, &'a Node>,
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
            component_slots: HashMap::new(),
            local_types: HashMap::with_capacity(nodes.len()),
            computed: HashMap::new(),
            collection_elements: HashMap::new(),
            unknown: TypeTarget::Unknown,
            ambiguous: TypeTarget::Ambiguous,
            limits,
        };
        let initialized = initialize(&nodes, &by_name);
        for (node, facts, scope, returned) in initialized {
            if !scope.invalid
                && match node.language.as_str() {
                    "typescript" | "javascript" | "vue" => {
                        matches!(node.kind.as_str(), "function" | "method" | "lambda")
                    }
                    "python" => matches!(node.kind.as_str(), "function" | "method"),
                    _ => false,
                }
            {
                if let Some(async_type) = facts.async_return_type.as_ref() {
                    let returned =
                        resolve_type(node, async_type, node_position(node), resolver, limits);
                    if !matches!(returned, TypeTarget::Unknown | TypeTarget::Ambiguous) {
                        index.async_returns.insert(node.id.as_str(), returned);
                    }
                    if node.language == "python" {
                        if let Some(element) =
                            super::super::python::value_flow::collection_return_element(
                                node, async_type, resolver, limits,
                            )
                        {
                            index
                                .async_return_collection_elements
                                .insert(node.id.as_str(), element);
                        }
                    }
                }
            }
            if node.language == "python" && !scope.invalid {
                for fact in facts.collection_elements() {
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
                    if let Some(return_type) = facts.return_type.as_ref() {
                        if let Some(element) =
                            super::super::python::value_flow::collection_return_element(
                                node,
                                return_type,
                                resolver,
                                limits,
                            )
                        {
                            index
                                .return_collection_elements
                                .insert(node.id.as_str(), element);
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
            let facts = &raw[owner.id.as_str()];
            if owner.kind != "module"
                || !facts
                    .component_macros()
                    .any(|fact| fact.role == crate::core::semantic::ComponentMacroRole::Slots)
            {
                continue;
            }
            for slot in facts.vue_slots() {
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
                    .component_slots
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
            let facts = &raw[owner.id.as_str()];
            let typed = resolver.value_flow(owner);
            let mut bindings: Vec<_> = facts
                .bindings
                .iter()
                .enumerate()
                .map(|(event_index, binding)| BindingEvent {
                    name: typed.map_or_else(
                        || {
                            owner.details["value_flow"]["bindings"][event_index]["name"]
                                .as_str()
                                .expect("decoded binding name remains in arena")
                        },
                        |facts| facts.bindings[event_index].name.as_str(),
                    ),
                    position: binding.position,
                    value: &binding.value,
                    conditional: binding.conditional,
                    immutable_initializer: facts
                        .immutable_initializers
                        .binary_search(&binding.position)
                        .is_ok(),
                })
                .chain(
                    facts
                        .branch_joins
                        .iter()
                        .enumerate()
                        .map(|(event_index, join)| BindingEvent {
                            name: typed.map_or_else(
                                || {
                                    owner.details["value_flow"]["branch_joins"][event_index]["name"]
                                        .as_str()
                                        .expect("decoded branch-join name remains in arena")
                                },
                                |facts| facts.branch_joins[event_index].name.as_str(),
                            ),
                            position: join.position,
                            value: &join.value,
                            conditional: join.conditional,
                            immutable_initializer: false,
                        }),
                )
                .collect();
            bindings.sort_by_key(|binding| binding.position);
            let mut events = Vec::with_capacity(bindings.len());
            for binding in bindings {
                let loop_element = matches!(binding.value, ValueExpr::LoopElement { .. });
                let guarded_python =
                    owner.language == "python" && matches!(binding.value, ValueExpr::Scoped { .. });
                let target = if (binding.conditional && !loop_element && !guarded_python)
                    || index.scopes[owner.id.as_str()].invalid
                {
                    TypeTarget::Unknown
                } else {
                    index.evaluate(
                        owner,
                        binding.value,
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
                    let call = match binding.value {
                        ValueExpr::Call { callee, .. } => Some((callee.as_str(), false)),
                        ValueExpr::Await { value } => match value.as_ref() {
                            ValueExpr::Call { callee, .. } => Some((callee.as_str(), true)),
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
                                    .and_then(|names| names.get(binding.name))
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
                                .entry(binding.name.to_owned())
                                .or_default()
                                .push((binding.position, element));
                        }
                    }
                }
                let write = Binding {
                    position: binding.position,
                    target,
                    valid_until: match binding.value {
                        ValueExpr::LoopElement { body_end, .. } => Some(*body_end),
                        ValueExpr::Scoped { body_end, .. } => Some(*body_end),
                        _ => None,
                    },
                    restores_previous: owner.language != "python"
                        && matches!(binding.value, ValueExpr::Scoped { .. }),
                    immutable_initializer: binding.immutable_initializer,
                };
                use hashbrown::hash_map::Entry;
                let event_index = match index
                    .scopes
                    .get_mut(owner.id.as_str())
                    .expect("scope indexed")
                    .bindings
                    .entry(binding.name)
                {
                    Entry::Vacant(entry) => {
                        entry.insert(BindingList::One(write));
                        0
                    }
                    Entry::Occupied(mut entry) => {
                        let writes = entry.get_mut();
                        let event_index = writes.len();
                        writes.push(write);
                        event_index
                    }
                };
                events.push((binding, event_index));
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
        super::super::rust::value_flow::project_deref_fields(&mut index.fields, &owners, resolver);
        // Replay in source order so aliases of field-derived values see the summaries.
        for owner in &owners {
            for (binding, event_index) in &ordered[owner.id.as_str()] {
                let guarded_python =
                    owner.language == "python" && matches!(binding.value, ValueExpr::Scoped { .. });
                if (!binding.conditional || guarded_python)
                    && !index.scopes[owner.id.as_str()].invalid
                {
                    let writes = &index.scopes[owner.id.as_str()].bindings[binding.name];
                    if duplicate_position(writes, *event_index) {
                        index
                            .scopes
                            .get_mut(owner.id.as_str())
                            .expect("scope indexed")
                            .bindings
                            .get_mut(binding.name)
                            .expect("binding indexed")[*event_index]
                            .target = TypeTarget::Unknown;
                    } else if flow_dependent(binding.value) {
                        let target = index.evaluate(
                            owner,
                            binding.value,
                            binding.position,
                            resolver,
                            Some(&definitions),
                        );
                        index
                            .scopes
                            .get_mut(owner.id.as_str())
                            .expect("scope indexed")
                            .bindings
                            .get_mut(binding.name)
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
                                && matches!(binding.value, ValueExpr::Scoped { .. })))
                            && flow_dependent(binding.value)
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
                let writes = &index.scopes[owner.id.as_str()].bindings[binding.name];
                let trace = ReadTrace::new(&epochs, reads.take().unwrap_or_default());
                let target = if duplicate_position(writes, *event_index) {
                    TypeTarget::Unknown
                } else {
                    index.evaluate_tracked(
                        owner,
                        binding.value,
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
                    .get_mut(binding.name)
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
                super::super::rust::value_flow::project_deref_fields(
                    &mut fields,
                    &owners,
                    resolver,
                );
                if !same_fields(&index.fields, &fields) {
                    epochs.fields += 1;
                }
                index.fields = fields;
            }
        }
        for reference in all.values().flat_map(|facts| &facts.as_ref().references) {
            if !matches!(
                reference.receiver_hint,
                Some(
                    ReceiverHint::ConstructorResult { .. }
                        | ReceiverHint::CallResult { .. }
                        | ReceiverHint::TypedCallResult { .. }
                )
            ) {
                continue;
            }
            let Some(owner) = interner.lookup_node(by_id, reference.source.as_str()) else {
                continue;
            };
            let at = reference.position();
            let target = match reference.receiver_hint.as_ref() {
                Some(ReceiverHint::TypedCallResult { value, .. }) => {
                    if matches!(value, ValueExpr::Call { .. }) && valid_value(value, limits, 0) {
                        index.evaluate(owner, value, at, resolver, None)
                    } else {
                        TypeTarget::Unknown
                    }
                }
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
                    if let Some(rest) = callee.strip_prefix("value:") {
                        let rest = rest.trim();
                        if rest.starts_with('{') && rest.ends_with('}') {
                            if let Ok(value) = serde_json::from_str::<ValueExpr>(rest) {
                                if valid_value(&value, limits, 0) {
                                    index.evaluate(owner, &value, at, resolver, None)
                                } else {
                                    TypeTarget::Unknown
                                }
                            } else {
                                TypeTarget::Unknown
                            }
                        } else {
                            TypeTarget::Unknown
                        }
                    } else if callee.len() > limits.name_bytes
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
                        let (callee_name, is_try) = match callee.strip_suffix('?') {
                            Some(base) => (base, true),
                            None => (callee.as_str(), false),
                        };
                        let call_expr = ValueExpr::Call {
                            callee: callee_name.to_owned(),
                            site: None,
                        };
                        let expr = if is_try {
                            ValueExpr::Project {
                                value: Box::new(call_expr),
                                member: "?".to_owned(),
                            }
                        } else {
                            call_expr
                        };
                        index.evaluate(owner, &expr, at, resolver, None)
                    }
                }
                _ => continue,
            };
            index
                .computed
                .entry((
                    owner.id.as_str(),
                    at,
                    reference.expression.as_str(),
                    reference.kind.as_str(),
                ))
                .and_modify(|existing| *existing = merge(existing, &target))
                .or_insert(target);
        }
        index
    }
}
