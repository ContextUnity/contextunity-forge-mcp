# ContextUnity Forge MCP

[![Rust](https://img.shields.io/badge/rust-2021%20edition-blue.svg)](https://www.rust-lang.org)
[![MCP Protocol](https://img.shields.io/badge/MCP-Model%20Context%20Protocol-green.svg)](https://modelcontextprotocol.io)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](#license)

ContextUnity Forge MCP is a local-first, zero-daemon Rust engine for AI coding agents. It brings together deep code graph navigation, living documentation indexing, and an Augmented Context-Driven Delivery (ACDD) lifecycle state machine into a single high-performance binary.

The same binary acts as both a standard stdio [Model Context Protocol (MCP)](https://modelcontextprotocol.io) server (20 tools) for coding assistants and an autonomous command-line interface (CLI) for headless workflows.

---

## The Three Core Pillars

Forge is architected around three foundational pillars designed to eliminate LLM hallucinations, enforce architectural discipline, and enable reliable multi-agent software engineering:

```text
┌───────────────────────────────────────────────────────────────────────────┐
│                          CONTEXTUNITY FORGE MCP                           │
└─────────────────────────────────────┬─────────────────────────────────────┘
                                      │
         ┌────────────────────────────┼────────────────────────────┐
         ▼                            ▼                            ▼
┌──────────────────┐        ┌──────────────────┐        ┌──────────────────┐
│ 1. CODE          │        │ 2. LIVING        │        │ 3. ACDD STATE    │
│    INTELLIGENCE  │        │    DOCUMENTATION │        │    MACHINE       │
│                  │        │                  │        │                  │
│ • Tree-sitter AST│        │ • SQLite Graph   │        │ • Flexible Gates │
│ • code_map_*     │        │ • BM25 Sections  │        │ • DAG Wave Concur│
│ • Direct Symbol  │        │ • Code Traceable │        │ • Feedback Loops │
│ • ast_grep_search│        │ • INV-* Invariant│        │ • Lean Guidance  │
│ • Blast Radius   │        │   Auto-Surfacing │        │ • Delivery Guard │
└──────────────────┘        └──────────────────┘        └──────────────────┘
```

---

### 1. Code Intelligence and Structural Navigation

Forge parses repository source code into an incremental, queryable SQLite graph (`.forge/code-map.sqlite`) using native Tree-sitter grammars. Rather than relying on fuzzy text grep, agents navigate code using high-level structural semantics:

- **Direct Single-Shot Symbol Reads (`forge_read`)**: When an agent knows a declaration name, it reads the complete, digest-verified implementation in a single call via `forge_read(selector="SymbolName")` (or `forge_read(selector="path:SymbolName")`). This completely replaces wasteful two-step text grepping and partial line scanning.
- **Deep Relationship Graph**:
  - `code_map_search`: Find declarations by exact name, prefix pattern (`parse*`), or semantic query.
  - `code_map_inspect`: Inspect full signatures, docstrings, callers, callees, and linked documentation.
  - `code_map_explain` & `code_map_impact`: Calculate the blast radius of proposed changes across call and reference hierarchies.
  - `code_map_tests`: Identify covering tests reaching a symbol or discover production components exercised by a test.
  - `code_map_prove_removal`: Statically estimate structural side-effects before removing functions or types.
- **Syntactic Pattern Search (`ast_grep_search`)**: Match structural AST patterns (e.g. `fn $NAME($ARGS) -> Result<$T> { $$$ }`) across admitted source files.
- **Multi-Language & Multi-Repo Federation**: Built-in support for Python, Rust, TypeScript, JavaScript, Vue, HTML, Protocol Buffers, YAML, and TOML (extended with `--features all-languages` for Go, Java, C#, Kotlin, PHP, Ruby, C, C++).
- **Linked Workspaces & Cross-Repository Indexing**: Forge natively supports multi-repository architectures through `linked_workspaces` in `forge-mcp.yaml`. Sibling repositories are integrated into the unified code graph for cross-workspace symbol resolution, import linking, and cross-repo impact analysis:
  ```yaml
  # forge-mcp.yaml: federate external repositories into the code map
  linked_workspaces:
    - name: shared-core
      path: ../shared-core
      roots: [src]
      docs: [docs]
      ignore: [target, node_modules]
  ```

---

### 2. Living Documentation Alongside Code

Documentation in Forge is not an afterthought stored in disconnected wikis; it is indexed into the very same SQLite graph as the source code:

- **BM25 Searchable Section Chunks**: Markdown files under configured `docs` paths are parsed at `##` and `###` headings into distinct `DocSection` units. Agents search technical documentation using full-text BM25 queries (`search_docs`) filtered by `doc_type` (such as `architecture`, `guide`, `plan`, `adr`).
- **Bidirectional Symbol Traceability**: References to code declarations (e.g. `` `TokenValidator` `` or `` [`TokenValidator`](src/auth/token.rs) ``) are resolved against the AST code map. When unique matches occur, Forge creates verified edges connecting documentation sections directly to code declarations.
- **Automatic Invariant Surfacing (`INV-*`)**:
  - Architectural invariants defined in Markdown GitHub alert callouts (`> [!IMPORTANT] Invariant: ...`) containing `INV-*` identifiers are indexed as governance rules.
  - Whenever an agent inspects a symbol (`code_map_inspect`) or performs an impact analysis (`code_map_impact`), Forge automatically surfaces the governing invariants directly in the agent's context. Agents are alerted to architectural constraints *before* writing code.

---

### 3. ACDD — Lifecycle State Machine and Orchestration

**Augmented Context-Driven Delivery (ACDD)** replaces open-ended, unstructured coding loops with a deterministic verification state machine. It binds Git-tracked contracts (`docs/milestones/*.md`, `docs/plans/*.md`), persistent SQLite state (`.forge/tasks.sqlite`), and declarative YAML profiles so every worker receives only the context relevant to its active gate:

- **Composable Gates & Universal Profiles**: Workflows are assembled from hierarchical agent, command, and container gates. Repositories inherit and specialize the embedded baseline (`crates/forge-tasks/src/tasks/acdd.default.yaml`) through a thin `.forge/acdd/profile.yaml` overlay or replace it via `inherit: none`.
- **Topological DAG Waves & Path Isolation**: Task dependencies (`depends_on`) resolve into execution waves running inside isolated `.worktrees/` protected by exclusive path locks.
- **Session-Preserving Repair Loops & Dynamic Context**: Every gate claim delivers tailored `workflow_guidance`, an isolated `context_bundle`, and persistent `task_blackboard` notes; reviewer rejections resume the builder session with full context intact.
- **Three Execution Tiers**:
  1. **Milestones (`docs/milestones/*.md`)**: Reviewed team delivery contracts with upfront blueprint synthesis, isolated task DAG waves, independent integration review, and cryptographic handoff receipts.
  2. **Documentation Plans (`docs/plans/*.md`)**: Structured documentation-driven step execution in-place with snapshot persistence (*under active alpha testing*).
  3. **Standalone Workflows (`docs/workflows/*.yaml`)**: Lightweight taskless root-gate pipelines with ephemeral observations for sweeps, linting, authoring, and checks (*under active alpha testing*).

> [!TIP] Complete ACDD, Profile & Tool References
> - [**ACDD Architecture & Default Profile Guide (`docs/reference/acdd.md`)**](docs/reference/acdd.md) — full explanation of ACDD, how gates and universal profiles work, and a gate-by-gate walkthrough of `acdd.default.yaml`.
> - [**ACDD Profiles Reference (`docs/reference/profiles.md`)**](docs/reference/profiles.md) — profile schema, command pattern rules, and validation constraints.
> - [**MCP Tools Reference (`docs/reference/mcp-tools.md`)**](docs/reference/mcp-tools.md) — full reference for all 20 MCP tools exposed to AI agents.
> - [**CLI Reference (`docs/reference/cli.md`)**](docs/reference/cli.md) — complete reference for all command-line operations and terminal automation.
> - [**Task Lifecycle Reference (`docs/reference/tasks.md`)**](docs/reference/tasks.md) — in-depth guide to task state machines, blackboard, and evidence envelopes.

---

## Architectural Principles

- **Zero-Daemon, Local-First**: No background daemons, cloud dependencies, or idle RAM consumption. The server starts instantly over stdio or CLI, incrementally updates `.forge/code-map.sqlite`, and releases resources immediately.
- **Strict Context Budgeting**: All tool outputs are formatted for high signal-to-noise ratio in Markdown or JSON. Output sizes are bounded by configurable limits (default 64–96 KiB) to prevent context window saturation.
- **Branch-Local State Isolation**: Each Git worktree maintains its own `.forge/code-map.sqlite` and configuration, while linked worktrees share the task database to coordinate milestone execution safely.

```text
repository files
       │
       ▼
 forge-mcp.yaml          which roots, docs, milestones, and linked repos
       │
       ├──► .forge/code-map.sqlite     symbols, AST edges, docs sections, invariants
       │
       └──► .forge/tasks.sqlite        queue, claims, proofs, receipts, blackboard
                   ▲
                   │
       docs/milestones/*.md            reviewed contracts committed in Git
```

---

## Dual-Surface Interface: MCP Tools and CLI

Every core operation is available both to AI agents over MCP and to developers/scripts via the CLI (detailed in [**`docs/reference/mcp-tools.md`**](docs/reference/mcp-tools.md) and [**`docs/reference/cli.md`**](docs/reference/cli.md)):

| Capability | MCP Tool (AI Agents) | CLI Command (Terminal / Automation) |
|---|---|---|
| **Build / Refresh Graph** | Automatic on first call & file change | `scan`, `build`, `delta` |
| **Search Symbols** | `code_map_search` | `query search` |
| **AST Pattern Match** | `ast_grep_search` | `ast grep` |
| **Inspect Declarations** | `code_map_inspect` | `query inspect` |
| **Read Symbol Source** | `forge_read(selector="...")` | `read --selector "..."` |
| **Blast Radius & Impact** | `code_map_impact`, `code_map_explain` | `query impact`, `query explain` |
| **Test Coverage Seams** | `code_map_tests` | `query tests` |
| **Safe Deletion Analysis** | `code_map_prove_removal` | `query remove` |
| **Documentation Search** | `search_docs`, `get_doc` | `docs search`, `docs get` |
| **Task Queue & Claims** | `task_list`, `task_claim`, `task_submit` | `task list`, `task claim`, `task submit` |
| **Task Gate Verification** | `task_manage(action: "verify")` | `task verify` |
| **Blackboard Messaging** | `task_blackboard` | `task blackboard post / read / inspect` |
| **Subtask Tracking** | `task_manage(action: "subtask_*")` | `task subtask add / update / list` |
| **Scope Extension** | `task_manage(action: "extend_scope")` | `task extend-scope` |
| **Headless Milestone Run**| *CLI only* | `milestone run` |
| **Milestone Handoff** | *CLI only* | `milestone handoff-context`, `verify`, `handoff` |
| **Structured Plans** | *CLI only* | `plan list`, `plan show`, `plan run` |
| **Standalone Workflows** | *CLI only* | `workflow list`, `workflow show`, `workflow run` |
| **Built-in Reference** | `forge_guide` | `guide init`, `guide validate`, `guide lifecycle` |

---

## Installation

Install ContextUnity Forge MCP binary, companion skills, and project client configuration in one command:

### Quick Install (`install.sh`)

```bash
# Latest stable release (recommended)
curl -fsSL https://raw.githubusercontent.com/ContextUnity/contextunity-forge-mcp/main/install.sh | sh

# Nightly pre-release (active development & latest milestone features)
curl -fsSL https://raw.githubusercontent.com/ContextUnity/contextunity-forge-mcp/main/install.sh | sh -s -- --nightly
```

If you cloned the repository locally:
```bash
./install.sh             # Interactive prompt: choose between latest and nightly
./install.sh --latest    # Force latest stable release
./install.sh --nightly   # Force nightly pre-release
```

### What `install.sh` Does Automatically
1. **Auto-Detects Platform**: Identifies OS and architecture (Linux `x86_64`/`aarch64`, macOS `x86_64`/`arm64`). No Rust toolchain, Python, Node, or C compiler required.
2. **Cryptographic Verification**: Downloads pre-built stripped binaries and verifies SHA-256 integrity against `SHA256SUMS`.
3. **Atomic Binary Placement**: Installs the binary into `~/.local/bin/contextunity-forge-mcp`.
4. **Agent Skills Hub**: Syncs companion skills to `~/.agents/skills/` and symlinks them to detected agent directories (Antigravity/Gemini CLI, OpenAI Codex, Anthropic Claude, Cursor).
5. **Project MCP Configuration**: Atomically creates or updates `.mcp.json` in the current working directory.
6. **Live Reload**: If upgrading an existing installation, gracefully signals running MCP instances.

### Recommendations
- **Add to `PATH`**: Ensure `~/.local/bin` is in your shell `PATH`:
  ```bash
  export PATH="$HOME/.local/bin:$PATH"
  ```
- **Verify Installation**: Confirm by checking the version:
  ```bash
  contextunity-forge-mcp --version
  ```
- **Channel Selection**:
  - Use **`latest`** (default) for stable, production coding workflows.
  - Use **`--nightly`** when you need unreleased milestones, latest prompt optimizations, or recently merged bug fixes.

---

> [!TIP] Comprehensive Installation & Configuration Guide
> For detailed instructions, building from source (`cargo install`), multi-agent IDE configurations (Antigravity, Codex, Claude, Cursor), air-gapped/enterprise installations, and workspace initialization, see the dedicated [**Installation and Operational Guide (`INSTALL.md`)**](INSTALL.md).

---

## Companion Skills

Companion skills teach AI agents how to leverage Forge MCP tools, structure tests, handle review rejections, and maintain documentation:

| Skill | Purpose |
|---|---|
| [`contextunity-forge`](skills/contextunity-forge/SKILL.md) | Primary operational skill: code navigation, single-shot symbol reads, task claims, blackboard, delivery |
| [`test-contract-seam`](skills/test-contract-seam/SKILL.md) | Synthesizing architectural contracts and authoring compiling runtime-red seam tests |
| [`close-review-findings`](skills/close-review-findings/SKILL.md) | Diagnosing review rejections and implementing evidence-closed remediations |
| [`test-suite-refactor`](skills/test-suite-refactor/SKILL.md) | Refactoring and modularizing test suites without losing production coverage |
| [`agent-instructions`](skills/agent-instructions/SKILL.md) | Authoring and updating `AGENTS.md`, `SKILL.md`, and documentation |
| [`lean-ctx`](skills/lean-ctx/SKILL.md) | Reading source and command output with minimal token consumption |

---

## Contributing and Verification

We welcome contributions, bug fixes, documentation improvements, language profiles, and framework manifests.

> [!NOTE]
> - **Workflows and Plans**: Standalone root-gate workflows (`workflow run`) and structured documentation plans (`plan run`) are currently under active testing and validation.
> - **Extended Language Scope**: While default languages are fully stable, the extended language profiles (`--features all-languages` covering Go, Java, C#, Kotlin, PHP, Ruby, C, C++) need further refinement and community contributions to improve language linkers and AST extractions.

Before submitting changes, ensure all verification gates pass:

```bash
# Code formatting
cargo fmt --check

# Strict Clippy analysis
cargo clippy --all-targets --all-features -- -D warnings

# Task engine and integration tests
cargo test -p forge-tasks --lib
cargo test --locked
```

Refer to [TESTS.md](TESTS.md) for domain-specific verification commands and count rules, [docs/AGENTS.md](docs/AGENTS.md) for documentation authoring governance, and [CHANGELOG.md](CHANGELOG.md) for release notes.

---

## License

ContextUnity Forge MCP is licensed under the [MIT License](LICENSE).
