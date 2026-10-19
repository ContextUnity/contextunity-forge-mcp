use crate::core::{
    commitments,
    models::{Diagnostic, DocSection, Edge, Node, Reference},
    typed_facts::TypedFacts,
};
#[cfg(test)]
use crate::core::typed_facts::FlowState;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const MAGIC_V3: &[u8] = b"FORGE-EXTRACTION-FACTS\0\x03";
const MAGIC_V2: &[u8] = b"FORGE-EXTRACTION-FACTS\0\x02";
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

pub(crate) fn key(
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

pub(crate) fn effective_ruleset_hash(adapter_digest: &str, dependency_digest: &str) -> String {
    let mut input = Vec::with_capacity(adapter_digest.len() + dependency_digest.len() + 32);
    input.extend_from_slice(b"forge-effective-ruleset-v1\0");
    input.extend_from_slice(&(adapter_digest.len() as u64).to_le_bytes());
    input.extend_from_slice(adapter_digest.as_bytes());
    input.extend_from_slice(&(dependency_digest.len() as u64).to_le_bytes());
    input.extend_from_slice(dependency_digest.as_bytes());
    commitments::hash(&input)
}

pub(crate) struct ExtractionCache {
    root: Option<PathBuf>,
}

impl ExtractionCache {
    pub(crate) fn new(root: Option<PathBuf>) -> Self {
        Self { root }
    }

    pub(crate) fn load(&self, key: &str, max_payload_bytes: u64) -> Option<TypedFacts> {
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
    pub(crate) fn store(&self, key: &str, facts: &TypedFacts) -> bool {
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

pub(crate) fn default_cache_root() -> Option<PathBuf> {
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BinaryValue {
    Null,
    Bool(bool),
    PosInt(u64),
    NegInt(i64),
    Float(f64),
    String(String),
    Array(Vec<BinaryValue>),
    Object(Vec<(String, BinaryValue)>),
}

impl From<&serde_json::Value> for BinaryValue {
    fn from(val: &serde_json::Value) -> Self {
        match val {
            serde_json::Value::Null => BinaryValue::Null,
            serde_json::Value::Bool(b) => BinaryValue::Bool(*b),
            serde_json::Value::Number(n) => {
                if let Some(u) = n.as_u64() {
                    BinaryValue::PosInt(u)
                } else if let Some(i) = n.as_i64() {
                    BinaryValue::NegInt(i)
                } else if let Some(f) = n.as_f64() {
                    BinaryValue::Float(f)
                } else {
                    BinaryValue::Null
                }
            }
            serde_json::Value::String(s) => BinaryValue::String(s.clone()),
            serde_json::Value::Array(arr) => {
                BinaryValue::Array(arr.iter().map(BinaryValue::from).collect())
            }
            serde_json::Value::Object(obj) => BinaryValue::Object(
                obj.iter()
                    .map(|(k, v)| (k.clone(), BinaryValue::from(v)))
                    .collect(),
            ),
        }
    }
}

impl From<BinaryValue> for serde_json::Value {
    fn from(bv: BinaryValue) -> Self {
        match bv {
            BinaryValue::Null => serde_json::Value::Null,
            BinaryValue::Bool(b) => serde_json::Value::Bool(b),
            BinaryValue::PosInt(u) => serde_json::Value::Number(u.into()),
            BinaryValue::NegInt(i) => serde_json::Value::Number(i.into()),
            BinaryValue::Float(f) => serde_json::Number::from_f64(f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            BinaryValue::String(s) => serde_json::Value::String(s),
            BinaryValue::Array(arr) => {
                serde_json::Value::Array(arr.into_iter().map(serde_json::Value::from).collect())
            }
            BinaryValue::Object(obj) => {
                let mut map = serde_json::Map::with_capacity(obj.len());
                for (k, v) in obj {
                    map.insert(k, serde_json::Value::from(v));
                }
                serde_json::Value::Object(map)
            }
        }
    }
}

struct CacheFactsDto {
    nodes: Vec<NodeDto>,
    edges: Vec<Edge>,
    references: Vec<Reference>,
    docs: Vec<DocSection>,
    errors: Vec<Diagnostic>,
}

struct NodeDto {
    id: String,
    kind: String,
    name: String,
    qualname: String,
    path: String,
    line: usize,
    end_line: usize,
    is_test: bool,
    language: String,
    generated: bool,
    details: BinaryValue,
}

fn write_u8(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}
fn write_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn write_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn write_i64(buf: &mut Vec<u8>, v: i64) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn write_f64(buf: &mut Vec<u8>, v: f64) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn write_str(buf: &mut Vec<u8>, s: &str) {
    write_u32(buf, s.len() as u32);
    buf.extend_from_slice(s.as_bytes());
}
fn write_opt_str(buf: &mut Vec<u8>, opt: Option<&str>) {
    if let Some(s) = opt {
        write_u8(buf, 1);
        write_str(buf, s);
    } else {
        write_u8(buf, 0);
    }
}

fn write_binary_value(buf: &mut Vec<u8>, val: &BinaryValue) {
    match val {
        BinaryValue::Null => write_u8(buf, 0),
        BinaryValue::Bool(b) => {
            write_u8(buf, 1);
            write_u8(buf, *b as u8);
        }
        BinaryValue::PosInt(u) => {
            write_u8(buf, 2);
            write_u64(buf, *u);
        }
        BinaryValue::NegInt(i) => {
            write_u8(buf, 3);
            write_i64(buf, *i);
        }
        BinaryValue::Float(f) => {
            write_u8(buf, 4);
            write_f64(buf, *f);
        }
        BinaryValue::String(s) => {
            write_u8(buf, 5);
            write_str(buf, s);
        }
        BinaryValue::Array(arr) => {
            write_u8(buf, 6);
            write_u32(buf, arr.len() as u32);
            for item in arr {
                write_binary_value(buf, item);
            }
        }
        BinaryValue::Object(entries) => {
            write_u8(buf, 7);
            write_u32(buf, entries.len() as u32);
            for (k, v) in entries {
                write_str(buf, k);
                write_binary_value(buf, v);
            }
        }
    }
}

fn write_receiver_hint(buf: &mut Vec<u8>, hint: &crate::core::models::ReceiverHint) {
    use crate::core::models::ReceiverHint;
    match hint {
        ReceiverHint::StringLiteral { member } => {
            write_u8(buf, 0);
            write_str(buf, member);
        }
        ReceiverHint::CallResult { callee, member } => {
            write_u8(buf, 1);
            write_str(buf, callee);
            write_str(buf, member);
        }
        ReceiverHint::ConstructorResult { callee, member } => {
            write_u8(buf, 2);
            write_str(buf, callee);
            write_str(buf, member);
        }
        ReceiverHint::Super { member } => {
            write_u8(buf, 3);
            write_str(buf, member);
        }
        ReceiverHint::VueSetupBinding => write_u8(buf, 4),
        ReceiverHint::VueComponentTag => write_u8(buf, 5),
        ReceiverHint::VueTypedPropMember { field_id } => {
            write_u8(buf, 6);
            write_str(buf, field_id);
        }
        ReceiverHint::VueSetupMacroCallable => write_u8(buf, 7),
        ReceiverHint::VueSetupRuntimeCallable => write_u8(buf, 8),
        ReceiverHint::TypeOnlyImport => write_u8(buf, 9),
        ReceiverHint::HtmlClassicScriptSource => write_u8(buf, 10),
    }
}

fn write_dto(buf: &mut Vec<u8>, dto: &CacheFactsDto) {
    write_u32(buf, dto.nodes.len() as u32);
    for n in &dto.nodes {
        write_str(buf, &n.id);
        write_str(buf, &n.kind);
        write_str(buf, &n.name);
        write_str(buf, &n.qualname);
        write_str(buf, &n.path);
        write_u64(buf, n.line as u64);
        write_u64(buf, n.end_line as u64);
        write_u8(buf, n.is_test as u8);
        write_str(buf, &n.language);
        write_u8(buf, n.generated as u8);
        write_binary_value(buf, &n.details);
    }
    write_u32(buf, dto.edges.len() as u32);
    for e in &dto.edges {
        write_str(buf, &e.src);
        write_str(buf, &e.dst);
        write_str(buf, &e.kind);
        write_str(buf, &e.path);
        write_u64(buf, e.line as u64);
        write_str(buf, &e.evidence);
        write_str(buf, &e.confidence);
    }
    write_u32(buf, dto.references.len() as u32);
    for r in &dto.references {
        write_str(buf, &r.source);
        write_str(buf, &r.expression);
        write_str(buf, &r.kind);
        write_u64(buf, r.line as u64);
        write_u64(buf, r.column as u64);
        write_opt_str(buf, r.alias.as_deref());
        write_opt_str(buf, r.module.as_deref());
        write_u8(buf, r.dynamic as u8);
        if let Some(hint) = &r.receiver_hint {
            write_u8(buf, 1);
            write_receiver_hint(buf, hint);
        } else {
            write_u8(buf, 0);
        }
    }
    write_u32(buf, dto.docs.len() as u32);
    for d in &dto.docs {
        write_str(buf, &d.doc_id);
        write_str(buf, &d.path);
        write_str(buf, &d.section_title);
        write_str(buf, &d.doc_type);
        write_str(buf, &d.content);
        write_u32(buf, d.invariants.len() as u32);
        for inv in &d.invariants {
            write_str(buf, inv);
        }
        write_u32(buf, d.referenced_symbols.len() as u32);
        for sym in &d.referenced_symbols {
            write_str(buf, sym);
        }
        write_f64(buf, d.mtime);
        write_u64(buf, d.size);
        write_u8(buf, d.is_invariant as u8);
    }
    write_u32(buf, dto.errors.len() as u32);
    for err in &dto.errors {
        write_str(buf, &err.path);
        write_u64(buf, err.line as u64);
        write_str(buf, &err.message);
    }
}

fn facts_to_dto(facts: &TypedFacts) -> anyhow::Result<CacheFactsDto> {
    let mut nodes = Vec::with_capacity(facts.facts.nodes.len());
    for node in &facts.facts.nodes {
        let flow = facts.flows.state(node);
        let details = if let Some(flow) = flow {
            let flow_value = serde_json::to_value(flow)?;
            let flow_binary = BinaryValue::from(&flow_value);
            let mut binary_details = BinaryValue::from(&node.details);
            match &mut binary_details {
                BinaryValue::Object(entries) => {
                    if let Some(pos) = entries.iter().position(|(k, _)| k == "value_flow") {
                        entries[pos].1 = flow_binary;
                    } else {
                        entries.push(("value_flow".to_string(), flow_binary));
                    }
                }
                _ => {
                    binary_details =
                        BinaryValue::Object(vec![("value_flow".to_string(), flow_binary)]);
                }
            }
            binary_details
        } else {
            BinaryValue::from(&node.details)
        };
        nodes.push(NodeDto {
            id: node.id.clone(),
            kind: node.kind.clone(),
            name: node.name.clone(),
            qualname: node.qualname.clone(),
            path: node.path.clone(),
            line: node.line,
            end_line: node.end_line,
            is_test: node.is_test,
            language: node.language.clone(),
            generated: node.generated,
            details,
        });
    }
    Ok(CacheFactsDto {
        nodes,
        edges: facts.facts.edges.clone(),
        references: facts.facts.references.clone(),
        docs: facts.facts.docs.clone(),
        errors: facts.facts.errors.clone(),
    })
}

fn dto_to_facts(dto: CacheFactsDto) -> TypedFacts {
    let mut nodes = Vec::with_capacity(dto.nodes.len());
    for node in dto.nodes {
        nodes.push(Node {
            id: node.id,
            kind: node.kind,
            name: node.name,
            qualname: node.qualname,
            path: node.path,
            line: node.line,
            end_line: node.end_line,
            is_test: node.is_test,
            language: node.language,
            generated: node.generated,
            details: serde_json::Value::from(node.details),
        });
    }
    let facts = crate::core::models::Facts {
        nodes,
        edges: dto.edges,
        references: dto.references,
        docs: dto.docs,
        errors: dto.errors,
    };
    TypedFacts::from_public(facts)
}

fn encode_entry(key: &str, facts: &TypedFacts) -> anyhow::Result<Vec<u8>> {
    let dto = facts_to_dto(facts)?;
    let mut payload = Vec::with_capacity(4096);
    write_dto(&mut payload, &dto);
    let checksum = Sha256::digest(&payload);
    let mut bytes =
        Vec::with_capacity(MAGIC_V3.len() + 4 + key.len() + 8 + CHECKSUM_BYTES + payload.len());
    bytes.extend_from_slice(MAGIC_V3);
    bytes.extend_from_slice(&(key.len() as u32).to_le_bytes());
    bytes.extend_from_slice(key.as_bytes());
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&checksum);
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

#[cfg(test)]
fn encode_entry_v2(key: &str, facts: &TypedFacts) -> Result<Vec<u8>, serde_json::Error> {
    use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct, Serializer};

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

    let payload = serde_json::to_vec(&CacheFacts(facts))?;
    let checksum = Sha256::digest(&payload);
    let mut bytes =
        Vec::with_capacity(MAGIC_V2.len() + 4 + key.len() + 8 + CHECKSUM_BYTES + payload.len());
    bytes.extend_from_slice(MAGIC_V2);
    bytes.extend_from_slice(&(key.len() as u32).to_le_bytes());
    bytes.extend_from_slice(key.as_bytes());
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&checksum);
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

fn decode_entry(bytes: &[u8], expected_key: &str, max_payload_bytes: u64) -> Option<TypedFacts> {
    let is_v3 = bytes.starts_with(MAGIC_V3);
    let is_v2 = bytes.starts_with(MAGIC_V2);
    if !is_v3 && !is_v2 {
        return None;
    }
    let magic_len = if is_v3 {
        MAGIC_V3.len()
    } else {
        MAGIC_V2.len()
    };
    let mut cursor = magic_len;
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
    if is_v3 {
        let mut payload_cursor = 0;
        let dto = read_dto(payload, &mut payload_cursor)?;
        if payload_cursor != payload.len() {
            return None;
        }
        Some(dto_to_facts(dto))
    } else {
        serde_json::from_slice(payload).ok()
    }
}

fn read_u8(bytes: &[u8], cursor: &mut usize) -> Option<u8> {
    let v = *bytes.get(*cursor)?;
    *cursor += 1;
    Some(v)
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

fn read_i64(bytes: &[u8], cursor: &mut usize) -> Option<i64> {
    let end = cursor.checked_add(8)?;
    let value = i64::from_le_bytes(bytes.get(*cursor..end)?.try_into().ok()?);
    *cursor = end;
    Some(value)
}

fn read_f64(bytes: &[u8], cursor: &mut usize) -> Option<f64> {
    let end = cursor.checked_add(8)?;
    let value = f64::from_le_bytes(bytes.get(*cursor..end)?.try_into().ok()?);
    *cursor = end;
    Some(value)
}

fn read_str<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<&'a str> {
    let len = usize::try_from(read_u32(bytes, cursor)?).ok()?;
    let end = cursor.checked_add(len)?;
    let s = std::str::from_utf8(bytes.get(*cursor..end)?).ok()?;
    *cursor = end;
    Some(s)
}

fn read_string(bytes: &[u8], cursor: &mut usize) -> Option<String> {
    read_str(bytes, cursor).map(str::to_owned)
}

fn read_opt_string(bytes: &[u8], cursor: &mut usize) -> Option<Option<String>> {
    match read_u8(bytes, cursor)? {
        0 => Some(None),
        1 => Some(Some(read_string(bytes, cursor)?)),
        _ => None,
    }
}

fn read_binary_value(bytes: &[u8], cursor: &mut usize) -> Option<BinaryValue> {
    match read_u8(bytes, cursor)? {
        0 => Some(BinaryValue::Null),
        1 => Some(BinaryValue::Bool(read_u8(bytes, cursor)? != 0)),
        2 => Some(BinaryValue::PosInt(read_u64(bytes, cursor)?)),
        3 => Some(BinaryValue::NegInt(read_i64(bytes, cursor)?)),
        4 => Some(BinaryValue::Float(read_f64(bytes, cursor)?)),
        5 => Some(BinaryValue::String(read_string(bytes, cursor)?)),
        6 => {
            let len = usize::try_from(read_u32(bytes, cursor)?).ok()?;
            let remaining = bytes.len().saturating_sub(*cursor);
            if len > remaining {
                return None;
            }
            let mut arr = Vec::with_capacity(len);
            for _ in 0..len {
                arr.push(read_binary_value(bytes, cursor)?);
            }
            Some(BinaryValue::Array(arr))
        }
        7 => {
            let len = usize::try_from(read_u32(bytes, cursor)?).ok()?;
            let remaining = bytes.len().saturating_sub(*cursor);
            if len > remaining {
                return None;
            }
            let mut entries = Vec::with_capacity(len);
            for _ in 0..len {
                let k = read_string(bytes, cursor)?;
                let v = read_binary_value(bytes, cursor)?;
                entries.push((k, v));
            }
            Some(BinaryValue::Object(entries))
        }
        _ => None,
    }
}

fn read_receiver_hint(bytes: &[u8], cursor: &mut usize) -> Option<crate::core::models::ReceiverHint> {
    use crate::core::models::ReceiverHint;
    match read_u8(bytes, cursor)? {
        0 => Some(ReceiverHint::StringLiteral {
            member: read_string(bytes, cursor)?,
        }),
        1 => Some(ReceiverHint::CallResult {
            callee: read_string(bytes, cursor)?,
            member: read_string(bytes, cursor)?,
        }),
        2 => Some(ReceiverHint::ConstructorResult {
            callee: read_string(bytes, cursor)?,
            member: read_string(bytes, cursor)?,
        }),
        3 => Some(ReceiverHint::Super {
            member: read_string(bytes, cursor)?,
        }),
        4 => Some(ReceiverHint::VueSetupBinding),
        5 => Some(ReceiverHint::VueComponentTag),
        6 => Some(ReceiverHint::VueTypedPropMember {
            field_id: read_string(bytes, cursor)?,
        }),
        7 => Some(ReceiverHint::VueSetupMacroCallable),
        8 => Some(ReceiverHint::VueSetupRuntimeCallable),
        9 => Some(ReceiverHint::TypeOnlyImport),
        10 => Some(ReceiverHint::HtmlClassicScriptSource),
        _ => None,
    }
}

fn read_dto(bytes: &[u8], cursor: &mut usize) -> Option<CacheFactsDto> {
    let node_count = usize::try_from(read_u32(bytes, cursor)?).ok()?;
    let remaining = bytes.len().saturating_sub(*cursor);
    if node_count > remaining {
        return None;
    }
    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        nodes.push(NodeDto {
            id: read_string(bytes, cursor)?,
            kind: read_string(bytes, cursor)?,
            name: read_string(bytes, cursor)?,
            qualname: read_string(bytes, cursor)?,
            path: read_string(bytes, cursor)?,
            line: usize::try_from(read_u64(bytes, cursor)?).ok()?,
            end_line: usize::try_from(read_u64(bytes, cursor)?).ok()?,
            is_test: read_u8(bytes, cursor)? != 0,
            language: read_string(bytes, cursor)?,
            generated: read_u8(bytes, cursor)? != 0,
            details: read_binary_value(bytes, cursor)?,
        });
    }
    let edge_count = usize::try_from(read_u32(bytes, cursor)?).ok()?;
    let remaining = bytes.len().saturating_sub(*cursor);
    if edge_count > remaining {
        return None;
    }
    let mut edges = Vec::with_capacity(edge_count);
    for _ in 0..edge_count {
        edges.push(Edge {
            src: read_string(bytes, cursor)?,
            dst: read_string(bytes, cursor)?,
            kind: read_string(bytes, cursor)?,
            path: read_string(bytes, cursor)?,
            line: usize::try_from(read_u64(bytes, cursor)?).ok()?,
            evidence: read_string(bytes, cursor)?,
            confidence: read_string(bytes, cursor)?,
        });
    }
    let ref_count = usize::try_from(read_u32(bytes, cursor)?).ok()?;
    let remaining = bytes.len().saturating_sub(*cursor);
    if ref_count > remaining {
        return None;
    }
    let mut references = Vec::with_capacity(ref_count);
    for _ in 0..ref_count {
        references.push(Reference {
            source: read_string(bytes, cursor)?,
            expression: read_string(bytes, cursor)?,
            kind: read_string(bytes, cursor)?,
            line: usize::try_from(read_u64(bytes, cursor)?).ok()?,
            column: usize::try_from(read_u64(bytes, cursor)?).ok()?,
            alias: read_opt_string(bytes, cursor)?,
            module: read_opt_string(bytes, cursor)?,
            dynamic: read_u8(bytes, cursor)? != 0,
            receiver_hint: match read_u8(bytes, cursor)? {
                0 => None,
                1 => Some(read_receiver_hint(bytes, cursor)?),
                _ => return None,
            },
        });
    }
    let doc_count = usize::try_from(read_u32(bytes, cursor)?).ok()?;
    let remaining = bytes.len().saturating_sub(*cursor);
    if doc_count > remaining {
        return None;
    }
    let mut docs = Vec::with_capacity(doc_count);
    for _ in 0..doc_count {
        let doc_id = read_string(bytes, cursor)?;
        let path = read_string(bytes, cursor)?;
        let section_title = read_string(bytes, cursor)?;
        let doc_type = read_string(bytes, cursor)?;
        let content = read_string(bytes, cursor)?;
        let inv_count = usize::try_from(read_u32(bytes, cursor)?).ok()?;
        let remaining = bytes.len().saturating_sub(*cursor);
        if inv_count > remaining {
            return None;
        }
        let mut invariants = Vec::with_capacity(inv_count);
        for _ in 0..inv_count {
            invariants.push(read_string(bytes, cursor)?);
        }
        let sym_count = usize::try_from(read_u32(bytes, cursor)?).ok()?;
        let remaining = bytes.len().saturating_sub(*cursor);
        if sym_count > remaining {
            return None;
        }
        let mut referenced_symbols = Vec::with_capacity(sym_count);
        for _ in 0..sym_count {
            referenced_symbols.push(read_string(bytes, cursor)?);
        }
        let mtime = read_f64(bytes, cursor)?;
        let size = read_u64(bytes, cursor)?;
        let is_invariant = read_u8(bytes, cursor)? != 0;
        docs.push(DocSection {
            doc_id,
            path,
            section_title,
            doc_type,
            content,
            invariants,
            referenced_symbols,
            mtime,
            size,
            is_invariant,
        });
    }
    let err_count = usize::try_from(read_u32(bytes, cursor)?).ok()?;
    let remaining = bytes.len().saturating_sub(*cursor);
    if err_count > remaining {
        return None;
    }
    let mut errors = Vec::with_capacity(err_count);
    for _ in 0..err_count {
        errors.push(Diagnostic {
            path: read_string(bytes, cursor)?,
            line: usize::try_from(read_u64(bytes, cursor)?).ok()?,
            message: read_string(bytes, cursor)?,
        });
    }
    Some(CacheFactsDto {
        nodes,
        edges,
        references,
        docs,
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::semantic::ValueFlowFacts;
    use serde_json::json;

    #[test]
    fn corrupted_collection_lengths_are_cache_misses() {
        let mut prefixes = Vec::new();
        for preceding_collections in 0..5 {
            prefixes.push(vec![0; preceding_collections * 4]);
        }
        let mut doc_prefix = vec![0; 3 * 4];
        write_u32(&mut doc_prefix, 1);
        for _ in 0..5 {
            write_str(&mut doc_prefix, "");
        }
        prefixes.push(doc_prefix.clone());
        write_u32(&mut doc_prefix, 0);
        prefixes.push(doc_prefix);
        for tag in [6, 7] {
            let mut node_prefix = Vec::new();
            write_u32(&mut node_prefix, 1);
            for _ in 0..5 {
                write_str(&mut node_prefix, "");
            }
            write_u64(&mut node_prefix, 0);
            write_u64(&mut node_prefix, 0);
            write_u8(&mut node_prefix, 0);
            write_str(&mut node_prefix, "");
            write_u8(&mut node_prefix, 0);
            write_u8(&mut node_prefix, tag);
            prefixes.push(node_prefix);
        }
        for mut payload in prefixes {
            write_u32(&mut payload, u32::MAX);
            let mut bytes = MAGIC_V3.to_vec();
            write_str(&mut bytes, "key");
            write_u64(&mut bytes, payload.len() as u64);
            bytes.extend_from_slice(&Sha256::digest(&payload));
            bytes.extend_from_slice(&payload);
            assert!(decode_entry(&bytes, "key", 1024).is_none());
        }
    }

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

    #[test]
    fn test_v2_json_cache_backward_compatibility() {
        let mut facts = TypedFacts::default();
        let node = Node {
            id: "fn:src/lib.rs:hello".into(),
            kind: "function".into(),
            name: "hello".into(),
            qualname: "crate::hello".into(),
            path: "src/lib.rs".into(),
            line: 10,
            end_line: 20,
            is_test: false,
            language: "rust".into(),
            generated: false,
            details: json!({"async": true}),
        };
        facts.facts.nodes.push(node);

        let key = "ee".repeat(32);
        // Encode using legacy v2 JSON format
        let encoded_v2 = encode_entry_v2(&key, &facts).expect("v2 encodes");
        assert!(encoded_v2.starts_with(MAGIC_V2));

        // Decode must seamlessly accept legacy v2
        let decoded_v2 = decode_entry(&encoded_v2, &key, 1024 * 1024).expect("v2 decodes");
        assert_eq!(decoded_v2.facts.nodes.len(), 1);
        assert_eq!(decoded_v2.facts.nodes[0].name, "hello");

        // Encode using v3 binary format
        let encoded_v3 = encode_entry(&key, &facts).expect("v3 encodes");
        assert!(encoded_v3.starts_with(MAGIC_V3));

        let decoded_v3 = decode_entry(&encoded_v3, &key, 1024 * 1024).expect("v3 decodes");
        assert_eq!(decoded_v3.facts.nodes.len(), 1);
        assert_eq!(decoded_v3.facts.nodes[0].name, "hello");
    }
}
