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
Use only tools exposed by the active profile. Use the harness's native
patch/edit tool for changes.
Do not enable extra tools or change permissions merely to follow a recipe.

## Read And Verify

- Use `map` or `signatures` for orientation, `lines:N-M` for a known region,
  and `full` for exact source or edit preparation.
- If a cached response omits source needed for verification, request a fresh
  read. A signature or cache receipt is not proof of a function's behavior.
- Retrieve needed archive sections with `ctx_expand`; do not rerun a command
  merely because its output was compressed.
- Follow the host's process-management rules for long commands. Retain the
  actual terminal exit code; a detached job or progress message is not success.

## Boundaries

Respect path, shell, and approval restrictions. For an authorized task outside
lean-ctx's supported scope, use a permitted native tool; do not bypass a denial
or expand access automatically. Verify the working directory before commands
that change files; stop if the requested directory was rejected or replaced.

Compression does not prescribe response length, jargon, or a separate workflow.
