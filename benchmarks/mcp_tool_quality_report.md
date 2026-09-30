# Forge MCP vs Codebase MCP: Response Quality

Date: 2026-09-30

Raw responses: [`mcp_tool_quality_results.json`](mcp_tool_quality_results.json)

Response collector: [`mcp_tool_quality_benchmark.py`](mcp_tool_quality_benchmark.py)

## Conclusion

Forge generally produces more agent-ready answers for specialized operations: diagnostics, removal assessment, AST search, and documentation. Codebase returns precise symbol identities, source, and simple TESTS/impact witnesses; for other graph tasks it often returns rows the agent must interpret.

The earlier quality assessment needs correction. It scored the symbol-search scenario as successful for both tools, although Forge returned zero results. The repeat used the documented `admit*` prefix form; Forge again returned no results, while Codebase found the exact `Server.admit` method. The earlier report also overstated Codebase's documentation reading: `search_code("Native code graph")` returned `src/cli/mod.rs` and `src/mcp/tools.rs`, not the README.

## Method

- Binary versions: ContextUnity Forge MCP 0.2.0 and Codebase Memory MCP 0.10.8. Both servers were called directly over MCP stdio using isolated temporary indexes.
- Source: the current working tree at the time of the repeat, copied into a temporary corpus. Benchmark scripts, results, and reports were excluded from the snapshot. Existing changes under `src/` and `tests/` were preserved and not edited.
- Coverage: 17 Forge scenarios (15 tools; `code_map_analyze` used in three scenarios), 16 closest Codebase counterparts, and two additional Codebase low-confidence diagnostics. Three responses were captured per scenario/backend: 105 responses total, all without MCP `isError`.
- Repeats check response stability, not performance. The `elapsed_ms`, `duration_ms`, `latency_ms`, `wall_ms`, `cpu_ms`, and `rss_kib` fields were removed. Time, throughput, CPU, RSS, and response size were not evaluated or included in the results.
- The indexes are not fully equivalent: Forge follows `forge-mcp.yaml`, while Codebase uses `mode=full` on the copied root files. The comparison therefore evaluates answers to matching tasks, not completeness over identical indexed corpora.
- Quality is manually assessed for relevance/correctness, completeness, evidence, and agent usability. A score of 5 means direct, complete, evidence-grounded, and easy to use; 3 means a useful partial answer requiring manual interpretation or a significant caveat; 1 means no answer or an irrelevant result. These are analytical ratings of observed responses, not formal correctness proofs.

## Quality Matrix

Scores are Forge/Codebase. `—` means no counterpart exists. For approximate counterparts, the score applies to the stated scenario, not to overall MCP capability.

| Scenario | F/C | Response evidence and assessment |
|---|---:|---|
| Workspace overview | 5/4 | Forge returns counts, components, freshness, and a resolver summary. Codebase provides a more detailed catalog of labels, edges, and packages, but not the same focused status/evidence. |
| Find `Server::admit` | 1/5 | Forge `code_map_search(pattern="admit*")` returns `total=0`; Codebase `search_graph` finds one Method with file, line, signature, and in/out counts. Forge inspect and AST selectors still resolve the symbol, so this finding is specific to this search query, not all search forms. |
| Inspect a symbol | 3/3 | Forge confirms node ID/path/lines, but the first page contains 30 of 88 reference/coverage expressions. Codebase returns one row with name, signature, location, and in/out counts; docs are absent. Neither answer provides all requested fields in one result. |
| Read `Server::admit` | 5/5 | Both return the exact method from `src/mcp/server.rs` with the correct boundaries and implementation. This is a strong direct equivalent. |
| Find tests for `Server::read` | 4/3 | Forge returns four scoped/transitive candidates and labels the relation instead of presenting them as direct calls. Codebase returns two direct `TESTS` edges. Forge offers broader discovery; Codebase offers narrower graph witnesses. Neither proves runnable test coverage. |
| Workspace unresolved evidence | 5/2 | Forge returns workspace unresolved/ambiguous counts, cause classifications, and top files. The Codebase query returns only three `CALL_REFERENCE` rows; these are not an unresolved ledger, and the result does not explain that limitation. |
| Codebase low-confidence summary | —/4 | Returns 21 confidence/strategy buckets and counts. Useful for triage, but the `<0.5` threshold is analyst-selected, not canonical unresolved status. |
| Codebase low-confidence examples | —/2 | Shows confidence, strategy, and candidate sets, but includes doubtful matches such as `std::path::Path::new → BoundedWriter.new` with confidence `0.03` and 48 candidates. Caller claims require manual validation. |
| Incoming impact for `admit` | 4/4 | Both find `Server::read`. Forge also returns the target/context implementation; Codebase gives one caller row. Node/caller counts are not fully equivalent. |
| Explain ownership/relationships | 2/3 | Forge `code_map_explain` returns the node and a page of reference coverage, but not the expected concise ownership/incoming/outgoing explanation. Codebase returns up to 30 raw Cypher edges without synthesis: it provides evidence, not a conclusion. |
| Graph slice around `admit` | 4/3 | Forge returns a bounded 10-node slice around the selected symbol. Codebase returns 30 raw rows, including suspicious `USAGE` edges such as `Server.admit → SearchSymbols.path` and `Server.admit → Checkpoints.entries`. |
| Workspace diagnostics and cycles | 5/3 | Forge directly reports resolver/error diagnostics and cycles. Codebase `index_status` clearly reports `ready`, 0 skipped/partial/not-indexed, but that is index coverage, not equivalent graph diagnostics. |
| Coverage for one source file | 4/4 | Forge returns scoped diagnostics with 152 unresolved references and 0 parse errors; 152 unresolved references do not necessarily mean 152 compile errors. Codebase returns `metadata_match`/`no_recorded_issue` with a clear `best_effort` caveat: no recorded issue does not prove completeness. The answers cover different meanings of “coverage.” |
| Can `admit` be safely removed? | 5/2 | Forge returns `blocked`, one incoming dependency, 167 target unresolved references, and the proof-scope limitation. Codebase finds raw edges but provides no verdict or unresolved-reference analysis. |
| AST function search | 5/3 | Forge returns one Tree-sitter match with the correct path, lines, and text. Codebase text search finds the same method/source, but does not prove the AST structure requested. |
| Search documentation about paging | 5/3 | Forge finds two README sections with title/type/rank/excerpt. Codebase finds a README window with a literal match, but no doc-section or ranking contract. |
| Read README/workflow | 5/1 | Forge `get_doc(README.md)` returns Markdown sections. Codebase `search_code("Native code graph")` returns `src/cli/mod.rs` and `src/mcp/tools.rs`, not documentation; nearby mentions do not answer a README-reading request. After removing embedded timing fields, all three Codebase results are identical. |
| Guidance on choosing MCP tools | 5/2 | Forge guide explains selectors, tool choice, paging, and recovery. Codebase `get_graph_schema` describes labels/properties/edges, but does not provide workflow or tool-selection guidance. |
| List session checkpoints | 5/— | Forge returns the correct empty `{}` for an isolated snapshot with no checkpoint state. Codebase has no corresponding tool. |

## Practical Findings

- **Agent-ready answers:** Forge is stronger where tools include a conclusion: diagnostics, removal, AST, documentation read/search, and tool guidance. Its limitations are stated alongside results: `prove_removal` limits its proof to static indexed references, and test relationships distinguish direct from scoped/transitive candidates.
- **Raw graph data:** Codebase is useful for explicit graph-row queries: symbol identity, source snippets, direct test/impact edges, and graph schema. For explain/removal/unresolved tasks, the agent must synthesize a conclusion. Low-confidence suffix-match edges increase the risk of false claims.
- **Symbol search:** Forge binary/selector resolution found `admit` through inspect, snippet, and AST scenarios, but dedicated `code_map_search` found nothing with either `admit` or the documented `admit*` prefix. This is a reproducible mismatch between the `forge_guide` expectation and the search response for this corpus.
- **Documentation:** Forge section APIs separate “find a section” from “read it.” In the measured Codebase substitutes, literal `search_code` can return a README snippet for search, but the documentation-read request returned source modules; it should not be treated as a generic Markdown reader.
- **Response stability:** All 35 scenario/backend groups returned identical results across three repeats after embedded timing fields were removed. The three Codebase `search_code` scenarios included `elapsed_ms` in MCP text; raw differences were limited to this performance metadata, which is excluded from quality results.

## Limits

- Quality was assessed on one Rust repository snapshot and these specific queries. Findings do not automatically generalize to other languages, repositories, queries, or Codebase `search_code` options.
- Scenarios marked approximate/partial/not equivalent compare the closest available responses, not functionally identical APIs.
- Codebase graph queries may be useful with a different Cypher query. Low ratings apply to the tested response, not to the product's maximum capability.
- This is a manual, evidence-based assessment. Repeatability and source lines were checked, but each graph edge was not validated against a separate ground-truth test oracle.
