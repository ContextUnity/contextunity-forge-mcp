---
title: "ADR 0004: Deterministic Merkle Tree Commitments for Index Integrity"
doc_type: adr
status: accepted
date: 2026-10-01
---

# ADR 0004: Deterministic Merkle Tree Commitments for Index Integrity

## Status
Accepted

## Context
Codebase index storage requires cryptographic integrity verification to detect index corruption, validate incremental delta changes against cold builds, and support deterministic distributed cache replication.

## Decision
1. **Sha256 Merkle Tree Hierarchy**:
   - Construct a deterministic Merkle tree over indexed file nodes, symbol definitions, and graph edges.
   - Compute leaf digests using streaming `Sha256` hashing directly over canonical record bytes.
2. **In-Memory Dictionary Lookups**:
   - Resolve path IDs, node keys, and edge tuples through pre-loaded in-memory hash maps (`path_map`, `node_map`, `key_map`) during root computation.
   - Maintain zero heap reallocation per row across large record sets.
3. **Chunked Parallel Hashing**:
   - Use Rayon parallel iteration for collections exceeding 8,192 records while preserving deterministic lexicographical sort order.

## Consequences
- Every cold build and incremental delta produces a verifiable, bit-identical Merkle root (`meta.merkle_root`).
- Tamper-evident index verification executes in milliseconds.
- Storage integrity verification is enforced by `tests/commitment_integrity.rs`.
