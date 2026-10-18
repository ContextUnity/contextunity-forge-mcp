# Task 3 pre-change Forge cold indexing and coverage

Measured 2026-10-18 on the isolated `commerce-release-update` copy at
`/tmp/forge031-bench-wiysmu06/commerce`. The copy's enabled `traverse` and
`gridviewspec` workspaces also point to local copies under `/tmp/forge031-bench-wiysmu06/linked`.

The runner was `benchmarks/cold_test_coverage_benchmark.py --repeats 5`.
Both runner arms used the same pre-change Forge executable from commit
`35b2594dd2eae5e760de73735bd22496b947586c` (SHA-256
`61cad071e43077fe06aef029480c14cefd53fb2e397428a7dbe1c283b06ae303`).
This gives five cold-build samples per arm to establish repeatability; the
arm-to-arm difference is measurement variation, not a code change. The raw
report is `/tmp/forge031-task3-pre-forge-only.json`.

| Forge run group | Cold-build median | Five wall-time samples |
| --- | ---: | --- |
| baseline arm | 11.597 s | 13.552, 11.155, 11.597, 11.320, 12.175 s |
| repeatability arm (same binary) | 11.153 s | 11.555, 13.881, 11.153, 10.779, 10.309 s |

Both groups produced the same coverage and graph counts:

- 3,990 files, 66,482 nodes, 288,620 edges, 2,133 documentation sections, and 7 parse errors.
- 130,079 resolved, 38 ambiguous, and 45,514 unresolved references; 74.0638% resolved excluding external imports.
- 140,301 external imports and the same seven components: `gridviewspec`, `traverse`, `extensions`, `packages`, `scripts`, `services`, and `tests`.
- Database size: 197,165,056 bytes in both groups.

The runner also captured Forge peak RSS; it is retained in the raw JSON for the
milestone's memory profile.

## Sealing phase baseline

The Forge-only `cold_memory_budget_benchmark.py` runner was also run with
`--matrix-repeats 0`, so it captured build phase timings without running a
budget matrix. It used the same binary for both arms, five cold builds per arm,
the same isolated workspace, and no explicit thread override. The raw report is
`/tmp/forge031-task3-pre-phase-profile.json`.

| Phase | Baseline arm median | Same-binary repeat median |
| --- | ---: | ---: |
| Full cold build wall time | 10.158 s | 10.042 s |
| `index_and_seal_ms` | 1,877.626 ms | 1,982.756 ms |
| `seal_ms` | 1,012.102 ms | 1,075.404 ms |
| Peak RSS | 1,464,897,536 bytes | 1,473,359,872 bytes |

The first baseline-arm build was a 32.152 s outlier; the other baseline builds
were 9.753–12.900 s. Since both arms use the same executable, these values are
repeatability evidence, not an implementation comparison. All ten runs had
39,211 persisted fixture commitment rows with SHA-256
`062460698b8aba316c3edfd590acd5f03f1a2d017b9aeebfdd9c204eda85eef3`; output
roots and graph coverage were bit-identical across the runs.

## Second-worktree baseline

A second independent Commerce copy was created at
`/tmp/forge031-bench-wiysmu06/commerce-worktree-b`; its `forge-mcp.yaml` is
byte-identical to the first copy and both use the same local linked-workspace
copies. The same pre-change Forge binary was run there for five repeats per
arm. Its baseline-arm cold-build median was 11.806 s (samples: 10.613,
11.118, 17.001, 11.806, 12.630 s). The same-binary repeatability-arm median
was 12.928 s. Coverage and graph counts exactly matched the first copy,
including 74.0638% resolved references excluding external imports. The raw
report is `/tmp/forge031-task3-cross-worktree-b-pre.json`.

The first worktree's median was 11.597 s, so the pre-change second-worktree
baseline was 1.8% slower. The repeatability samples included 35.202 s and
22.118 s outliers, so this small difference is within observed run variation;
it is recorded as the comparison point for the post-change run, not as a
performance claim.
