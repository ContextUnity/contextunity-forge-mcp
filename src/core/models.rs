use serde::{Deserialize, Serialize};
use serde_json::Value;
pub(crate) mod compact_graph;
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl Node {
    pub(crate) fn navigation_details(&self) -> NavigationDetails<'_> {
        NavigationDetails(&self.details)
    }
}

pub(crate) struct NavigationDetails<'a>(&'a Value);

impl Serialize for NavigationDetails<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let Some(details) = self.0.as_object() else {
            return self.0.serialize(serializer);
        };
        let mut map = serializer.serialize_map(None)?;
        for (key, value) in details {
            if matches!(
                key.as_str(),
                "value_flow" | "bindings" | "rebindings" | "param_types" | "column"
            ) || value.is_null()
                || value.as_str().is_some_and(str::is_empty)
                || value.as_array().is_some_and(Vec::is_empty)
                || (matches!(key.as_str(), "async" | "default_export")
                    && value == &Value::Bool(false))
            {
                continue;
            }
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
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
/// Syntax evidence that helps infer the receiver of a member reference.
pub enum ReceiverHint {
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
/// Extracted per-file facts before cross-file linking and persistence.
pub struct Facts {
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
