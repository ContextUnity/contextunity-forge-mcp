---
name: test-suite-refactor
description: Refactor typed test suites without weakening production-path coverage.
---

# Test Suite Refactor

Use this skill to split, type, consolidate, improve, or remove tests in an
existing suite. Load `test-contract-seam` when the refactor strengthens a
settled product contract; this skill governs the refactor mechanics and does
not replace its production-seam evidence rules.

## Order

Inventory and classify → **replace and delete** → type remaining tests →
**decompose monolithic files along domain and behavioral boundaries** → verify.

Do not split first. A split that keeps every old test is not a refactor.
Do not close a cluster whose unique test count rose without `added` rows that
name new unique observable dimensions.

## 1. Inventory Before Editing

1. Read repository and owning-subtree instructions, then the add-on if present.
2. Freeze a collect-only unique-test baseline (`path::class-or-module::name`)
   for the owner target before any edit.
3. Record module line counts, existing support modules, and production callers.
4. Locate each test's production owner and the smallest public or composition
   seam that can prove its behavior.
5. Identify duplicates only after inspecting local fixtures, support modules,
   Protocols, factories, and production test utilities.

Do not treat a green suite, line coverage, or a mock-call assertion as proof of
product behavior. Inventory against current HEAD. Do not re-apply splits or
helpers from another branch, stash, or stale suite snapshot.

## 2. Test Reachability Audit

For every suspicious fixture, fake, mock, or direct state mutation, trace:

```text
test state -> ingress/parser -> normalized type -> persistence/restore -> runtime consumer
```

Classify each result before editing:

| Classification | Meaning | Action |
| --- | --- | --- |
| `KEEP defensive` | Unique reachable malformed, corruption, outage, race, auth, cleanup, restart, or persistence dimension, with a named threat model. | Retain only if no retained test already covers that dimension. |
| `REWRITE` | The asserted behavior is real, but the test bypasses required construction, authorization, parsing, or wiring. | Drive it through the narrowest real seam; fake only unavailable external boundaries. |
| `REMOVE` | Unreachable, or fully subsumed by a stronger retained seam test. | Delete in the same cluster as the replacement. |
| `UNCERTAIN` | Reachability is not proven. | Preserve the test and **block cluster closure**. |

`KEEP defensive` requires production or a supported compatibility path that can
produce the state, or a real defense-in-depth boundary that consumes it. A
second test of the same dimension is `REMOVE`, not `KEEP`.

Do not dismiss tests merely because they exercise malformed payloads, failures,
cleanup, persistence corruption, concurrency, security, or fail-closed paths.

## 3. Replace, Then Delete

For each weak or duplicate cluster:

1. Identify the strongest existing public or composed seam test.
2. If absent, rewrite one test through that seam (load `test-contract-seam`
   when the contract is settled).
3. Record `old test -> retained seam test -> preserved observable dimensions`.
4. Delete the old private, mock-shaped, partial, or review-probe test in the
   **same** cluster. Do not keep both.

Preserve unique accepted, rejected, malformed, boundary, persistence, cleanup,
restart, concurrency, and alternate-caller dimensions. Do not add a new test
solely to preserve reviewer history.

A cluster without a complete replacement map for every `REMOVE`/`REWRITE`
source test does not close.

## 3a. Harness Versus Assertion

Classify construction separately from the observable assertion.

- Ordinary call sites that need a migrated store, server, client, or temp
  schema must use the owner's canonical fixture or context manager. Replace
  duplicated outer `try`/`finally`, manual create/drop, and ad-hoc connect
  loops with that helper. This is a harness `REWRITE`; it is not a new test
  and must not raise unique test count.
- Keep a direct driver/engine/process handle only when that handle **is** the
  assertion: corruption, extra session, lock, concurrency, RLS/security
  bypass, or fail-closed cleanup the public seam cannot see.
- Do not fold those direct handles into the disposable helper if that would
  hide the threat. Do not keep a private lifecycle beside the canonical one
  for ordinary paths.

If a canonical helper already exists in the owner support module, reuse it.
Do not invent a second schema-disposal pattern.

## 3b. Duplicates, Noise, And Junk

Delete or collapse these even when they are green. They are not unique
dimensions.

Do not retain or add an absence or reintroduction test for abandoned,
unshipped, review-only, or agent-generated intermediate code. Retain that kind
of guard only when a current public API, security, migration, or operational
contract requires the surface to remain absent; otherwise delete the stale test
or fixture rather than preserving its history.

| Smell | Action |
| --- | --- |
| Two tests, same observable, different mock/fixture spelling | Keep the stronger seam; `REMOVE` the other |
| Denial/error paths that only change the token factory and assert a private mock | One parametrized test on observable status, details, and non-mutation |
| `assert_called*` / `call_count` on a private method (`._foo`) | `REWRITE` through public output or `REMOVE` if a public test already covers it |
| Module or docstring named facade, legacy seam, or patching | `REMOVE` once a public runtime test covers invoke/stream/error |
| `test_*_exports_*`, `has_no_dead_*`, hasattr inventories per method | One architecture-guard module; do not keep per-wrapper copies. Keep a single public-API freeze test for removed kwargs |
| Two parametrize tables encoding the same mapping (profile list + access matrix + known-names) | One table, one test |
| N identical nested fakes/stubs in one file | One helper or one table-driven test; harness `REWRITE` |
| `__repr__` / wording / "human readable" unless redaction or client-safety is the contract | `REMOVE` |
| Inner `_Stub` class per SDK/RPC test | Table of RPC → request/response fields through the real client |
| `type: ignore`, `cast`, method-assign, `object.__new__` | `REWRITE` construction; do not leave the suppression |
| "Regression for review/finding/legacy" with no unique dimension | `REMOVE` after the seam test exists |
| Wall-clock `sleep`/`join` used to simulate timeout, hang, or slow peer | `REWRITE`: `wait_for`/deadline + **cancel pending tasks**; never `sleep` longer than the asserted deadline |


Do not leave pending `sleep` tasks after `asyncio.wait(..., timeout=...)`.
Cancel them in the test or production path. A 10s/60s sleep that only exists
to trip a 10ms deadline is junk and blocks the suite. Subprocess/thread
`join(timeout=N)` loops that wait on `sleep(30)`/`sleep(60)` helpers are the
same smell: signal completion, do not wait out the clock.

Do not replace one weak test with many mutation-specific tests. Prefer one
parameterized seam when construction and lifecycle are shared.

A count that stays flat while these remain is not a successful cluster.

## 4. Domain Decomposition And Splitting Monoliths
 
Split only after section 3. The primary goal is proper domain decomposition and
cohesion; strongly recommend decomposing monolithic test files along natural
architectural and contract boundaries rather than applying an arbitrary mechanical
line-count cap.
 
1. Split by behavioral, subsystem, or contract seam, never mechanically.
2. Carry forward only `KEEP` and unfinished `REWRITE` tests. Do not copy
   `REMOVE` tests into the new modules.
3. Preserve markers, fixture scope, and ordering assumptions for retained tests.
4. Reuse an existing local support module first.
5. Extract a helper only when it represents repeated setup or data construction
   across at least two remaining tests and has one coherent package-local
   ownership seam.
6. Keep assertions, scenario-specific fakes, failure paths, and security
   checks local. Do not build a cross-package mega-framework.
7. Do not merge doubles with different protocols, failure semantics, lifecycle,
   or authorization behavior.

## 5. Type Tests From Production Contracts

1. Use production types, Protocols, TypedDicts, narrowing helpers, and public
   adapters wherever available.
2. Give fakes only the protocol surface their production boundary uses.
3. Prefer a real constructor plus one external-boundary fake over
   `object.__new__`, direct private-map injection, or patching below the
   asserted seam.
4. Do not introduce `Any`, `cast`, `type: ignore`, `pyright: ignore`, or rule
   suppressions to make a test type-check.
5. BasedPyright 0/0 is not a pass if the test still reaches production
   privates by relocating the same write/call. Forbidden pyright workarounds:
   subclass/`__init__` whose only job is `self._foo = …`; `exec`/`eval` of
   `_Name` calls; `getattr`/`setattr`/`importlib` of `_` symbols to dodge
   `reportPrivateUsage`. REWRITE through the public constructor, parser, or
   RPC. Direct private access stays only as `KEEP defensive` with a named
   threat the public seam cannot see.
6. Restore the required static baseline for every file newly touched by the
   refactor. Pre-existing diagnostics are not permission to add a file to a
   changed set without remediating its applicable errors.

## 6. Strengthen Rather Than Mirror Implementation

Each retained or rewritten scenario should identify:

- observable output, rejection, persisted effect, cleanup, or non-mutation;
- the production caller or composition root reached;
- one plausible mutation the test must reject;
- material accepted, rejected, malformed, boundary, persistence/restart, and
  alternate-caller dimensions.

Classify private implementation-shaped tests as supplemental at most. Rewrite
them when a public or composition seam can prove the same contract, then delete
the private test.

## 7. Select Sufficient Evidence

For each behavioral seam, record whether these levels are required, covered,
or not applicable: static contract, component, in-process transport, live
persistence/process, and cross-service.

Escalate beyond component tests when behavior depends on serialization,
interceptor or auth wiring, transactions, shared storage, filesystem effects,
queues, processes, restart, or producer-consumer propagation. Put fakes behind
the real claimed boundary. A metadata-only or in-memory test is not integration
proof merely because it lives in an integration directory.

## 8. Production Edit Boundary

Default write set: tests, test support, and fixtures owned by the current
cluster. Do not edit production, migrations, generated API dumps, lockfiles,
or CLI/MCP runtime as formatting or convenience.

A production repair is allowed only when all of these hold:

- the defect was found by the refactor;
- the fix is bounded to the same canonical owner;
- it does not change public API, schema, config, migration, or a cross-owner
  security contract;
- `close-review-findings` is loaded;
- the user explicitly approved the production path set.

Otherwise return the defect to delivery and keep the failing test.

## 9. Verification And Review

When `ctx_shell` is available, run full owner suites, required integration
lanes, and the final completion gate with
`ctx_shell(timeout_ms=3600000, run_in_background=true)`. Poll
`ctx_shell(background_action="status", job_id=...)` until the job completes
and record its final exit code. A detached call or compressed output is not
proof that tests finished. For a lane expected to exceed the one-hour job
limit, use a durable runner with a retained log and exit status. Follow the
repository's test commands and any stricter timeout requirements.

Run the narrowest focused test command after each bounded cluster, then:

1. collect-only unique-test count vs the frozen baseline: after ≤ before,
   except `added` rows that name new unique dimensions;
2. static checking with zero errors and warnings on every touched test/support
   file;
3. formatter/linter and whitespace checks;
4. domain cohesion and architectural decomposition check for all changed tests;
5. split parity for retained tests only: imports, collection, markers, coverage;
6. every required integration level, with skipped required evidence failing the
   cluster;
7. after the final test/support edit, run the repository's designated full
   completion gate on the settled diff; focused, owner, integration, lint, and
   typing receipts do not substitute for it;
8. independent review for lost coverage, import collisions, state leakage,
   fixture scope drift, bypassed seams, insufficient integration, weakened
   fail-closed assertions, leftover duplicates, and unauthorized production
   edits.

Do not claim a refactor complete while a touched file retains applicable static
errors, a coverage classification remains `UNCERTAIN`, a replacement map is
incomplete, or unique test count rose without justified `added` rows.

## Output

Report the bounded cluster, production seam, classifications, harness rewrites, replacement map,
before/after unique test counts, `added` rows, evidence levels, helper reuse,
focused and integration validation, static diagnostics, domain layout result, review
result, retained defensive tests, and every production path touched.
