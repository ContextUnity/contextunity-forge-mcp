# MCP context protection

## Request and response flow

MCP tool arguments resolve against the adapter's typed response policy. Database
queries count matching rows, select a bounded page and omit heavy fields in
compact mode. Source requests verify indexed bytes before constructing a
preview. The response serializer measures the escaped tool result and prunes
page tails when needed. A final stdio writer enforces the protocol ceiling.

The hard maximum is **65,536 bytes per MCP response**, including JSON escaping
and the protocol envelope. The adapter can lower the tool budget. A byte limit
is not a token-count guarantee; encoding and the client's accumulated context
also affect context use.

## Pages and continuation

`forge_guide` with `topic: "query"` recommends the narrowest relevant tool,
pagination arguments, and a recovery path for computation or byte limits.
For symbol search, FTS terms and `prefix*` use the search index; a leading
wildcard can scan the full node table before returning a page.

List arguments use `limit` (default 30, range 1–100), `offset` (default 0),
`detail` (`compact` or `full`) and `generation`. Collections retain their names,
such as `nodes`, but contain a page object:

```json
{
  "nodes": {
    "total": 112,
    "offset": 0,
    "limit": 30,
    "has_more": true,
    "next_offset": 30,
    "items": [],
    "generation": "<indexed-output-hash>",
    "continuation_hint": "Repeat the same query with next_offset as offset and this generation."
  }
}
```

The example omits the item bodies. Actual `next_offset` advances by the number
of emitted items. If byte pruning removes items, the next request starts after
the last item actually returned. It does not skip the removed tail. Counts
describe the complete matching collection when computation succeeds.

Pass the returned `generation` on subsequent pages. An index change rejects
continuation and requests a restart at offset zero. This prevents silent skips
or duplicates caused by paging across different snapshots. A computation limit
or an item that cannot fit produces explicit bounded evidence or an actionable
error; it does not invent an exact total or a zero-progress continuation.

AST search has a 10,000-match horizon and a computation budget. Reaching a
computation boundary requires narrowing the path or pattern. The response
marks incomplete computation and does not advertise an unusable next offset.
Offsets below the horizon remain valid; the last page's limit is clipped to
the remaining range, including when byte pruning returns an earlier offset.

Each file shares a budget of one million traversal/matching steps and two
seconds for parsing and matching. Recursive wildcard branches consume the
same budget. A five-second query budget begins after inventory construction.
These cooperative limits do not time out filesystem operations. Ordinary CLI
structural matching retains its existing behavior without this MCP budget.

Inspection can contain several independent pages, for example coverage and
documents. Use each collection's own continuation fields with the same selector
and filters. Presentation limits do not determine removal safety: that decision
uses complete underlying dependency and diagnostic counts.

Checkpoint values are saved snapshots. Their contents are returned intact when
they fit the response budget. An oversized value returns a bounded error with
the local checkpoint path; snapshot fields are not treated as live query pages.

## Compact projections and diagnostics

Compact mode selects identifying fields in SQL before row conversion. It omits
large AST `details` dictionaries and document bodies from ordinary listings.
`detail: "full"` requests those fields while retaining the same output ceiling.
Use `get_doc` for document content.

Global or directory `code_map_analyze` requests return diagnostic totals,
aggregates, the five most affected files and optional cycle summaries. An exact file
path returns separate paginated unresolved diagnostics and known external imports.
Use `target: ""` for the workspace. `diagnostics` and `cycles` are path names,
not analysis modes. Cycles are omitted by default; set `include_cycles: true`
to compute them for an indexed path.
Overview and analysis totals keep `external_imports` separate from unresolved
references. Each SQLite count or row statement has a two-second computation
budget; a paged request may execute both statements. Row conversion has an
8 MiB budget and returns a page. The MCP response has a separate 64 KiB
limit. Budget errors suggest a narrower path or selector, compact detail, or
fewer SQL columns. A depth greater than one is rejected before recursive
traversal when the starting node has more than 1,000 immediate graph links.
Use depth one and page its direct neighbors, or select a narrower module or
symbol. Test mapping rejects a selected scope with more than 1,000 direct
containment or dependency links before its unbounded walk. Reducing the page
limit alone does not reduce recursive traversal work. Cycle computation retains
its separate safety limits.

## Source previews

Source is off by default. `show_source: true` or `get_code_snippet` requests a
preview with up to five leading lines, 35 body lines and zero trailing lines.
Leading context stops before a preceding declaration; body and trailing limits
respect the selected symbol's AST boundaries. UTF-8 and original line endings
are preserved.

`source_preview` reports the returned range, omitted lines and
`next_source_offset`. Continue using `source_offset` and the returned
`generation`. Source continuation counts body lines rather than list items.
An oversized individual line gets a bounded excerpt and a local path/line hint;
there is no fabricated byte cursor or unavailable file-reading tool.

The command-line `query inspect --show-source` retains exact source extraction.
MCP previews are deliberately bounded and expose omission metadata.

## Adapter settings

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

Allowed bounds are 1–100 items, 1,024–65,536 output bytes, 0–20 leading lines and
1–100 body lines. Unknown response fields and invalid values are rejected.
Absent settings use the defaults above. These presentation settings do not
alter extracted facts or force an index rebuild.

For an agent session with a tighter context budget, an adapter can use
`page_size: 10` and `max_output_bytes: 16384`. This limits each response;
the agent still decides how many pages to request.

## Source freshness

Before each MCP database read, the server validates ownership and semantics,
then compares the current admitted inventory with persisted file identities.
Changed metadata causes a content hash check. Content edits, additions and
deletions update the index before the query. A changed scope or incompatible
index uses the owned-database rebuild path. Unchanged reads leave the index
untouched. A failed admission returns an error.

Returned freshness evidence identifies the admitted snapshot. Confirmation of
the workspace root alone does not establish source freshness. External source
changes during a read remain subject to filesystem and indexed-byte checks;
the tool does not provide compiler-equivalent or runtime dependency coverage.

## Verification

`tests/query_context.rs` checks SQLite projections, counts, continuation,
diagnostic summaries and source boundaries. `tests/mcp_context.rs` checks actual
stdio frames, escaping, byte pruning and adapter limits. `tests/mcp_freshness.rs`
checks same-process source changes, linked workspaces, external deltas and
unchanged reads. The source and removal contracts remain covered by the legacy
integration tests.
