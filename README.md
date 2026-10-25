# ContextUnity Forge MCP

[![Rust](https://img.shields.io/badge/rust-2021%20edition-blue.svg)](https://www.rust-lang.org)
[![MCP Protocol](https://img.shields.io/badge/MCP-Model%20Context%20Protocol-green.svg)](https://modelcontextprotocol.io)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](#license)

Forge is a local Rust engine for AI coding agents. It keeps a structural map of the repository, indexes the documentation next to that map, and runs milestone work through a task queue with proof at each gate. The same binary is a stdio [Model Context Protocol](https://modelcontextprotocol.io) server (`contextunity-forge-mcp`, 20 tools) and a command-line interface.

It follows a **zero-daemon, local-first architecture**: pure execution over stdio or CLI without background daemons, cloud dependencies, or idle RAM consumption. High-speed Tree-sitter indexing creates `.forge/code-map.sqlite` incrementally on the first call and refreshes only changed files on subsequent reads.

Each tool call returns a bounded slice: a symbol and its neighbors, a documentation section linked to that symbol, or the task bundle for the gate just claimed. MCP responses have a 96 KiB default and maximum; smaller configured limits remain authoritative, including this repository's 64 KiB setting. The agent continues from that slice — the contract, the code, and the docs that bear on the current step — while the rest of the repository stays on disk.

Tool content and CLI command results default to Markdown. Select MCP
`format: "json"` or CLI `--format json` for structured processing. Stdio MCP
keeps JSON-RPC framing. See the [presentation contract](docs/reference/mcp-markdown-contracts.md)
for shared formatting, preserved evidence, and continuations.

## How the pieces work together

Three local stores share one workspace. `forge-mcp.yaml` in the repository root decides what is admitted.

```text
repository files
        │
        ▼
 forge-mcp.yaml          which roots, docs, milestones, and linked repos
        │
        ├──► .forge/code-map.sqlite     symbols, calls, docs sections, edges
        │
        └──► .forge/tasks.sqlite        claims, proofs, receipts
                    ▲
                    │
        docs/milestones/*.md            the contracts people review in Git
```

A session moves through those stores in order:

1. The first `code_map_overview` builds `.forge/code-map.sqlite` from the admitted source and Markdown. Later reads refresh it when files change.
2. The agent looks up a symbol, then asks for callers, impact, or tests. Inspection also returns documentation sections that mention that symbol, so an architectural rule arrives with the code it governs.
3. People write milestones in Git. Forge syncs those specifications into `.forge/tasks.sqlite`. Rebuilding the code index leaves the task queue alone.
4. When an agent claims a gate, Forge returns `workflow_guidance` for that gate and a context bundle sized to the role. A contract claim carries the write scope, governing decisions, in-scope symbols, and covering tests. A review claim carries the candidate snapshot and the invariants to check. Gate history stays in SQLite and comes back through `task inspect` when someone asks for it.
5. A passing delivery writes a receipt into SQLite and into the milestone document.

The built-in profile walks a task through `contract`, `build`, `review`, and `deliver`. A repository profile can rename gates, change proof, and assign roles. The claim response is what the agent follows.

### Worktrees and multi-repo workspaces

- **Branch-local workspace state**: Each worktree keeps its own `.forge/code-map.sqlite`, adapter/configuration, milestone manifests, and `.forge/checkpoints.json`, so indexed and file-backed state follows the checked-out branch.
- **Git-shared task state**: Git-linked worktrees use the shared task database resolved by the task store. Standalone repositories keep independent databases unless explicitly configured to share one.
- **Linked Workspaces (Multi-Repo)**: Sibling repositories listed under `linked_workspaces` in `forge-mcp.yaml` join the workspace code graph for cross-repo symbol resolution, import linking, and task federation.


## ACDD

Augmented Context-Driven Development is the work loop around that queue.

A milestone is a Markdown contract in `docs/milestones/`. It names the goal, the tasks, the files each task may change, and the proof expected. People create and close milestones from the CLI, so admission and archival stay a human action. Concurrent agents work in separate Git worktrees under `.worktrees/`. Each task holds an exclusive lock on its paths, so two agents do not edit the same file under one milestone.

The built-in gates are:

| Gate | Who claims it | What "pass" means |
|---|---|---|
| `contract` | Contract author | A failing seam test that states the behavior, or a direct proof when the profile allows it |
| `build` | Builder | The implementation and a test command that exits 0 |
| `review` | A worker other than the builder | The candidate checked against the profile's review contours |
| `deliver` | A worker other than the builder | A receipt written into SQLite and the milestone |

Review in the built-in profile uses five contours: paths, claims, concurrency, project isolation, and administration. A rejection sends the task back to `build` with the findings attached. The next claim for the builder includes those findings, so the repair starts from the review.

On every claim the agent also receives the role, the steps, and the tools that fit the gate. Contract work is pointed at overview, docs, and inspect. Build work is pointed at structural search and explain. Review work is pointed at impact, tests, and removal analysis. That list is advisory context: it tells the agent where to look before it starts reading unrelated files.

Evidence submitted at each gate is a JSON object bound to the claim. The field shapes live in the [task operations](docs/reference/tasks.md) reference.

### What the agent receives

`task claim` is the moment Forge assembles context. The bundle depends on the gate:

- **Contract.** The task target, allowed write scope, dependencies, invariants, architectural decisions that apply, the symbol skeleton inside the scope, covering tests, and recent blackboard notes.
- **Build.** The same contract material, plus the accepted seam test and any review findings still open after a rejection.
- **Review.** The contract's scope and invariants, the candidate commit and how to inspect it, and blackboard notes. The reviewer works from the diff.
- **Deliver.** The contract, the snapshot to land, and the receipt rules.

Blackboard text in that bundle is shortened. The full message is still there via `task_blackboard` inspect. Full gate logs stay available through `task inspect`. The trim exists so a long task history does not crowd out the current gate. The accepted decision for this shape is [ADR 0016](docs/adr/0016-lean-stage-tailored-task-context-bundles.md).

## Workspace adapter

`forge-mcp.yaml` is the only project file Forge needs. It lists indexing roots, documentation paths, milestone and plan directories, linked repositories, the task database, and response limits. Generate a starter with:

```bash
contextunity-forge-mcp guide init
```

A minimal starter looks like:

```yaml
roots:
  - src
docs:
  - docs
milestones:
  - docs/milestones
ignore:
  - target
  - node_modules
tasks_db: .forge/tasks.sqlite
agents_guidance: AGENTS.md
```

With no adapter, Forge scans the selected workspace root, respects `.gitignore`, skips common build and cache directories, and writes that worktree's `.forge/code-map.sqlite`. The task database defaults to `.forge/tasks.sqlite`; Git-linked worktrees share it through task-store resolution, while standalone repositories remain independent unless configured to share a store.

| Path | What it holds | In Git |
|---|---|---|
| `forge-mcp.yaml` | Roots, docs, milestones, linked workspaces, response limits | Yes |
| `docs/milestones/*.md` | Milestone contracts and task specifications | Yes |
| `.forge/acdd/profile.yaml` | Gate order, roles, proof, and review contours for this repo | Yes, when you pin a profile |
| `.forge/frameworks/*.{toml,yaml,yml}` | Project rules using existing typed framework operations | Yes, when you add one |
| `.forge/code-map.sqlite` | Symbols, AST facts, documentation sections, edges | No |
| `.forge/tasks.sqlite` | Queue, claims, proofs, receipts | No |
| `.forge/checkpoints.json` | Named session bookmarks | No |

Commit the profile and any framework manifests you rely on. Ignore the SQLite files and checkpoints. The [gitignore example](skills/contextunity-forge/references/gitignore.example) matches that split. Field-by-field rules are in the [configuration reference](docs/reference/configuration.md).

## Planning and the task queue

### Milestones

Milestone commands are CLI-only. Creating and archiving a milestone is a decision about scope, and it stays outside the agent's tool list.

```bash
contextunity-forge-mcp milestone init --num 045 --slug auth
contextunity-forge-mcp milestone list --status active
contextunity-forge-mcp milestone show 045 --full
contextunity-forge-mcp milestone handoff-context 045 --target-base main > /tmp/045-context.json
# Obtain independent review and run project verification for this exact context.
contextunity-forge-mcp milestone handoff 045 --evidence-file /tmp/045-evidence.json
```

`handoff` validates immutable task proofs and the [content-bound integration evidence](docs/reference/tasks.md#milestone-cli-lifecycle), then archives the milestone. Supply independent review and verification for the final content and selected target base. Rewriting branch commit SHAs does not invalidate unchanged content evidence; Forge records results without launching agents or running tests.

### Tasks

`task sync` reads task blocks from milestone Markdown into `.forge/tasks.sqlite`. From then on the agent works the queue:

```bash
contextunity-forge-mcp task list
contextunity-forge-mcp task claim <task-id> --stage contract --worker agent-1 --worktree .
contextunity-forge-mcp task submit <task-id> --stage contract --action pass --evidence '<JSON>'
```

`task list` defaults to ready tasks in the selected workspace's unique active milestone. No active milestone returns an empty page; multiple active milestones require an explicit milestone or milestone-status filter. Claim is atomic: a second worker asking for the same gate gets `TASK_ALREADY_CLAIMED`. Submit records pass or reject against that claim revision. Scope can grow during the task with `extend-scope`, and Forge refuses paths another active task already owns. Profiles under `.forge/acdd/` are protected from casual scope extension, so an agent in an ordinary task leaves the gate rules alone.

Subtasks record discoveries under the current task. They do not become new milestone roots. The [task reference](docs/reference/tasks.md) has the evidence schemas, state transitions, and the matching MCP arguments.

### Blackboard

`task_blackboard` is a small message board in the same SQLite file, at milestone, task, or subtask scope. Agents post drafts, notes, findings, blockers, decisions, and deferred items there instead of committing scratch discussion. A claim includes the recent notes that apply to the current gate, so the next worker sees them without a separate search. On delivery, decisions and deferred items are copied into the receipt and the task's messages are cleared.

### Profiles

A profile is a YAML file that names gates, proof kinds, review contours, roles, and recommended models. Forge resolves it in this order: a pin on the task, a pin on the milestone, `acdd_profile` in `forge-mcp.yaml`, `.forge/acdd/profile.yaml`, then the built-in profile compiled into the binary.

Pin a profile as `path:sha256-prefix`. On later claims Forge recomputes the hash and stops with `TASK_PROFILE_TAMPERED` if the file changed. That keeps the gate rules stable for the life of the task.

Writing a profile for a milestone or a task — gates, roles, models, contours, and the pin — is documented in [ACDD profiles](docs/reference/profiles.md). The executable example is [`acdd_profile.yaml.example`](skills/contextunity-forge/references/acdd_profile.yaml.example).

## Code graph

The code map is a SQLite graph built from Tree-sitter. It records declarations, imports, and the references the linker can resolve inside the indexed roots. An edge means a candidate was found in that scope. Dynamic dispatch, macros, and generated code can leave a reference unresolved; the [language reference](docs/reference/languages.md) lists those limits per language.

The agent walks the graph through a short series of calls. Each one adds a narrower fact to context:

1. `code_map_overview` reports the workspace, generation, language coverage, and documentation counts. This is the check that the index matches the repository you think you opened.
2. `code_map_search` finds symbols by exact name, prefix (`parse*`), or full text. `ast_grep_search` matches a syntax pattern (`print($VALUE)`) in admitted files of one language.
3. `code_map_inspect` returns the signature, docstring, and a short caller and callee preview. Linked documentation sections come with it. `get_code_snippet` adds a bounded source window after checking that the file still matches the indexed digest.
4. `code_map_explain` lists direct relationships. `code_map_impact` walks the inbound blast radius, one edge deep unless you ask for more.
5. `code_map_tests` names tests that reach the symbol, or the production code a test depends on. `code_map_prove_removal` estimates what a deletion would disturb from static references. It is a structural estimate, and runtime safety still belongs to the tests.
6. `code_map_analyze` returns stored syntax diagnostics and, on request, dependency cycles. `code_map_query` runs the same graph operations or one read-only `SELECT`.

Search hits include the selector to pass into inspect. Pages continue with `offset` and the `generation` token from the previous page, so a long result arrives in parts.

### Languages

A default build indexes Python, Rust, TypeScript, JavaScript, Vue, HTML, Protocol Buffers, YAML, and TOML. Markdown and MDX are indexed as documentation, separate from those code profiles. `--features all-languages` adds Go, Java, C#, Kotlin, PHP, Ruby, C, and C++.

The graph matches the binary you installed. After changing features, rebuild the index. Extensions, static limits, and linker behavior are in the [language support](docs/reference/languages.md) reference.

### Adding a language or a framework

These are different jobs.

A **framework** adds rules for a language Forge already parses. Put a TOML or YAML manifest under `.forge/frameworks/`; its filename stem is the pack ID. A same-ID project file merges with the bundle by default, with local rules taking precedence. Use `replace = true` for full replacement or `enabled = false` to disable that effective pack. Bundled files are inventoried automatically at build time. Supported typed operations and pack examples are in [framework manifests](docs/reference/framework-manifests.md).

A **language** needs a compiled profile. Implement `LanguageProfile` for discovery and extraction, and `LanguageLinker` when import or receiver resolution is language-specific. Register a `lang-*` Cargo feature and rebuild. The traits, the directory layout, and where framework data plugs into a linker are in [engine modularity](docs/architecture/modularity.md). The same [framework manifests](docs/reference/framework-manifests.md) page has the section on extending language support, including what a project manifest can supply only when a linker already asks for it.

### Documentation in the same graph

Markdown under the adapter's `docs` paths is split into sections at `##` and `###` headings. `search_docs` runs full-text search over those sections, optionally filtered by `doc_type`. `get_doc` returns one document or one heading.

Uniquely resolved documentary symbol claims, such as `LanguageProfile`, become edges between the section and the code entity; ambiguous candidates remain navigation only. `code_map_inspect` and `code_map_explain` return verified sections with the symbol. A GitHub alert containing `Invariant:` marks an invariant section. An `INV-*` rule creates a governance edge only when a uniquely resolved code claim with an explicit source-file target appears on the same line, so verified rules can arrive with impact and inspection.

That is why documentation pages use YAML frontmatter, stable headings, and backticked symbols. The authoring rules are in [documentation instructions](docs/AGENTS.md).

## Tools and the command line

MCP is the surface agents use. The CLI is the same operations for a terminal, plus indexing commands and milestone administration. MCP reads refresh the index when source changes. CLI queries read the database as it is; run `build` or `delta` after edits when you are working from the terminal.

| You want | MCP | CLI |
|---|---|---|
| Build or refresh the map | First overview, then automatic refresh | `scan`, `build`, `delta` |
| Find and read symbols | `code_map_search`, `code_map_inspect`, `get_code_snippet`, `ast_grep_search` | `query search`, `query inspect`, `ast grep` |
| Callers, impact, tests | `code_map_explain`, `code_map_impact`, `code_map_tests`, `code_map_prove_removal` | `query explain`, `query impact`, `query tests`, `query remove` |
| Diagnostics and SQL | `code_map_analyze`, `code_map_query` | `query analyze`, `query run sql` |
| Documentation | `search_docs`, `get_doc` | `docs search`, `docs get` |
| Task queue | `task_list`, `task_claim`, `task_submit`, `task_manage`, `task_blackboard` | `task …` |
| Session notes and help | `session_checkpoint`, `forge_guide` | `guide init`, `guide validate` |
| Open or archive a milestone | CLI only | `milestone init`, `list`, `show`, `handoff-context`, `handoff` |

`forge_guide` with a topic (`acdd`, `query`, `adapter`, `docs`, `ast`) returns the short built-in guide for that area. Field-level contracts are in the [MCP tools](docs/reference/mcp-tools.md) reference and the [CLI reference](docs/reference/cli.md).

## Install and connect

You need a stable Rust toolchain and a C compiler. The C compiler builds the bundled SQLite and the Tree-sitter grammars.

```bash
cargo install --path . --locked
cargo install --path . --locked --features all-languages   # optional languages
```

Point the MCP client at the binary. The process working directory is the workspace. If the client starts the server somewhere else, set `cwd` or pass `--root`. Both forms, plus what to check when the server does not appear, are in the [MCP setup guide](docs/reference/mcp-setup.md).

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

Call `code_map_overview` once. That first call creates the index. On Unix, `contextunity-forge-mcp reload` sends `SIGHUP` so a running server picks up a new binary.

### Skills

Install the `contextunity-forge` skill where the agent loads skills (`~/.agents/skills/contextunity-forge/` or `.agents/skills/contextunity-forge/` in the repo). Claim and inspect report `TASK_GUIDANCE_MISSING` when that skill is absent, when `AGENTS.md` does not name it, when the guidance file is missing, or when `.gitignore` lacks the canonical protective rules.

In the repository `AGENTS.md`, tell the agent to load `contextunity-forge`, to follow `workflow_guidance` from the claim, and to use this repo's `TESTS.md` for verification commands. The copy-paste router is [`agents.md.example`](skills/contextunity-forge/references/agents.md.example).

Companion skills cover the work around a Forge session. Copy a skill into the agent skills directory when you want it; a local copy with the same name shadows a global one.

| Skill | When it helps |
|---|---|
| [`contextunity-forge`](skills/contextunity-forge/SKILL.md) | Every session: which tool to call, how a claim is answered, how delivery and handoff work |
| [`test-contract-seam`](skills/test-contract-seam/SKILL.md) | Writing the failing seam test the contract gate asks for |
| [`test-suite-refactor`](skills/test-suite-refactor/SKILL.md) | Splitting a test suite while production-path coverage stays |
| [`close-review-findings`](skills/close-review-findings/SKILL.md) | Turning review findings into a fix with evidence |
| [`agent-instructions`](skills/agent-instructions/SKILL.md) | Editing `AGENTS.md`, skills, and docs |
| [`lean-ctx`](skills/lean-ctx/SKILL.md) | Reading source and command output in less context |

The [skills index](skills/README.md) is the same list with install notes.

## Contributing

Bug reports, documentation, language profiles, and framework manifests are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) covers the toolchain, the branch, and the gates every change passes:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

Domain commands and milestone handoff counts are in [TESTS.md](TESTS.md). A change to a CLI command, MCP tool, or configuration field updates the matching page under `docs/reference/` in the same change. Pages that Forge indexes follow the [documentation instructions](docs/AGENTS.md).

The rest of the map — architecture, decisions, runbooks, roadmap, and milestones — starts at [docs/README.md](docs/README.md).

## License

ContextUnity Forge MCP is licensed under the [MIT License](LICENSE).
