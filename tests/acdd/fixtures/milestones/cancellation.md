---
id: m-cancellation
title: Mixed task outcomes
doc_type: contract
status: active
started_at: 2026-10-01T10:00:00Z
---
# Mixed task outcomes

### task: planned
```yaml
task_ref: planned
target: Deliver planned work
proof_policy: seam-test-first
scope: [src/planned.rs]
status: planned
```

### task: delivered
```yaml
task_ref: delivered
target: Preserve a completed task
proof_policy: seam-test-first
scope: [src/delivered.rs]
status: completed
receipt:
  contract_revision: 1
  passed_at: 2026-10-01T10:00:00Z
  evidence:
    seam_test_proof:
      exit_code: 0
      seam_test_refs: [tests/acdd/tasks.rs]
      tests_passed: 1
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths: {applicable: true, evidence: public-seam verification}
        claims: {applicable: true, evidence: public-seam verification}
        concurrency: {applicable: true, evidence: public-seam verification}
        project_isolation: {applicable: true, evidence: public-seam verification}
        administration: {applicable: true, evidence: public-seam verification}
  decision: pass
```

### task: cancelled
```yaml
task_ref: cancelled
target: Preserve cancelled work
proof_policy: seam-test-first
scope: [src/cancelled.rs]
status: cancelled
```
