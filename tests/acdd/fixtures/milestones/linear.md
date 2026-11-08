---
id: m-linear
title: Sequential milestone
doc_type: contract
status: active
started_at: 2026-10-01T10:00:00Z
---
# Sequential milestone

### task: first
```yaml
task_ref: first
target: Deliver the first task
proof_policy: seam-test-first
scope: [src/first.rs]
```

### task: second
```yaml
task_ref: second
target: Deliver the second task
proof_policy: seam-test-first
scope: [src/second.rs]
depends_on: [first]
```
