//! Merkle commitment calculations and domain state verification.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Standard hashing algorithm identifier.
pub const ALGORITHM: &str = "forge-owner-sha256-v2";

/// The logical table domain list for code graph commitments.
pub const DOMAINS: &[&str] = &[
    "source_inventory",
    "files",
    "nodes",
    "edges",
    "local_facts",
    "edge_occurrences",
    "dependencies",
    "shared_owners",
    "errors",
    "resolution_coverage",
    "coverage_owner_language",
    "coverage_language_counts",
    "doc_sections",
];

/// Returns a lowercase SHA-256 digest of the supplied bytes.
pub fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Commitment for a single logical table domain.
pub struct DomainCommitment {
    /// Domain table name.
    pub table: String,
    /// Aggregated hexadecimal SHA-256 digest of the domain.
    pub digest: String,
    /// Detailed owner entries (if retained).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Merkle commitment root digest and domain proofs.
pub struct MerkleCommitment {
    /// Merkle root digest over all domains.
    pub root_digest: String,
    /// Domain commitments composing the tree.
    pub domains: Vec<DomainCommitment>,
}

/// Aggregates leaf hashes into a domain-level digest using SHA-256.
pub fn aggregate_domain_digest(table: &str, entries: &BTreeMap<String, String>) -> String {
    let mut h = Sha256::new();
    h.update(ALGORITHM);
    h.update(table);
    for (k, v) in entries {
        h.update((k.len() as u64).to_le_bytes());
        h.update(k);
        h.update(v);
    }
    hex::encode(h.finalize())
}

/// Computes the Merkle root digest over domain commitments and metadata in canonical order.
pub fn compute_merkle_root(
    domain_commitments: &[(String, String)],
    metadata_entries: &[(String, String)],
) -> String {
    let mut h = Sha256::new();
    h.update(ALGORITHM);
    let mut sorted_domains = domain_commitments.to_vec();
    sorted_domains.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in sorted_domains {
        h.update((k.len() as u64).to_le_bytes());
        h.update(k);
        h.update(v);
    }
    let mut sorted_meta = metadata_entries.to_vec();
    sorted_meta.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in sorted_meta {
        h.update((k.len() as u64).to_le_bytes());
        h.update(k);
        h.update((v.len() as u64).to_le_bytes());
        h.update(v);
    }
    hex::encode(h.finalize())
}
