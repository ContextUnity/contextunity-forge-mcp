---
id: m-branching
title: Branching milestone
doc_type: contract
status: active
started_at: 2026-10-01T10:00:00Z
---
# Branching milestone

### task: first
```yaml
task_ref: first
target: Prepare the shared foundation
proof_policy: seam-test-first
scope: [src/first.rs]
```

### task: second
```yaml
task_ref: second
target: Deliver the first branch
proof_policy: seam-test-first
scope: [src/second.rs]
depends_on: [first]
```

### task: third
```yaml
task_ref: third
target: Deliver the second branch
proof_policy: seam-test-first
scope: [src/third.rs]
depends_on: [first]
```

### task: final
```yaml
task_ref: final
target: Integrate both branches
proof_policy: seam-test-first
scope: [src/final.rs]
depends_on: [second, third]
```
