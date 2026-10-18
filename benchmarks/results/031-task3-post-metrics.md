# Task 3 post-change Forge indexing and coverage metrics

Measured 2026-10-18. Every measured build used a Forge executable. The control
Commerce workspaces were isolated copies under `/tmp`; their source files were
not modified. Raw reports are stored beside this file.

## Control repository: `commerce-release-update`

Ran `cold_memory_budget_benchmark.py --repeats 5 --matrix-repeats 0` with
`RAYON_NUM_THREADS=8`. The baseline executable is commit `cf4aa29` (SHA-256
`e8f801fe7bb073c2c84759f0c2e0b800c3da94f1517707ca655e6b8ae8332e7b`). The
candidate is the Task 3 worktree build (SHA-256
`8a8b4c9f892dc5836d4b35c8b1e4188231ed0e4c18f6f8d02f1d071cec3c340b`).

| Metric | Baseline median | Candidate median | Change |
| --- | ---: | ---: | ---: |
| Cold build wall time | 16.414 s | 18.753 s | +14.250% |
| Paired wall-time change | — | — | +21.549% median |
| `index_and_seal_ms` | 2,897.208 ms | 5,821.678 ms | +100.9% |
| `seal_ms` | 1,508.789 ms | 4,566.382 ms | +202.7% |
| Peak RSS | 1,481,297,920 bytes | 1,344,118,784 bytes | -9.3% |
| Database size | 196,608,000 bytes | 197,132,288 bytes | +0.267% |

The milestone limit is at most 80% of the `cf4aa29` median: **13.131 s**.
The candidate median is **18.753 s**, so the cold-indexing performance gate
fails. The runner also reports its no-regression gate as failed.

Candidate coverage was 74.0638% resolved excluding external imports across
3,990 files, 66,482 nodes, 288,620 edges, and 2,133 documentation sections;
there were 7 parse errors. Candidate runs had identical output roots and
fixture commitment digests. Against `cf4aa29`, graph counts changed by +309
edges, -359 unresolved references, and +1 external import; resolved coverage
increased by 0.2185 percentage points. The baseline and candidate fixture
commitment fingerprints differ.

Baseline wall samples: 12.466, 18.258, 43.036, 15.428, 16.414 s. Candidate
wall samples: 16.715, 18.343, 22.947, 18.753, 42.176 s. The host was a
16-CPU Linux system; both arms used eight Rayon threads.

Raw report: [`031-task3-post-commerce-cf4aa-rayon8.json`](031-task3-post-commerce-cf4aa-rayon8.json).

## Forge source workspace

Ran `cold_test_coverage_benchmark.py --repeats 5` against this Task 3 worktree,
comparing the pre-task `35b2594` executable (SHA-256
`61cad071e43077fe06aef029480c14cefd53fb2e397428a7dbe1c283b06ae303`) with the
candidate above. Both arms used eight Rayon threads and fresh databases and
extraction caches.

| Metric | `35b2594` median | Candidate median | Change |
| --- | ---: | ---: | ---: |
| Cold build wall time | 1.946 s | 2.142 s | +10.072% |
| Paired wall-time change | — | — | +14.551% median |
| `index_and_seal_ms` | 455.258 ms | 644.211 ms | +41.5% |
| `seal_ms` | 355.553 ms | 531.513 ms | +49.5% |
| Peak RSS | 209,039,360 bytes | 219,848,704 bytes | +5.2% |

Both executables indexed the same graph: 309 files, 6,744 nodes, 20,272 edges,
291 documentation sections, and 2 parse errors. Resolution was 7,275 resolved,
578 ambiguous, and 38,317 unresolved references, with 22,678 external imports;
resolved coverage excluding external imports was **15.7570%**. Graph count and
coverage deltas were zero. Each executable produced one stable output root
across its five runs, but the roots differed between binaries: baseline
`9deb9785b79e98aba78163ae1e63339aa69d5ca7db5740734c34176473ae8135`, candidate
`0b79c67973f2d0735ed7bf2a26e476edb56a681f0fcf6267e7c078e2f9abb6b7`.

Raw report: [`031-task3-post-self-forge.json`](031-task3-post-self-forge.json).

## Cross-worktree extraction-cache check

Ran `cross_worktree_extraction_cache_benchmark.py` for five requested repeats
on the two isolated Commerce copies using the candidate executable and eight
Rayon threads. The copies had identical `forge-mcp.yaml` files (SHA-256
`5cc23854d59e2f689814c7899ec8bae267a54f01d5d1f7c1be0215b699b350a6`) and
matching workspace contents. The first repetition stopped because the
workspace B build recorded **0 extraction-cache hits** after workspace A had
seeded the cache. The runner exited 2 before recording a cold-versus-cached
timing report, so this run provides no cross-worktree speedup measurement.

## Result

Forge graph coverage on its own source workspace remained unchanged against
the pre-task executable. Both measured cold-build comparisons were slower with
the candidate, and cross-worktree cache reuse produced no hits. The milestone
cold-indexing gate is not met by these measurements.
