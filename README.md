---
title: "ContextUnity Forge MCP"
doc_type: guide
---

# ContextUnity Forge MCP

ContextUnity Forge MCP is a high-performance local code graph engine, AST search indexer, documentation server, and **ACDD (Augmented Contract-Driven Development)** task coordinator for AI coding agents.

Implemented in Rust, Forge operates as both a command-line interface and a stdio **Model Context Protocol (MCP)** server providing **20 specialized tools**. It indexes codebases into a repository-local SQLite database (`.forge/code-map.sqlite`) and orchestrates task state, gates, and receipts in an independent SQLite store (`.forge/tasks.sqlite`).

---

## Capabilities Overview

- **Code Graph & Symbol Discovery**: Fast exact, prefix, and full-text symbol search (`code_map_search`), deep symbol inspection (`code_map_inspect`), receiver-aware call hierarchies, and workspace inventory (`code_map_overview`).
- **Structural AST Pattern Matching**: Language-aware syntax search via Tree-sitter (`ast_grep_search` / `ast grep`), matching structural AST expressions across Python, Rust, TypeScript, JavaScript, Vue, Protobuf, HTML, YAML, TOML, Markdown, and optional language profiles.
- **Dependency & Impact Analysis**: Directional call and import graph traversal (`code_map_impact`), test coverage and regression seam discovery (`code_map_tests`), and statically proven safe symbol removal (`code_map_prove_removal`).
- **Architecture & Documentation Indexing**: Heading-level search and retrieval for Markdown/MDX (`search_docs`, `get_doc`), cross-linking documentation sections with code symbols and ADRs.
- **ACDD (Augmented Contract-Driven Development)**: Bridges declarative Git milestone contracts directly to an executable, deterministic task queue. The active profile defines gate identifiers and order; the default profile uses four gates:
  - `contract`: Red test seam or verified invariant baseline (`direct-proof`).
  - `build`: Green implementation with passing tests.
  - `review`: Independent verification across 5 quality contours by a worker distinct from the builder.
  - `deliver`: Snapshot-backed durable receipt recorded into SQLite and milestone Markdown.
- **Milestone & Task Lifecycle Management**: CLI commands initialize, list, inspect, and close milestones (`milestone init/list/show/handoff`). CLI and MCP task operations manage task queues and subtasks (`task list/claim/submit/subtask/blackboard`).
- **Temporary Collaboration Memory**: Task-scoped message bus (`task_blackboard`) across milestone, task, and subtask levels, plus persistent session bookmarks (`session_checkpoint`).
- **Zero-Daemon Architecture**: Pure local execution over stdio or CLI. No background daemons, cloud services, or centralized databases required.
- **Protective Agent Budgets**: Automatic response paging (default 30 items), output payload caps (64 KiB), and bounded AST previews to prevent agent context exhaustion.

---

## Installation

Compile and install with Cargo and a C compiler:

```bash
cargo install --path . --locked
command -v contextunity-forge-mcp
```

To include all extended language profiles (C/C++, Go, Java, Kotlin, PHP, Ruby, Scala, Swift, etc.), compile with:

```bash
cargo install --path . --locked --features all-languages
```

---

## MCP Client Configuration

Forge serves MCP over standard input and output (`stdio`). **No manual indexing or initialization is required.** On the first database-backed MCP call, Forge automatically inspects the workspace, creates `.forge/code-map.sqlite`, and maintains freshness incrementally.

Add Forge to your MCP configuration (e.g. `.mcp.json` or client settings):

```json
{
  "mcpServers": {
    "contextunity-forge": {
      "type": "stdio",
      "command": "contextunity-forge-mcp"
    }
  }
}
```

When started without subcommands, `contextunity-forge-mcp` defaults to `serve`. On Unix systems, running `contextunity-forge-mcp reload` sends `SIGHUP` to active server instances, seamlessly replacing running processes with updated binaries without breaking client connections.

---

## Workspace Adapters (`forge-mcp.yaml`)

### Where Adapters Live
- **Primary Workspace Adapter**: Located directly at the root of the repository as `forge-mcp.yaml`.
- **Linked Workspace Adapters**: Sibling repositories configured under `linked_workspaces`. Each linked repository can also maintain its own `forge-mcp.yaml` adapter.
- **Agent Guidance**: Pointed to by `agents_guidance` (defaults to `AGENTS.md` in the workspace root).

### How to Connect and Configure Adapters
Create a starter template using the built-in generator:
```bash
contextunity-forge-mcp guide init
```
Or pass an explicit adapter file in CLI commands:
```bash
contextunity-forge-mcp build . --adapter /path/to/custom-adapter.yaml
```

Example configuration covering both code indexing and ACDD task orchestration:
```yaml
# Code indexing roots and filters
roots:
  - src
docs:
  - docs
milestones:
  - docs/milestones
plans:
  - docs/plans
ignore:
  - target
  - node_modules

# Linked workspaces for multi-repo code graphs and task federation
linked_workspaces:
  - name: shared-core
    path: ../shared-core
    enabled: true
    roots:
      - src
    tasks:
      enabled: true
      repository: shared-core
      project: shared-core
      agents_guidance: AGENTS.md

# ACDD Task store & identity
tasks_db: .forge/tasks.sqlite
task_repository: forge-mcp
task_project: forge-mcp
agents_guidance: AGENTS.md

# MCP output bounds
response:
  page_size: 30
  max_output_bytes: 65536
```

### Default Behavior (Zero-Config / Without Adapter)
When `forge-mcp.yaml` is absent, Forge runs out-of-the-box with sensible defaults:
- **Code Graph**: Scans the workspace root (`.`), respects `.gitignore`, automatically excludes standard build and cache directories (`target`, `node_modules`, `.git`, `.venv`, `__pycache__`, `dist`, `build`), and writes the index to `.forge/code-map.sqlite`.
- **Tasks Database**: Defaults to `.forge/tasks.sqlite`.
- **Automatic Git Worktree Resolution**: When executing inside a Git worktree, relative `tasks_db` paths automatically resolve to the primary worktree root (discovered via `git rev-parse --git-common-dir`). All parallel worktree workers seamlessly coordinate through the single centralized SQLite task database without needing absolute paths or manual configuration.
- **Project Identity**: `task_repository` and `task_project` default to `forge-mcp` (or repository name).
- **Milestones & Guidance**: Milestones are discovered under `docs/milestones/`, and agent guidance defaults to `AGENTS.md`.

---

## ACDD (Augmented Contract-Driven Development) & Tasks

ACDD connects declarative Git milestone contracts (`docs/milestones/*.md`) to an executable, deterministic task queue in SQLite (`.forge/tasks.sqlite`). Code index rebuilds never alter or reset task state. The active profile defines each task's gate identifiers and order; CLI and MCP stage arguments use those identifiers.

```text
Milestone Contract (YAML frontmatter + Task specs)
   ├── task_sync ──> SQLite Tasks Queue (.forge/tasks.sqlite)
   │                  ├── claim (contract) -> Submit red test seam (or direct-proof)
   │                  ├── claim (build)    -> Submit green passing test
   │                  ├── claim (review)   -> Independent audit (different worker_id)
   │                  └── claim (deliver)  -> Write durable receipt to Markdown
   └── milestone handoff ──> Record final verification, archive contract
```

### 1. Default Profile Gates
The active profile defines a task's gates and their order. The default profile uses four gates:
1. `contract`:
   - **`seam-test-first`** (Default for features/fixes): Worker claims the gate, introduces a failing red seam test, and submits proof with a non-zero exit code.
   - **`direct-proof`** (Refactoring/hardening): Directly verifies existing seams; exit code 0 accepted at contract gate.
   - **`deferred-final-test`**: Contract proof accepts exit code 0. Build still requires a passing test. The milestone gate runs at handoff.
2. `build`: The builder implements code to satisfy the contract and submits proof with passing tests (exit code 0).
3. `review`: An independent reviewer audits the candidate against 5 contours (`paths`, `claims`, `concurrency`, `project_isolation`, `administration`).
   - **Worker Separation Rule**: The reviewer's `worker_id` MUST differ from the accepted builder's `worker_id`.
4. `deliver`: A delivery worker (distinct from builder) writes the durable receipt into SQLite and updates milestone Markdown with verified proof and architectural notes, then clears the task blackboard.

### 2. Task Taxonomy & Scope Boundaries
- **Feature Tasks**: Deliver a single architectural capability with exactly one root seam test.
- **Scope Tasks**: Address cross-cutting domains via table-driven test harnesses (`cases: [...]`).
- **Scope Roots (`scope_roots`)**: Tasks declare file targets (`scope`) and allowed directory roots (`scope_roots`). `extend_scope` inside those roots keeps `contract_revision`. Changing `scope_roots` requires a higher `contract_revision` and `task sync`. Active tasks hold exclusive ownership of their files; a completed task releases that lock.
- **Subtasks**: Iterative discoveries and checklists are tracked in SQLite (`task subtask add/update/list`) without invalidating the parent contract digest or cluttering the milestone queue.

### 3. Temporary Memory: Task Blackboard
The `task_blackboard` tool and CLI provide ephemeral SQLite messaging across `milestone`, `task`, and `subtask` scopes:
- `contract_draft`: Proposed test paths, commands, and failure outputs.
- `contract_findings`: Unsupported assumptions and contract repairs.
- `build_proof`: Candidate commit SHAs, test outputs, and clippy status.
- `architectural_notes`: Significant decisions that automatically carry over into the durable receipt at `deliver`.

### 4. Step-by-Step Task Execution Walkthrough

Use stage identifiers from the active task profile and claim context. The commands below use the default profile. Copy `task_id`, `stage`, `claim_revision`, `contract_revision`, `worker_id`, and `worktree` from the claim into `--evidence`. Proof shapes are in [task operations](docs/reference/tasks.md#gates-and-evidence).

```bash
contextunity-forge-mcp task list --stage contract
contextunity-forge-mcp task claim <task-id> --stage contract --worker agent-1 --worktree .
contextunity-forge-mcp task submit <task-id> --stage contract --action pass --evidence '{"task_id":"<task-id>","stage":"contract","claim_revision":1,"contract_revision":1,"worker_id":"agent-1","worktree":"/absolute/worktree","proof":{"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs::test_name","red_exit_code":101}}}'

contextunity-forge-mcp task claim <task-id> --stage build --worker agent-1 --worktree .
contextunity-forge-mcp task submit <task-id> --stage build --action pass --evidence '{"task_id":"<task-id>","stage":"build","claim_revision":2,"contract_revision":1,"worker_id":"agent-1","worktree":"/absolute/worktree","proof":{"test_proof":{"command":"cargo test --test acdd test_name","exit_code":0,"tests_passed":1,"tests_failed":0}}}'

contextunity-forge-mcp task claim <task-id> --stage review --worker reviewer-2 --worktree .
contextunity-forge-mcp task submit <task-id> --stage review --action pass --evidence '{"task_id":"<task-id>","stage":"review","claim_revision":3,"contract_revision":1,"worker_id":"reviewer-2","worktree":"/absolute/worktree","commit":"<build-snapshot-sha>","proof":{"review_proof":{"decision":"pass","contours":{"paths":{"applicable":true,"evidence":"edits stay in scope"},"claims":{"applicable":true,"evidence":"seam matches the contract"},"concurrency":{"applicable":false,"evidence":"no shared state"},"project_isolation":{"applicable":true,"evidence":"repository boundary holds"},"administration":{"applicable":false,"evidence":"no configuration change"}}}}}'

contextunity-forge-mcp task claim <task-id> --stage deliver --worker reviewer-2 --worktree .
contextunity-forge-mcp task submit <task-id> --stage deliver --action pass --evidence '{"task_id":"<task-id>","stage":"deliver","claim_revision":4,"contract_revision":1,"worker_id":"reviewer-2","worktree":"/absolute/worktree","commit":"<build-snapshot-sha>","proof":{"delivered":true}}'
```

### 5. Closing a Milestone (`milestone handoff`)

Milestone lifecycle commands are CLI-only. After every task is delivered, run the gate in [`TESTS.md`](TESTS.md#milestone-handoff-gate), then record its caller-verified result:

```bash
contextunity-forge-mcp milestone handoff <id> \
  --verification-command "<TESTS.md gate>" \
  --tests-passed <count> \
  --tests-failed 0
```

The handoff contract requires the CLI to check that every milestone task is
completed, record the caller-verified result, and archive the milestone. The CLI
does not execute the verification command.

The contract also requires each task's `receipt.commit` to identify the commit
that landed the task on the milestone branch, and `handoff.commit` to record the
branch `HEAD` at handoff. Commit the archive afterward; that later archive commit
is not recorded in either receipt.
After handoff succeeds, commit the archived milestone and merge its branch into the target branch under the standing ACDD permission in root `AGENTS.md`.

---

## MCP Tool Reference (20 Tools)

Milestone lifecycle commands are intentionally CLI-only. Use the CLI for
`milestone init/list/show/handoff`; these operations have no MCP equivalents.

| Category | Tool | Description |
|---|---|---|
| **Discover** | `code_map_overview` | Summarizes indexed files, symbols, languages, components, and diagnostic coverage. |
| **Discover** | `code_map_search` | Searches symbols via exact prefix, wildcard, or full-text BM25 rankings. |
| **Discover** | `ast_grep_search` | Matches structural AST syntax patterns across admitted workspace files. |
| **Inspect** | `code_map_inspect` | Inspects symbol definition, signatures, docstrings, callers, and callees. |
| **Inspect** | `get_code_snippet` | Retrieves a bounded, digest-verified source preview around a target symbol. |
| **Relationships** | `code_map_explain` | Explains direct inbound/outbound relationships and architecture context. |
| **Relationships** | `code_map_impact` | Computes transitive upstream/downstream dependency impact graph. |
| **Relationships** | `code_map_tests` | Identifies tests exercising a symbol, or production dependencies of a test. |
| **Verify** | `code_map_prove_removal`| Evaluates indexed callers and unresolved references to prove safe removal. |
| **Docs** | `search_docs` | Full-text searches indexed Markdown sections, architecture guides, and ADRs. |
| **Docs** | `get_doc` | Reads an indexed document or specific heading section. |
| **Query** | `code_map_query` | Executes specialized graph queries or paged read-only SQL queries. |
| **Query** | `code_map_analyze` | Analyzes stored diagnostics, syntax lint errors, or graph cycles. |
| **Session** | `session_checkpoint` | Manages local workflow checkpoints in `.forge/checkpoints.json`. |
| **Help** | `forge_guide` | Returns built-in guidance on tool selection, query syntax, and recovery. |
| **Tasks** | `task_list` | Queries tasks with milestone, stage, repository, and status filters. |
| **Tasks** | `task_claim` | Claims a task gate and returns stage-specific guidance and context bundles. |
| **Tasks** | `task_submit` | Submits claim-bound JSON proof to transition gates and write receipts. |
| **Tasks** | `task_manage` | Syncs milestone YAML into SQLite, inspects task state, or extends scope roots. |
| **Tasks** | `task_blackboard` | Posts and reads ephemeral collaboration messages across milestone/task scopes. |

---

## CLI Reference Summary

### Code Graph & Queries
```bash
contextunity-forge-mcp scan .                                  # Report admitted file inventory
contextunity-forge-mcp build .                                 # Perform cold SQLite index build
contextunity-forge-mcp delta . path/to/file.rs                 # Incrementally index changed files
contextunity-forge-mcp query overview                          # Print workspace overview
contextunity-forge-mcp query search 'run_*' --kind function    # Symbol search
contextunity-forge-mcp query inspect 'src/lib.rs:run'          # Symbol inspection
contextunity-forge-mcp query impact 'src/lib.rs:run' --depth 1 # Impact graph traversal
contextunity-forge-mcp query tests 'src/lib.rs:run'            # Related test discovery
contextunity-forge-mcp query remove 'src/lib.rs:run'           # Removal safety assessment
contextunity-forge-mcp ast grep 'fn $NAME($$$)' --lang rust    # Tree-sitter AST grep
contextunity-forge-mcp docs search 'ACDD' --doc-type guide     # Search documentation sections
contextunity-forge-mcp docs get 'docs/reference/tasks.md'       # Retrieve documentation section
```

### Milestones & Tasks
```bash
contextunity-forge-mcp milestone list --status active          # List active milestones
contextunity-forge-mcp milestone show 043 --full               # Inspect milestone contract
contextunity-forge-mcp milestone init --num 045 --slug feature # Create new milestone contract
contextunity-forge-mcp milestone handoff 043 ...               # Record verification and archive completed milestone

contextunity-forge-mcp task list --milestone 043               # List milestone tasks
contextunity-forge-mcp task context <task-id>                  # Inspect unified task context bundle
contextunity-forge-mcp task blackboard post --topic "note"     # Post temporary coordination note
contextunity-forge-mcp task subtask add <task-id> sub-1 "Note" # Add iterative subtask item
```

---

## Documentation Links

- [Test and verification gates](TESTS.md)
- [Documentation Index](docs/README.md)
- [MCP Setup & Client Configuration](docs/reference/mcp-setup.md)
- [MCP Tool Reference](docs/reference/mcp-tools.md)
- [CLI Reference](docs/reference/cli.md)
- [Configuration Reference](docs/reference/configuration.md)
- [ACDD Execution Runbook](docs/runbooks/acdd.md)
- [Task Schema & Operations](docs/reference/tasks.md)
- [Indexing Architecture](docs/architecture/indexing.md)
- [Language Support Matrix](docs/reference/languages.md)
- [Benchmarks & Performance Profiling](benchmarks/AGENTS.md)
