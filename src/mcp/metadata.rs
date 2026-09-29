use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SHORT_HASH_LEN: usize = 8;

/// Shortens a hash string to the first `SHORT_HASH_LEN` (8) characters.
pub fn shorten_hash(hash: &str) -> &str {
    if hash.len() > SHORT_HASH_LEN {
        &hash[..SHORT_HASH_LEN]
    } else {
        hash
    }
}

/// Recursively shortens hash fields (`generation`, `output_root`, `corpus_hash`) across JSON values.
pub fn shorten_hashes_in_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if key == "generation" || key == "output_root" || key == "corpus_hash" {
                    if let Value::String(s) = val {
                        if s.len() > SHORT_HASH_LEN {
                            *s = s[..SHORT_HASH_LEN].to_string();
                        }
                    }
                } else {
                    shorten_hashes_in_value(val);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                shorten_hashes_in_value(item);
            }
        }
        _ => {}
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
/// 3. All hash identifiers (`generation`, `output_root`, `corpus_hash`) are shortened to first 8 characters.
/// 4. Metadata fields (`metadata`, `generation`, `freshness`) are moved to the VERY END of the object.
pub fn finalize_mcp_metadata(value: &mut Value) {
    shorten_hashes_in_value(value);

    if let Value::Object(map) = value {
        // Strip verbose freshness timestamps
        if let Some(Value::Object(fmap)) = map.get_mut("freshness") {
            fmap.remove("checked_at_unix_ms");
            fmap.remove("inventory_scan_ms");
            if let Some(Value::String(hash)) = fmap.get_mut("corpus_hash") {
                if hash.len() > SHORT_HASH_LEN {
                    *hash = hash[..SHORT_HASH_LEN].to_string();
                }
            }
            if let Some(Value::String(hash)) = fmap.get_mut("output_root") {
                if hash.len() > SHORT_HASH_LEN {
                    *hash = hash[..SHORT_HASH_LEN].to_string();
                }
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
        assert_eq!(keys, vec!["components", "metadata", "generation", "freshness"]);

        assert_eq!(response["generation"], "abcdef01");
        assert_eq!(response["freshness"]["output_root"], "11223344");
        assert_eq!(response["freshness"]["corpus_hash"], "aabbccdd");
        assert!(response["freshness"].get("checked_at_unix_ms").is_none());
        assert!(response["freshness"].get("inventory_scan_ms").is_none());
    }
}
