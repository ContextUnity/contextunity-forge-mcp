---
title: "ADR 0005: Filesystem Root Guard and Symlink Traversal Safety"
doc_type: adr
status: accepted
date: 2026-10-01
---

# ADR 0005: Filesystem Root Guard and Symlink Traversal Safety

## Status
Accepted

## Context
Running code scanners with broad root paths (`/`, `/home`, `..`) risks scanning system files, consuming unbounded memory, and exposing sensitive user credentials. Symlinks targeting paths outside the project root introduce infinite recursion cycles or escape intended isolation boundaries.

## Decision
1. **Root Scope Guard**:
   - The scanner validates the configured workspace root against a blacklist of forbidden targets: filesystem root (`/`), user home directories (`~`, `/home/*`, `/Users/*`), system roots (`/usr`, `/var`, `/etc`), and multi-repository parent containers.
   - Any attempt to initialize indexing targeting a forbidden root aborts immediately with an explicit configuration error.
2. **Strict Symlink Confinement**:
   - File traversal resolves canonical paths for every entry.
   - Symlinks resolving outside the configured workspace root or linked workspaces are rejected.
   - Recursive symlink loops are detected and pruned during file discovery.

## Consequences
- Indexing remains strictly confined to authorized repository perimeters.
- Host system resources, memory limits, and sensitive directories remain protected.
- Workspace boundaries remain deterministic across all tool runs.
