# Security-audit indexing speed check — 2026-09-27

The local `security-audit` worktree supplies the same admitted inventory to two
release binaries. The baseline is the previous local release artifact saved
before the current release build; it is not a clean `HEAD` build. SHA-256 values
identify the binaries used:

| Binary | SHA-256 |
| --- | --- |
| Baseline | `edc2e46bcd3e9dd5db534a5588084a2a292f800cafceb1335b8574ffb7bab594` |
| Candidate | `abc26294a0b6483d528051c7aa344ff963910b6391ad08f678e14e9d55b42111` |

The comparison alternates binary order, uses one warmup and five measured runs
per binary, and sets `RAYON_NUM_THREADS=4`. Both builds admit 3,064 identical
files and produce 42,543 nodes and 103,493 edges. Node and edge snapshots
match. Coverage snapshots differ because the candidate records known external
imports as `external`.

| Median, milliseconds | Baseline | Candidate |
| --- | ---: | ---: |
| Full build | 13,830.7 | 13,352.1 |
| Build extraction | 2,949.6 | 3,164.3 |
| Build persistence | 6,134.3 | 5,465.6 |
| `delta` with one named file | 18,015.2 | 17,724.1 |
| Delta extraction | 4.40 | 4.39 |

The `delta` input is
`services/shield/src/contextunity/shield/audit.py`. Each run reparses and
rewrites one file, while dependency invalidation can relink many owners. A
follow-up run on the candidate index reports 2,262 affected owners and 2,261
loaded fact files. The `contextunity` reference key alone occurs for 2,216
owners in this index. Full-build runs range from 12.7 to 16.4 seconds for the
baseline and from 12.9 to 16.3 seconds for the candidate; delta runs range
from 15.4 to 18.6 and from 16.3 to 18.8 seconds. The overlapping ranges do
not establish a small speed difference. No large end-to-end regression appears
in these local runs, while the build extraction median is about 7% higher.

Cold build writes a new temporary database with deferred indexes and then
atomically replaces the output. Delta updates an indexed database under a
durable WAL transaction, recalculates affected graph rows, seals commitments,
and verifies them. The candidate delta median spends 9.77 seconds in
persistence, 2.14 seconds sealing and 1.93 seconds verifying; file extraction
takes 4.39 milliseconds. The broad affected-owner set and in-place durable
write explain why this delta is slower than a cold build.

`compare_releases.py` reproduces the full-build comparison with `--repository`.
The complete local sample records are in
`/tmp/forge-security-audit-speed/results.json` and
`/tmp/forge-security-audit-speed/delta-results.json`. The query-only guard and
guide edits made after the candidate release build do not affect build or
delta execution.

## Delta speed resolution (2026-09-27)

- **Root cause 1**: `src/db/writer.rs` used `local_module.split('.')`, inserting root namespace tokens (such as `"contextunity"`) into `names`, causing all 2,216 files referencing `contextunity.*` to be marked as affected.
- **Root cause 2**: `f.nodes` and `owned_nodes` added bare `name` for fields and methods (`timestamp`, `details`, `to_dict`, `__init__`), matching an additional 110 unrelated owners.
- **Root cause 3**: `src/engine/linker.rs` did not register module namespaces relative to `src/` roots (`contextunity.shield.audit`), leaving direct imports unresolved.

### Post-fix benchmark (`audit.py` single-file delta)

| Metric | Baseline | Post-fix | Improvement |
| --- | ---: | ---: | ---: |
| Affected owners | 2,262 | 6 | **~377x fewer** |
| Total `delta` latency | 18,015 ms | 1,520 ms | **~12x faster** |
| Extract | 4.4 ms | 3.6 ms | ~1.2x |
| Hydrate | ~9,770 ms | 78.5 ms | **~124x faster** |
| Link | ~1,200 ms | 19.0 ms | **~63x faster** |
| Persist & Seal | ~4,070 ms | 826 ms | **~5x faster** |
| Loaded fact files | 2,261 | 5 | **~452x fewer** |
