# Milestone 048 cold-build measurements

The final task-2 control completed all twenty builds and passed the cold-build
performance gate: baseline median 15.751 s, candidate 15.010 s (-4.704%);
median paired change -3.624%. All 38,818 persisted fixture Merkle rows remain
identical, graph counts and resolution coverage match, and each binary's global
root is deterministic. This is an observed improvement on this host and copy,
without a claim of statistical significance. The candidate binary SHA-256 is
`011c9f0ffdd95515bcb0164ef893cf4faa083e84258b3e6e9b9383a07bb1f877`.
The series started at 2026-10-16 05:15:10 UTC; complete measurements, binary and
runner hashes are in [the source-buffer control report](048-cold-build-capacity.json).

The control followed a bounded allocation correction: the limited reader now
reserves the already validated file length before reading. This restores the
regular-file size hint lost when replacing `fs::read_to_string` with a limited
reader; file-size, metadata, digest and source-budget guards remain in place.
Ten alternating pairs were fixed before measurements. Acceptance required both
the unrounded candidate median wall time and median paired change to be no worse
than baseline, with no early stop or discarded runs. The runner enforces this
gate as well as fixture equality and per-binary root determinism.

The preceding repaired-source follow-up failed the performance gate: baseline
14.348 s, candidate 14.692 s (+2.398%); paired change +3.501%. Its ten builds are
retained in [the repaired-source raw report](048-cold-build-repaired.json), started
2026-10-16 04:58:26 UTC, binary SHA-256
`af9e8b2b855c7c4d186bcd3d6d0a651f43851c972f7eb487cbb41584ede399ed`.
That historical runner's zero exit code scored semantic determinism only; it does
not turn the latency regression into a pass. Timing variation between the series
limits causal claims about the allocation correction.

The task-2 matrix series at candidate `858a2ac` completed all 22 builds successfully. Adaptive cold-build
median latency decreased from 13.551 s to 13.461 s (0.664%); the median paired
change was -2.359%. This is a modest measured improvement on this copy, without
a claim of statistical significance. Graph counts, components, resolution
coverage, database size, and every persisted fixture Merkle row were unchanged.

## Reproduction

The user authorized benchmarks on isolated copies. The snapshot includes
`commerce-release-update` and copied linked `grid-view-spec`/`traverse` workspaces;
commerce's absolute links point to those copies. Original workspaces were not
modified. Each build received a fresh temporary database. Builds ran sequentially
without concurrent Cargo or Forge indexing, alternating baseline/candidate order
and reversing budget order between repetitions. OS page caches were not flushed.

For the final task-2 control, use the command below with `--repeats 10
--matrix-repeats 0 --output benchmarks/results/048-cold-build-capacity.json`.
The historical memory matrix uses its original parameters below.

```sh
python3 benchmarks/cold_memory_budget_benchmark.py \
  --profile benchmarks/profiles/commerce-release-update.json \
  --workspace /tmp/forge-048-cold-build/commerce-release-update \
  --baseline-binary /tmp/048-baseline-forge \
  --candidate-binary "$PWD/target/release/contextunity-forge-mcp" \
  --repeats 5 --matrix-repeats 3 --budgets-mb 50 200 500 1000 \
  --output benchmarks/results/048-cold-build.json
```

The series started at 2026-10-16 04:15:54 UTC on Linux 7.2.3-arch1-3, x86_64,
glibc 2.44, with 16 logical CPUs and no `RAYON_NUM_THREADS` override. Both binaries
used the release profile. The baseline was saved before task-2 production edits;
the intervening task-1 repack changed documentation rather than runtime code.
Binary SHA-256 values and the exact runner command/hash are retained in
[the complete raw report](048-cold-build.json).

## Memory-budget matrix

Each row contains three candidate builds. Budgets use MiB (1,048,576 bytes), and
the runner checked the effective budget reported by every build.

| Extraction budget (MiB) | Median wall time (s) | Median peak RSS (bytes) |
| ---: | ---: | ---: |
| 50 | 23.210 | 1,414,230,016 |
| 200 | 16.785 | 1,434,763,264 |
| 500 | 14.895 | 1,449,660,416 |
| 1000 | 12.825 | 1,475,256,320 |

The predeclared selection rule chooses the lowest median RSS within 5% of the
fastest median time, breaking ties by latency and then budget. It selected
**1000 MiB** in this series. This is a measured operating point for this host and
snapshot, not a universal default. The extraction budget bounds source working
buffers; retained facts, parsers, linking, and SQLite also contribute to process
RSS. Adaptive median RSS was 1,471,410,176 bytes before and 1,477,234,688 after.
Database size remained 192,741,376 bytes.

## Semantic commitments

All 38,818 persisted `domain_commitments` rows across 17 domains were
bit-identical across the five baseline and seventeen candidate builds. The
ordered, length-framed row aggregate was
`d98db364a389041b75c69959b2a522d590c287d83afdf3261b098fe061133e50`.
The graph retained 3,950 files, 65,635 nodes, 280,718 edges, and the same existing
seven parse errors. The persisted adapter-policy SHA-256 also matched exactly.

Each binary's overall output root was deterministic; the candidate root was
identical across adaptive, 50, 200, 500, and 1000 MiB builds. Cross-version global
roots differ because that root also commits the engine source fingerprint and
manifest-discovery inventory. The old redundant root walk included two Pulumi
`pyproject.toml` files outside configured source roots; the admitted scanner plus
direct root/ancestor discovery omits those unrelated nested manifests. No fixture
domain digest changed, and the root algorithm/version fingerprint was preserved.

## Earlier series

[The pre-policy-fix series](048-cold-build-before-policy-fix.json) is retained to
avoid discarding earlier observations. It measured a 2.21% adaptive median
improvement and selected 500 MiB. Subsequent inspection found that the new
transient memory option unnecessarily changed serialized adapter policy; the
final candidate excludes it from that persisted policy and has a regression
test. The complete matrix series above supersedes the earlier operating-point
recommendation. Variation between series limits the strength of timing claims.

Independent review subsequently identified a remaining duplicate SHA-256 call,
Vue script-context leakage, ignored-manifest basename leakage, and missing public
configuration proof. This matrix records the measured candidate honestly; a
paired follow-up and subsequent source-buffer control above cover the repairs. The
runner subsequently gained `--matrix-repeats 0` for that follow-up, retaining
the same timing and fixture-commitment oracle without repeating this matrix.
