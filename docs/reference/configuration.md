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

`roots` and `docs` are literal, relative directories or paths inside the workspace. `milestones` and `plans` default to `docs/milestones` and `docs/plans` when omitted. Their `*` segments match one directory component (for example, `extensions/*/docs/milestones`). Files below these directories are excluded from the code and documentation search index. Milestone commands read contracts from the configured milestone directories. `ignore` matches file or directory basenames. The scanner admits compiled source extensions and Markdown/MDX within these roots; scan roots do not define import or package roots. Unavailable linked workspaces are skipped. A linked entry with `enabled: false` is excluded; omission of `enabled` means true.

The default basename exclusions cover `.git`, `.forge`, `target`, `node_modules`, `.venv`, `__pycache__`, `build`, `dist`, `coverage`, `.cache`, `.next`, `.nuxt`, `.svelte-kit`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`, and `.gradle`. The scanner also honors `.gitignore` and skips symbolic links. Add project-specific generated directories with `ignore`; authored source under an excluded basename can be scoped explicitly with a different directory name.

The index records linked paths under their configured workspace names. Changing source scope or linked-workspace settings causes the MCP server to rebuild or update its owned index on the next database read. CLI readers use the index already on disk, so run `build` or `delta` after source changes when using only the CLI.

## Repository documentation admission

This repository lists current documentation files and directories explicitly in
`docs`: governance, navigation, roadmap, ADRs, architecture, reference,
runbooks, and testing. Milestones and plans are declared separately in `milestones`
and `plans`, keeping them out of the general code search index.

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

Set root `agents_guidance` to a path inside the root repository. It defaults to
`AGENTS.md`. Claim and inspect include that path and stage-specific
`workflow_guidance`. If `docs/reference/acdd.md` is missing, they include inline
steps, a `TASK_GUIDANCE_MISSING` warning, and the canonical URL of that page.
A missing `agents_guidance` file does not raise that warning.

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

`page_size` accepts 1–100; `max_output_bytes` accepts 1,024–65,536. Source previews accept 0–20 leading lines and 1–100 body lines. `detail` is `compact` or `full`. These settings control presentation and do not change extracted facts or require an index rebuild. Unknown response fields and out-of-range values are rejected.

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
- **Delivery auto_commit, baseline HEAD consistency, and landed commit rewriting**: When `auto_commit: true` (default in profile), delivery automatically creates a Git commit from the candidate snapshot tree of the nearest predecessor gate with `sha_snapshot: true` (which is `build` in the default profile, whose candidate tree SHA is verified across all `review_sources`) plus the milestone receipt, with parent `HEAD`. Forge verifies that `git rev-parse HEAD` matches `candidate_baseline_head` (the exact commit SHA of HEAD captured when the candidate snapshot was created). If HEAD has diverged (i.e. unreviewed commits exist on the branch), delivery fails closed with `TASK_DELIVERY_COMMIT_FAILED`. Hooks are enforced. If `git commit` fails, the SQLite transaction rolls back atomically, leaving the task uncompleted at `deliver` with an explicit error, allowing the deliverer to reject the task with findings back to `reject_to` (e.g. `build`) or retry. The created delivery commit SHA is saved in the task's delivery receipt in SQLite. When tasks are subsequently merged into the milestone branch, `milestone handoff` resolves the landed commit for each task in the milestone branch (the commit that landed the task's work into the milestone branch, whether via fast-forward or merge commit), rewrites `receipt.commit` in each task block and SQLite with this landed SHA, records `handoff.commit` in frontmatter, and archives the document. The subsequent milestone archive commit occurs after handoff and is excluded from `receipt.commit`, avoiding any hash cycle.

### Role and model configuration

Agent execution and model guidance are configured per **role**, not per task.
`mode` accepts `subagent` or `inline` and effectively defaults to `subagent`.
`reuse_on_reject` effectively defaults to `true`; when a review rewinds to a
builder stage, this retains the prior builder's worker ID in rejection guidance.
`models` is an ordered list of `ModelSpec` values, with `models[0]` as the
primary recommendation. `ModelSpec` owns `model` and optional `reasoning`.
An empty model list defaults to the active session model. Recommendations merge
field-wise across profile overrides.

```yaml
# .forge/acdd/profile.yaml
roles:
  independent_reviewer:
    mode: subagent
    reuse_on_reject: true
    models:
      - model: "sol-6.1"
        reasoning: "high"
    recommendation: "Run independent review on a dedicated high-reasoning model."
```

During `task_claim` or `task_manage(inspect)`, Forge provides the resolved
`subagent_role` and `role_spec` in `workflow_guidance`. `role_spec` contains
effective `mode` and `reuse_on_reject`, the ordered `models`, and the role
recommendation. After a `reject_to` rewind, `workflow_guidance.rejection`
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
