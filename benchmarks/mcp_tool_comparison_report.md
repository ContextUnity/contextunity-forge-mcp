# Forge MCP vs Codebase Memory MCP

Date: 2026-09-29

Full measurements: [`mcp_tool_comparison_results.json`](mcp_tool_comparison_results.json)

Benchmark harness: [`mcp_tool_comparison_benchmark.py`](mcp_tool_comparison_benchmark.py)

The quality-only assessment at [`mcp_tool_quality_report.md`](mcp_tool_quality_report.md) supersedes the Q/clarity scores in this mixed performance report.

## Conclusion

- For local coding-agent tasks in this repository, Forge is the stronger primary tool: it has lower latency for most symbol/source/impact/AST/documentation calls, more compact responses, and specialized freshness, unresolved-reference, and removal-safety explanations.
- Codebase Memory MCP complements Forge with a broader graph, architecture views, data-flow/cross-service tracing, and runtime trace ingestion. Its `query_graph` is flexible but requires the agent to write and interpret Cypher.
- In this run, Forge SQLite occupies 8.52 MB and the isolated Codebase cache 13.14 MB. Codebase indexes more edges, but indexed file scope and edge types differ, so this is not a normalized “quality per byte” test.
- Forge provides an explicit unresolved/ambiguous resolver ledger: 11,221 unresolved+ambiguous records, including 11,207 `unresolved` and 14 `ambiguous`; 3,524 resolved, 320 external, and 0 parse errors. Codebase reports 0 skipped, 0 parse-partial, and 0 not-indexed files, but has no compatible workspace-wide unresolved count. Its `CALL_REFERENCE` edges are not equivalent to that count.
- Codebase `index_status` and file coverage are useful for checking index state, but `no_recorded_issue` with `signal=best_effort` does not prove graph completeness.
- Removal safety differs substantially: Forge returned `blocked`, one inbound `CALLS` edge, and 93 target unresolved references. Codebase returned graph rows without a safety verdict or unresolved count.

## Method

- Binaries: `contextunity-forge-mcp` 0.2.0 and `codebase-memory-mcp` 0.10.8, both called directly over MCP stdio. Codebase MCP was disabled in the global Kilo configuration; that configuration was not changed.
- Both servers read the same immutable temporary source snapshot for each scenario, with matching relative paths and target symbols. Forge follows `forge-mcp.yaml` roots; Codebase uses `mode=full` over all copied root files, including manifest/config files. AST and checkpoint scenarios were rerun separately with the same copy policy and five repeats; their source-copy metadata is recorded in `scenario_overrides`. Node/edge counts, workspace overview, and disk/cache sizes therefore do not compare identical indexed corpora.
- The snapshot excludes `.git`, `.forge`, `.codebase-memory`, `target`, `vendor`, `AGENTS.md`, `.agents`, `skills`, and benchmark artifacts. Codebase cache, HOME, XDG, and daemon state are isolated in a temporary directory. Timed runs mutate only the temporary Forge DB and Codebase cache; they do not call mutating Codebase tools or modify source files/global configuration.
- The study covers 17 Forge scenarios across 15 unique tools (three `code_map_analyze` cases), 16 closest Codebase counterparts, and two Codebase-only low-confidence diagnostics. Each scenario/backend has five cold-process and five warm-process measurements. The JSON contains 70 rows, each with five samples: 170 Forge and 180 Codebase tool calls.
- `cold_process`: a new Forge server/SQLite connection or a restarted isolated Codebase daemon/graph connection for every repeat. One fresh index is built and retained across repeats; the Linux page cache is not flushed. `warm_process`: one discarded warm-up followed by five requests in the same MCP stdio client; Forge reuses its server/SQLite connection and Codebase reuses its separate daemon/graph connection behind the stdio proxy. Codebase is restarted between scenarios, so startup cost is amortized across tools in a long-lived session.
- `wall_ms` covers `tools/call` after MCP initialization; cold process startup is reported separately. This is not end-to-end first-agent startup time. Codebase starts a new daemon for cold samples, so startup commonly takes seconds; in a normal long-lived session this cost is amortized.
- CPU is Linux `schedstat` runtime delta across threads in tracked processes/daemons. RSS is the sum of simultaneous `VmRSS` samples every 5 ms. `VmHWM` is the sum of process-lifetime highs, not a simultaneous peak. Short-lived children between samples and children created by non-leader threads may be missed. The Codebase `/proc` daemon scan runs in the sampler, and its small overhead remains in `wall_ms`.
- The local runner has no RPC timeout, does not drain server stderr while running, and does not respond to server-initiated JSON-RPC requests. All observed local calls completed. Do not use this one-off harness against an unstable/remote server without hardening it.
- Tables report medians of five repeats. JSON preserves every sample, content/wire bytes, CPU, RSS, process count, and response hash. The hash includes the JSON-RPC request ID, so different warm hashes do not by themselves indicate different tool results.
- `content bytes` counts MCP text content; `wire bytes` counts stdout bytes consumed through the matching response. Codebase may return both text and `structuredContent`; these are not token counts. `bytes/4` is only a rough token estimate.
- Forge tool schemas were checked against `src/mcp/tools.rs` and [`docs/reference/mcp-tools.md`](../docs/reference/mcp-tools.md). Codebase schemas came from live `tools/list` on the installed 0.10.8 binary. Upstream registry: [`DeusData/codebase-memory-mcp` v0.10.8](https://github.com/DeusData/codebase-memory-mcp/tree/v0.10.8).
- Quality and agent clarity were assessed manually from JSON evidence, not by an LLM judge. Q (quality): 5 is direct, scoped, and sufficient; 3 is useful but approximate or requires interpretation; 1 means no compatible function. Clarity reflects structure, explicit evidence/limits, and the number of manual steps.
- `session_checkpoint` was limited to read-only `action=list` against a temporary snapshot without `.forge/checkpoints.json`; five repeats confirmed `{}`. Response examples were removed from the artifact; byte metrics and hashes remain.
- The first AST query returned zero matches because the pattern omitted the return type. That result was discarded; the final table uses a positive pattern that both tools checked against `Server::admit`.

## Tool Contracts

Shared Forge paging fields: `limit` (default 30, maximum 100), `offset`, `detail=compact|full`, and `generation`. Core inputs and the closest Codebase contract:

| Forge tool | Main Forge inputs | Closest Codebase contract |
|---|---|---|
| `code_map_overview` | Paging fields | `get_architecture(project, path?, aspects?)`, `index_status(project)`, `get_graph_schema(project)` |
| `code_map_search` | Required `pattern`; optional `kind`, `path`, `group_by_file`, `include_docs`, paging | `search_graph(project, query/name_pattern/qn_pattern/file_pattern/label/relationship/fields/limit/offset)` |
| `code_map_inspect` | Required `selector`; `show_doc`, `show_source`, `leading_lines`, `max_body_lines`, `source_offset`, paging | Combination of `search_graph` and `get_code_snippet(project, qualified_name)` |
| `get_code_snippet` | Required `selector`; source window/offset and paging | `get_code_snippet(project, qualified_name, include_neighbors?)` |
| `code_map_explain` | Required `selector`; optional `direction`, docs/source controls, paging | `query_graph(project, query)` or `trace_path(project, function_name, direction, depth)` |
| `code_map_impact` | Required `selector`; `depth`, paging | `trace_path(project, function_name, direction, depth, mode, include_tests, edge_types)` |
| `code_map_tests` | Required `selector`; `direction=inbound|outbound`, paging | `query_graph` over `TESTS`; `trace_path(include_tests?)` |
| `code_map_prove_removal` | Required `selector`, paging | No removal-proof API; only `query_graph`/`search_graph` approximations |
| `code_map_query` | Required `operation`; optional `selector`, `depth`, paging | `query_graph(project, query, graph=code|missed, max_rows)`; a direct analogue exists only for Cypher |
| `code_map_analyze` | Required `target`; optional `include_cycles`, `lint`, paging; bounded read-only SQL | `index_status(project)`, `check_index_coverage(project, paths|scopes)`, `get_architecture(aspects=['cycles'])` |
| `ast_grep_search` | Required `pattern` and `language`; optional `path`, paging | `search_code(project, pattern, regex?, path_filter?, mode)` — text, not AST |
| `search_docs` | Required `query`; optional `doc_type`, `component`, `include_excerpt`, paging | No document-search API; `search_code` workaround |
| `get_doc` | Required `path_or_id`; optional `section`, paging | No generic Markdown reader; `search_code` returns match-centered source windows |
| `session_checkpoint` | Required `action`; optional `name`, `content` | No checkpoint API; `manage_adr` is a separate ADR contract |
| `forge_guide` | Optional `topic`, `force` | No MCP guide; `get_graph_schema(project)` returns only the graph schema |

## Index and Startup

| Metric | Forge | Codebase Memory MCP |
|---|---:|---:|
| On-disk index size | 8,519,680 B / 8.52 MB / 8.12 MiB | 13,139,122 B / 13.14 MB / 12.53 MiB cache |
| Nodes | 1,893 | 1,986 |
| Edges | 3,971 | 9,109 |
| Files as counted by the tool | 109 | 112 `File` nodes in `get_graph_schema` |
| Documentation sections | 49 | 49 `Section` nodes |
| Index build wall time | 1,681 ms (full build command) | 6,352 ms (`index_repository` tool call) |
| Server/daemon startup | 2.8 ms median cold | 5.4 s median cold; separate index-setup startup in this run was 8.24 s |
| Index build CPU | 2,784 ms | 7,334 ms for the tool call |
| Index build sampled peak RSS | 64.6 MiB | 215.0 MiB |
| Codebase coverage | — | `not_indexed=0`, `skipped=0`, `parse_partial=0`, `artifact_present=false` |

Codebase's first isolated index setup takes about 14.59 s: 8.24 s daemon startup plus the 6.35 s `index_repository` call. This excludes configuration commands. Forge's 1.68 s covers the full measured build command.

The largest Codebase edge types in `get_graph_schema` are `USAGE` (2,888), `CALLS` (2,508), `DEFINES` (1,838), and `DEFINES_METHOD` (505); it also includes `SIMILAR_TO` (229), `SEMANTICALLY_RELATED` (223), and other typed relationships. This provides a richer graph, but edge totals do not directly measure correctness: edge taxonomies and indexed file scopes differ.

The Codebase cache is 1.54x larger than Forge SQLite in this run; Forge is about 35% smaller. Codebase counts a cache directory rather than one equivalent SQLite artifact, and `mode=full` covers a broader set of files/edges, so this percentage is only a practical footprint comparison of these benchmark configurations, not pure storage-format overhead.

## Latency and Response Size

Times are `cold/warm ms`; sizes are `warm text/wire bytes`. Codebase cold startup is not included in these values.

| Scenario (Forge / closest Codebase tool) | Forge cold/warm ms | Codebase cold/warm ms | Text bytes F/C | Wire bytes F/C |
|---|---:|---:|---:|---:|
| Workspace overview (`code_map_overview` / `get_architecture`) | 42.49 / 28.66 | 15.22 / 11.61 | 1,333 / 1,531 | 1,605 / 1,690 |
| Symbol search (`code_map_search` / `search_graph`) | 9.83 / 2.70 | 8.34 / 11.69 | 346 / 324 | 480 / 801 |
| Symbol inspect (`code_map_inspect` / `search_graph`) | 8.78 / 2.97 | 11.75 / 5.93 | 3,057 / 324 | 3,549 / 801 |
| Source (`get_code_snippet` / `get_code_snippet`) | 10.40 / 4.42 | 11.54 / 6.37 | 2,403 / 6,266 | 2,631 / 12,860 |
| Tests (`code_map_tests` / `query_graph`) | 10.29 / 3.47 | 166.17 / 33.04 | 1,847 / 482 | 2,137 / 576 |
| Unresolved (`code_map_analyze` / `query_graph`) | 29.53 / 21.61 | 143.95 / 56.90 | 1,723 / 712 | 2,011 / 807 |
| Impact (`code_map_impact` / `trace_path`) | 13.41 / 4.05 | 88.45 / 11.53 | 1,026 / 313 | 1,272 / 767 |
| Symbol explain (`code_map_explain` / `query_graph`) | 14.56 / 8.08 | 144.23 / 19.84 | 4,557 / 5,609 | 5,233 / 5,731 |
| Graph slice (`code_map_query(slice)` / `query_graph`) | 6.45 / 1.71 | 152.56 / 22.32 | 1,682 / 5,609 | 2,012 / 5,731 |
| Workspace diagnostics (`code_map_analyze` / `index_status`) | 38.94 / 26.00 | 9.31 / 8.78 | 2,377 / 382 | 2,731 / 919 |
| Path diagnostics (`code_map_analyze` / `check_index_coverage`) | 9.62 / 5.96 | 10.08 / 9.07 | 4,725 / 787 | 5,434 / 1,755 |
| Removal safety (`code_map_prove_removal` / `query_graph`) | 28.73 / 20.85 | 352.44 / 214.44 | 1,353 / 435 | 1,583 / 531 |
| AST (`ast_grep_search` / `search_code`) | 10.70 / 5.72 | 14.10 / 16.44 | 12,390 / 3,172 | 12,922 / 6,603 |
| Documentation search (`search_docs` / `search_code`) | 7.29 / 2.45 | 19.77 / 21.56 | 901 / 1,739 | 1,086 / 3,666 |
| Documentation read (`get_doc` / `search_code`) | 6.71 / 1.83 | 17.39 / 14.74 | 12,761 / 3,561 | 13,446 / 7,438 |
| Tool guide (`forge_guide` / `get_graph_schema`) | 1.11 / 0.35 | 30.48 / 30.37 | 1,424 / 5,056 | 1,546 / 11,019 |
| Checkpoint list (`session_checkpoint`) | 0.84 / 0.33 | n/a | 2 / n/a | 92 / n/a |

Calls with matching names are semantically closer; other rows are the best available approximations, not interface parity. For example, Codebase `query_graph` is Cypher, while Forge `code_map_query(slice)` is a ready-made bounded operation. Do not interpret their latency ratio as identical work.

## CPU and RSS

The table shows warm medians; cold values and every individual measurement are in JSON.

| Scenario | CPU ms F/C | Sampled peak RSS MiB F/C |
|---|---:|---:|
| Workspace overview | 25.40 / 0.83 | 14.0 / 13.4 |
| Symbol search | 2.64 / 0.80 | 12.9 / 13.4 |
| Symbol inspect | 2.73 / 0.70 | 12.9 / 13.4 |
| Source snippet | 4.31 / 1.09 | 13.7 / 13.4 |
| Tests | 3.36 / 12.08 | 14.3 / 23.0 |
| Unresolved evidence | 21.45 / 25.82 | 14.6 / 23.1 |
| Impact | 3.79 / 2.22 | 12.6 / 13.4 |
| Symbol explain | 5.65 / 4.50 | 12.8 / 13.8 |
| Graph slice | 1.67 / 1.31 | 12.7 / 13.6 |
| Workspace diagnostics | 25.66 / 0.54 | 14.9 / 13.4 |
| Path coverage | 5.68 / 1.49 | 12.6 / 13.4 |
| Removal safety | 20.65 / 194.80 | 14.2 / 61.5 |
| AST search | 5.56 / 0.84 | 15.0 / 13.4 |
| Documentation search | 2.36 / 1.35 | 12.6 / 13.4 |
| Documentation read | 1.68 / 0.99 | 12.0 / 13.4 |
| Tool guide/schema | 0.33 / 16.90 | 7.7 / 16.4 |
| Checkpoint list | 0.31 / n/a | 7.8 / n/a |

Codebase process CPU can be much lower than wall time when a request waits on the graph daemon/IPC, so interpret both metrics together. The `query_graph` removal probe is a clear warm-request hot spot: about 195 ms CPU, 214 ms wall, and 61.5 MiB sampled RSS versus about 21 ms CPU, 21 ms wall, and 14.2 MiB for Forge `code_map_prove_removal`. These are not identical contracts: Codebase returns edges for manual interpretation; Forge returns a risk verdict.

## Tool-by-Tool Assessment

`Q/clarity` are Forge/Codebase scores on the 1–5 scale described above. `n/a` means no counterpart exists.

| Forge tool | Codebase counterpart | Equivalence | Q F/C | Clarity F/C | Agent-facing observation |
|---|---|---|---:|---:|---|
| `code_map_overview` | `get_architecture` (+ `index_status`, `get_graph_schema`) | Similar overviews, different emphasis | 5/4 | 5/4 | Forge returns components/coverage/freshness; Codebase provides architecture aspects and a rich graph schema. Codebase is faster for this overview call. |
| `code_map_search` | `search_graph` | Similar symbol lookup | 5/5 | 5/4 | Both find the requested symbol/file. Forge returns a compact symbol-oriented result; Codebase returns qn/label/edges and BM25/regex/semantic filters. |
| `code_map_inspect` | `search_graph` + `get_code_snippet` | No single counterpart | 5/3 | 5/3 | Forge combines selector resolution, docs, metadata, and references; Codebase `search_graph` is only a search row, so source must be read separately. |
| `get_code_snippet` | `get_code_snippet` | Direct functional analogue | 4/5 | 5/4 | Both find `Server::admit`. Forge returns a bounded preview (~2.4 KB); Codebase returns the full method (~6.3 KB), which is more complete but uses more tokens. |
| `code_map_tests` | `query_graph` over `TESTS` | Graph-based approximation | 4/3 | 5/3 | For `Server::read`, Forge returned four scoped/transitive test entries and 19 unresolved references; Codebase returned two direct `TESTS` edges. Forge is broader but warns about lexical fallback; Codebase is simpler but has fewer witnesses. |
| `code_map_analyze` unresolved | `query_graph` over `CALL_REFERENCE` | Non-equivalent metrics | 5/2 | 5/2 | Forge aggregates statuses/causes/top files. Codebase returns individual call-reference rows; the extra custom low-confidence query finds candidates but is not a canonical unresolved count. |
| `code_map_impact` | `trace_path` | Similar incoming traversal | 4/4 | 5/4 | Both provide dependency paths. `trace_path` has calls/data-flow/cross-service modes; Forge adds paging and relation evidence. Compare witnesses, not only milliseconds/counts. |
| `code_map_explain` | `query_graph` / `trace_path` | Approximate | 5/3 | 5/2 | Forge returns ownership/docs/incoming/outgoing in an explanatory contract; Codebase returns a Cypher table with `type(r)` and requires agent synthesis. |
| `code_map_query` | `query_graph` | Direct only for Cypher | 4/5 | 5/3 | Codebase Cypher is flexible; Forge adds bounded `slice`, `unwired`, and a constrained query model. Complex Codebase graph queries are notably slower in this test. |
| `code_map_analyze` workspace | `index_status` / `get_architecture(aspects=['cycles'])` | Partial | 5/3 | 5/4 | Forge reports unresolved/status/cycles. Codebase status clearly reports skipped/partial/not-indexed inventory, but does not replace graph diagnostics. |
| `code_map_analyze` file | `check_index_coverage` | Partial | 4/4 | 5/5 | Codebase reports `metadata_match`, `no_recorded_issue`, and a best-effort caveat; Forge returns per-file diagnostics/resolution evidence. |
| `code_map_prove_removal` | `query_graph` edge listing | Not equivalent | 5/2 | 5/1 | Forge returns `blocked`, inbound count=1, target unresolved=93, parse_errors=0. Codebase finds raw edges but has no safety conclusion. |
| `ast_grep_search` | `search_code` | Not equivalent: AST vs text | 5/3 | 5/2 | The corrected positive query returns a Forge AST match on lines 158–290. Codebase literal search finds the same method/source but cannot answer structural AST-pattern queries in general. |
| `search_docs` | `search_code` literal | Workaround | 5/3 | 5/3 | Forge finds two README sections with `doc_type`, rank, excerpt, and anchor. Codebase finds the README module/source window but has no doc-type/section ranking. |
| `get_doc` | `search_code` | Not equivalent: section read vs snippet | 5/2 | 5/3 | Forge returns eight README sections (~12.8 KB) with headings. Codebase returns a matching section/window with `source_truncated=true`, not the full document. |
| `forge_guide` | `get_graph_schema` | No functional analogue | 5/2 | 5/2 | Forge explains operation choice, paging, and budget recovery. Codebase schema helps with Cypher but describes labels/edges, not workflow. |
| `session_checkpoint` | — | No analogue | 5/n/a | 5/n/a | Forge has list/get/save/delete checkpoint actions. Only isolated read-only list was compared; mutating actions were not called. |

## Unresolved and Completeness

Forge `code_map_analyze(target='')` reports the following for the benchmark snapshot:

- `total_unresolved=11,221`, including `unresolved=11,207` and `ambiguous=14`; `resolved=3,524`, `external=320`, `total_errors=0`.
- Unresolved causes: `dynamic_callee=3,745` (86 files), `no_lexical_candidate=3,617` (89), `shadowed_binding=3,453` (86), `missing_indexed_import=360` (72), `alias_target_missing=32` (28).
- Files with the most unresolved references: `src/db/writer.rs` 815, `src/engine/linker.rs` 526, `tests/query_context.rs` 451, `tests/tool_evolution.rs` 435, `tests/mcp_freshness.rs` 400.
- Workspace parse errors: 0. Cycle analysis finds 7 cycles / 11 nodes and 2 more omitted cycles.
- Target `src/mcp/server.rs:admit` has 93 target unresolved references; the explicit incoming edge is `Server::read -> Server::admit` at line 300. Forge's `blocked` removal verdict is therefore supported, although the static proof warns about dynamic/external callers.
- `Server::read` test mapping has four candidates, `discovery_method=lexical_fallback`, and 19 unresolved references in its scoped neighborhood. This is evidence of possible misses, not proof that all four are direct runnable tests.

Codebase `index_status` reports `parse_partial=0`, `skipped=0`, and `not_indexed=0`; `check_index_coverage` for `src/mcp/server.rs` reports `no_recorded_issue` and `freshness=metadata_match`. These are coverage/indexing signals, not resolver status. `get_graph_schema` contains only three `CALL_REFERENCE` edges in this snapshot; the query returns those specific graph facts, not a summary of all unresolved calls.

Additional Codebase-only `query_graph` calls over `CALLS` found 383 edges with `confidence < 0.5`, grouped by confidence/strategy. This is an analyst-selected triage threshold, not official unresolved status. `suffix_match` examples include `std::path::Path::new -> BoundedWriter.new` (`confidence=0.04`, 45 candidates), `Vec::new -> QueryBudget.new` (`0.02`, 45), and `PathBuf::new -> BoundedWriter.new` (`0.04`, 45). These look weak/implausible from the source names; agents should validate them before impact/removal claims. Largest confidence groups: `0.04` 118, `0.38` 64, `0.02` 38, `0.18` 33, `0.07` 25. The low-confidence summary call has median 103.97 ms warm / 213.44 ms cold; the candidate query has 74.91 / 199.58 ms.

| Codebase-only query | Cold/warm wall ms | Warm CPU ms | Warm peak RSS MiB | Warm text/wire bytes |
|---|---:|---:|---:|---:|
| Confidence/strategy counts (`confidence < 0.5`) | 213.44 / 103.97 | 97.85 | 33.4 | 609 / 806 |
| Candidate examples (`confidence < 0.5`, limit 30) | 199.58 / 74.91 | 58.93 | 34.1 | 6,814 / 7,076 |

Conclusion: Codebase provides useful candidate/confidence fields for manual unresolved review, but has no equivalent to Forge's resolver ledger or caller-scoped unresolved count. “Codebase has 0 unresolved” would be a false claim.

## Further Directions

- **Compactness without quality loss:** Forge's index is 8.52 MB with 1,893 nodes / 3,971 edges; Codebase's cache is 13.14 MB with 1,986 / 9,109. Codebase has about 2.29x more edges but a broader file scope and additional edge types. This benchmark does not prove Forge tables/indexes can be removed without loss. A sound next experiment uses the same explicit file manifest, ablates individual derived tables/indexes, and replays all 17 task fixtures while checking recall/witnesses, cold/warm latency, and database bytes.
- **Sources of Codebase quality:** the graph includes `CALLS`, `USAGE`, `TESTS`, `WRITES`, `OVERRIDE`, `IMPLEMENTS`, `SIMILAR_TO`, `SEMANTICALLY_RELATED`, coverage/complexity properties, and other structures. This enables rich architecture/data-flow/Cypher queries; zero parse-partial does not prove complete semantic links.
- **Forge strengths to retain:** inexpensive agent-ready verdicts, bounded symbol/source operations, unresolved-cause ledger, test lexical fallback with caveats, AST, documentation section APIs, removal proof, and operational tool guide.
- **Codebase strengths to adopt:** cross-service/data-flow/risk-labelled `trace_path`, architecture aspects, richer semantic/usage edges, runtime trace ingestion, and project index coverage.
- **Measurement stability:** warm Codebase query latency is higher than Forge for most comparable narrow calls, while cold samples show hundreds of milliseconds for graph-path/query tasks and several seconds of startup. Optimize cold startup separately for a persistent daemon; do not mix it with warm tool latency.

## Codebase-Only Inventory

Codebase MCP advertises 15 tools. `list_projects`, `delete_project`, `detect_changes`, `manage_adr`, and `ingest_traces` are outside the paired query matrix. `index_repository` ran once for index setup, not five times as a query tool. `delete_project`, `manage_adr(update)`, and `ingest_traces` mutate state and were intentionally not called. `detect_changes` and project-management tools have no direct Forge tool with a matching contract.

Forge also has 15 tools; all were checked: `code_map_overview`, `code_map_search`, `code_map_inspect`, `get_code_snippet`, `code_map_explain`, `code_map_impact`, `code_map_tests`, `code_map_prove_removal`, `code_map_query`, `code_map_analyze`, `ast_grep_search`, `search_docs`, `get_doc`, `session_checkpoint`, and `forge_guide`.

## Reproduction

```bash
python benchmarks/run_benchmarks.py \
  --profile benchmarks/profiles/<profile>.json \
  --workspace <workspace-root> \
  --repeats 5
```

The runner creates temporary indexes and an isolated Codebase profile under `/tmp/kilo`; it does not use the user's `.forge` or Codebase caches. JSON results preserve five raw repeats per scenario, response examples (except checkpoint content), hashes, tool arguments, and resource counters.
