# Forge MCP and CLI tool catalog

Use the active server registry and schemas as the final authority. This catalog is a compact navigation aid; repository-specific behavior and current schemas belong in that repository's reference docs.

## Task lifecycle

- `task_list(repository?, milestone_ref?, milestone_status?, status?, stage?, detail?)` lists tasks; defaults to ready tasks in active milestones. `milestone` aliases `milestone_ref`; `parked` selects work waiting for a scope owner, and `cancelled` selects retained cancelled tasks.
- `task_claim(task_id, stage, worker_id, worktree, bundle?)` claims the current gate and returns its guidance/context. `task_submit(task_id, stage, action, evidence, findings?)` submits claim-bound proof or review findings.
- `task_manage` supports `create`, `sync`, `inspect`, `context`, `next`, `delete`, `extend_scope`, `subtask_add`, `subtask_update`, `subtask_list`, `reset`, and `reopen`. `task next --worker WORKER --worktree PATH [--workspace WS]` and `task_manage(action="next", ...)` atomically dispatch: independent delivery, independent review, the caller's unblocked parked work, then ordinary ready work by wave and task ID. Results are claim context or `status: "idle"`.
- `task show TASK_ID` aliases `task inspect TASK_ID`. Inspect and submit accept `--task TASK_ID`; sync accepts `--file PATH`. Use `task list --detail compact|full`; `--full` is supported but cannot be combined with `--detail`.
- CLI-only milestone operations are `milestone init`, `list`, `show`, `handoff-context`, and `handoff`.
- `task_blackboard` posts, reads, or inspects `draft`, `notes`, `findings`, `blockers`, `decisions`, and `deferred` at milestone, task, or subtask scope. `gate` filters or targets posts/reads. `decisions` and `deferred` are copied with topic labels to delivery `architectural_notes`.

## Discovery and search

Code discovery and inspection tools accept `worktree`: an absolute or server-relative path, exact directory name, or unique hyphen-delimited name suffix. Without it, retain the server's linked worktree; otherwise select one unique active claimed worktree when configuration is loaded or `forge-mcp.yaml` exists (an empty `{}` suffices). Without configuration or an active claim, use the primary root. Multiple active directories or name matches return `WORKTREE_AMBIGUOUS`; pass an explicit path/name. Use `worktree: "."` for the server root. MCP requests do not transmit the client's current directory.

- `code_map_overview(aspects?, worktree?, limit?, offset?, detail?, generation?)` returns workspace root, components, counts, language coverage, profiles, and persisted primary/linked `coverage_by_workspace`; cycles are opt-in.
- `code_map_search(pattern, kind?, path?, exact?, include_docs?, include_preview?, group_by_file?, worktree?, detail?, limit?, offset?, generation?)` returns eligible exact name/qualname matches alone when present by default, otherwise ranked FTS/prefix results. `query` aliases `pattern`; `exact=true` requires an indexed exact name/qualname; `pattern="*"` needs a path or kind scope. Empty `path` is unfiltered. Results include signatures and `inspect_selector`; previews cover at most three hits, automatically for up to three total hits or on request for larger sets.
- `ast_grep_search(pattern, language?, path?, worktree?, detail?, limit?, offset?, generation?)` uses Tree-sitter `$NAME` and `$$$ARGS` captures. Omit `language` only with a recognized path extension; otherwise use a compiled profile ID from `code_map_overview`. CLI: `contextunity-forge-mcp ast grep --lang rust --path file.rs 'PATTERN'`; CLI `--lang` is required.
- `search_docs(query, doc_type?, component?, worktree?, limit?)` searches indexed docs, guides, and ADRs. `get_doc(path_or_id, section?, worktree?)` reads a Markdown document or exact heading anchor.

## Inspection and hierarchy

- `code_map_inspect(selector, worktree?, include_coverage?, show_doc?, show_source?, leading_lines?, max_body_lines?)` returns symbol summary, receiver metadata, exact callers/callees, verified `documented_in`, compact governing invariants, paged `analysis_gaps`, and candidate dispatch targets (`possible_calls`).
- `get_code_snippet(selector, worktree?, leading_lines?, max_body_lines?, source_offset?, generation?)` returns a bounded, digest-verified preview. Inspect/snippet defaults are 5 leading and 200 body lines; body requests clamp to 1..500 within byte caps. Continue with `next_source_offset` and the same generation.
- `code_map_explain(selector, direction?, worktree?, include_coverage?, show_doc?, show_source?)` returns endpoint IDs, query-relative direction, ownership, invariants, and candidate dispatch; full detail includes stored edge evidence.
- `code_map_impact(selector?, changes?, direction?, depth?, worktree?, detail?, limit?, offset?, generation?)` traces inbound dependencies (default; exact `calls` and candidate `possible_calls`) or outbound dependencies, or accepts exactly one Git `changes` mode: `staged`, `unstaged`, or `head`. Depth-one non-seed nodes include selected edge endpoints, direction, location, and confidence. Change continuations bind graph and captured snapshot; inspect `unmapped_changes` and `complete`. A range maps only when its selected postimage matches the indexed file.
- `code_map_tests(selector, direction?, detail?, limit?, offset?, generation?)` follows exact call paths from `test_case` roots or from tests into production. Helpers/fixtures may be path nodes, not roots. Bounded inbound lexical suggestions lack `call_path` and do not prove coverage.

## Verification and utility

- `code_map_prove_removal(selector)` considers static references and blocks removal for candidate callers (`possible_calls`).
- `code_map_analyze(target, lint?, include_cycles?)`: empty target means workspace diagnostics; a path means scoped diagnostics with digest-verified snippets/reasons where known. Exact Markdown/MDX targets can include paged `doc_diagnostics` (`broken_doc_link`, `dead_doc_symbol`, `stale_doc_sections`); suggestions are not verified links. It also reports paged declared-public symbols unreachable from indexed test cases by exact calls. `lint=true` reads stored syntax diagnostics; `include_cycles=true` detects cycles.
- `code_map_query(operation, selector?, include_coverage?, depth?, limit?, offset?, generation?)` returns graph slices, unwired nodes, or read-only SQL with `operation="sql"`.
- `session_checkpoint(action, name?, content?)` manages local JSON in `.forge/checkpoints.json`; `forge_guide(topic?, force?)` provides `acdd`, `query`, `adapter`, `docs`, or `ast` help.
- Selector forms include canonical node IDs; paths (`src/scanner.rs`, also stripping `file://`, `file:`, `./`); path+symbol (`src/scanner.rs:scan`); and path+line (`src/scanner.rs:42`, `src/scanner.rs#L42`, innermost node). Ambiguous bare names return candidates and never hijack to modules. `.md` selectors route to `get_doc` or `search_docs`.

## Documentation and invariant graph

Admitted Markdown/MDX, including milestones and plans, is indexed with section locations and frontmatter status. Confined relative links/wikilinks create `cites_doc`/`cited_by_doc` only when one section resolves. Documentary code claims are eligible in the source document's workspace unless an explicit normalized file path confines them elsewhere. Document, invariant, and component nodes do not verify claims. In each eligible claim occurrence, the first non-empty tier wins: exact normalized qualified name (`::` and `.` equivalent), exact declared name, then qualified-name segment suffix. Only one target in that tier creates a verified `referenced_symbols` relation; lower tiers are not combined.

Full `get_doc` pages ambiguous winning-tier `referenced_symbol_candidates`, each with source claim (`doc_id`, path, name, line) and target identity (`id`, name, qualname, kind, path, line). Candidates are navigation only: they create no `documents`, `references_doc`, `governs`, or `governed_by` edges and do not establish verified coverage. Compact detail omits candidate data and returns a full-detail hint; MCP and CLI `docs get` default to full. `code_map_inspect` exposes `documented_in`; impact exposes `affected_docs`, including a retained source-file claim after declaration deletion.

`INV-*` declarations become invariant nodes; only a verified unique symbol reference creates `governs`. Inspect, impact, and task context show at most five direct rules and five one-hop IDs with source locations and omitted counts. Suggestions, ambiguous candidates, and token similarity do not prove governance. This projection supplements, never replaces, task invariants, profile guidance, decisions, commands, or proof receipts.
