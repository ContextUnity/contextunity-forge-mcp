pub(crate) mod binary;
mod flow_store;
mod presentation;
mod semantic_store;

pub(crate) use flow_store::{encoded_scope_fact_count, scope_fact_count, FlowState, FlowStore};
pub(crate) use presentation::{NodeView, NodesView, ParameterTypesView, PublicNodesView};
pub use semantic_store::{NodeSemanticEntry, SemanticStore};
pub(crate) use semantic_store::{SemanticEntry, SemanticState};

use super::models::{Facts, Node};
use binary::{BinaryDecode, BinaryEncode};
use rayon::prelude::*;
use serde_json::Value;

const LOCAL_FACTS_BINARY_MAGIC: &[u8] = b"FORGE-LOCAL-FACTS\0\x03";

thread_local! {
    static LOCAL_FACTS_BUFFER: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    static LOCAL_FACTS_COMPRESSOR: std::cell::RefCell<Option<zstd::bulk::Compressor<'static>>> =
        const { std::cell::RefCell::new(None) };
    static LOCAL_FACTS_DECODER: std::cell::RefCell<Option<zstd::zstd_safe::DCtx<'static>>> =
        const { std::cell::RefCell::new(None) };
}

pub(crate) fn encode_facts(facts: &TypedFacts) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        !facts.flows.has_invalid_named_uncertainty_carriers(),
        "malformed named-definition uncertainty carrier"
    );
    LOCAL_FACTS_BUFFER.with(|buffer| -> anyhow::Result<Vec<u8>> {
        let mut raw = buffer.borrow_mut();
        raw.clear();
        facts.encode(&mut raw);
        LOCAL_FACTS_COMPRESSOR.with(|slot| -> anyhow::Result<Vec<u8>> {
            let mut slot = slot.borrow_mut();
            if slot.is_none() {
                *slot = Some(zstd::bulk::Compressor::new(1)?);
            }
            let compressor = slot.as_mut().expect("facts compressor initialized");
            let mut encoded = compressor.compress(&raw)?;
            encoded.reserve(LOCAL_FACTS_BINARY_MAGIC.len());
            encoded.splice(0..0, LOCAL_FACTS_BINARY_MAGIC.iter().copied());
            Ok(encoded)
        })
    })
}

pub(crate) fn decode_facts(blob: &[u8]) -> anyhow::Result<TypedFacts> {
    let compressed = blob
        .strip_prefix(LOCAL_FACTS_BINARY_MAGIC)
        .ok_or_else(|| anyhow::anyhow!("unsupported local facts format"))?;
    let facts = LOCAL_FACTS_DECODER.with(|slot| -> anyhow::Result<TypedFacts> {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(zstd::zstd_safe::DCtx::try_create().ok_or_else(|| {
                std::io::Error::other("failed to allocate facts decompression context")
            })?);
        }
        let context = slot.as_mut().expect("facts decoder initialized");
        context
            .init()
            .map_err(|code| std::io::Error::other(zstd::zstd_safe::get_error_name(code)))?;
        LOCAL_FACTS_BUFFER.with(|buffer| -> anyhow::Result<TypedFacts> {
            let mut raw = buffer.borrow_mut();
            raw.clear();
            let mut decoder = zstd::stream::read::Decoder::with_context(compressed, context);
            std::io::Read::read_to_end(&mut decoder, &mut raw)?;
            let mut cursor = 0;
            let facts = TypedFacts::decode(&raw, &mut cursor)
                .ok_or_else(|| anyhow::anyhow!("malformed binary local facts"))?;
            anyhow::ensure!(cursor == raw.len(), "trailing bytes in binary local facts");
            Ok(facts)
        })
    })?;
    anyhow::ensure!(
        !facts.flows.has_invalid_named_uncertainty_carriers(),
        "malformed named-definition uncertainty carrier"
    );
    Ok(facts)
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TypedFacts {
    pub(crate) facts: Facts,
    pub(crate) flows: FlowStore,
}

fn named_selector_reference(
    node: &Node,
    key: &super::semantic::StaticString,
    kind: &str,
    position: super::semantic::SourcePosition,
    domain: &str,
) -> super::models::Reference {
    super::models::Reference {
        source: node.id.clone(), line: position.line, column: position.column,
        expression: match key {
            super::semantic::StaticString::Literal { value } => value.clone(),
            super::semantic::StaticString::Unknown => String::new(),
        },
        kind: kind.to_owned(), alias: None, module: Some(domain.to_owned()), dynamic: false, receiver_hint: None,
    }
}

fn normalize_empty_details(node: &mut Node) {
    if node.details.as_object().is_some_and(serde_json::Map::is_empty) {
        node.details = Value::Null;
    }
}

pub(super) fn remap_identifier(
    value: &mut String,
    ids: &std::collections::HashMap<String, String>,
) -> bool {
    let Some(replacement) = ids.get(value) else { return false };
    value.clone_from(replacement);
    true
}

impl TypedFacts {
    /// Project compact selectors into existing indexed reference storage.
    pub(crate) fn project_named_selectors(&mut self) {
        use super::semantic::{NamedBindingRole, StaticString};
        for node in &self.facts.nodes {
            for marker in self.flows.named_uncertainties(&node.id) {
                self.facts.references.push(named_selector_reference(
                    node,
                    &marker.key,
                    "semantic_named_definition",
                    super::semantic::SourcePosition {
                        line: node.line,
                        column: 0,
                    },
                    &marker.domain,
                ));
            }
            let Some(flow) = self.flows.get(node) else {
                continue;
            };
            for fact in flow.named_bindings() {
                let kind = if matches!(
                    fact.role,
                    NamedBindingRole::Definition { .. }
                        | NamedBindingRole::RegistryDefinition { .. }
                ) {
                    "semantic_named_definition"
                } else {
                    "semantic_named_reference"
                };
                self.facts.references.push(named_selector_reference(
                    node,
                    &fact.key,
                    kind,
                    fact.position,
                    &fact.domain,
                ));
            }
            for fact in flow.endpoint_requests() {
                let (StaticString::Literal { value: url }, StaticString::Literal { value: method }) =
                    (&fact.url, &fact.method)
                else {
                    continue;
                };
                self.facts.references.push(super::models::Reference {
                    source: node.id.clone(),
                    expression: format!("{method} {url}"),
                    kind: "calls_endpoint".into(),
                    line: fact.position.line,
                    column: fact.position.column,
                    alias: None,
                    module: None,
                    dynamic: false,
                    receiver_hint: Some(super::models::ReceiverHint::ConfiguredReference {
                        origin: fact.origin.clone(),
                    }),
                });
            }
        }
    }
    pub(crate) fn nodes_view(&self) -> NodesView<'_, '_> { PublicNodesView::new(&self.facts, &self.flows) }
    pub(crate) fn push_node(&mut self, mut node: Node) {
        NodeSemanticEntry::ingest(&mut node);
        self.flows.ingest(&mut node);
        self.facts
            .semantic
            .ingest(self.facts.nodes.len(), &mut node);
        normalize_empty_details(&mut node);
        self.facts.nodes.push(node);
    }
    pub(crate) fn from_public(mut facts: Facts) -> Self {
        let mut flows = std::mem::take(&mut facts.semantic.flows);
        for (index, node) in facts.nodes.iter_mut().enumerate() {
            NodeSemanticEntry::ingest(node);
            flows.ingest(node);
            facts.semantic.ingest(index, node);
            normalize_empty_details(node);
        }
        facts.semantic.mark_complete();
        facts.pending_flow_nodes.clear();
        Self { facts, flows }
    }

    pub(crate) fn into_public(mut self) -> Facts {
        self.facts.semantic.flows = self.flows; self.facts.pending_flow_nodes.clear(); self.facts
    }

    /// Extracts canonical semantic reference keys from typed facts.
    pub(crate) fn reference_keys(&self) -> Vec<&str> { self.collect_reference_keys(false) }

    /// Extracts semantic reference keys from typed facts, optionally including component declarations.
    pub(crate) fn collect_reference_keys<'a>(&'a self, include_components: bool) -> Vec<&'a str> {
        let mut keys = Vec::new();
        let mut insert = |expression: &'a str| {
            keys.push(expression);
            for token in expression.split(['.', ':', '/']) {
                if !token.is_empty() && token.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    keys.push(token);
                }
            }
        };
        for expression in self
            .facts
            .references
            .iter()
            .filter(|r| !r.dynamic)
            .flat_map(|r| std::iter::once(r.expression.as_str()).chain(r.module.as_deref()))
            .chain(
                self.facts
                    .nodes
                    .iter()
                    .filter(|n| n.kind == "impl")
                    .map(|n| n.name.split('<').next().unwrap_or(&n.name).trim()),
            )
            .chain(
                self.facts
                    .nodes
                    .iter()
                    .filter(|n| include_components && n.kind == "component")
                    .map(|n| n.name.as_str()),
            )
            .chain(
                self.facts
                    .docs
                    .iter()
                    .flat_map(|d| d.referenced_symbols.iter().map(String::as_str)),
            )
        {
            insert(expression);
        }
        for reference in &self.facts.references {
            if let Some(crate::core::models::ReceiverHint::TypedCallResult { value, .. }) =
                &reference.receiver_hint
            {
                let mut current = value;
                for _ in 0..=8 {
                    let super::semantic::ValueExpr::Call { callee, site } = current else {
                        break;
                    };
                    insert(callee);
                    let Some(receiver) = site.as_ref().and_then(|site| site.receiver.as_deref())
                    else {
                        break;
                    };
                    current = receiver;
                }
            }
            if let Some(
                crate::core::models::ReceiverHint::CallResult { callee, .. }
                | crate::core::models::ReceiverHint::ConstructorResult { callee, .. },
            ) = &reference.receiver_hint
            {
                insert(callee);
            }
        }
        for node in &self.facts.nodes {
            if let Some(flow) = self.flows.get(node) {
                flow.reference_keys(&mut insert);
            } else if let Some(raw) = self.flows.raw(node) {
                crate::core::semantic::ValueFlowFacts::reference_keys_from_value(raw, &mut insert);
            } else {
                crate::core::semantic::ValueFlowFacts::reference_keys_from_details(
                    &node.details,
                    &mut insert,
                );
            }
            if let Some(exports) = node.details.get("exports").and_then(Value::as_array) {
                for export in exports {
                    for key in ["local", "module"] {
                        if let Some(name) = export.get(key).and_then(Value::as_str) {
                            insert(name);
                        }
                    }
                }
            }
            for (_, ty) in node.parameter_types() {
                if let Some(ty) = ty {
                    insert(ty);
                }
            }
        }
        if keys.len() >= 8192 {
            keys.par_sort_unstable();
        } else {
            keys.sort_unstable();
        }
        keys.dedup();
        keys
    }
}

impl std::ops::Deref for TypedFacts {
    type Target = Facts;
    fn deref(&self) -> &Facts { &self.facts }
}

impl std::ops::DerefMut for TypedFacts {
    fn deref_mut(&mut self) -> &mut Facts { &mut self.facts }
}

impl AsRef<Facts> for TypedFacts {
    fn as_ref(&self) -> &Facts { &self.facts }
}
