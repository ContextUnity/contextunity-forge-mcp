# Repository onboarding to Forge and ACDD

Use this guide when the user asks to set up a repository for Forge navigation or
ACDD delivery. It works with agents that can read and edit repository files and
use the installed Forge MCP or CLI, including Antigravity, Codex, Claude, and
Cursor. Onboarding is agent work over companion assets; it requires no Rust
compilation, Rust CLI scaffolder, or embedded templates.

## Establish the requested result

Read the complete request, repository instructions, existing configuration,
architecture, and accepted decisions before proposing changes. Distinguish
navigation setup, ACDD setup, and delivery planning: complete the requested
parts without assuming permission to execute proposed milestones. Preserve
tailored files and unrelated changes; merge relevant settings instead of
replacing files wholesale. Resolve a conflicting architectural decision with
its owner before committing to that decision.

Installation and MCP client registration are separate from repository
onboarding. Use the already installed tools. If unavailable, report the missing
prerequisite and continue preparing repository files where possible; do not
install tools, change client configuration, publish, or write to agent home
directories merely because this guide is loaded.

## Discover the repository

Start with `code_map_overview()` when Forge is available for the target checkout.
Select its `worktree` explicitly when it differs from the server root. Check
coverage before treating an absent node as absent source. Use indexed symbol,
AST, and documentation queries for subsequent discovery. Until Forge is
available, use local file inventory and manifest reads, then return to Forge
after configuration and indexing.

Record only facts needed for setup:

- Repository root, Git worktrees, source/test/generated roots, languages, and
  package/workspace manifests. Inspect actual package scripts, Makefiles,
  build configuration, CI commands, and test targets.
- Existing `AGENTS.md`, verification rules, `forge-mcp.yaml`, selected ACDD
  profile, documentation, plans, milestones, and ignore rules.
- Sibling repositories explicitly relevant to the request or confirmed by
  dependency manifests. Verify their paths, purpose, roots, and ownership;
  adjacency alone does not establish a dependency.

For example, a useful discovery request is: “Onboard this repository using its
existing test and lint commands; include the confirmed sibling API repository
for navigation, preserve its independent task lifecycle, and propose one first
milestone.” Translate that request into observed paths and commands before
writing configuration. Ask only for consequential facts the repository and
conversation cannot establish.

## Configure the adapter

Read [adapter defaults](adapters.md) and adapt the
[canonical adapter example](forge-mcp.yaml.example) into `forge-mcp.yaml`.
`contextunity-forge-mcp guide init` can create a starter when no adapter exists;
do not use `--force` over a tailored adapter.

- Set `roots` to real repository-relative source roots, or `.` when appropriate.
  Include all relevant workspace source trees, such as both `src` and `crates`
  in a multi-crate Rust workspace. Keep generated outputs out of the graph.
- Set `ignore` from actual build/cache directory basenames, respecting existing
  `.gitignore`; do not use invented glob semantics for `ignore`. Use `sensitive`
  patterns for confidential paths that readers must block.
- Point `docs`, `milestones`, `plans`, and `agents_guidance` to maintained paths.
  Set stable `task_repository` and `task_project` identities for this project,
  rather than carrying the example's `forge-mcp` identity into another project.
- Retain the default relative `tasks_db` unless the repository requires another
  location. Git-linked worktrees share the primary worktree's relative task
  database. Independent repositories keep independent stores unless sharing
  is an explicit architectural choice.
- Add only verified `linked_workspaces`: unique `name`, existing relative
  sibling `path`, `enabled`, and its real `roots` (plus relevant docs/ignores).
  Adapt the commented sibling entry in the canonical example rather than
  copying its fictitious path. Linked navigation does not itself authorize
  sibling edits, a shared task store, or cross-repository delivery. Enable its
  `tasks` settings only when linked task discovery is requested and supported
  by the live schema; use that repository's milestone and guidance paths.

Check the installed `guide adapter` output and selected configuration when a
field's semantics are uncertain. Keep configuration inside the consuming
repository. Do not add scanners or template-rendering code to Forge.

## Synthesize a project profile

For ACDD setup, adapt the [canonical v2 profile](acdd_profile.yaml.example) to
`.forge/acdd/profile.yaml` and select it through the adapter's `acdd_profile`.
Use the [domain examples catalog](acdd-profile-examples/README.md) only for a
relevant domain; examples are starting points, and the installed schema is the
authority. Do not copy Forge's Rust-specific guidance, models, protected files,
or commands into an unrelated project.

Bind registered `commands` to verified local tooling. For each command, establish
its working directory, prerequisites, failure exit behavior, timeout, and actual
output. Configure `output` counts/test matching when the proof policy requires
them; a successful process with no recognized tests is not test proof. Use
`scope_args_by_pattern` only with arguments the real command accepts, and keep
full repository verification distinct from focused task checks. For example,
derive a JavaScript check from that package's script and lockfile rather than
guessing `npm test`, and preserve its existing package manager.

Keep `version: 2`, module requirements, roles, reusable guidance, proofs,
commands, and gate references consistent. Adapt the canonical upfront
`contract_synthesis` and nested `tasks` structure to the admitted workflow;
resolve every referenced command, role, proof, guidance key, and failure target.
Preserve independent review and the chosen persistence/immutable candidate
boundaries. Optional role runners must match tools actually available to the
project; Forge does not create a runner merely because instructions mention one.

Validate the profile before claims. Follow the resulting claim's
`workflow_guidance`, expected evidence, and proof policy for actual execution;
do not prescribe a universal gate list or hardcoded model in `AGENTS.md`.

## Merge the agent router

Adapt the [AGENTS router example](agents.md.example) into the repository's
existing root `AGENTS.md`. Preserve local instructions and authorization rules.
Replace example document routes with real paths; omit Forge repository naming
taxonomies and delivery conventions unless adopted by this project.

Require the shared `contextunity-forge` skill and Forge MCP/CLI navigation when
available, including overview/coverage, indexed symbol and AST search, direct
`forge_read(selector="KnownSymbol")`, task lifecycle tools, and Blackboard
context. State the local fallback when Forge is unavailable. Route verification,
test placement, architecture, and documentation to their owning files. Record
only established project invariants and Git permissions; setup does not grant
permission to commit on protected branches or publish changes.

## Bootstrap documentation and propose delivery

Use the [documentation layout](documentation-layout.md),
[docs router example](docs-readme.md.example), and
[documentation rules example](docs-agents.md.example). Reuse existing pages;
create directories only when they contain needed material. Document observed
topology in `docs/architecture/`, accepted decisions in `docs/adr/`, and current
interfaces and operations in their owner pages. Do not turn an unapproved idea
into an accepted ADR. State invariants with stable `INV-*` identifiers and a
precise uniquely resolvable code claim when a verified governance edge is
intended; otherwise distinguish a proposed constraint from implemented behavior.
Resolve all copied links from their destination files.

Propose a bounded first milestone from the user's actual goal. Give it an
outcome, acceptance criteria, owner paths, invariants, and independently scoped
tasks. For each task specify `task_ref`, target, proof policy, contract revision,
write scope including affected tests/docs, real dependencies, and its production
seam proof. Use the
[manifest example](acdd-profile-examples/milestone-manifest-with-skills.md.example)
for shape, removing unrelated skills, repositories, dependencies, and domains.
Check required skills are actually installed before admitting execution.

Keep speculative sequencing in `docs/plans/` as proposals, following the live
plan schema. Leave the milestone unstarted when execution was not requested.
When admission is authorized, use the installed milestone lifecycle tools and
their validation rather than editing SQLite or manufacturing status/receipts.
Independent tasks need no dependency edge; order tasks only for real contracts
or overlapping writes. Cross-repository participation requires explicit owner
scope and integration proof, beyond merely linking their code graphs.

## Validate and hand back

Run installed commands from the consuming repository root:

```sh
contextunity-forge-mcp validate .forge/acdd/profile.yaml
contextunity-forge-mcp guide validate
```

Run the first command only when a profile was created or changed. It validates
profiles and milestone manifests; it is not an adapter validator. `guide
validate` scans with the selected adapter. Validate each proposed milestone with
`contextunity-forge-mcp validate <repository-relative-manifest.md>` and inspect
diagnostics rather than accepting example YAML on appearance alone. Use the
installed command help for options when versions differ.

After indexing, inspect `code_map_overview` for the correct root, roots/coverage,
languages, and linked workspaces. Check a representative source symbol and
documentation page. Review the diff for preserved settings, valid links, and
scope; run `git diff --check`. Adapt the [ignore example](gitignore.example) if
runtime state needs exclusion while profiles remain tracked.

Report changed files, observed verification, remaining prerequisites, and the
proposed next delivery step. Schema and link checks establish artifact validity;
they do not prove that configured project commands pass or that an agent made
correct domain decisions. Run meaningful project checks when authorized and
feasible, and state precisely what remains unverified.
