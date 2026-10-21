//! Compatibility exports for the shared core response compactor.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::core::format::shorten_hash;

/// Short hash width retained for callers that display verified metadata hashes.
pub const SHORT_HASH_LEN: usize = 8;

/// Shortens only explicit, verified metadata hash fields.
/// Continuation generations and nested/user values remain opaque.
pub fn shorten_hashes_in_value(value: &mut Value) {
    crate::core::format::shorten_metadata_hashes(value);
}

/// Unified tool metadata format retained for source compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMetadata {
    /// The generation value.
    pub generation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional status value.
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional refresh value.
    pub refresh: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional files checked value.
    pub files_checked: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional output root value.
    pub output_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional corpus hash value.
    pub corpus_hash: Option<String>,
}

/// Compacts only recognized navigation metadata.
pub fn finalize_mcp_metadata(value: &mut Value) {
    crate::core::format::finalize_metadata(value);
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

        assert!(response.get("generation").is_none());
        assert_eq!(response["components"], json!(["alpha", "beta"]));
        assert_eq!(response["metadata"], json!([{"key":"schema","value":"7"}]));
        assert_eq!(
            response["freshness"],
            json!({"status":"matched","files_checked":100,"refresh":"none"})
        );
        assert!(response["freshness"].get("checked_at_unix_ms").is_none());
        assert!(response["freshness"].get("inventory_scan_ms").is_none());
    }

    #[test]
    fn hash_shortening_is_scoped_to_verified_metadata_envelopes() {
        let mut response = serde_json::json!({
            "generation": "a".repeat(64),
            "pages": {"offset": 0, "limit": 10, "generation": "b".repeat(64), "items": [1]},
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

        assert!(response.get("generation").is_none());
        assert_eq!(response["pages"]["generation"], "b".repeat(64));
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
        assert_eq!(
            response["freshness"],
            json!({"status":"matched","files_checked":4,"refresh":"none"})
        );
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
        assert_eq!(
            tool_metadata["metadata"]["generation"],
            "opaque-generation-1234"
        );
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
        assert!(
            extended_freshness["freshness"]
                .get("checked_at_unix_ms")
                .is_some()
        );
    }

    #[test]
    fn shortening_is_limited_to_verified_metadata_hashes() {
        let mut value = json!({
            "generation":"complete-page-token-1234",
            "metadata":{"generation":"opaque-token-1234","status":"matched",
                "refresh":"none","output_root":"root-hash-123456","corpus_hash":"corpus-hash-123456"},
            "rows":{"items":[{"generation":"sql-value-123456"}]}
        });
        shorten_hashes_in_value(&mut value);
        assert_eq!(value["generation"], "complete-page-token-1234");
        assert_eq!(value["metadata"]["generation"], "opaque-token-1234");
        assert_eq!(value["metadata"]["output_root"], "root-has");
        assert_eq!(value["metadata"]["corpus_hash"], "corpus-h");
        assert_eq!(value["rows"]["items"][0]["generation"], "sql-value-123456");
    }

    #[test]
    fn metadata_compaction_preserves_active_tokens_and_durable_values() {
        let mut value = json!({
            "count":0,"complete":false,
            "nodes":{"total":1,"offset":0,"limit":1,"has_more":false,"next_offset":null,
                "generation":"inactive-token","items":[{"name":"x"}]},
            "outgoing":{"total":2,"offset":0,"limit":1,"has_more":true,"next_offset":1,
                "generation":"active-token","items":[{"name":"y"}]},
            "task":{"candidate_hash":"candidate-hash-must-remain-complete",
                "workflow_guidance":{"commands":{"test":"cargo test --test mcp"}}}
        });
        finalize_mcp_metadata(&mut value);
        assert!(value.get("generation").is_none());
        assert!(value["nodes"].get("generation").is_none());
        assert_eq!(value["outgoing"]["generation"], "active-token");
        assert_eq!(value["count"], 0);
        assert_eq!(value["complete"], false);
        assert_eq!(
            value["task"]["candidate_hash"],
            "candidate-hash-must-remain-complete"
        );
    }
}
