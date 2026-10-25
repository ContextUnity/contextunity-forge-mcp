#[cfg(test)]
use crate::core::models::Node;
#[cfg(test)]
use crate::core::typed_facts::FlowState;
use crate::core::{commitments, models::Reference, typed_facts::TypedFacts};
use anyhow::Context;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const MAGIC_V4: &[u8] = b"FORGE-EXTRACTION-FACTS\0\x04";
const CHECKSUM_BYTES: usize = 32;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static EXTRACTION_CACHE_BUFFER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

pub(super) struct PreparedCatalogStorage {
    pub(super) facts: TypedFacts,
    pub(super) search_texts: Vec<String>,
    pub(super) reference_keys: Vec<String>,
    pub(super) leaves: BTreeMap<String, BTreeMap<String, String>>,
}

crate::core::typed_facts::binary::struct_codecs! {
    PreparedCatalogStorage {
        facts,
        search_texts,
        reference_keys,
        leaves
    };
}

pub(super) fn encode_prepared_catalog_storage_payload(
    prepared: &PreparedCatalogStorage,
) -> anyhow::Result<Vec<u8>> {
    use crate::core::typed_facts::binary::BinaryEncode;

    thread_local! {
        static COMPRESSOR: RefCell<(Option<zstd::bulk::Compressor<'static>>, Vec<u8>)> = const {
            RefCell::new((None, Vec::new()))
        };
    }
    Ok(COMPRESSOR.with(|slot| -> std::io::Result<Vec<u8>> {
        let mut scratch = slot.borrow_mut();
        let (compressor, raw) = &mut *scratch;
        if compressor.is_none() {
            *compressor = Some(zstd::bulk::Compressor::new(-3)?);
        }
        raw.clear();
        prepared.encode(raw);
        compressor
            .as_mut()
            .expect("compressor initialized")
            .compress(raw)
    })?)
}

pub(super) fn decode_prepared_catalog_storage_payload(
    bytes: &[u8],
) -> anyhow::Result<PreparedCatalogStorage> {
    use crate::core::typed_facts::binary::BinaryDecode;
    use std::io::Read;

    thread_local! {
        static DECODER: RefCell<(zstd::zstd_safe::DCtx<'static>, Vec<u8>)> =
            RefCell::new((zstd::zstd_safe::DCtx::create(), Vec::new()));
    }
    DECODER.with(|slot| {
        let mut scratch = slot.borrow_mut();
        let (context, decoded) = &mut *scratch;
        context
            .init()
            .map_err(|code| std::io::Error::other(zstd::zstd_safe::get_error_name(code)))?;
        decoded.clear();
        zstd::stream::read::Decoder::with_context(bytes, context).read_to_end(decoded)?;
        let mut cursor = 0;
        let prepared = PreparedCatalogStorage::decode(decoded, &mut cursor)
            .context("corrupted prepared catalog storage")?;
        anyhow::ensure!(
            cursor == decoded.len(),
            "corrupted prepared catalog storage"
        );
        Ok(prepared)
    })
}

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
        EXTRACTION_CACHE_BUFFER.with(|buffer| {
            let mut bytes = buffer.borrow_mut();
            bytes.clear();
            file.take(max_entry_bytes.saturating_add(1))
                .read_to_end(&mut bytes)
                .ok()?;
            if bytes.len() as u64 > max_entry_bytes {
                return None;
            }
            decode_entry(&bytes, key, max_payload_bytes)
        })
    }

    /// Publishes a complete entry atomically. Cache failures never affect extraction.
    pub(crate) fn store(&self, key: &str, facts: &TypedFacts) -> bool {
        let Some(path) = self.entry_path(key) else {
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
        let result = EXTRACTION_CACHE_BUFFER.with(|buffer| {
            let mut bytes = buffer.borrow_mut();
            if encode_entry_into(key, facts, &mut bytes).is_err() {
                return false;
            }
            options
                .open(&temporary)
                .and_then(|mut file| file.write_all(&bytes))
                .and_then(|()| fs::rename(&temporary, &path))
                .is_ok()
        });
        if !result {
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
use crate::core::typed_facts::binary::{BinaryDecode, BinaryEncode};

#[cfg(test)]
fn encode_entry(key: &str, facts: &TypedFacts) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(4096);
    encode_entry_into(key, facts, &mut bytes)?;
    Ok(bytes)
}

fn encode_entry_into(key: &str, facts: &TypedFacts, bytes: &mut Vec<u8>) -> anyhow::Result<()> {
    anyhow::ensure!(
        !facts.flows.has_invalid_named_uncertainty_carriers(),
        "malformed named-definition uncertainty carrier"
    );
    bytes.clear();
    bytes.extend_from_slice(MAGIC_V4);
    key.encode(bytes);
    let payload_len_offset = bytes.len();
    bytes.extend_from_slice(&[0; 8]);
    let checksum_offset = bytes.len();
    bytes.extend_from_slice(&[0; CHECKSUM_BYTES]);
    let payload_offset = bytes.len();
    facts.encode(bytes);
    let payload_len = bytes.len() - payload_offset;
    let checksum = Sha256::digest(&bytes[payload_offset..]);
    bytes[payload_len_offset..payload_len_offset + 8]
        .copy_from_slice(&(payload_len as u64).to_le_bytes());
    bytes[checksum_offset..checksum_offset + CHECKSUM_BYTES].copy_from_slice(&checksum);
    Ok(())
}

fn decode_entry(bytes: &[u8], expected_key: &str, max_payload_bytes: u64) -> Option<TypedFacts> {
    if !bytes.starts_with(MAGIC_V4) {
        return None;
    }
    let mut cursor = MAGIC_V4.len();
    let key_len = usize::try_from(u32::decode(bytes, &mut cursor)?).ok()?;
    let stored_key = bytes.get(cursor..cursor.checked_add(key_len)?)?;
    cursor += key_len;
    if stored_key != expected_key.as_bytes() {
        return None;
    }
    let payload_len = u64::decode(bytes, &mut cursor)?;
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
    let mut payload_cursor = 0;
    let facts = TypedFacts::decode(payload, &mut payload_cursor)?;
    if payload_cursor != payload.len() || facts.flows.has_invalid_named_uncertainty_carriers() {
        return None;
    }
    Some(facts)
}

pub(crate) fn encode_references(references: &[Reference], buf: &mut Vec<u8>) {
    references.encode(buf);
}

pub(crate) fn decode_references(bytes: &[u8]) -> Option<Vec<Reference>> {
    let mut cursor = 0;
    let references = Vec::decode(bytes, &mut cursor)?;
    (cursor == bytes.len()).then_some(references)
}

#[cfg(test)]
mod tests;
