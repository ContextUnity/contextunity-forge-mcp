---
doc_type: api
title: Workspace configuration
---

# Workspace configuration

`forge-mcp.yaml` is optional. Without it, Forge scans the workspace root and excludes common build and cache directories. `guide init` writes a starter file. Put the adapter in the workspace root; `build` and `scan` can select another in-root file with `--adapter`.

## Indexing scope

```yaml
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
linked_workspaces:
  - name: shared-library
    path: ../shared-library
    enabled: true
    roots:
      - src
    docs:
      - docs
```

`roots` and `docs` are literal, relative directories or paths inside the workspace. `milestones` and `plans` default to `docs/milestones` and `docs/plans` when omitted. Their `*` segments match one directory component (for example, `extensions/*/docs/milestones`). Markdown and MDX in admitted documentation locations, including milestone and plan documents, remain searchable and available for graph navigation; this does not change which configured directories task discovery reads. Non-document files in those directories stay outside the source index. `ignore` matches file or directory basenames. The scanner admits compiled source extensions and Markdown/MDX within these roots; scan roots do not define import or package roots. Unavailable linked workspaces are skipped. A linked entry with `enabled: false` is excluded; omission of `enabled` means true.

The default basename exclusions cover `.git`, `.forge`, `target`, `node_modules`, `.venv`, `__pycache__`, `build`, `dist`, `coverage`, `.cache`, `.next`, `.nuxt`, `.svelte-kit`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`, and `.gradle`. The scanner also honors `.gitignore` and skips symbolic links. Add project-specific generated directories with `ignore`; authored source under an excluded basename can be scoped explicitly with a different directory name.

The index records linked paths under their configured workspace names. Changing source scope or linked-workspace settings causes the MCP server to rebuild or update its owned index on the next database read. CLI readers automatically build a missing or schema-incompatible index, but do not refresh existing indexed files after source changes; run `build` or `delta` to update CLI results.

Dependency manifest discovery combines scanner-admitted files with supported
manifest files directly in the primary and enabled linked workspace roots.
These direct-root files participate even when the source roots are narrower.
Nested manifests outside admitted roots do not participate: with `roots: [src]`,
a root `requirements.txt` is included, while `unindexed/requirements.txt` is
excluded. Discovery preserves ignore and symlink guards and the 4 MiB manifest
read limit. Cold build, delta, and MCP freshness use the same selected paths;
MCP checks changes after its normal freshness TTL.

## Cold-build memory budget

Set a transient source-extraction budget in MiB under `limits`:

```yaml
limits:
  memory_budget_mb: 200
```

`memory_budget_mb` must be a positive integer whose MiB-to-byte conversion fits in `u64`. When omitted, `FORGE_MEMORY_BUDGET_MB` supplies the same setting; when both are omitted, the builder chooses an adaptive budget from available memory. The YAML value takes precedence over the environment. The budget controls extraction batch sizing and the SQLite cache. Changing the budget preserves extracted facts and per-domain commitments, but an explicit YAML value is part of the workspace configuration fingerprint and can change the overall output root. An environment-only override is transient and leaves the adapter policy fingerprint unchanged. A single admitted file can exceed the budget because the builder must hold that file while parsing it, but source memory remains bounded by the configured batch and file-size limit.

## Repository documentation admission

This repository lists current documentation files and directories explicitly in
`docs`: governance, navigation, roadmap, ADRs, architecture, reference,
runbooks, testing, and the milestone and plan trees configured separately in
`milestones` and `plans`. Those Markdown and MDX contracts are searchable too;
this indexing scope does not change task-discovery or authority rules.

## Source-only linked libraries

Set `ignore` inside each linked entry to exclude that library's vendor directories and build outputs. A linked entry does not inherit the target library's `forge-mcp.yaml` settings. Keep the same roots and exclusions in the library's own adapter and its consumers.

```yaml
linked_workspaces:
  - name: shared-library
    path: ../shared-library
    roots: [src, frontend, tests]
    docs: [docs]
    ignore: [vendor, vendors, dist, build]
```

`code_map_overview` with `coverage_by_workspace` reports counts from the persisted primary and linked-workspace identities. Reopening the index retains those logical names and separates linked coverage from the primary workspace.

Ignore names are literal basenames, not glob patterns. Add a directory such as `static` only when the library builds all of its contents from sources retained elsewhere. Keep authored host assets and source-level type contracts. Excluded bundles neither enter the index nor trigger source refresh when rebuilt.

## Task storage

Task operations read `forge-mcp.yaml`; an omitted `tasks_db` defaults to
`.forge/tasks.sqlite`. Relative paths automatically resolve against the primary
worktree root in linked git worktrees (via `git rev-parse --git-common-dir`)
or the local configuration directory in standalone workspaces, enabling zero-config
cross-worktree task coordination without machine-specific absolute paths.
`task_repository` and `task_project` select the
qualified task namespace. See [repository tasks](tasks.md) for identity,
concurrency, receipts, and administration. Code indexing works without task
storage configuration.

Code-map output guards reject unreadable existing database targets rather than
replacing them, including custom task database names not configured in this
workspace. Readable non-SQLite cache files remain rebuildable.

Subproject identity uses milestone `project` frontmatter, then the longest
matching `owners` prefix, then path inference, then the workspace default.
Root `owners` maps repository-relative directory prefixes to project names;
prefixes match whole path components. Root `aliases` maps the selected name to a
canonical name once, including the configured default project. Alias lookup is
not recursive: with `old: current` and `current: next`, selecting `old` yields
`current`. Discovery, synchronization, task authority, receipts, and
milestone show/handoff use this same identity. Directories containing only declared
projects register those projects; a directory with undeclared manifests also
retains its configured or inferred owner. Empty directories and directories with
only a README do not create subproject workspaces. When milestone initialization
emits `project` frontmatter, it writes the pre-alias spelling so later parsing
applies aliases exactly once.

```yaml
owners:
  extensions/base: base
aliases:
  legacy-base: base
```

Code-map output and task storage use separate databases. Index commands reject
reserved task filenames, the configured `tasks_db` path, and existing task schemas
with `TASK_DATABASE_COLLISION`. A present invalid task configuration also rejects
index output because its reserved path cannot be validated. Opening a task store
rejects code-map filenames and schemas. New task-store initialization shares the
index publication lock; opening an initialized task store needs no writer lock.

Set root `agents_guidance` to a path inside the root repository. It defaults to
`AGENTS.md`. Claim and inspect include that path and stage-specific
`workflow_guidance`. A `TASK_GUIDANCE_MISSING` warning is raised if the `agents_guidance`
file is missing, does not reference `contextunity-forge`, the `contextunity-forge` skill
is not installed, or `.gitignore` lacks the canonical protective rules.

Linked entries opt into the primary task store with `tasks.enabled: true`.
`tasks.milestones_dir` defaults to `docs/milestones`, and `tasks.agents_guidance`
defaults to `AGENTS.md`, both relative to the linked repository root.
Omitted or disabled task entries are excluded. See [linked repository tasks](tasks.md#linked-repository-tasks)
for synchronization, namespace selection, and scope confinement.

## MCP response policy

```yaml
response:
  detail: compact
  page_size: 30
  max_output_bytes: 65536
  source_context:
    enabled_by_default: false
    leading_lines: 5
    max_body_lines: 35
```

`page_size` accepts 1–100; `max_output_bytes` accepts 1,024–65,536. `code_map_inspect` and `code_map_explain` source previews default to 5 leading lines and 200 body lines. They accept 0–20 leading lines and 1–500 body lines; configured `source_context` values use those same bounds. This repository overrides the preview body limit to 35 in `forge-mcp.yaml`. `get_code_snippet` uses the configured leading-line default but returns the complete selected symbol when its serialized response fits `max_output_bytes`; an explicit `max_body_lines` is normalized to at least one and limits the returned body. Oversized snippets page at source-line boundaries, and a single oversized line is returned as a safe prefix without a line continuation. `detail` is `compact` or `full`. These settings control presentation and do not change extracted facts or require an index rebuild. Unknown response fields and out-of-range values are rejected.

The root `forge-mcp.yaml` and the file selected with CLI `--adapter` are configuration rather than indexed sources. Nested files with that name and linked-workspace adapters remain ordinary YAML sources when admitted by their configured roots. A linked workspace's own adapter does not control the parent index.

## Declarative ACDD profile and role configuration

The ACDD workflow engine supports declarative customization of gates, review contours, and agent roles via YAML profiles.

### Loading precedence
1. Task specification: `acdd_profile: "<path>:<sha256_prefix>"` in `task.spec.acdd_profile`
2. Milestone frontmatter: `acdd_profile: "<path>:<sha256_prefix>"` in milestone YAML frontmatter
3. Adapter path link in `forge-mcp.yaml` under `acdd_profile: "<path>"`
4. Default repository path `.forge/acdd/profile.yaml`
5. Embedded `acdd.default.yaml` (baseline defaults)

Profiles in linked workspaces are strictly **ignored**; only the active workspace root defines the ACDD profile.

In `forge-mcp.yaml`, `acdd_profile` specifies a path link to the profile file (e.g. `acdd_profile: ".forge/acdd/profile.yaml"`), keeping the adapter decoupled from the profile specification.

Any present or referenced configuration file with invalid YAML syntax or unrecognized fields fails closed with `ACDD_PROFILE_SCHEMA_INVALID` diagnostics. Task stage operations (claims, submissions, CLI/MCP stage arguments) validate that the stage exists in the active profile. If a task references a stage not defined in the active profile's `gates`, Forge fails closed with `TASK_STAGE_UNKNOWN`, naming the unrecognized stage and the active profile's available gates, advising to either migrate the task records or pin the intended profile via `acdd_profile: <path>:<sha256_prefix>`.

### Gate IDs, proof kinds, commands, and policies

Each gate declares a stable `id` and a closed `proof` behavior. The built-in proof kinds are `contract`, `command`, `review`, `delivery`, and `none`. A profile can also define a recursive evidence schema under `proof: { scheme: ... }`; the submitted payload must then use `scheme_proof` and match that schema. Gate IDs contain ASCII letters, digits, `_`, and `-`, and task rows persist those IDs directly in `stage`.

`commands` is a registry keyed by command ID. Each entry can be a shell command string or an object with `command` and optional `description`. A command-proof gate accepts only a registered command ID or its registered command text. `policies` holds nested YAML settings; overrides merge recursively. Named review contour sets live under `contours`. Role entries merge by role, and a contour override replaces the named contour set. Defining `gates` replaces the whole ordered gate sequence, so include all required gates and the terminal delivery gate. References such as `reject_to`, `independent_from`, `receipt_review`, and `review_sources` must name valid preceding gates; delivery review sources must be review-proof gates.

Schema nodes support `object`, `array`, `string`, `integer`, `number`, `boolean`, and `null`. Objects can declare `properties`, `required`, `additionalProperties`, and `enum`; arrays require an `items` schema. The compiler rejects invalid fields, undefined required properties, invalid child schemas, unresolved gate references, and profiles without exactly one terminal delivery gate. See the [complete profile example](../../skills/contextunity-forge/references/acdd_profile.yaml.example) for an executable multi-gate workflow using a recursive schema proof.

### Profile pinning, SQLite metadata, and tampering protection

Custom profiles pinned at planning time use the single-line string format combining file path and SHA-256 hash prefix:

```yaml
acdd_profile: ".forge/acdd/profile.yaml:a1b2c3d"
```

- **Format**: `<path>:<sha256_prefix>`, where `<path>` is the relative path to the profile file, followed by a colon and at least 7 lowercase hexadecimal characters representing the SHA-256 hash prefix of the file.
- **SQLite Metadata**: During `task sync`, the pinned `<path>:<sha256_prefix>` is persisted into task metadata in SQLite.
- **Integrity Validation**: On every gate claim and execution, Forge verifies that the file hash on disk matches the recorded prefix in SQLite. If mismatched or missing, Forge fails closed with `TASK_PROFILE_TAMPERED`.
- **Targeted Loading**: Profile compilation strictly loads the targeted profile file (compiled over embedded defaults); neighbor files in `.forge/acdd/` are not implicitly merged.
- **Protected scope**: Configuration paths under `.forge/acdd/**` are protected system paths and cannot be added dynamically via `extend_scope` (`TASK_SCOPE_PROTECTED`) during execution.
- **Dirty system file prohibition**: During `task_claim`, gate execution, and `task_submit`, Forge checks worktree git status. If any file under `.forge/acdd/**` is dirty (modified, unstaged, staged, or untracked) and is not explicitly admitted in the active task's initial planning-time `task.spec.scope`, Forge fails closed with `TASK_SCOPE_VIOLATION`. Only tasks explicitly contracted to alter profiles at planning time (declaring the path in initial `scope`) can legally modify it during `build`.
- **Zero runtime backward compatibility boundary**: Zero backward compatibility applies strictly to SQLite runtime storage and active milestone manifests. Runtime code speaks exclusively clean entities (`command_proof`, clean gate tokens, canonical topics). Historical archive receipts in `docs/milestones/archive/` are historical artifacts exempt from active profile receipt schema validation.
- **Delivery auto_commit, baseline HEAD consistency, and handoff navigation**: When `auto_commit: true` (default in profile), delivery automatically creates a Git commit from the candidate snapshot tree of the nearest predecessor gate with `sha_snapshot: true` (which is `build` in the default profile, whose candidate tree SHA is verified across all `review_sources`) plus the milestone receipt, with parent `HEAD`. Forge verifies that `git rev-parse HEAD` matches `candidate_baseline_head` (the exact commit SHA of HEAD captured when the candidate snapshot was created). If HEAD has diverged (i.e. unreviewed commits exist on the branch), delivery fails closed with `TASK_DELIVERY_COMMIT_FAILED`. Hooks are enforced. If `git commit` fails, the SQLite transaction rolls back atomically, leaving the task uncompleted at `deliver` with an explicit error, allowing the deliverer to reject the task with findings back to `reject_to` (e.g. `build`) or retry. The created delivery commit SHA is saved in the task's delivery receipt in SQLite. After branch integration, handoff validates content-bound final review/verification and immutable task proofs. Branch history may be rebased or squashed without preserving per-task landing SHAs. Task commit pointers are optional navigation: known or uniquely content-equivalent pointers are refreshed; unresolved pointers are omitted. `record_task_commits: false` (or `commit_tracking: off` / `milestone_only`) omits individual navigation pointers in both Markdown and SQLite. Accepted gate proofs and protected Git proof objects remain unchanged. See [handoff evidence](tasks.md#milestone-cli-lifecycle).

### Role and model configuration

Agent execution and model guidance are configured per **role**, not per task.
`mode` accepts `subagent` or `inline` and effectively defaults to `subagent`.
`reuse_on_reject` effectively defaults to `true`; when a review rewinds to a
builder stage, this retains the prior builder's worker ID in rejection guidance.
`models` is an ordered list of model specifications, with `models[0]` as the
primary recommendation. Entries can be specified as compact strings with optional
reasoning (`model:reasoning` or `model`, e.g. `'gpt6-luna:xhigh'`, `'composer-2.5'`)
or structured mappings (`{ model: "sol-6.1", reasoning: "high" }`).
An empty model list defaults to the active session model. Recommendations merge
field-wise across profile overrides.

```yaml
# .forge/acdd/profile.yaml
roles:
  builder:
    models:
      - 'gpt6-luna:xhigh'
  independent_reviewer:
    mode: subagent
    reuse_on_reject: true
    models:
      - 'sol-6.1:high'
    guidance: "Run independent review on a dedicated high-reasoning model."
```

During `task_claim` or `task_manage(inspect)`, Forge provides the resolved
`role`, `mode` (`subagent` or `inline`), and `role_guide` in `workflow_guidance`. `role` specifies the active role
identifier, `mode` specifies whether the role executes as an isolated subagent or inline in the current agent (with role guidance automatically adapted for inline execution), and `role_guide` combines advisory guidance with configured
recommended models (e.g. `Recommended model: gpt6-luna:xhigh or sol-6.1:high.`).
After a `reject_to` rewind, `workflow_guidance.rejection`
contains `rejected_from`, the prior builder `worker_id` when reuse is enabled,
and the persisted `findings`; a clean forward run has `rejection: null`.
When reuse is enabled, guidance adds the targeted step
`Return task to builder '{worker_id}' with review rejection findings to repair defects.`

### Repository gitignore and profile versioning

Because declarative profiles and custom adapters live inside `.forge/` (including `.forge/acdd/profile.yaml`, other `.forge/acdd/**` files, and `.forge/frameworks/**` extensions), repositories must not exclude the entire `.forge/` directory in `.gitignore`. Ignore only Forge runtime state:

```gitignore
!/.forge/
!/.forge/acdd/
!/.forge/acdd/**
!/.forge/frameworks/
!/.forge/frameworks/**
.forge/*.sqlite*
.forge/*.lock
.forge/*.log
.forge/*.jsonl
.forge/tasks/
.forge/checkpoints.json
```

Place this active-rule block after all other project ignore patterns. The five negations restore `.forge/` and every path under the complete profile and framework trees; the final six patterns keep Forge runtime state ignored. Forge requires this exact final active-rule suffix and checks its effective matches during task guidance validation, so earlier wildcard ignores and negations cannot hide nested configuration files or expose runtime paths. Blank lines and comments may follow; another active pattern may not. Forge warns when `.gitignore` is missing, omits a rule, or has another active pattern after this block. A shallow exception such as `!.forge/*.yaml` does not restore nested configuration beneath an ignored `.forge/` directory. The canonical template is `skills/contextunity-forge/references/gitignore.example`.

### Guidance file and skill reference validation

Task operations require repository guidance (configured via `agents_guidance`, defaulting to `AGENTS.md`) and the `contextunity-forge` skill:
- **Skill reference**: The guidance file must explicitly reference the `contextunity-forge` skill (exact token matching or contextual `contextunity-forge-mcp skill` mentions). Match visible Markdown text, including link labels; link destinations do not count.
- **Skill installation**: The skill must be installed either in the repository (`.agents/skills/contextunity-forge/SKILL.md`) or globally (`~/.agents/skills/contextunity-forge/SKILL.md`).
- **Profile tracking**: Keep `.forge/acdd/**` and `.forge/frameworks/**` trackable with the final protection block and six canonical runtime ignore rules above. Keep the block after all other active `.gitignore` rules.

See [MCP tools](mcp-tools.md) for continuation arguments and [limits and freshness](../runbooks/limits-and-freshness.md) for computation budgets.
