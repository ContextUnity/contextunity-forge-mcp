use crate::core::{
    commitments,
    models::Node,
    typed_facts::{FlowState, TypedFacts},
};
use serde::{
    ser::{SerializeMap, SerializeSeq, SerializeStruct, Serializer},
    Serialize,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const MAGIC: &[u8] = b"FORGE-EXTRACTION-FACTS\0\x02";
const CHECKSUM_BYTES: usize = 32;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize)]
struct CacheIdentity<'a> {
    source_digest: &'a str,
    relative_path: &'a str,
    grammar_id: &'a str,
    extractor_version: &'a str,
    effective_ruleset_hash: &'a str,
}

pub(super) fn key(
    source_digest: &str,
    relative_path: &str,
    grammar_id: &str,
    extractor_version: &str,
    effective_ruleset_hash: &str,
) -> String {
    let identity = CacheIdentity {
        source_digest,
        relative_path,
        grammar_id,
        extractor_version,
        effective_ruleset_hash,
    };
    commitments::hash(&serde_json::to_vec(&identity).expect("cache identity is JSON serializable"))
}

pub(super) fn effective_ruleset_hash(adapter_digest: &str, dependency_digest: &str) -> String {
    let mut input = Vec::with_capacity(adapter_digest.len() + dependency_digest.len() + 32);
    input.extend_from_slice(b"forge-effective-ruleset-v1\0");
    input.extend_from_slice(&(adapter_digest.len() as u64).to_le_bytes());
    input.extend_from_slice(adapter_digest.as_bytes());
    input.extend_from_slice(&(dependency_digest.len() as u64).to_le_bytes());
    input.extend_from_slice(dependency_digest.as_bytes());
    commitments::hash(&input)
}

pub(super) struct ExtractionCache {
    root: Option<PathBuf>,
}

impl ExtractionCache {
    pub(super) fn new(root: Option<PathBuf>) -> Self {
        Self { root }
    }

    pub(super) fn load(&self, key: &str, max_payload_bytes: u64) -> Option<TypedFacts> {
        let path = self.entry_path(key)?;
        let file = open_regular_file(&path).ok()?;
        let max_entry_bytes = max_payload_bytes.saturating_add(256);
        if file.metadata().ok()?.len() > max_entry_bytes {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(max_entry_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() as u64 > max_entry_bytes {
            return None;
        }
        decode_entry(&bytes, key, max_payload_bytes)
    }

    /// Publishes a complete entry atomically. Cache failures never affect extraction.
    pub(super) fn store(&self, key: &str, facts: &TypedFacts) -> bool {
        let Some(path) = self.entry_path(key) else {
            return false;
        };
        let Ok(bytes) = encode_entry(key, facts) else {
            return false;
        };
        let Some(parent) = path.parent() else {
            return false;
        };
        if fs::create_dir_all(parent).is_err() {
            return false;
        }
        let temporary = parent.join(format!(
            ".cache-{}-{}-{}.tmp",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            key
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = options
            .open(&temporary)
            .and_then(|mut file| file.write_all(&bytes))
            .and_then(|()| fs::rename(&temporary, &path));
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
            return false;
        }
        true
    }

    fn entry_path(&self, key: &str) -> Option<PathBuf> {
        if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        Some(
            self.root
                .as_ref()?
                .join(&key[..2])
                .join(format!("{key}.facts")),
        )
    }
}

pub(super) fn default_cache_root() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FORGE_EXTRACTION_CACHE_DIR") {
        return Some(PathBuf::from(path));
    }
    #[cfg(test)]
    return Some(std::env::temp_dir().join(format!(
        "contextunity-forge-cache-tests-{}",
        std::process::id()
    )));
    #[cfg(not(test))]
    {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
            .map(|base| base.join("contextunity-forge-mcp").join("extractions"))
    }
}

fn open_regular_file(path: &std::path::Path) -> std::io::Result<File> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "extraction cache entry is not a regular file",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

fn encode_entry(key: &str, facts: &TypedFacts) -> Result<Vec<u8>, serde_json::Error> {
    let payload = serde_json::to_vec(&CacheFacts(facts))?;
    let checksum = Sha256::digest(&payload);
    let mut bytes =
        Vec::with_capacity(MAGIC.len() + 4 + key.len() + 8 + CHECKSUM_BYTES + payload.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(key.len() as u32).to_le_bytes());
    bytes.extend_from_slice(key.as_bytes());
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&checksum);
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

struct CacheFacts<'a>(&'a TypedFacts);

impl Serialize for CacheFacts<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Facts", 5)?;
        state.serialize_field("nodes", &CacheNodes(self.0))?;
        state.serialize_field("edges", &self.0.facts.edges)?;
        state.serialize_field("references", &self.0.facts.references)?;
        state.serialize_field("docs", &self.0.facts.docs)?;
        state.serialize_field("errors", &self.0.facts.errors)?;
        state.end()
    }
}

struct CacheNodes<'a>(&'a TypedFacts);

impl Serialize for CacheNodes<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.facts.nodes.len()))?;
        for node in &self.0.facts.nodes {
            sequence.serialize_element(&CacheNode {
                node,
                flow: self.0.flows.state(node),
            })?;
        }
        sequence.end()
    }
}

struct CacheNode<'a> {
    node: &'a Node,
    flow: Option<&'a FlowState>,
}

impl Serialize for CacheNode<'_> {
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
        state.serialize_field(
            "details",
            &CacheDetails {
                details: &node.details,
                flow: self.flow,
            },
        )?;
        state.end()
    }
}

struct CacheDetails<'a> {
    details: &'a serde_json::Value,
    flow: Option<&'a FlowState>,
}

impl Serialize for CacheDetails<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Some(flow) = self.flow else {
            return self.details.serialize(serializer);
        };
        let Some(details) = self.details.as_object() else {
            return Err(<S::Error as serde::ser::Error>::custom(
                "cannot cache a value-flow state on non-object node details",
            ));
        };
        let has_flow_key = details.contains_key("value_flow");
        let len = if has_flow_key {
            details.len()
        } else {
            details.len() + 1
        };
        let mut map = serializer.serialize_map(Some(len))?;
        for (key, value) in details {
            if key == "value_flow" {
                map.serialize_entry(key, flow)?;
            } else {
                map.serialize_entry(key, value)?;
            }
        }
        if !has_flow_key {
            map.serialize_entry("value_flow", flow)?;
        }
        map.end()
    }
}

fn decode_entry(bytes: &[u8], expected_key: &str, max_payload_bytes: u64) -> Option<TypedFacts> {
    if !bytes.starts_with(MAGIC) {
        return None;
    }
    let mut cursor = MAGIC.len();
    let key_len = usize::try_from(read_u32(bytes, &mut cursor)?).ok()?;
    let stored_key = bytes.get(cursor..cursor.checked_add(key_len)?)?;
    cursor += key_len;
    if stored_key != expected_key.as_bytes() {
        return None;
    }
    let payload_len = read_u64(bytes, &mut cursor)?;
    if payload_len > max_payload_bytes {
        return None;
    }
    let checksum_end = cursor.checked_add(CHECKSUM_BYTES)?;
    let stored_checksum = bytes.get(cursor..checksum_end)?;
    cursor = checksum_end;
    let payload_len = usize::try_from(payload_len).ok()?;
    let payload_end = cursor.checked_add(payload_len)?;
    let payload = bytes.get(cursor..payload_end)?;
    if payload_end != bytes.len() || Sha256::digest(payload).as_slice() != stored_checksum {
        return None;
    }
    serde_json::from_slice(payload).ok()
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Option<u32> {
    let end = cursor.checked_add(4)?;
    let value = u32::from_le_bytes(bytes.get(*cursor..end)?.try_into().ok()?);
    *cursor = end;
    Some(value)
}

fn read_u64(bytes: &[u8], cursor: &mut usize) -> Option<u64> {
    let end = cursor.checked_add(8)?;
    let value = u64::from_le_bytes(bytes.get(*cursor..end)?.try_into().ok()?);
    *cursor = end;
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::semantic::ValueFlowFacts;
    use serde_json::json;

    #[test]
    fn cache_round_trip_preserves_flow_without_details_placeholder() {
        let mut facts = TypedFacts::default();
        let node = Node {
            id: "scope:src/main.js:1:1".into(),
            kind: "scope".into(),
            name: "main".into(),
            qualname: "main".into(),
            path: "src/main.js".into(),
            line: 1,
            end_line: 1,
            is_test: false,
            language: "javascript".into(),
            generated: false,
            details: json!({"scope": "module"}),
        };
        let node_id = node.id.clone();
        facts.facts.nodes.push(node);
        let flow = ValueFlowFacts {
            fact_limit_exceeded: true,
            ..ValueFlowFacts::default()
        };
        facts.flows.insert(node_id.clone(), flow);

        let key = "ab".repeat(32);
        let encoded = encode_entry(&key, &facts).expect("cache entry encodes");
        let decoded = decode_entry(&encoded, &key, 1024 * 1024).expect("cache entry decodes");
        let decoded_node = decoded
            .facts
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .expect("node round-trips");

        assert!(matches!(
            decoded.flows.state(decoded_node),
            Some(FlowState::Invalid(value)) if value["fact_limit_exceeded"] == true
        ));
    }

    #[test]
    fn test_classic_global_cache_roundtrip() {
        use crate::core::semantic::{BindingFact, SourcePosition, ValueExpr};
        use crate::core::typed_facts::encode_facts;

        let mut facts = TypedFacts::default();
        let node = Node {
            id: "html-js:change_form.html:classic".into(),
            kind: "template_scope".into(),
            name: "classic scripts".into(),
            qualname: "module.script".into(),
            path: "change_form.html".into(),
            line: 1,
            end_line: 10,
            is_test: false,
            language: "javascript".into(),
            generated: false,
            details: json!({
                "column": 43,
                "embedded_language": "javascript",
                "classic_global": true,
                "lexical_boundary": true,
                "html_import_owner": "module:change_form.html",
                "bindings": ["PRODUCT_PK"],
                "rebindings": ["PRODUCT_PK"],
                "value_flow": serde_json::Value::Null,
            }),
        };
        facts.facts.nodes.push(node);
        facts.flows.insert_classic(
            "html-js:change_form.html:classic".into(),
            ValueFlowFacts {
                bindings: vec![BindingFact {
                    name: "PRODUCT_PK".into(),
                    position: SourcePosition { line: 244, column: 6 },
                    value: ValueExpr::Unknown,
                    conditional: false,
                }],
                immutable_initializers: vec![SourcePosition { line: 256, column: 2 }],
                ..ValueFlowFacts::default()
            },
        );

        let key = "cc".repeat(32);
        let encoded_cache = encode_entry(&key, &facts).expect("cache entry encodes");
        let decoded = decode_entry(&encoded_cache, &key, 1024 * 1024).expect("cache entry decodes");

        let cold_encoded = encode_facts(&facts).expect("cold encode");
        let warm_encoded = encode_facts(&decoded).expect("warm encode");

        assert_eq!(
            cold_encoded, warm_encoded,
            "cold and warm facts must encode identically"
        );
    }
}
