---
name: test-contract-seam
description: Prove admitted contract claims with production-seam tests under the selected proof policy.
---

# Contract-Seam Testing

Use this skill to prove the admitted contract through observable production
behavior, not to mirror private implementation details. Follow the admitted
proof policy for test timing and role requirements.

Prove one admitted task through one production seam. A subtask adds a case to
that seam. It does not add a test function or a test binary. A new test file
is a new domain boundary, named by the repository's test instructions.
Before adding a helper, factory, or fixture, use the one already in the suite.
`test-suite-refactor` owns the inventory of factories, fixtures, and duplicate
helpers. Do not add a second factory for the same production object.

## Admission

Require settled authority and accepted claim IDs, the proof policy and
repository adapter, the production owner and relevant seam, and the required
test lanes.

Read proof policy semantics from the active task engine's canonical reference
and returned workflow guidance. For Forge, use `forge_guide(topic="acdd")`
and the task evidence schema. The repository supplies test commands and lanes.
Execute the admitted test-authoring step and record the observed result.
Bind expected-red claims to an observed failure or recorded pre-fix replay;
prove existing behavior through the policy's accepted evidence format.

Require only the authorship and review separation selected by the proof policy
and repository adapter. Missing policy-required inputs block authoring.

## Routing

1. Read the authority, accepted claims, proof policy, current production path,
   relevant callers, adjacent tests, and required repository lanes.
2. Read `.agents/skills/test-contract-seam/ADDON.md` when present. It may add
   routes, fixtures, commands, and stricter evidence requirements.
3. Build one evidence map for the slice before authoring; update that same map
   as evidence changes instead of creating phase reports.
   The notes in `phases/` are optional detail, not separate gates or artifacts.
4. Author only tests, fixtures, and the host-owned evidence map. Return
   production, contract, adapter, and migration changes to their owner.
5. Validate the stable test diff and record commands, results, limitations, and
   blockers in the same map.

## Contract Coverage

- Cover every accepted claim with scenarios that prove its complete observable
  behavior. A test of one function calling a mock is not contract-seam evidence.
- Enter through a supported production caller and traverse real collaborators
  through the claimed boundary. Use real clients, services, parsers, stores, and
  stateful collaborators where available. Put protocol-faithful fakes only
  behind unavailable external dependencies, outside the boundary being proved.
- Assert observed results and effects at the consumer boundary, including
  durable state, revisions, or rejected-operation non-mutation when claimed.
  Mock-call assertions alone do not prove production behavior.
- Cover each claim's material accepted, rejected, malformed, exact-limit/boundary, security,
  failure/cleanup, persistence/restart, concurrency, alternate-caller/backend,
  and external-effect dimensions. Record inapplicable dimensions with a reason.
- Keep tests deterministic and scoped to accepted claims; do not rewrite
  unrelated suites.
- Select the evidence level required by each claim before authoring. Local
  projections and architecture guards are supplemental and cannot replace a
  required component, transport, live persistence/process, or cross-service
  boundary.
- Name and demonstrate a plausible contract-breaking mutation for each material
  contract scenario; use controlled mutation, fault injection, pre-fix replay, or equivalent
  deliberate break. Record whether the test detects it.
- Name the real environment and readback boundary for integration proof.
- Classify evidence accurately:

| Class | Purpose | Contract proof |
| --- | --- | --- |
| `contract-seam` | Observable behavior through a production seam | Yes |
| `integration-boundary` | Protocol, serialization, persistence, or adapter behavior | Yes, when it covers the claim |
| `local-projection` | Local mapping, dispatch, or calculation | Supplemental only |
| `architecture-guard` | Structural constraint or caller inventory | Supplemental only |
| `brittle-structural` | Private internals or organization-sensitive assertions | No; refactor candidate |

- Remove a weaker test in the same test-only diff only when the retained seam
  proves its full observable contract. Preserve unique negative, cleanup,
  persistence, restart, concurrency, and alternate-caller dimensions; record the
  old-to-new test mapping. Do not keep subsumed tests or add duplicate proof.
  When invoked from `test-suite-refactor`, cover only missing dimensions and
  return to that skill for its removal gate.
- Do not preserve tests for abandoned, unshipped, review-only, or intermediate
  code unless a current public, security, or operational contract requires its
  absence.
- If a test exposes a production defect, preserve the test and evidence and
  return the defect to delivery. Refresh only evidence invalidated by a repair.

Required evidence that is skipped, unavailable, or mocked inside the claimed
boundary blocks completion.

## Long-Running Test Commands

Follow the active host's process-management rules and the repository's test
commands. Set a timeout sufficient for the required suite. If the runner's
lifetime is too short, use an authorized durable runner with retained logs and
exit status. Observe terminal completion and record the exit code; a detached
job or compressed progress output is not proof that tests finished.

## Host Boundary

The host provides sessions, role separation, filesystem isolation, test
execution, evidence storage, and any write permissions. Host runtime mechanics
belong to the host or repository.

## Output

Use `templates/evidence-map.md` as the single evidence artifact for the slice.
Record every accepted claim, scenario, production seam, material dimension,
test/evidence class, sensitivity result, required command, limitation, and
blocker. Include a replacement map only when tests are replaced. Completion
requires every accepted claim and required gate to be proven; report all gaps
honestly.
