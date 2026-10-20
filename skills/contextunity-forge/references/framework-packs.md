# Declarative framework packs and language support

## Framework packs

Store project rules under `.forge/frameworks/<framework-id>.toml`, `.yaml`, or `.yml`. Use the [TOML](frameworks/declarative-rules.toml.example) or [YAML](frameworks/declarative-rules.yaml.example) examples for class receivers, route decorators/calls, dependency-injection calls, and imports of actual template files.

Keep the four legacy arrays `receivers`, `builtins`, `filters`, and `routes`, even when empty. Rules extend the bundled identifier by default; `replace = true` replaces it. Rebuild or update the index after editing.

The Forge repository's [framework manifest reference](https://github.com/ContextUnity/contextunity-forge-mcp/blob/main/docs/reference/framework-manifests.md) owns schema, merge identities, activation, and validation limits. Use manifests only for data supported by existing language hooks. A new language or extraction behavior needs profile code and production-seam proof.

After replacing bundled rules, inspect `framework_rule_diagnostics` in build and delta reports. Compare removed eligible identities, not rule counts; no removals means an empty list. Diagnostics are ephemeral, not parser errors or persisted facts. Effective typed rules compile against the actual Tree-sitter grammar and invalidate with registry changes.

## Structural language support

Provide all six `SyntaxGrammarSpec` categories and use the shared `SinglePassExtractor` enter/exit owner frames. Keep language-specific body events and linker semantics in their existing owners. Preserve public facts and commitment domains against the delivered baseline; do not add a recursive body collector. Tree-sitter query experiments are opt-in research and stay outside default indexing.
