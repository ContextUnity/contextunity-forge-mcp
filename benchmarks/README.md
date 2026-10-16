# Manual Forge benchmarks

These runners preserve paired baseline/candidate profiling outside Cargo's test inventory.
Run them from the repository root with the release profile for meaningful timings.

Use `cold_test_coverage_benchmark.py` to compare fresh database builds on a configured
workspace. It reads that workspace's `forge-mcp.yaml` directly, so its enabled
`linked_workspaces` are indexed using their configured paths. Every run gets a new
database under the system temporary directory, and the script removes it after
collecting graph counts, resolution coverage, elapsed time, CPU, peak RSS, and database
size. Builds run one at a time and alternate baseline/candidate order.

```sh
python3 benchmarks/cold_test_coverage_benchmark.py \
  --repo /path/to/workspace \
  --baseline-binary /path/to/older/contextunity-forge-mcp \
  --candidate-binary /path/to/newer/contextunity-forge-mcp \
  --repeats 5
```

Use `cold_memory_budget_benchmark.py` on an isolated workspace copy to compare
adaptive cold builds and measure candidate extraction budgets. Keep the copy's
enabled linked workspaces inside the isolated snapshot, and omit an explicit
`limits.memory_budget_mb` there so the matrix's environment override takes effect.
The runner uses the selected profile, alternates paired build order, reverses
budget order between repetitions, and records every wall time, peak RSS, graph
summary, and Merkle output root. Its JSON report includes medians, an aggregate of
every persisted domain commitment, and root determinism for each binary across
repetitions and candidate budgets. The overall root also commits the engine source
fingerprint and discovery policy, so cross-version root equality is reported
separately from fixture Merkle equality. The runner fails when fixture commitments
differ or roots vary within one binary. The budget controls extraction working memory;
peak process RSS also includes retained facts, linking, and SQLite.
The recommended budget has the lowest median peak RSS among settings within 5%
of the fastest median build; ties prefer lower latency, then the smaller budget.
Use `--matrix-repeats 0` for a paired-only follow-up without repeating the budget
matrix; that report does not select a new recommended budget.
The runner succeeds only when fixture commitments and per-binary roots are
deterministic and both the candidate wall median and median paired wall change
are no worse than baseline. Timing checks use unrounded run values; every run is
retained in the report, including a failed performance comparison.

```sh
python3 benchmarks/cold_memory_budget_benchmark.py \
  --profile benchmarks/profiles/commerce-release-update.json \
  --workspace /path/to/isolated/workspace \
  --baseline-binary /path/to/older/contextunity-forge-mcp \
  --candidate-binary /path/to/newer/contextunity-forge-mcp \
  --repeats 5 --matrix-repeats 3 --budgets-mb 50 200 500 1000 \
  --output /tmp/cold-memory-budget.json
```

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
