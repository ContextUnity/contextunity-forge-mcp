---
name: contract-planning
description: Build, audit, or amend implementation contracts, tasks, and milestone plans. Use to evaluate architectural viability, reconcile ADRs, uncover hidden risks or ambiguities, and constructively propose cleaner designs.
---

# Contract planning and architectural audit

Use before creating, reopening, materially amending, or admitting an
implementation task or milestone plan. Use it again when review or repair shows
that the current contract omitted an owner, consumer, failure mode, or proof
obligation, or whenever a plan requires architectural critique.

Before a review repair, compare the latest unresolved findings with the current
contract and limit the repair to those findings; do not infer its scope from a
summary or the task's general blocker list.

---

## 1. Load the repository context

1. Read the repository's `AGENTS.md` and this skill's `ADDON.md`, when one
   exists. The add-on supplies repository-specific planning authority, ADR
   locations, architectural invariants, tools, commands, and local lesson
   records; it may strengthen this procedure, never weaken it.
2. Read all current local lessons and active architectural decisions named by the
   add-on before drafting or amending the contract. Validate them against the
   governing authority.

---

## 2. Audit architectural viability and appropriateness

Before committing to an implementation plan, critically evaluate whether the proposed
architecture is appropriate for the problem:

1. **Occam's razor and essential complexity:**
   - Question whether the proposed change is the simplest viable approach to achieve
     the desired outcome.
   - Challenge unnecessary code creation: can the requirement be satisfied by
     leveraging existing capabilities, platform conventions, declarative configuration,
     or existing dependencies rather than introducing bespoke logic?
   - Reject premature generality: avoid shadow registries, parallel lookup mechanisms,
     speculative abstractions, or heavyweight indirection when direct, standard
     interfaces suffice.
2. **Layering and architectural placement:**
   - Ensure the solution is placed at the appropriate architectural tier (e.g. data
     layer, core domain, interface adapter, or external client).
   - Verify that responsibilities reside in their authoritative components, avoiding
     abstraction leaks, circular dependencies, or inappropriate cross-layer coupling.

---

## 3. Reconcile with architecture decisions and invariants

1. **Architectural authority and decision reconciliation:**
   - Reconcile the proposal with the repository's architectural decisions (ADRs),
     design documentation, and established structural patterns.
   - Verify that the plan builds cohesively on prior architectural decisions rather
     than inadvertently reversing past trade-offs or introducing contradictory patterns.
2. **System invariants and quality attributes:**
   - Confirm that the proposed changes preserve established non-functional invariants
     (e.g. subsystem purity, latency/throughput envelopes, resource bounds, safety
     guarantees).
   - If an architectural tension or necessary trade-off emerges, state it explicitly
     as an `AUTHORITY_GAP` or decision candidate rather than concealing it in
     implementation tasks.

---

## 4. Uncover weak spots, risks, and ambiguities

Probe the contract for vulnerabilities, unstated assumptions, and edge cases:

1. **Ambiguity and hidden assumptions:**
   - Identify vague scope statements or underspecified acceptance criteria that could
     allow differing interpretations during implementation.
   - Ensure every capability specifies explicit, observable, and testable completion
     criteria.
2. **Operational and runtime risks:**
   - Identify performance and scalability bottlenecks: algorithmic complexity, I/O
     amplification, unbounded concurrency, resource leaks, or high allocation overhead.
   - Evaluate boundary conditions, platform-specific differences, and external
     dependency failure modes.
3. **Failure semantics and recovery:**
   - Analyze failure modes: partial mutations, network partitions, interrupted
     workflows, and degraded states.
   - Require fail-closed behavior, clear error taxonomies, and idempotent state recovery.

---

## 5. Propose constructive improvements ("Make it better")

Never stop at identifying architectural shortcomings; synthesize actionable, superior
alternatives:

1. **Streamline the design:**
   - Formulate concrete alternative designs that satisfy the requirements with fewer
     moving parts, simpler data flow, and minimal ongoing maintenance overhead.
2. **Reuse proven patterns:**
   - Identify and reference existing, robust design patterns and canonical mechanisms
     already present in the codebase to maintain system-wide architectural consistency.
3. **Clarify negative boundaries:**
   - Explicitly articulate known architectural traps, dead ends, or subtle
     misinterpretations so implementing agents do not pursue unviable or overly complex
     paths.

---

## 6. Discover the whole contract boundary

1. Read the active task, its parent plan or phase, its milestone, and its
   explicit prerequisites and successors.
2. Search the bounded planning scope for the proposed capability's task ID,
   type or interface, configuration key, runtime caller, test name, and named
   consumer. Read every matching producer and consumer before choosing an
   owner.
3. Verify present callers, contracts, configuration resolution, storage, and
   test seams in the live implementation. Record an unavailable architecture
   index or currentness response as a coverage gap.
4. Make a producer-to-consumer map that names current callers and planned
   consumers separately. A planned consumer defines a compatibility or deferral
   boundary; it does not authorize speculative implementation.

---

## 7. Freeze an executable architecture

For each affected surface, record its owner, input and output, authority,
durable state, configuration/default, current and planned consumer, fail-closed
behavior, proof, and deferred owner. Resolve each as change, prove unchanged,
linked defer, or justified N/A.

Trace at least one complete path:

```text
trigger -> production caller -> typed contract -> canonical owner -> storage or runtime propagation -> consumer -> observable result or rejection
```

For every path decide authorization, tenancy or equivalent scope, validation,
bounds, retry and cleanup behavior, alternate backend or transport behavior,
rollback or compensation, and forbidden effects. Put unresolved architectural
choices in `AUTHORITY_GAP`; do not hide them in Build or review repairs.

---

## 8. Specify proof before Build

Name the exact test seam and sensitivity mutation for every accepted claim. Keep
hermetic and live lanes distinct: each declares its configuration source,
required infrastructure, data shape, and cleanup owner.

Every latency, throughput, size, or count promise names the production
operation, warmup policy, fixture/data cardinality, measured interval, sample
count, percentile method, target, environment, and deliberate regression
mutation. A helper microbenchmark cannot replace a caller-path measurement when
the task claims caller-path performance.

---

## 9. Bound the planning context and return findings

1. **Context hygiene:**
   - Keep planning context bounded. When continuation would exhaust the
     available context, rotate to a fresh planning agent with the current
     Contract, governing authority, unresolved findings, decisions, evidence
     links, and remaining proof obligations in a bounded packet.
2. **Verification feedback:**
   - Return Contract verification findings to the same planning agent that drafted
     or last amended the open Contract. Resume its session only when the host
     supports continuation and the context budget permits it.
   - Do not route contract-level findings to Build or open an unrelated new
     Contract to bypass failed verification.

---

## 10. Learn at Handoff

Review the session's Contract changes, dispositioned review findings, and failed
or misleading checks. Derive zero or more candidates from verified evidence.
Admit each candidate only when it prevents a contract or handoff failure,
applies beyond the session, and is independent of every admitted candidate.

Before adding a rule, compare its prevented failure and required action with all
current local lessons: skip an equivalent or subsumed candidate, and amend an
existing rule only when evidence supports a strictly clearer or broader rule.
When no candidate qualifies, do not edit the lesson record.
