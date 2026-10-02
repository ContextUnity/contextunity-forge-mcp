use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const SHORT_HASH_LEN: usize = 8;

/// Returns an eight-byte prefix when it ends on a UTF-8 character boundary.
pub fn shorten_hash(hash: &str) -> &str {
    if hash.len() > SHORT_HASH_LEN {
        hash.get(..SHORT_HASH_LEN).unwrap_or(hash)
    } else {
        hash
    }
}

/// Shortens hashes in verified top-level MCP metadata envelopes and paging envelopes.
pub fn shorten_hashes_in_value(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    // Shorten top-level generation if present
    if let Some(Value::String(gen)) = object.get_mut("generation") {
        *gen = shorten_hash(gen).to_owned();
    }

    if object
        .get("freshness")
        .and_then(Value::as_object)
        .is_some_and(is_freshness_envelope)
    {
        if let Some(freshness) = object.get_mut("freshness").and_then(Value::as_object_mut) {
            shorten_fields(freshness, &["output_root", "corpus_hash"]);
        }
    }

    if object
        .get("metadata")
        .and_then(Value::as_object)
        .is_some_and(is_tool_metadata)
    {
        if let Some(metadata) = object.get_mut("metadata").and_then(Value::as_object_mut) {
            shorten_fields(metadata, &["output_root", "corpus_hash", "generation"]);
        }
    }

    // Shorten generation in top-level paging envelopes (e.g. nodes, components, languages, items, matches, rows)
    for val in object.values_mut() {
        if let Some(child_map) = val.as_object_mut() {
            if is_paging_envelope(child_map) {
                if let Some(Value::String(gen)) = child_map.get_mut("generation") {
                    *gen = shorten_hash(gen).to_owned();
                }
            }
        }
    }
}

fn is_paging_envelope(object: &Map<String, Value>) -> bool {
    object.contains_key("offset")
        && object.contains_key("limit")
        && object.get("generation").is_some_and(Value::is_string)
}

fn is_freshness_envelope(object: &Map<String, Value>) -> bool {
    const ALLOWED: [&str; 7] = [
        "status",
        "output_root",
        "corpus_hash",
        "checked_at_unix_ms",
        "files_checked",
        "inventory_scan_ms",
        "refresh",
    ];

    object.get("status").is_some_and(Value::is_string)
        && object.get("output_root").is_some_and(Value::is_string)
        && object.get("corpus_hash").is_some_and(Value::is_string)
        && object.get("refresh").is_some_and(Value::is_string)
        && object.keys().all(|k| ALLOWED.contains(&k.as_str()))
}

fn is_tool_metadata(object: &Map<String, Value>) -> bool {
    object.get("generation").is_some_and(Value::is_string)
        && object.iter().all(|(key, value)| match key.as_str() {
            "generation" | "status" | "refresh" | "output_root" | "corpus_hash" => {
                value.is_string()
            }
            "files_checked" => value.as_u64().is_some(),
            _ => false,
        })
}

fn shorten_fields(object: &mut Map<String, Value>, fields: &[&str]) {
    for field in fields {
        if let Some(Value::String(hash)) = object.get_mut(*field) {
            *hash = shorten_hash(hash).to_owned();
        }
    }
}

/// Unified tool metadata format for MCP responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMetadata {
    pub generation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_checked: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corpus_hash: Option<String>,
}

/// Formats and orders metadata at the end of an MCP tool response object.
///
/// Ensures:
/// 1. All primary content fields appear FIRST.
/// 2. Verbose internal timing/diagnostics are stripped from `freshness`.
/// 3. Display hashes are shortened only in verified top-level freshness or metadata envelopes.
/// 4. Metadata fields (`metadata`, `generation`, `freshness`) are moved to the VERY END of the object.
pub fn finalize_mcp_metadata(value: &mut Value) {
    shorten_hashes_in_value(value);

    if let Value::Object(map) = value {
        // Strip verbose freshness timestamps
        if let Some(Value::Object(fmap)) = map.get_mut("freshness") {
            if is_freshness_envelope(fmap) {
                fmap.remove("checked_at_unix_ms");
                fmap.remove("inventory_scan_ms");
            }
        }

        // Extract metadata fields
        let metadata = map.remove("metadata");
        let generation = map.remove("generation");
        let freshness = map.remove("freshness");

        // Re-insert at the end in deterministic order
        if let Some(meta) = metadata {
            map.insert("metadata".into(), meta);
        }
        if let Some(gen) = generation {
            map.insert("generation".into(), gen);
        }
        if let Some(fresh) = freshness {
            map.insert("freshness".into(), fresh);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_shorten_hash() {
        assert_eq!(
            shorten_hash("b484bce6e865a296f3b52c5cde481af3e94dab2f974b0295af7609d7ea0c279e"),
            "b484bce6"
        );
        assert_eq!(shorten_hash("1234"), "1234");
        assert_eq!(shorten_hash("1234567é"), "1234567é");
    }

    #[test]
    fn test_finalize_mcp_metadata_ordering_and_shortening() {
        let mut response = json!({
            "generation": "abcdef0123456789abcdef",
            "components": ["alpha", "beta"],
            "metadata": [{"key": "schema", "value": "7"}],
            "freshness": {
                "status": "source_inventory_matched",
                "output_root": "112233445566778899",
                "corpus_hash": "aabbccddeeff001122",
                "checked_at_unix_ms": 1727630000000_u64,
                "inventory_scan_ms": 12.5,
                "files_checked": 100,
                "refresh": "none"
            }
        });

        finalize_mcp_metadata(&mut response);

        let map = response.as_object().unwrap();
        let keys: Vec<&String> = map.keys().collect();
        assert_eq!(
            keys,
            vec!["components", "metadata", "generation", "freshness"]
        );

        assert_eq!(response["generation"], "abcdef01");
        assert_eq!(response["freshness"]["output_root"], "11223344");
        assert_eq!(response["freshness"]["corpus_hash"], "aabbccdd");
        assert!(response["freshness"].get("checked_at_unix_ms").is_none());
        assert!(response["freshness"].get("inventory_scan_ms").is_none());
    }

    #[test]
    fn hash_shortening_is_scoped_to_verified_metadata_envelopes() {
        let mut response = serde_json::json!({
            "generation": "a".repeat(64),
            "pages": {"offset": 0, "limit": 10, "generation": "b".repeat(64), "items": []},
            "metadata": {"output_root": "metadata-root-1234", "corpus_hash": "metadata-hash-1234"},
            "rows": {"items": [{"generation": "1234567é", "output_root": "0123456789"}]},
            "nodes": {"items": [{"output_root": "node-root-12345", "corpus_hash": "node-hash-123456"}]},
            "symbols": [{"output_root": "symbol-root-123", "corpus_hash": "symbol-hash-1234"}],
            "doc_sections": [{"output_root": "section-root-12", "corpus_hash": "section-hash-123"}],
            "payload": {
                "freshness": {"output_root": "nested-root-1234", "corpus_hash": "nested-hash-12345"},
                "metadata": {"output_root": "nested-meta-1234", "corpus_hash": "nested-meta-12345"}
            },
            "freshness": {
                "status": "source_inventory_matched",
                "output_root": "0123456789abcdef",
                "corpus_hash": "abcdef0123456789",
                "checked_at_unix_ms": 1727630000000_u64,
                "files_checked": 4,
                "inventory_scan_ms": 1.5,
                "refresh": "none"
            }
        });

        finalize_mcp_metadata(&mut response);

        assert_eq!(response["generation"], "a".repeat(8));
        assert_eq!(response["pages"]["generation"], "b".repeat(8));
        assert_eq!(response["rows"]["items"][0]["generation"], "1234567é");
        assert_eq!(response["rows"]["items"][0]["output_root"], "0123456789");
        assert_eq!(response["metadata"]["output_root"], "metadata-root-1234");
        assert_eq!(response["metadata"]["corpus_hash"], "metadata-hash-1234");
        assert_eq!(
            response["nodes"]["items"][0]["output_root"],
            "node-root-12345"
        );
        assert_eq!(
            response["nodes"]["items"][0]["corpus_hash"],
            "node-hash-123456"
        );
        assert_eq!(response["symbols"][0]["output_root"], "symbol-root-123");
        assert_eq!(response["symbols"][0]["corpus_hash"], "symbol-hash-1234");
        assert_eq!(
            response["doc_sections"][0]["output_root"],
            "section-root-12"
        );
        assert_eq!(
            response["doc_sections"][0]["corpus_hash"],
            "section-hash-123"
        );
        assert_eq!(
            response["payload"]["freshness"]["output_root"],
            "nested-root-1234"
        );
        assert_eq!(
            response["payload"]["freshness"]["corpus_hash"],
            "nested-hash-12345"
        );
        assert_eq!(
            response["payload"]["metadata"]["output_root"],
            "nested-meta-1234"
        );
        assert_eq!(
            response["payload"]["metadata"]["corpus_hash"],
            "nested-meta-12345"
        );
        assert_eq!(response["freshness"]["output_root"], "01234567");
        assert_eq!(response["freshness"]["corpus_hash"], "abcdef01");
        assert!(response["freshness"].get("checked_at_unix_ms").is_none());
        assert!(response["freshness"].get("inventory_scan_ms").is_none());

        let mut tool_metadata = serde_json::json!({
            "metadata": {
                "generation": "opaque-generation-1234",
                "status": "source_inventory_matched",
                "refresh": "none",
                "files_checked": 4,
                "output_root": "0123456789abcdef",
                "corpus_hash": "abcdef0123456789"
            }
        });
        finalize_mcp_metadata(&mut tool_metadata);
        assert_eq!(tool_metadata["metadata"]["generation"], "opaque-g");
        assert_eq!(tool_metadata["metadata"]["output_root"], "01234567");
        assert_eq!(tool_metadata["metadata"]["corpus_hash"], "abcdef01");

        let mut extended_tool_metadata = serde_json::json!({
            "metadata": {
                "generation": "opaque-generation-1234",
                "output_root": "0123456789abcdef",
                "corpus_hash": "abcdef0123456789",
                "user_payload": "not tool metadata"
            }
        });
        finalize_mcp_metadata(&mut extended_tool_metadata);
        assert_eq!(
            extended_tool_metadata["metadata"]["output_root"],
            "0123456789abcdef"
        );
        assert_eq!(
            extended_tool_metadata["metadata"]["corpus_hash"],
            "abcdef0123456789"
        );

        let mut malformed_freshness = serde_json::json!({
            "freshness": {
                "output_root": "0123456789abcdef",
                "corpus_hash": "abcdef0123456789",
                "checked_at_unix_ms": 1727630000000_u64
            }
        });
        finalize_mcp_metadata(&mut malformed_freshness);
        assert_eq!(
            malformed_freshness["freshness"]["output_root"],
            "0123456789abcdef"
        );
        assert_eq!(
            malformed_freshness["freshness"]["corpus_hash"],
            "abcdef0123456789"
        );
        assert_eq!(
            malformed_freshness["freshness"]["checked_at_unix_ms"],
            1727630000000_u64
        );

        let mut extended_freshness = serde_json::json!({
            "freshness": {
                "status": "source_inventory_matched",
                "output_root": "0123456789abcdef",
                "corpus_hash": "abcdef0123456789",
                "checked_at_unix_ms": 1727630000000_u64,
                "files_checked": 4,
                "inventory_scan_ms": 1.5,
                "refresh": "none",
                "user_payload": "not freshness metadata"
            }
        });
        finalize_mcp_metadata(&mut extended_freshness);
        assert_eq!(
            extended_freshness["freshness"]["output_root"],
            "0123456789abcdef"
        );
        assert_eq!(
            extended_freshness["freshness"]["corpus_hash"],
            "abcdef0123456789"
        );
        assert!(extended_freshness["freshness"]
            .get("checked_at_unix_ms")
            .is_some());
    }
}
