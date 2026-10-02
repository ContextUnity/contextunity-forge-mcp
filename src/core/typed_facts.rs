use super::models::{Facts, Node};
use super::semantic::ValueFlowFacts;
use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use std::collections::HashMap;

pub(crate) fn encode_facts(facts: &TypedFacts) -> anyhow::Result<Vec<u8>> {
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 1)?;
    {
        let mut writer = std::io::BufWriter::with_capacity(64 * 1024, &mut encoder);
        serde_json::to_writer(&mut writer, facts)?;
        std::io::Write::flush(&mut writer)?;
    }
    Ok(encoder.finish()?)
}

pub(crate) fn decode_facts(blob: &[u8]) -> anyhow::Result<TypedFacts> {
    let decoder = zstd::stream::read::Decoder::new(blob)?;
    Ok(serde_json::from_reader(decoder)?)
}

#[derive(Clone, Debug)]
pub(crate) enum FlowState {
    Valid {
        facts: Box<ValueFlowFacts>,
        original: Option<Value>,
        classic_fields_first: Option<bool>,
    },
    Invalid(Value),
}

impl Serialize for FlowState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Valid {
                original: Some(value),
                ..
            }
            | Self::Invalid(value) => value.serialize(serializer),
            Self::Valid {
                facts,
                original: None,
                classic_fields_first: Some(fields_first),
            } => ClassicFlow {
                facts,
                fields_first: *fields_first,
            }
            .serialize(serializer),
            Self::Valid {
                facts,
                original: None,
                classic_fields_first: None,
            } => facts.serialize(serializer),
        }
    }
}

struct ClassicFlow<'a> {
    facts: &'a ValueFlowFacts,
    fields_first: bool,
}

impl Serialize for ClassicFlow<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        let bindings = (!self.facts.bindings.is_empty()).then_some(&self.facts.bindings);
        let fields = (!self.facts.fields.is_empty()).then_some(&self.facts.fields);
        if self.fields_first {
            map.serialize_entry("fields", &fields)?;
            map.serialize_entry("bindings", &bindings)?;
        } else {
            map.serialize_entry("bindings", &bindings)?;
            map.serialize_entry("fields", &fields)?;
        }
        map.end()
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FlowStore {
    entries: HashMap<String, FlowState>,
}

impl FlowStore {
    pub(crate) fn state(&self, node: &Node) -> Option<&FlowState> {
        self.entries.get(&node.id)
    }

    pub(crate) fn get(&self, node: &Node) -> Option<&ValueFlowFacts> {
        match self.state(node) {
            Some(FlowState::Valid { facts, .. }) => Some(facts),
            _ => None,
        }
    }

    pub(crate) fn raw(&self, node: &Node) -> Option<&Value> {
        match self.state(node) {
            Some(FlowState::Valid { original, .. }) => original.as_ref(),
            Some(FlowState::Invalid(value)) => Some(value),
            None => None,
        }
    }

    pub(crate) fn get_mut(&mut self, id: &str) -> Option<&mut ValueFlowFacts> {
        match self.entries.get_mut(id) {
            Some(FlowState::Valid {
                facts, original, ..
            }) => {
                *original = None;
                Some(facts)
            }
            _ => None,
        }
    }

    pub(crate) fn insert(&mut self, id: String, facts: ValueFlowFacts) {
        self.entries.insert(
            id,
            FlowState::Valid {
                facts: Box::new(facts),
                original: None,
                classic_fields_first: None,
            },
        );
    }

    pub(crate) fn insert_classic(&mut self, id: String, facts: ValueFlowFacts) {
        let fields_first = facts.bindings.is_empty() && !facts.fields.is_empty();
        self.entries.insert(
            id,
            FlowState::Valid {
                facts: Box::new(facts),
                original: None,
                classic_fields_first: Some(fields_first),
            },
        );
    }

    pub(crate) fn ingest(&mut self, node: &mut Node) {
        let classic = node.kind == "template_scope"
            && node.language == "javascript"
            && node.details["classic_global"] == true;
        let Some(value) = node.details.get_mut("value_flow") else {
            return;
        };
        if value.is_null() {
            return;
        }
        let original = value.take();
        let limit = super::semantic::DEFAULT_FACTS_PER_SCOPE;
        if original["bindings"]
            .as_array()
            .map_or(0, Vec::len)
            .saturating_add(original["fields"].as_array().map_or(0, Vec::len))
            > limit
        {
            self.entries
                .insert(node.id.clone(), FlowState::Invalid(original));
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
        if let Some(state) = self.entries.remove(old) {
            self.entries.insert(new, state);
        }
    }

    pub(crate) fn append(&mut self, other: &mut Self) {
        self.entries.extend(other.entries.drain());
    }

    pub(crate) fn take(&mut self, id: &str) -> Option<ValueFlowFacts> {
        match self.entries.remove(id) {
            Some(FlowState::Valid { facts, .. }) => Some(*facts),
            _ => None,
        }
    }

    pub(crate) fn details<'a>(&'a self, node: &'a Node) -> DetailsView<'a> {
        DetailsView {
            node,
            flow: self.state(node),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TypedFacts {
    pub(crate) facts: Facts,
    pub(crate) flows: FlowStore,
}

impl TypedFacts {
    pub(crate) fn nodes_view(&self) -> NodesView<'_> {
        NodesView(self)
    }
    pub(crate) fn push_node(&mut self, mut node: Node) {
        self.flows.ingest(&mut node);
        self.facts.nodes.push(node);
    }
    pub(crate) fn from_public(mut facts: Facts) -> Self {
        let mut flows = FlowStore::default();
        for node in &mut facts.nodes {
            flows.ingest(node);
        }
        Self { facts, flows }
    }

    pub(crate) fn into_public(mut self) -> Facts {
        for node in &mut self.facts.nodes {
            if let Some(state) = self.flows.state(node) {
                node.details["value_flow"] = serde_json::to_value(state)
                    .expect("value-flow facts contain only JSON-compatible values");
            }
        }
        self.facts
    }
}

impl std::ops::Deref for TypedFacts {
    type Target = Facts;
    fn deref(&self) -> &Facts {
        &self.facts
    }
}

impl std::ops::DerefMut for TypedFacts {
    fn deref_mut(&mut self) -> &mut Facts {
        &mut self.facts
    }
}

impl AsRef<Facts> for TypedFacts {
    fn as_ref(&self) -> &Facts {
        &self.facts
    }
}

pub(crate) struct NodesView<'a>(&'a TypedFacts);

impl Serialize for NodesView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.facts.nodes.len()))?;
        for node in &self.0.facts.nodes {
            sequence.serialize_element(&NodeView {
                node,
                flows: &self.0.flows,
            })?;
        }
        sequence.end()
    }
}

pub(crate) struct DetailsView<'a> {
    node: &'a Node,
    flow: Option<&'a FlowState>,
}

impl std::fmt::Display for DetailsView<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        struct Writer<'a, 'b>(&'a mut std::fmt::Formatter<'b>);
        impl std::io::Write for Writer<'_, '_> {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                let text = std::str::from_utf8(bytes).map_err(std::io::Error::other)?;
                self.0.write_str(text).map_err(std::io::Error::other)?;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        serde_json::to_writer(Writer(formatter), self).map_err(|_| std::fmt::Error)
    }
}

impl Serialize for DetailsView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Some(flow) = self.flow else {
            return self.node.details.serialize(serializer);
        };
        let Some(details) = self.node.details.as_object() else {
            return self.node.details.serialize(serializer);
        };
        let mut map = serializer.serialize_map(Some(details.len()))?;
        for (key, value) in details {
            if key == "value_flow" {
                map.serialize_entry(key, flow)?;
            } else {
                map.serialize_entry(key, value)?;
            }
        }
        map.end()
    }
}

pub(crate) struct NodeView<'a> {
    pub(crate) node: &'a Node,
    pub(crate) flows: &'a FlowStore,
}

impl Serialize for NodeView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let node = self.node;
        let mut state = serializer.serialize_struct("Node", 11)?;
        state.serialize_field("id", &node.id)?;
        state.serialize_field("kind", &node.kind)?;
        state.serialize_field("name", &node.name)?;
        state.serialize_field("qualname", &node.qualname)?;
        state.serialize_field("path", &node.path)?;
        state.serialize_field("line", &node.line)?;
        state.serialize_field("end_line", &node.end_line)?;
        state.serialize_field("is_test", &node.is_test)?;
        state.serialize_field("language", &node.language)?;
        state.serialize_field("generated", &node.generated)?;
        state.serialize_field("details", &self.flows.details(node))?;
        state.end()
    }
}

impl Serialize for TypedFacts {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Facts", 5)?;
        state.serialize_field("nodes", &self.nodes_view())?;
        state.serialize_field("edges", &self.facts.edges)?;
        state.serialize_field("references", &self.facts.references)?;
        state.serialize_field("docs", &self.facts.docs)?;
        state.serialize_field("errors", &self.facts.errors)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for TypedFacts {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Facts::deserialize(deserializer).map(Self::from_public)
    }
}
