#[doc(hidden)]
pub use super::typed_facts::NodeSemanticEntry;
#[doc(hidden)]
pub use super::typed_facts::SemanticStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
pub(crate) mod compact_graph;
#[derive(Debug, Clone, Deserialize)]
/// Indexed source declaration with a stable identity and compact metadata projection.
pub struct Node {
    /// Stable symbol identifier used by graph edges and selectors.
    pub id: String,
    /// Semantic category such as `function`, `method`, or `class`.
    pub kind: String,
    /// Unqualified declaration name.
    pub name: String,
    /// Fully qualified name within the language namespace.
    pub qualname: String,
    /// Workspace-relative source path.
    pub path: String,
    /// One-based starting source line.
    pub line: usize,
    /// One-based final source line.
    pub end_line: usize,
    /// Whether the declaration belongs to test code.
    pub is_test: bool,
    /// Language profile identifier used for extraction.
    pub language: String,
    /// Whether the declaration came from generated source.
    pub generated: bool,
    /// Small JSON projection needed for navigation; full AST facts stay separate.
    pub details: Value,
    #[serde(skip)]
    #[doc(hidden)]
    pub native_semantic: Option<Box<NodeSemanticEntry>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
/// Typed semantic metadata for declaration bindings, rebindings, and receiver hints.
pub struct NodeSemanticMeta {
    /// Resolved bindings for the declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<String>,
    /// Resolved rebindings or alias names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rebindings: Vec<String>,
    /// Raw receiver expression (e.g. `(s *Service)` in Go or `self` in Python).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
    /// Resolved receiver type name (e.g. `Service` in Go, struct name in Rust, or prototype in TS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_type: Option<String>,
    /// Optional receiver type hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_hint: Option<String>,
    /// Parameter type annotations.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub param_types: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Represents a scanned source or documentation file candidate.
pub struct FileEntry {
    /// Workspace-relative path to the file.
    pub path: String,
    /// Content digest of the file bytes.
    pub digest: String,
    /// File size in bytes.
    pub bytes: u64,
    /// Modification time in nanoseconds.
    pub mtime_ns: u128,
    #[serde(default)]
    /// Device, inode, and mtime identity triple.
    pub identity: [u64; 3],
    /// Canonical language profile ID or detected language.
    pub language: String,
    /// Whether the file is indexed as documentation.
    pub is_doc: bool,
}

impl Serialize for Node {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        super::typed_facts::NodeView::new(self, &super::typed_facts::FlowStore::default(), None)
            .serialize(serializer)
    }
}

impl Node {
    fn semantic_entry(&self, key: &str) -> Option<&NodeSemanticEntry> {
        self.native_semantic
            .as_deref()
            .filter(|entry| entry.has(key))
    }

    pub(crate) fn has_binding_names(&self) -> bool {
        self.semantic_entry("bindings").is_some() || self.details["bindings"].is_array()
    }
    pub(crate) fn binding_count(&self) -> usize {
        self.semantic_entry("bindings").map_or_else(
            || self.details["bindings"].as_array().map_or(0, Vec::len),
            |entry| entry.bindings.len(),
        )
    }
    pub(crate) fn rebinding_count(&self) -> usize {
        self.semantic_entry("rebindings").map_or_else(
            || self.details["rebindings"].as_array().map_or(0, Vec::len),
            |entry| entry.rebindings.len(),
        )
    }
    pub(crate) fn parameter_count(&self) -> usize {
        self.semantic_entry("param_types").map_or_else(
            || {
                self.details["param_types"]
                    .as_object()
                    .map_or(0, serde_json::Map::len)
            },
            |entry| entry.param_types.len(),
        )
    }
    pub(crate) fn has_parameter(&self, name: &str) -> bool {
        self.semantic_entry("param_types").map_or_else(
            || self.details["param_types"].get(name).is_some(),
            |entry| entry.param_types.iter().any(|(key, _)| key == name),
        )
    }
    pub(crate) fn binding_names(&self) -> impl Iterator<Item = &str> {
        self.semantic_entry("bindings")
            .into_iter()
            .flat_map(|entry| entry.bindings.iter().map(String::as_str))
            .chain(
                self.details["bindings"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str),
            )
    }
    pub(crate) fn rebinding_names(&self) -> impl Iterator<Item = &str> {
        self.semantic_entry("rebindings")
            .into_iter()
            .flat_map(|entry| entry.rebindings.iter().map(String::as_str))
            .chain(
                self.details["rebindings"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str),
            )
    }
    pub(crate) fn parameter_types(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.semantic_entry("param_types")
            .into_iter()
            .flat_map(|entry| entry.parameters().map(|(name, ty)| (name, Some(ty))))
            .chain(
                self.details["param_types"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(name, ty)| (name.as_str(), ty.as_str())),
            )
    }
    pub(crate) fn parameter_type(&self, name: &str) -> Option<&str> {
        if let Some(entry) = self.semantic_entry("param_types") {
            entry
                .parameters()
                .find_map(|(key, ty)| (key == name).then_some(ty))
        } else {
            self.details["param_types"]
                .get(name)
                .and_then(Value::as_str)
        }
    }
    pub(crate) fn parameter_types_value(&self) -> Value {
        if let Some(entry) = self.semantic_entry("param_types") {
            serde_json::to_value(super::typed_facts::ParameterTypesView(entry))
                .expect("typed parameters serialize")
        } else {
            self.details["param_types"].clone()
        }
    }
}

impl Node {
    #[allow(dead_code)]
    pub(crate) fn navigation_details(&self) -> NavigationDetails<'_> {
        NavigationDetails {
            node: self,
            documentation: None,
        }
    }

    pub(crate) fn navigation_details_with_documentation<'a>(
        &'a self,
        documentation: &'a Value,
    ) -> NavigationDetails<'a> {
        NavigationDetails {
            node: self,
            documentation: Some(documentation),
        }
    }

    fn semantic_field<T: serde::de::DeserializeOwned + Default>(&self, key: &str) -> T {
        self.details
            .get(key)
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default()
    }

    /// Extracts typed semantic metadata from node details.
    pub fn semantic_meta(&self) -> NodeSemanticMeta {
        let bindings = self
            .semantic_entry("bindings")
            .map(|entry| entry.bindings.clone())
            .unwrap_or_else(|| self.semantic_field("bindings"));
        let rebindings = self
            .semantic_entry("rebindings")
            .map(|entry| entry.rebindings.clone())
            .unwrap_or_else(|| self.semantic_field("rebindings"));
        let receiver = self
            .details
            .get("receiver")
            .and_then(|v| v.as_str().map(str::to_owned));
        let receiver_type = self
            .details
            .get("receiver_type")
            .and_then(|v| v.as_str().map(str::to_owned));
        let receiver_hint = self
            .details
            .get("receiver_hint")
            .and_then(|v| v.as_str().map(str::to_owned));
        let param_types = self
            .semantic_entry("param_types")
            .map(|entry| entry.param_types.iter().cloned().collect())
            .unwrap_or_else(|| self.semantic_field("param_types"));
        NodeSemanticMeta {
            bindings,
            rebindings,
            receiver,
            receiver_type,
            receiver_hint,
            param_types,
        }
    }

    /// Applies typed semantic metadata into node details.
    pub fn apply_semantic_meta(&mut self, meta: NodeSemanticMeta) {
        self.native_semantic = None;
        if !self.details.is_object() {
            self.details = serde_json::json!({});
        }
        if let Some(details) = self.details.as_object_mut() {
            for (key, value) in [
                (
                    "bindings",
                    (!meta.bindings.is_empty()).then(|| serde_json::json!(meta.bindings)),
                ),
                (
                    "rebindings",
                    (!meta.rebindings.is_empty()).then(|| serde_json::json!(meta.rebindings)),
                ),
                (
                    "param_types",
                    (!meta.param_types.is_empty()).then(|| serde_json::json!(meta.param_types)),
                ),
                ("receiver", meta.receiver.map(Value::String)),
                ("receiver_type", meta.receiver_type.map(Value::String)),
                ("receiver_hint", meta.receiver_hint.map(Value::String)),
            ] {
                if let Some(value) = value {
                    details.insert(key.into(), value);
                } else {
                    details.remove(key);
                }
            }
        }
    }
}

pub(crate) struct NavigationDetails<'a> {
    node: &'a Node,
    documentation: Option<&'a Value>,
}

impl Serialize for NavigationDetails<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let details = self.node.details.as_object();
        let documentation = self.documentation.and_then(Value::as_object);
        let semantic = self.node.native_semantic.as_deref();
        if details.is_none() && documentation.is_none() && semantic.is_none() {
            return self.node.details.serialize(serializer);
        }
        let mut map = serializer.serialize_map(None)?;
        if let Some(details) = details {
            for (key, value) in details {
                if self.documentation.is_some() && matches!(key.as_str(), "doc" | "docstring") {
                    continue;
                }
                if !matches!(key.as_str(), "bindings" | "rebindings" | "param_types")
                    && !include_navigation_detail(key, value)
                {
                    continue;
                }
                map.serialize_entry(key, value)?;
            }
        }
        if let Some(semantic) = semantic {
            let has_detail = |key: &str| details.is_some_and(|details| details.contains_key(key));
            if semantic.has("bindings") && !has_detail("bindings") {
                map.serialize_entry("bindings", &semantic.bindings)?;
            }
            if semantic.has("rebindings") && !has_detail("rebindings") {
                map.serialize_entry("rebindings", &semantic.rebindings)?;
            }
            if semantic.has("param_types") && !has_detail("param_types") {
                map.serialize_entry(
                    "param_types",
                    &super::typed_facts::ParameterTypesView(semantic),
                )?;
            }
        }
        if let Some(documentation) = documentation {
            for key in ["doc", "docstring"] {
                if let Some(value) = documentation
                    .get(key)
                    .filter(|value| include_navigation_detail(key, value))
                {
                    map.serialize_entry(key, value)?;
                }
            }
        }
        map.end()
    }
}

fn include_navigation_detail(key: &str, value: &Value) -> bool {
    !matches!(
        key,
        "value_flow"
            | "bindings"
            | "rebindings"
            | "param_types"
            | "column"
            | "semantic_facts"
            | "named_definition_uncertainties"
            | "named_definition_uncertainty_fault"
    ) && !value.is_null()
        && !value.as_str().is_some_and(str::is_empty)
        && !value.as_array().is_some_and(Vec::is_empty)
        && !(matches!(key, "async" | "default_export") && value == &Value::Bool(false))
}
#[derive(Debug, Clone, Serialize, Deserialize)]
/// Directed relationship between two indexed declarations.
pub struct Edge {
    /// Stable identifier of the source declaration.
    pub src: String,
    /// Stable identifier of the target declaration.
    pub dst: String,
    /// Relationship category, such as a call or import.
    pub kind: String,
    /// Workspace-relative path containing the occurrence.
    pub path: String,
    /// One-based source line of the occurrence.
    pub line: usize,
    /// Evidence expression captured at the occurrence.
    pub evidence: String,
    /// Resolution confidence or status.
    pub confidence: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
/// Syntax evidence that helps resolve a reference or its receiver.
pub enum ReceiverHint {
    /// A typed component field admitted by a source-proven macro rule.
    ConfiguredTypedMember {
        /// Actual effective operation identity.
        origin: crate::core::semantic::ExtractionRuleOrigin,
        /// Exact indexed field identity.
        field_id: String,
    },
    /// A callable result admitted by an existing component macro role.
    ConfiguredMacroCallable {
        /// Actual effective operation identity.
        origin: crate::core::semantic::ExtractionRuleOrigin,
        /// Lexical producer whose shadowing invalidates this evidence.
        producer: String,
    },
    /// A reference whose interpretation was admitted by a finite extraction rule.
    ConfiguredReference {
        /// Actual effective pack and operation identity.
        origin: crate::core::semantic::ExtractionRuleOrigin,
    },
    /// A named route key admitted by an effective extraction rule.
    NamedRoute {
        /// Authoritative originating manifest and rule.
        origin: crate::core::semantic::ExtractionRuleOrigin,
        /// Literal route key, independent of display expression spelling.
        key: String,
    },
    /// Member access on a string literal receiver.
    StringLiteral {
        /// Accessed member name.
        member: String,
    },
    /// Member access on the result of a call.
    CallResult {
        /// Called expression that supplies the receiver.
        callee: String,
        /// Accessed member name.
        member: String,
    },
    /// Member access on a bounded, canonical typed call result.
    TypedCallResult {
        /// Canonical call expression and its original source evidence.
        #[serde(deserialize_with = "crate::core::semantic::deserialize_typed_call")]
        value: crate::core::semantic::ValueExpr,
        /// Accessed member.
        member: String,
    },
    /// Member access on an object constructed by a call.
    ConstructorResult {
        /// Constructor expression that supplies the receiver.
        callee: String,
        /// Accessed member name.
        member: String,
    },
    /// Member access through the superclass receiver.
    Super {
        /// Accessed member name.
        member: String,
    },
    /// Vue template identifier declared by the same file's script setup.
    VueSetupBinding,
    /// Custom Vue template element, distinct from a JavaScript expression.
    VueComponentTag,
    /// Vue template member proven by a same-file typed `defineProps` declaration.
    VueTypedPropMember {
        /// Stable ID of the indexed type-literal property.
        field_id: String,
    },
    /// Call through a proven same-file `defineEmits` setup binding.
    VueSetupMacroCallable,
    /// Legacy runtime-callable hint; current runtime admission requires an effective rule.
    VueSetupRuntimeCallable,
    /// TypeScript import binding available only in a type position.
    TypeOnlyImport,
    /// Exact synchronous classic script source declared by an HTML script tag.
    HtmlClassicScriptSource,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
/// Unlinked source reference awaiting cross-file resolution.
pub struct Reference {
    /// Stable identifier of the containing declaration.
    pub source: String,
    /// Referenced source expression.
    pub expression: String,
    /// Reference category used by the linker.
    pub kind: String,
    /// One-based source line.
    pub line: usize,
    #[serde(default)]
    /// Byte column within the source line.
    pub column: usize,
    /// Import alias when the source binds another name.
    pub alias: Option<String>,
    /// Imported module path when known.
    pub module: Option<String>,
    #[serde(default)]
    /// Whether resolution depends on runtime behavior.
    pub dynamic: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Syntax-derived receiver evidence, if available.
    pub receiver_hint: Option<ReceiverHint>,
}
impl Reference {
    pub(crate) fn position(&self) -> super::semantic::SourcePosition {
        super::semantic::SourcePosition {
            line: self.line,
            column: self.column,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Source-scoped extraction or linking diagnostic.
pub struct Diagnostic {
    /// Workspace-relative source path.
    pub path: String,
    /// One-based source line.
    pub line: usize,
    /// Human-readable diagnostic message.
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
/// Indexed documentation section with extracted invariant and symbol links.
pub struct DocSection {
    /// Stable section identifier.
    pub doc_id: String,
    /// Workspace-relative document path.
    pub path: String,
    /// Heading for this section.
    pub section_title: String,
    /// Document classification.
    pub doc_type: String,
    /// Section body used for search and inspection.
    pub content: String,
    /// Invariant statements extracted from the section.
    pub invariants: Vec<String>,
    /// Symbol references named in the section.
    pub referenced_symbols: Vec<String>,
    /// Source modification time used for freshness checks.
    pub mtime: f64,
    /// Source document size in bytes.
    pub size: u64,
    /// Whether the section expresses an invariant.
    pub is_invariant: bool,
}
#[derive(Debug, Clone, Default, Deserialize)]
/// Extracted per-file facts before cross-file linking and persistence.
pub struct Facts {
    #[serde(skip)]
    #[doc(hidden)]
    pub semantic: SemanticStore,
    #[serde(skip)]
    #[doc(hidden)]
    pub pending_flow_nodes: Vec<usize>,
    /// Declarations found in the file.
    pub nodes: Vec<Node>,
    /// Relationships resolved within the file.
    pub edges: Vec<Edge>,
    /// References requiring linker resolution.
    pub references: Vec<Reference>,
    /// Documentation sections extracted from the file.
    pub docs: Vec<DocSection>,
    /// Extraction diagnostics.
    pub errors: Vec<Diagnostic>,
}
impl Serialize for Facts {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Facts", 5)?;
        state.serialize_field(
            "nodes",
            &super::typed_facts::PublicNodesView::new(self, &self.semantic.flows),
        )?;
        state.serialize_field("edges", &self.edges)?;
        state.serialize_field("references", &self.references)?;
        state.serialize_field("docs", &self.docs)?;
        state.serialize_field("errors", &self.errors)?;
        state.end()
    }
}

impl Facts {
    pub(crate) fn retain_nodes(&mut self, mut keep: impl FnMut(&Node) -> bool) {
        let tracked: std::collections::HashSet<_> = self
            .pending_flow_nodes
            .iter()
            .copied()
            .chain(self.semantic.entries().map(|(i, _)| *i))
            .collect();
        if tracked.is_empty() {
            self.nodes.retain(keep);
            let ids = self.nodes.iter().map(|node| node.id.clone()).collect();
            self.semantic.flows.retain_nodes(&ids);
            return;
        }
        let mut mapping = std::collections::HashMap::new();
        let (mut old, mut new) = (0, 0);
        self.nodes.retain(|node| {
            let retained = keep(node);
            if retained {
                if tracked.contains(&old) {
                    mapping.insert(old, new);
                }
                new += 1;
            }
            old += 1;
            retained
        });
        self.pending_flow_nodes.retain_mut(|index| {
            if let Some(new) = mapping.get(index) {
                *index = *new;
                true
            } else {
                false
            }
        });
        self.semantic.retain_remapped(&mapping);
        let ids = self.nodes.iter().map(|node| node.id.clone()).collect();
        self.semantic.flows.retain_nodes(&ids);
    }
    pub(crate) fn remap_deferred_nodes(&mut self, base: usize, batches: &[(usize, usize)]) {
        let mut mapping = std::collections::HashMap::new();
        for old in self
            .pending_flow_nodes
            .iter()
            .copied()
            .chain(self.semantic.entries().map(|(i, _)| *i))
        {
            let mut appended = 0;
            let mut mapped = None;
            for &(frontier, count) in batches {
                if old < base {
                    if frontier <= old {
                        appended += count;
                    }
                } else if old - base >= appended && old - base < appended + count {
                    mapped = Some(frontier + appended + (old - base - appended));
                    break;
                } else {
                    appended += count;
                }
            }
            mapping.insert(
                old,
                mapped.unwrap_or(old + if old < base { appended } else { 0 }),
            );
        }
        self.remap_node_order(&mapping);
    }
    pub(crate) fn remap_node_order(
        &mut self,
        old_to_new: &std::collections::HashMap<usize, usize>,
    ) {
        for index in &mut self.pending_flow_nodes {
            if let Some(new) = old_to_new.get(index) {
                *index = *new;
            }
        }
        self.semantic.remap_order(old_to_new);
    }
}

impl AsRef<Facts> for Facts {
    fn as_ref(&self) -> &Facts {
        self
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
/// Resolution outcome for an import or reference expression.
pub struct Coverage {
    /// Workspace-relative source path.
    pub path: String,
    /// One-based source line.
    pub line: usize,
    /// Expression evaluated by the linker.
    pub expression: String,
    /// Resolution state such as `resolved`, `external`, or `unresolved`.
    pub status: String,
    /// Evidence for the resolution state.
    pub evidence: String,
}
#[derive(Debug, Clone, Default)]
/// Linked graph and resolution coverage ready for persistence.
pub struct Graph {
    /// Resolved relationships.
    pub edges: Vec<Edge>,
    /// Resolution outcomes for source references.
    pub coverage: Vec<Coverage>,
}

/// Computes the deterministic FNV-1a 64-bit hash used for indexed identifiers.
pub fn stable_hash64(value: &str) -> i64 {
    value
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        }) as i64
}

/// Classifies a workspace-relative path as test code by path conventions.
pub fn is_test(path: &str) -> bool {
    path.split('/').any(|p| p == "tests" || p == "test")
        || path.rsplit('/').next().is_some_and(|p| {
            p.starts_with("test_")
                || p.ends_with("_test.go")
                || p.contains(".test.")
                || p.contains(".spec.")
        })
}

#[cfg(test)]
mod tests {
    use super::stable_hash64;

    #[test]
    fn stable_hash64_uses_fnv1a_vectors() {
        assert_eq!(stable_hash64("") as u64, 0xcbf29ce484222325);
        assert_eq!(stable_hash64("a") as u64, 0xaf63dc4c8601ec8c);
        assert_eq!(stable_hash64("hello") as u64, 0xa430d84680aabd0b);
    }
}
