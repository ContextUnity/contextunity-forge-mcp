---
title: "Framework pack authoring procedure"
doc_type: guide
---

# Framework pack authoring procedure

Read the [canonical schema](https://github.com/ContextUnity/contextunity-forge-mcp/blob/main/docs/reference/framework-manifests.md)
and the task's admitted scope before changing a pack. Keep framework spellings in
data; new grammar or semantics require their owning code and production-seam proof.

1. Choose the filename stem as the pack ID. Embedded manifests are inventoried
   automatically from the package manifest directory; project files are discovered
   under each workspace's `.forge/frameworks/`. Keep `receivers`, `builtins`,
   `filters` and `routes` in every complete manifest.
2. Start with the equivalent [TOML](frameworks/declarative-rules.toml.example) or
   [YAML](frameworks/declarative-rules.yaml.example) example. Use the existing
   typed operation and actual API identities. Default merging gives local rules
   precedence; `replace = true` replaces the whole pack. A valid manifest with
   `enabled = false` disables the complete effective same-ID pack even when its
   rules are nonempty. Removing that local file restores ordinary bundled eligibility.
3. For setup factories, select `factory_members` with `shape = "setup_return"`.
   Keep option-only state/action fields empty. A global API also needs
   `configured_module`, declared packages, exact persisted module configuration
   and lexical proof. Configure module `exports` using indexed named exports and
   supported directory/extension data; never derive a factory name from a filename.
   Allow initialization before the sole final setup return only within the
   supported non-returning `if`/`try` contour; preserve abrupt-exit, alternate-return
   and unsupported-write guards rather than treating it as general control flow.
4. Verify each returned callable against an actual indexed declaration and source
   position. Preserve public aliases only when the RHS proves that target. Keep
   non-node state and uncertain members unresolved. For module `call_results`,
   preserve the destructured local binding's external provenance with no local
   node or exact edge. Keep parameters, shadowed names, writes and ambiguous
   providers nonexact. Component enablement is independent of these module contracts.
5. Validate both formats through the production manifest loader and the existing
   writer/reopened-reader seam. Inspect effective rule identities, persisted
   R/A/U/E coverage and exact target IDs; external classification is not a local
   resolved edge. Check rule removal, activation, replacement and unrelated names.
6. Warm-edit actual provider source and effective TOML while leaving consumers
   unchanged. Verify existing changed-provider selection, cached hydration and
   SQL outcomes, including missing-provider addition and config-only removal.
   Compare equal-final cold/delta commitments. Preserve canonical facts and cache
   identity; add no source recovery, second provider mechanism or new AST pass.

Use `call_reference.provider_scopes` only for explicit enabled workspaces and
already collected project/own-package identities. Keep installation aliases in
local manifests and preserve local-plus-configured ambiguity. For template
registries, pair Python `assignment_definition` imported receiver proof and a
provider-relative `template_root` with HTML `template_reference` in the same
domain; preserve unknown receiver/target evidence and nearest-scope isolation.

Use HTTP-client `target_context` only as a source-verified application assertion
with exact request-owner and route-owner paths. Preserve default matching outside
the request set, and unresolved results without global fallback when an applicable
configured provider is missing. Verify configuration-only and provider-only edits
through the same selective hydration path. Compare real controls by retained
source/configuration/binary/DB identities and report R/A/U/E separately; external
classification is not an exact local edge or proof of a coverage threshold.
