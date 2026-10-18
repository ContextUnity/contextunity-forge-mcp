# 031 Task 3: Forge self-index performance

Five alternating cold-build pairs ran against the Forge worktree with `RAYON_NUM_THREADS=8`. Each run used a fresh temporary database and extraction cache. The baseline was `35b2594` (`61cad071e43077fe06aef029480c14cefd53fb2e397428a7dbe1c283b06ae303`); the candidate was the updated release binary (`f1939724833715790bd031361d4df662e1b26b44012e28723c7edfd29c178be9`).

The accepted run started after the observed Task 022 compile had finished. `ps` checks immediately before and after it found no active `cargo`, `rustc`, `lld`, or `cc1` process. An earlier run that overlapped that compile was discarded and is not included here.

| Forge metric | Baseline median | Candidate median | Change |
| --- | ---: | ---: | ---: |
| Cold indexing wall time | 1.701 s | 1.793 s | +5.409% |
| Paired wall-time change | — | — | +5.386% |
| `seal_ms` | 356.029 ms | 452.388 ms | +27.1% |
| `index_and_seal_ms` | 429.316 ms | 556.185 ms | +29.5% |

Graph coverage was identical: 309 files, 6,744 nodes, 20,273 edges, 291 document sections, 2 parse errors, and 38,898 unresolved references. Resolution stayed at 7,277 resolved, 578 ambiguous, and 38,320 unresolved references (15.7596% resolved excluding external imports); all graph-count deltas were zero and components matched. Each binary produced a stable output root across its five repetitions; roots differ between versions.

This Forge-sized corpus preserved coverage but did not show a speed improvement over the baseline. Candidate pre-encoding of facts had a 20.294 ms median and is included in the cold-build wall time. The raw runner output is `/tmp/forge031-task3-post-self-forge-clean2.json`.
