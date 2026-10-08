# Benchmarks & Performance Profiling — Agent Instructions

## Mandatory Invariant: User Consent Required

> [!CAUTION]
> **Explicit User Approval Required**: Benchmark scripts run full cold builds, MCP stress workloads, and multi-process comparisons.
> **DO NOT** execute benchmarks autonomously, in the background, or during normal development tasks without explicit user consent and instructions.

## Profile-Based Benchmarks

All benchmark runs are driven by declarative JSON profiles located in `benchmarks/profiles/`:

- **Main Runner**:
  ```bash
  python3 benchmarks/run_benchmarks.py --profile benchmarks/profiles/commerce-release-update.json
  ```
- **Options**:
  - `--profile <path>`: Path to benchmark profile (required).
  - `--workspace <path>`: Override profile's `workspace_root` (resolves relative to CWD if relative).
  - `--mode {all,comparison,quality}`: Choose between resource comparison, answer quality, or both (default: `all`).
  - `--repeats <N>`: Repetitions per scenario (default: 5).
  - `--scenario <id>`: Run only a single scenario from the profile.
  - `--output-dir <path>`: Destination directory for JSON receipts (default: temporary directory).

## Path & Workspace Resolution

- Profiles configure `workspace_root` as a path relative to the Forge repository root (e.g. `"../../worktrees/commerce-release-update"`).
- Profile loader (`benchmarks/benchmark_profile.py`) automatically resolves relative workspace paths against the repository root.
- Never hardcode user-specific absolute filesystem paths in profiles or scripts.

## Subsystem Scripts

- `benchmarks/run_benchmarks.py`: Primary CLI orchestrator.
- `benchmarks/benchmark_profile.py`: Profile loader and workspace snapshot isolation logic.
- `benchmarks/mcp_tool_comparison_benchmark.py`: Measures tool latency, RSS/HWM memory, and CPU utilization across MCP servers.
- `benchmarks/mcp_tool_quality_benchmark.py`: Evaluates tool answer completeness and schema compliance.
- `benchmarks/mcp_adapter_single_load.py`: Linux inotify probe verifying that adapter configurations (`forge-mcp.yaml`) are not redundantly re-read on every request.

## Staged Lifecycle

When explicitly instructed to run benchmarks:
1. Compile release binary: `cargo build --release`.
2. Allow host to settle to idle baseline.
3. Execute single controlled run: `python3 benchmarks/run_benchmarks.py --profile <profile>`.
4. Run full test suite: `cargo test --all-targets`.
