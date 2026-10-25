use crate::core::{models::Node, semantic::ValueFlowFacts};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub(crate) enum FlowState {
    Valid { facts: Box<ValueFlowFacts>, original: Option<Value>, classic_fields_first: Option<bool> },
    Invalid(Value),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FlowStore {
    entries: std::collections::BTreeMap<String, FlowState>,
    uncertainties: std::collections::BTreeMap<String, Vec<crate::core::semantic::NamedDefinitionUncertainty>>,
    uncertainty_faults: std::collections::BTreeSet<String>, invalid_uncertainty_carriers: std::collections::BTreeSet<String>,
}

const SCOPE_FACT_KEYS: [&str; 16] = [
    "bindings", "branch_joins", "immutable_initializers", "fields",
    "symbol_attachments", "factory_results", "named_bindings", "endpoint_requests",
    "component_macros", "rule_origins", "component_configurations", "collection_elements",
    "template_contexts", "type_only_imports", "annotation_references", "vue_slots",
];

macro_rules! saturating_total {
    ($first:expr $(, $rest:expr)* $(,)?) => {
        [$first $(, $rest)*].into_iter().fold(0usize, usize::saturating_add)
    };
}

macro_rules! append_fields {
    ($target:ident, $incoming:ident; $($field:ident),+ $(,)?) => {
        $($target.$field.append(&mut $incoming.$field);)+
    };
}

macro_rules! merge_optional_fields {
    ($target:ident, $incoming:ident; $($field:ident),+ $(,)?) => {
        $($target.fact_limit_exceeded |= merge_optional(
            &mut $target.$field, $incoming.$field,
        );)+
    };
}

/// Returns the total count of all bounded scope fact arrays.
pub fn scope_fact_count(facts: &ValueFlowFacts) -> usize {
    saturating_total!(facts.bindings.len(), facts.branch_joins.len(), facts.immutable_initializers.len(),
        facts.fields.len(), facts.framework.as_deref().map_or(0, |framework| framework.facts.len()))
}

/// Counts the same bounded arrays on a persisted value-flow object.
pub fn encoded_scope_fact_count(encoded: &Value) -> usize {
    SCOPE_FACT_KEYS
        .into_iter()
        .map(|key| encoded[key].as_array().map_or(0, Vec::len))
        .fold(0usize, usize::saturating_add)
}

fn merge_optional<T: PartialEq>(previous: &mut Option<T>, incoming: Option<T>) -> bool {
    match (&*previous, incoming) { (None, value) => { *previous = value; false }, (Some(previous), Some(value)) => previous != &value, _ => false }
}

impl FlowStore {
    pub(crate) fn cache_entries(&self) -> impl ExactSizeIterator<Item = (&str, &FlowState)> { self.entries.iter().map(|(owner, state)| (owner.as_str(), state)) }
    pub(crate) fn restore_cache_entry(
        &mut self,
        owner: String,
        state: Option<FlowState>,
        uncertainties: Vec<crate::core::semantic::NamedDefinitionUncertainty>,
        uncertainty_fault: bool,
    ) {
        if let Some(state) = state { self.entries.insert(owner.clone(), state); }
        if !uncertainties.is_empty() { self.uncertainties.insert(owner.clone(), uncertainties); }
        if uncertainty_fault { self.uncertainty_faults.insert(owner); }
    }
    pub(crate) fn named_uncertainties(&self, owner: &str) -> &[crate::core::semantic::NamedDefinitionUncertainty] { self.uncertainties.get(owner).map_or(&[], Vec::as_slice) }
    pub(crate) fn named_uncertainty_entries(&self) -> impl ExactSizeIterator<Item = (&str, &[crate::core::semantic::NamedDefinitionUncertainty])> { self.uncertainties.iter().map(|(id, values)| (id.as_str(), values.as_slice())) }
    pub(crate) fn named_uncertainty_faults(&self) -> impl ExactSizeIterator<Item = &str> { self.uncertainty_faults.iter().map(String::as_str) }
    pub(crate) fn named_uncertainty_fault(&self, owner: &str) -> bool { self.uncertainty_faults.contains(owner) }
    pub(crate) fn has_invalid_named_uncertainty_carriers(&self) -> bool { !self.invalid_uncertainty_carriers.is_empty() }
    pub(crate) fn read_named_uncertainties(
        node: &Node,
    ) -> Result<Vec<crate::core::semantic::NamedDefinitionUncertainty>, ()> {
        let markers = Self::read_named_uncertainty_carrier(node)?;
        if node.details["named_definition_uncertainty_fault"] == true { Err(()) } else { Ok(markers) }
    }
    fn read_named_uncertainty_carrier(
        node: &Node,
    ) -> Result<Vec<crate::core::semantic::NamedDefinitionUncertainty>, ()> {
        let fault = node.details.get("named_definition_uncertainty_fault");
        if fault.is_some_and(|value| !value.is_boolean()) {
            return Err(());
        }
        let Some(raw) = node.details.get("named_definition_uncertainties") else {
            return Ok(Vec::new());
        };
        let values = raw.as_array().ok_or(())?;
        if values.len() > 128 * 128 {
            return Err(());
        }
        let markers: Vec<crate::core::semantic::NamedDefinitionUncertainty> =
            serde_json::from_value(raw.clone()).map_err(|_| ())?;
        let mut grouped: std::collections::BTreeMap<
            (&str, &str, &str),
            Vec<&crate::core::semantic::StaticString>,
        > = std::collections::BTreeMap::new();
        for marker in &markers {
            if !marker.valid() {
                return Err(());
            }
            let keys = grouped
                .entry((&marker.domain, &marker.origin.pack, &marker.origin.rule))
                .or_default();
            if keys.contains(&&marker.key) {
                return Err(());
            }
            keys.push(&marker.key);
            if keys.len() > 128
                || (keys.len() > 1 && keys.contains(&&crate::core::semantic::StaticString::Unknown))
            {
                return Err(());
            }
        }
        if grouped.len() > 128 {
            return Err(());
        }
        Ok(markers)
    }
    fn merge_uncertainties(
        &mut self,
        id: &str,
        incoming: impl IntoIterator<Item = crate::core::semantic::NamedDefinitionUncertainty>,
    ) {
        use crate::core::semantic::{NamedDefinitionUncertainty, StaticString};
        let mut groups: std::collections::BTreeMap<(String, String, String), Vec<StaticString>> =
            std::collections::BTreeMap::new();
        let previous = self.uncertainties.remove(id).unwrap_or_default();
        for marker in previous.into_iter().chain(incoming) {
            if !marker.valid() {
                self.uncertainty_faults.insert(id.to_owned());
                self.invalid_uncertainty_carriers.insert(id.to_owned());
                return;
            }
            let keys = groups
                .entry((marker.domain, marker.origin.pack, marker.origin.rule))
                .or_default();
            if !keys.contains(&StaticString::Unknown) && !keys.contains(&marker.key) {
                if marker.key == StaticString::Unknown || keys.len() == 128 {
                    keys.clear();
                    keys.push(StaticString::Unknown);
                } else {
                    keys.push(marker.key);
                }
            }
            if groups.len() > 128 {
                self.uncertainty_faults.insert(id.to_owned());
                return;
            }
        }
        let markers: Vec<_> = groups
            .into_iter()
            .flat_map(|((domain, pack, rule), keys)| {
                keys.into_iter().map(move |key| NamedDefinitionUncertainty {
                    domain: domain.clone(),
                    key,
                    origin: crate::core::semantic::ExtractionRuleOrigin {
                        pack: pack.clone(),
                        rule: rule.clone(),
                    },
                })
            })
            .collect();
        if !markers.is_empty() {
            self.uncertainties.insert(id.to_owned(), markers);
        }
    }
    fn capture_uncertainties(&mut self, id: &str, facts: &ValueFlowFacts) {
        let markers = facts
            .named_bindings()
            .filter(|fact| {
                matches!(
                    fact.role,
                    crate::core::semantic::NamedBindingRole::Definition { .. }
                        | crate::core::semantic::NamedBindingRole::RegistryDefinition { .. }
                )
            })
            .map(|fact| crate::core::semantic::NamedDefinitionUncertainty {
                domain: fact.domain.clone(),
                key: fact.key.clone(),
                origin: fact.origin.clone(),
            });
        self.merge_uncertainties(id, markers);
    }
    fn invalid_overflow(mut original: Value) -> FlowState {
        if let Some(facts) = original.as_object_mut() {
            facts.remove("type_only_imports"); facts.remove("annotation_references");
            facts.insert("fact_limit_exceeded".into(), Value::Bool(true));
        }
        FlowState::Invalid(original)
    }

    pub(crate) fn state(&self, node: &Node) -> Option<&FlowState> {
        self.entries.get(&node.id)
    }

    pub(crate) fn get(&self, node: &Node) -> Option<&ValueFlowFacts> {
        match self.state(node) {
            Some(FlowState::Valid { facts, .. }) => Some(facts), _ => None,
        }
    }

    pub(crate) fn raw(&self, node: &Node) -> Option<&Value> {
        match self.state(node) {
            Some(FlowState::Valid { original, .. }) => original.as_ref(), Some(FlowState::Invalid(value)) => Some(value), None => None,
        }
    }

    pub(crate) fn get_mut(&mut self, id: &str) -> Option<&mut ValueFlowFacts> {
        match self.entries.get_mut(id) {
            Some(FlowState::Valid { facts, original, .. }) => { *original = None; Some(facts) }, _ => None,
        }
    }

    fn exceeds_fact_limit(facts: &ValueFlowFacts) -> bool {
        scope_fact_count(facts) > crate::core::semantic::DEFAULT_FACTS_PER_SCOPE
            || facts.symbol_attachments().count() > 128 || facts.factory_results().count() > 128
            || facts.named_bindings().count() > 128 || facts.endpoint_requests().count() > 128
            || facts.factory_results().any(|fact| fact.members.len() > 128) || facts.fact_limit_exceeded
            || facts.bindings.iter().map(|fact| &fact.value).chain(facts.fields.iter().map(|fact| &fact.value))
                .chain(facts.branch_joins.iter().map(|fact| &fact.value)).chain(facts.return_value.iter())
                .any(|value| !value.valid_call_sites(0))
    }

    pub(crate) fn insert(&mut self, id: String, facts: ValueFlowFacts) { self.insert_with_mode(id, Box::new(facts), false); }
    pub(crate) fn insert_boxed(&mut self, id: String, facts: Box<ValueFlowFacts>) { self.insert_with_mode(id, facts, false); }
    pub(crate) fn insert_classic(&mut self, id: String, facts: ValueFlowFacts) { self.insert_with_mode(id, Box::new(facts), true); }

    fn insert_with_mode(&mut self, id: String, facts: Box<ValueFlowFacts>, classic: bool) {
        self.uncertainties.remove(&id);
        self.uncertainty_faults.remove(&id);
        self.invalid_uncertainty_carriers.remove(&id);
        if Self::exceeds_fact_limit(&facts) {
            self.capture_uncertainties(&id, &facts);
            let original = serde_json::to_value(facts)
                .expect("value-flow facts contain only JSON-compatible values");
            self.entries.insert(id, Self::invalid_overflow(original));
            return;
        }
        let classic_fields_first =
            classic.then_some(facts.bindings.is_empty() && !facts.fields.is_empty());
        self.entries.insert(
            id,
            FlowState::Valid {
                facts,
                original: None,
                classic_fields_first,
            },
        );
    }

    pub(crate) fn ingest(&mut self, node: &mut Node) {
        match Self::read_named_uncertainty_carrier(node) {
            Ok(markers) => {
                let has_markers = !markers.is_empty();
                self.merge_uncertainties(&node.id, markers);
                if has_markers {
                    if let Some(details) = node.details.as_object_mut() {
                        details.remove("named_definition_uncertainties");
                    }
                }
            }
            Err(()) => {
                self.uncertainty_faults.insert(node.id.clone());
                self.invalid_uncertainty_carriers.insert(node.id.clone());
            }
        }
        if node.details["named_definition_uncertainty_fault"] == true {
            self.uncertainty_faults.insert(node.id.clone());
            if let Some(details) = node.details.as_object_mut() {
                details.remove("named_definition_uncertainty_fault");
            }
        }
        let classic = node.kind == "template_scope"
            && node.language == "javascript"
            && node.details["classic_global"] == true;
        let Some(details) = node.details.as_object_mut() else {
            return;
        };
        let Some(original) = details.remove("value_flow") else {
            return;
        };
        if original.is_null() {
            return;
        }
        if encoded_scope_fact_count(&original) > crate::core::semantic::DEFAULT_FACTS_PER_SCOPE
            || original["fact_limit_exceeded"] == true
        {
            self.entries
                .insert(node.id.clone(), Self::invalid_overflow(original));
            return;
        }
        let classic_fields_first = classic.then(|| {
            original
                .as_object()
                .and_then(|object| object.keys().next())
                .is_some_and(|key| key == "fields")
        });
        let state = match ValueFlowFacts::deserialize_for_scope(&original, classic) {
            Ok(facts) => FlowState::Valid {
                facts: Box::new(facts),
                original: Some(original),
                classic_fields_first,
            },
            Err(_) => FlowState::Invalid(original),
        };
        self.entries.insert(node.id.clone(), state);
    }

    pub(crate) fn rename(&mut self, old: &str, new: String) {
        if self.invalid_uncertainty_carriers.remove(old) {
            self.invalid_uncertainty_carriers.insert(new.clone());
        }
        if let Some(markers) = self.uncertainties.remove(old) {
            self.uncertainties.insert(new.clone(), markers);
        }
        if self.uncertainty_faults.remove(old) {
            self.uncertainty_faults.insert(new.clone());
        }
        if let Some(state) = self.entries.remove(old) {
            self.entries.insert(new, state);
        }
    }

    /// Rewrite only explicit canonical operands after an embedded island is named.
    pub(crate) fn remap_nodes(&mut self, ids: &HashMap<String, String>) {
        use crate::core::semantic::{NamedBindingRole, SymbolOperand};
        let remap = |operand: &mut SymbolOperand| {
            if let SymbolOperand::Node { id } = operand {
                super::remap_identifier(id, ids);
            }
        };
        for state in self.entries.values_mut() {
            let FlowState::Valid {
                facts, original, ..
            } = state
            else {
                continue;
            };
            *original = None;
            let Some(framework) = facts.framework.as_mut() else {
                continue;
            };
            for fact in framework.symbol_attachments_mut() {
                remap(&mut fact.source);
                remap(&mut fact.target);
            }
            for fact in framework.named_bindings_mut() {
                match &mut fact.role {
                    NamedBindingRole::Definition { target, .. } => remap(target),
                    NamedBindingRole::RegistryDefinition { receiver, target } => {
                        remap(receiver);
                        remap(target);
                    }
                    NamedBindingRole::Reference => {}
                }
            }
        }
    }

    /// Move a complete classic fragment, retaining invalid raw evidence and bounds.
    pub(crate) fn merge_classic(&mut self, id: &str, other: &mut Self) {
        let incoming_invalid_carrier = other.invalid_uncertainty_carriers.remove(id);
        let invalid_carrier =
            self.invalid_uncertainty_carriers.contains(id) || incoming_invalid_carrier;
        let incoming_markers = other.uncertainties.remove(id).unwrap_or_default();
        let incoming_fault = other.uncertainty_faults.remove(id);
        let Some(incoming) = other.entries.remove(id) else {
            if invalid_carrier {
                self.invalid_uncertainty_carriers.insert(id.to_owned());
            }
            self.merge_uncertainties(id, incoming_markers);
            if incoming_fault {
                self.uncertainty_faults.insert(id.to_owned());
            }
            return;
        };
        let Some(previous) = self.entries.remove(id) else {
            let previous_markers = self.uncertainties.remove(id).unwrap_or_default();
            let previous_fault = self.uncertainty_faults.remove(id);
            match incoming {
                FlowState::Valid { facts, .. } => self.insert_classic(id.to_owned(), *facts),
                FlowState::Invalid(raw) => {
                    self.entries.insert(id.to_owned(), FlowState::Invalid(raw));
                }
            }
            self.merge_uncertainties(id, previous_markers);
            self.merge_uncertainties(id, incoming_markers);
            if incoming_fault || previous_fault {
                self.uncertainty_faults.insert(id.to_owned());
            }
            if invalid_carrier {
                self.invalid_uncertainty_carriers.insert(id.to_owned());
            }
            return;
        };
        if matches!(&previous, FlowState::Invalid(_)) || matches!(&incoming, FlowState::Invalid(_))
        {
            if let FlowState::Valid { facts, .. } = &previous {
                self.capture_uncertainties(id, facts);
            }
            if let FlowState::Valid { facts, .. } = &incoming {
                self.capture_uncertainties(id, facts);
            }
        }
        let merged = match (previous, incoming) {
            (FlowState::Invalid(raw), _) | (_, FlowState::Invalid(raw)) => FlowState::Invalid(raw),
            (
                FlowState::Valid {
                    mut facts,
                    classic_fields_first,
                    ..
                },
                FlowState::Valid {
                    facts: incoming, ..
                },
            ) => {
                let mut incoming = *incoming;
                append_fields!(facts, incoming; bindings, branch_joins, immutable_initializers, fields);
                if let Some(mut incoming_framework) = incoming.framework.take().map(|facts| *facts)
                {
                    if !incoming_framework.is_empty() {
                        let framework = facts.framework.get_or_insert_with(Default::default);
                        framework.fact_limit_exceeded |= incoming_framework.fact_limit_exceeded;
                        merge_optional_fields!(framework, incoming_framework;
                            object_ownership, async_return_type, return_value, return_position, alias_type,
                        );
                        framework.facts.append(&mut incoming_framework.facts);
                    }
                }
                facts.fact_limit_exceeded |=
                    merge_optional(&mut facts.return_type, incoming.return_type);
                if Self::exceeds_fact_limit(&facts) {
                    self.capture_uncertainties(id, &facts);
                    Self::invalid_overflow(
                        serde_json::to_value(facts).expect("serializable classic flow"),
                    )
                } else {
                    FlowState::Valid {
                        facts,
                        original: None,
                        classic_fields_first,
                    }
                }
            }
        };
        self.entries.insert(id.to_owned(), merged);
        if invalid_carrier {
            self.invalid_uncertainty_carriers.insert(id.to_owned());
        }
        self.merge_uncertainties(id, incoming_markers);
        if incoming_fault {
            self.uncertainty_faults.insert(id.to_owned());
        }
    }

    pub(crate) fn append(&mut self, other: &mut Self) {
        append_fields!(self, other; entries, uncertainties, uncertainty_faults, invalid_uncertainty_carriers);
    }

    pub(crate) fn retain_nodes(&mut self, ids: &std::collections::HashSet<String>) {
        self.entries.retain(|id, _| ids.contains(id)); self.uncertainties.retain(|id, _| ids.contains(id));
        self.uncertainty_faults.retain(|id| ids.contains(id)); self.invalid_uncertainty_carriers.retain(|id| ids.contains(id));
    }

    pub(crate) fn take(&mut self, id: &str) -> Option<ValueFlowFacts> {
        match self.entries.remove(id) {
            Some(FlowState::Valid { facts, .. }) => Some(*facts), _ => None,
        }
    }
}
