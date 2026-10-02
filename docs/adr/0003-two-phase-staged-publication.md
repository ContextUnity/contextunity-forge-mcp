---
title: "ADR 0003: Two-Phase Staged Indexing and Atomic Publication"
doc_type: adr
status: accepted
date: 2026-10-01
---

# ADR 0003: Two-Phase Staged Indexing and Atomic Publication

## Status
Accepted

## Context
Cold rebuilds parse thousands of source files and populate large SQLite databases. Writing directly into the active database file risks exposing partially populated tables, corrupted search indexes, or crashing active MCP reader processes during long-running builds.

## Decision
1. **Isolated Temporary Staging**:
   - Cold builds write all nodes, edges, FTS virtual tables, and Merkle tree commitments to a temporary staging file (`<path>.sqlite.tmp`).
   - The staging database executes with fast private pragmas (`synchronous = OFF; cache_size = -64000;`) to maximize write throughput.
2. **Pre-Publication Integrity Verification**:
   - The builder calculates and verifies the cryptographic Merkle root of the staged database prior to publication.
   - Database schema versions and vacuum states are validated before any swap occurs.
3. **Atomic Filesystem Rename**:
   - The builder acquires an exclusive filesystem lock (`flock`) on the target database lockfile.
   - Publication executes via an atomic filesystem rename (`std::fs::rename`), replacing the target database file instantaneously.
   - Stale WAL (`-wal`) and shared memory (`-shm`) companion files from earlier generations are cleaned safely during publication.

## Consequences
- Readers interact exclusively with fully built, verified, and sealed database generations.
- Cold build failures leave the existing active database intact and available.
- Publication is instantaneous and free of intermediate inconsistent states.
