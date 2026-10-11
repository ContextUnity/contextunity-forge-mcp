---
name: lean-ctx
description: Read source, run shell commands, and inspect compressed output or diffs.
---

# lean-ctx

Use available lean-ctx tools when their filtering or compression helps the task.
The active tool registry and schemas determine what can be called.

## Available Profile

| Intent | Tool |
|---|---|
| Read source | `ctx_read` |
| Run a command with compressed output | `ctx_shell` |
| Inspect directory structure | `ctx_tree` |
| Retrieve archived output | `ctx_expand` |
| Inspect edits since an earlier read | `ctx_delta` |

Follow the active agent instructions for repository discovery.
Use only tools exposed by the active profile. Use native tools for file edits.

## Read And Verify

- Use `map` or `signatures` for orientation, `lines:N-M` for a known region,
  and `full` for exact source or edit preparation.
- If a cached response omits source needed for verification, request a fresh read.
- Retrieve needed archive sections with `ctx_expand`; do not rerun a command
  merely because its output was compressed.
