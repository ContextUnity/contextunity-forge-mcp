---
title: "Verify workspace framework rules"
doc_type: guide
---

# Verify workspace framework rules

Use this procedure with the [manifest schema](../reference/framework-manifests.md)
and the actual APIs in the indexed project. A manifest supplies data for existing
operations; it does not add a parser or execute framework code.

## Extend or disable a pack

1. Copy a verified example into the workspace's `.forge/frameworks/` directory,
   retaining the intended filename stem. Use `vue-runtime`, not `vue`, for the
   bundled Vue rules. Add only supported typed rule sections.
2. Default merging retains other bundled rules and replaces matching identities
   with local entries. Use `replace = true` only for an intentional full pack
   replacement. To disable an effective pack, add `enabled = false` to a valid
   manifest; its rules may remain nonempty. Removing that file restores
   ordinary bundled eligibility. Linked workspaces have independent overrides.
3. Refresh the index with the ordinary CLI `build` or `delta` command, or let MCP
   freshness admission refresh it. Invalid manifests abort publication. Inspect
   `framework_rule_diagnostics` in the build/update report for removed eligible
   identities; equal rule counts do not prove equivalent rules.

## Inspect the actual result

Call `code_map_overview` to verify the workspace and generation. Search the real
consumer and provider with `code_map_search`, then use each returned selector in
`code_map_inspect` and `code_map_explain`. Check the exact destination declaration
or route, relation kind and source location. Use bounded read-only SQL through
`code_map_query` when persisted coverage or exact occurrence identities are needed;
the [MCP reference](../reference/mcp-tools.md) owns request shapes.

Test both eligibility and causality in the admitted production seam: a unique
static target, missing and duplicate targets, dynamic inputs and lexical shadows;
then rule removal, replacement, disable/restore and provider edits. Check ordinary
calls and imports survive. Warm-edit a provider while leaving its consumer
unchanged, reopen the database and inspect targets and coverage. Compare an
equal-final cold build with delta facts and commitments. External classification
is not a local resolved edge, and a higher aggregate percentage cannot establish
an individual exact target.

## Report a real-control comparison

Retain the baseline database and bind its SHA-256 to the source inventory, adapter
and binary used to create it. Compare a candidate on the same source bytes and
configuration. Report each control repository separately with complete resolved,
ambiguous, unresolved and external counts (R/A/U/E), exact occurrence identities
and canonical edge changes. A source-context cohort can overlap another cohort;
it does not by itself attribute a change to one rule. Record rule attribution and
activation-only effects separately, and use N/A when the denominator is zero.

Keep the original raw policy exit and precision exceptions. E is not R; a local
runtime override is not proof of embedded-default behavior. In particular, an
explicit isolated-copy Alpine activation is a separate configuration effect.
Use the final bound measurement report for current results rather than recycling
an earlier iteration's numbers. Coverage observations make no timing, memory or
throughput promise, and this workflow adds no benchmark command or numeric target.
