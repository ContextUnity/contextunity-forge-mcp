# Manual Forge benchmarks

These runners preserve paired baseline/candidate profiling outside Cargo's test inventory.
Run them from the repository root with the release profile for meaningful timings.

```sh
FORGE_BENCH_ROOT=/path/to/workspace \
FORGE_BENCH_BASELINE=/path/to/baseline/contextunity-forge-mcp \
cargo run --release --manifest-path benchmarks/Cargo.toml --bin frozen_workspace_profile

FORGE_BASELINE_BIN=/path/to/baseline/contextunity-forge-mcp \
cargo run --release --manifest-path benchmarks/Cargo.toml --bin document_suffix_delta_profile
```

`frozen_workspace_profile` copies scanner-selected source files into a temporary workspace
and reports cold builds, single and bulk deltas, and candidate MCP admission. Optional inputs
are `FORGE_BENCH_REVERSE` to reverse execution order, `FORGE_BENCH_INPUT_LOG` to reuse a
previous `PROFILE_INPUT` bulk-path selection, and `FORGE_BENCH_DELTA_PATH` to select the
single-file delta path. The source workspace remains unchanged.

`document_suffix_delta_profile` compares a baseline CLI build/delta with the current
production writer over 1,600 TOML files plus one documentation file.
