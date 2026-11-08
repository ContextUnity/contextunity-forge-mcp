---
id: m-defects
title: Defects test
doc_type: contract
deferred_defects:
  - id: DEFECT-FRONTMATTER
    source: task_blackboard
    title: Frontmatter defect stays typed
    path: src/engine/tasks.rs
    disposition: subsequent_milestone
    notes: Imported from contract metadata
---
# Defects test

### task: task-a
```yaml
task_ref: task-a
target: Task A target
proof_policy: direct-proof
scope: [src/]
```

## Deferred and out-of-scope defects

```yaml
deferred_defects:
  - id: DEFECT-001
    source: review_findings
    title: Unshadowed function resolution bug in edge case
    path: src/engine/languages/python.rs
    disposition: deferred
    notes: Follow up in next milestone
  - id: DEFECT-002
    source: task_blackboard
    title: Minor syntax warning
```
