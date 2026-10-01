---
title: "Testing and verification"
doc_type: guide
---

# Testing and verification

## Test ownership

Read [tests/AGENTS.md](../../tests/AGENTS.md) before placing, moving, or changing
tests. It owns domain grouping, public seams, module size, and benchmark placement.
[Root instructions](../../AGENTS.md) own engineering and verification requirements.

## Rust verification

Run from the repository root with the current Cargo manifest:

```sh
cargo test --test commitment_integrity
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

Performance-sensitive changes also use the benchmark procedure in root
instructions. Record the exact binary, workspace inventory, commands, and results
with the owning work. Documentation changes use metadata, link, scan-admission,
and retrieval checks as their focused proof.
