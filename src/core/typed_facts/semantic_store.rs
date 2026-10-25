use super::flow_store::FlowStore;
use crate::core::models::Node;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
#[doc(hidden)]
pub struct NodeSemanticEntry {
    pub(crate) bindings: Vec<String>,
    pub(crate) rebindings: Vec<String>,
    pub(crate) param_types: Vec<(String, String)>,
    pub(super) admitted: u8,
}

macro_rules! semantic_fields {
    ($($key:literal => $bit:literal => $field:ident),+ $(,)?) => {
        const SEMANTIC_FIELDS: &[(&str, u8)] = &[$(($key, $bit)),+];

        pub(crate) fn has(&self, key: &str) -> bool {
            match key { $($key => self.admitted & $bit != 0,)+ _ => false }
        }

        pub(crate) fn merge_from(&mut self, incoming: Self) {
            $(if incoming.admitted & $bit != 0 {
                self.$field.extend(incoming.$field);
                self.admitted |= $bit;
            })+
        }
    };
}

fn take_strings(values: Vec<Value>) -> Vec<String> {
    values.into_iter().map(|value| match value { Value::String(value) => value, _ => unreachable!() }).collect()
}

fn take_string_pairs(fields: serde_json::Map<String, Value>) -> Vec<(String, String)> {
    fields.into_iter().map(|(key, value)| match value { Value::String(value) => (key, value), _ => unreachable!() }).collect()
}

impl NodeSemanticEntry {
    semantic_fields!("bindings" => 1 => bindings, "rebindings" => 2 => rebindings, "param_types" => 4 => param_types);

    pub(crate) fn with_parameters(param_types: std::collections::BTreeMap<String, String>) -> Self {
        Self {
            param_types: param_types.into_iter().collect(),
            admitted: 4,
            ..Default::default()
        }
    }
    pub(crate) fn ingest(node: &mut Node) {
        let Some(details) = node.details.as_object_mut() else {
            return;
        };
        for &(key, bit) in Self::SEMANTIC_FIELDS {
            let Some(value) = details.remove(key) else {
                continue;
            };
            if value.is_null()
                && node
                    .native_semantic
                    .as_deref()
                    .is_some_and(|entry| entry.has(key))
            {
                continue;
            }
            let valid = match (key, &value) {
                ("param_types", Value::Object(fields)) => fields.values().all(Value::is_string),
                ("bindings" | "rebindings", Value::Array(values)) => values.iter().all(Value::is_string),
                _ => false,
            };
            if !valid {
                details.insert(key.to_owned(), value);
                if let Some(entry) = node.native_semantic.as_mut() {
                    entry.admitted &= !bit;
                    match key { "bindings" => entry.bindings.clear(), "rebindings" => entry.rebindings.clear(), _ => entry.param_types.clear() }
                }
                continue;
            }
            let empty = value.as_array().is_some_and(Vec::is_empty)
                || value.as_object().is_some_and(serde_json::Map::is_empty);
            if empty && node.native_semantic.is_none() {
                details.insert(key.to_owned(), value);
                continue;
            }
            let entry = node.native_semantic.get_or_insert_with(Default::default);
            entry.admitted |= bit;
            match (key, value) {
                ("param_types", Value::Object(fields)) => entry.param_types = take_string_pairs(fields),
                ("bindings", Value::Array(values)) => entry.bindings = take_strings(values),
                ("rebindings", Value::Array(values)) => entry.rebindings = take_strings(values),
                _ => unreachable!(),
            }
        }
        if node
            .native_semantic
            .as_deref()
            .is_some_and(|entry| entry.admitted == 0)
        {
            node.native_semantic = None;
        }
    }

    pub(crate) fn set_bindings(node: &mut Node, bindings: Vec<String>, rebindings: Vec<String>) {
        Self::ingest(node);
        if bindings.is_empty() && rebindings.is_empty() && node.native_semantic.is_none() {
            if let Some(details) = node.details.as_object_mut() {
                details.remove("bindings");
                details.remove("rebindings");
            }
            return;
        }
        let has_bindings = !bindings.is_empty();
        let has_rebindings = !rebindings.is_empty();
        let entry = node.native_semantic.get_or_insert_with(Default::default);
        entry.bindings = bindings;
        entry.rebindings = rebindings;
        entry.admitted |= 3;
        if let Some(details) = node.details.as_object_mut() {
            for (key, present) in [("bindings", has_bindings), ("rebindings", has_rebindings)] {
                if present && details.get(key).is_some_and(|value| value.as_array().is_some_and(Vec::is_empty)) { details.remove(key); }
            }
        }
    }

    pub(crate) fn parameters(&self) -> impl Iterator<Item = (&str, &str)> {
        self.param_types.iter().map(|(key, value)| (key.as_str(), value.as_str()))
    }
}

#[derive(Clone, Debug)]
pub(crate) enum SemanticState {
    Valid { facts: Vec<crate::core::semantic::SemanticFact>, original: Option<Value> },
    Invalid { original: Value, error: String },
}

#[derive(Clone, Debug)]
pub(crate) struct SemanticEntry {
    pub(crate) owner: String, pub(crate) line: usize, pub(crate) qualname: String, pub(crate) state: SemanticState,
}

#[derive(Clone, Debug, Default)]
#[doc(hidden)]
pub struct SemanticStore {
    entries: std::collections::BTreeMap<usize, SemanticEntry>, owners: HashMap<String, usize>,
    complete: bool, pub(crate) flows: FlowStore,
}

fn remap_semantic_fact(
    fact: &mut crate::core::semantic::SemanticFact,
    ids: &HashMap<String, String>,
    original: &mut Option<Value>,
) {
    let mut changed = super::remap_identifier(&mut fact.occurrence.owner_id, ids);
    changed |= match &mut fact.role {
        crate::core::semantic::SemanticFactRole::FileProvider { target } => {
            super::remap_identifier(target, ids)
        }
        crate::core::semantic::SemanticFactRole::CallToFile { import_owner, .. } => {
            super::remap_identifier(import_owner, ids)
        }
        _ => false,
    };
    if changed { *original = None; }
}

impl SemanticStore {
    fn decode(value: Value) -> SemanticState {
        match serde_json::from_value(value.clone()) {
            Ok(facts) => SemanticState::Valid {
                facts,
                original: Some(value),
            },
            Err(error) => SemanticState::Invalid {
                original: value,
                error: error.to_string(),
            },
        }
    }
    pub(crate) fn ingest(&mut self, index: usize, node: &mut Node) {
        let Some(value) = node
            .details
            .as_object_mut()
            .and_then(|details| details.remove("semantic_facts"))
        else {
            return;
        };
        // Native entries already own this reserved slot.
        if self.entries.contains_key(&index) {
            return;
        }
        let state = Self::decode(value);
        self.owners.entry(node.id.clone()).or_insert(index);
        self.entries.insert(
            index,
            SemanticEntry {
                owner: node.id.clone(),
                line: node.line,
                qualname: node.qualname.clone(),
                state,
            },
        );
    }
    pub(crate) fn push(
        &mut self,
        nodes: &mut [Node],
        fact: crate::core::semantic::SemanticFact,
    ) -> bool {
        let owner = &fact.occurrence.owner_id;
        let index = self
            .owners
            .get(owner)
            .copied()
            .filter(|i| nodes.get(*i).is_some_and(|n| n.id == *owner))
            .or_else(|| nodes.iter().position(|node| node.id == *owner));
        let Some(index) = index else {
            return false;
        };
        if !self.entries.contains_key(&index) {
            if let Some(details) = nodes[index].details.as_object_mut() {
                if details.get("semantic_facts").is_none_or(Value::is_null) {
                    details.remove("semantic_facts");
                }
            }
        }
        self.ingest(index, &mut nodes[index]);
        if let Some(entry) = self.entries.get_mut(&index) {
            if matches!(&entry.state,SemanticState::Invalid{original,..} if original.is_null()) {
                entry.state = SemanticState::Valid {
                    facts: Vec::new(),
                    original: None,
                };
            }
        }
        let entry = self.entries.entry(index).or_insert_with(|| {
            let node = &nodes[index];
            SemanticEntry {
                owner: node.id.clone(),
                line: node.line,
                qualname: node.qualname.clone(),
                state: SemanticState::Valid {
                    facts: Vec::new(),
                    original: None,
                },
            }
        });
        self.owners.entry(owner.clone()).or_insert(index);
        match &mut entry.state {
            SemanticState::Valid { facts, original } => {
                if let Some(original) = original {
                    let Some(values) = original.as_array_mut() else {
                        return false;
                    };
                    values.push(serde_json::to_value(&fact).expect("semantic DTO serializes"));
                }
                facts.push(fact);
                true
            }
            SemanticState::Invalid { original, error } => {
                let Some(values) = original.as_array_mut() else {
                    return false;
                };
                values.push(serde_json::to_value(fact).expect("semantic DTO serializes"));
                if let SemanticState::Invalid { error: new, .. } = Self::decode(original.clone()) {
                    *error = new;
                }
                true
            }
        }
    }
    pub(crate) fn state(&self, index: usize) -> Option<&SemanticState> { self.entries.get(&index).map(|entry| &entry.state) }
    pub(crate) fn entries(&self) -> impl ExactSizeIterator<Item = (&usize, &SemanticEntry)> { self.entries.iter() }
    pub(crate) fn is_complete(&self) -> bool { self.complete }
    pub(crate) fn mark_complete(&mut self) { self.complete = true; }
    pub(crate) fn restore_cache_entry(&mut self, index: usize, entry: SemanticEntry) {
        self.owners.entry(entry.owner.clone()).or_insert(index); self.entries.insert(index, entry);
    }
    pub(crate) fn remap_order(&mut self, map: &HashMap<usize, usize>) {
        self.entries = std::mem::take(&mut self.entries).into_iter().map(|(i, e)| (map.get(&i).copied().unwrap_or(i), e)).collect();
        for index in self.owners.values_mut() {
            if let Some(new) = map.get(index) {
                *index = *new;
            }
        }
    }
    pub(crate) fn retain_remapped(&mut self, map: &HashMap<usize, usize>) {
        self.entries.retain(|index, _| map.contains_key(index)); self.owners.retain(|_, index| map.contains_key(index)); self.remap_order(map);
    }
    pub(crate) fn append_remapped(
        &mut self,
        other: &mut Self,
        map: &HashMap<usize, usize>,
        ids: &HashMap<String, String>,
    ) {
        for (index, mut entry) in std::mem::take(&mut other.entries) {
            let Some(&index) = map.get(&index) else {
                continue;
            };
            if let Some(id) = ids.get(&entry.owner) { entry.owner.clone_from(id); }
            if let SemanticState::Valid { facts, original } = &mut entry.state {
                for fact in facts.iter_mut() { remap_semantic_fact(fact, ids, original); }
            }
            if let Some(previous) = self.entries.get_mut(&index) {
                if let (
                    SemanticState::Valid {
                        facts: previous,
                        original: None,
                    },
                    SemanticState::Valid {
                        facts: incoming,
                        original: None,
                    },
                ) = (&mut previous.state, &mut entry.state)
                {
                    previous.append(incoming);
                    continue;
                }
                let mut raw =
                    serde_json::to_value(previous.state.view()).expect("semantic facts serialize");
                let incoming =
                    serde_json::to_value(entry.state.view()).expect("semantic facts serialize");
                if let (Some(values), Value::Array(incoming)) = (raw.as_array_mut(), incoming) {
                    values.extend(incoming);
                }
                previous.state = Self::decode(raw);
            } else {
                self.owners.entry(entry.owner.clone()).or_insert(index);
                self.entries.insert(index, entry);
            }
        }
        other.owners.clear();
        other.flows.remap_nodes(ids);
        self.flows.append(&mut other.flows);
    }
}
