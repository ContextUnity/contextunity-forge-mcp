---
title: "ADR 0001: Dual-Surface Parity Between CLI and MCP"
doc_type: adr
status: accepted
date: 2026-10-01
---

# ADR 0001: Dual-Surface Parity Between CLI and MCP

## Status
Accepted

## Context
Forge MCP serves two primary environments: AI coding assistants over the JSON-RPC stdio protocol, and human developers, shell scripts, git hooks, and CI/CD pipelines directly from the terminal. Functional divergence between agent tools and terminal commands creates operational blind spots, complicates debugging, and prevents automated headless verification.

## Decision
1. **Full Surface Symmetry**:
   - Every operation exposed over the MCP protocol has an equivalent CLI subcommand under `contextunity-forge-mcp`.
   - Core discovery, inspection, relationship traversal, documentation search, and task lifecycle capabilities map directly between MCP tool arguments and CLI command-line flags.
2. **Shared Engine Core**:
   - Both `src/mcp/` and `src/cli/` serve as thin protocol adapters over shared storage (`src/db/`), analysis (`src/engine/`), and task execution engines.
   - Output formatting adheres to structured JSON across both interfaces, enabling seamless pipeline integration.
3. **Emergency Recovery & Scriptability**:
   - Administrative overrides, index maintenance, database migrations, and task state resets remain executable directly via CLI without an active MCP JSON-RPC session.
4. **Modularity Invariant: Domain-Driven Type and Language Decomposition**:
   - **Type Layering (`src/core/`)**: Shared domain models are grouped by functional responsibility into dedicated submodules (`graph`, `semantic`, `docs`, `compact`). Business logic and query traversal remain strictly separated from core data definitions.
   - **Self-Contained Language Subsystems (`src/engine/languages/<lang>/`)**: Complex language engines (exceeding 500 lines or requiring language-specific linking/value-flow rules) are organized as self-contained packages containing their AST visitor, language-specific value flow, and linker profile.
   - **Language-Agnostic Linker Core (`src/engine/linker/`)**: The core graph linker operates exclusively through the `LanguageLinker` trait contract, keeping graph assembly and SQLite persistence free of language-specific `match` branching.
   - **Cohesive Module Boundaries**: Group production and test code by cohesive responsibility, owned inputs and outputs, and concrete producer/consumer seams. Use file length to find candidates for review; do not set a universal line ceiling or target a fixed child-module size.

## Consequences
- Operations, task state, and graph queries are reproducible between human terminal sessions, CI workflows, and autonomous AI agents.
- Protocol adapters remain lightweight and decoupled from core business logic.
- Language support scales modularly without modifying shared core graph algorithms.
- System verification exercises both surfaces through unified integration test suites.
