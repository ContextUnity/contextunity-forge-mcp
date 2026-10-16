---
title: "Framework manifests"
doc_type: guide
---

# Framework manifests

Framework manifests provide rules to an existing language linker. They keep framework-specific receivers, builtins, template filters, and route rules out of language grammar and general AST extraction. A manifest does not install a dependency, add a parser, or register a new programming language.

## Bundled manifests

Bundled manifest tables live in `src/engine/languages/manifests/` as TOML files. The build embeds these files in the Forge binary with `include_str!`; when the dependency registry is constructed, Forge parses the embedded text into typed `FrameworkManifest` values. The bundled identifiers are `alpinejs`, `django`, `gridviewspec`, `jinja2`, and `nuxt`. The `gridviewspec` table recognizes the declared Python distribution `grid-view-spec` and maps the verified `GridViewSpec` receiver to `grid_view_spec.types.spec.GridViewSpec`.

To ship another bundled manifest with Forge, add a TOML file in that directory and register its identifier in `load_standard_framework_manifests`. Rebuild the Forge binary to include the new table. The table still needs a caller in the relevant `LanguageLinker`; adding a file alone does not teach a linker new semantics.

## Project manifests

A project can provide or override framework manifest data under its workspace root:

```text
<workspace>/.forge/frameworks/<framework-id>.toml
<workspace>/.forge/frameworks/<framework-id>.yaml
<workspace>/.forge/frameworks/<framework-id>.yml
```

Forge uses the file stem as the framework identifier. Put the manifest in each workspace that needs it; linked workspaces have their own `.forge/frameworks/` directory. A project manifest activates for its workspace even when the package is not declared. A bundled manifest activates only when its package is declared in that workspace's recognized package/dependency manifest. Forge scopes both the local manifest and declaration lookup to the source file's workspace before asking the language linker for rules.

Use the extension and values expected by the existing linker. For example, a project that wants to add Django rules can create `.forge/frameworks/django.toml`:

```toml
receivers = ["template_context=django.shortcuts.render"]
builtins = ["template.tag.csrf_token"]
filters = ["template.filter.safe"]
routes = ["template.tag.static=static"]
```

Project rules merge with the bundled table by default. `extend = true` selects the same merge behavior explicitly. Project rules appear first, so a project receiver or route with the same source key takes precedence; exact duplicate builtin and filter strings appear once. Use `replace = true` to replace the bundled table instead:

```toml
replace = true
receivers = ["template_context=django.shortcuts.render"]
builtins = ["template.tag.csrf_token"]
filters = ["template.filter.safe"]
routes = ["template.tag.static=static"]
```

Do not set both mode flags to true. All four rule arrays remain required in every mode.

The equivalent YAML shape is:

```yaml
receivers:
  - template_context=django.shortcuts.render
builtins:
  - template.tag.csrf_token
filters:
  - template.filter.safe
routes:
  - template.tag.static=static
```

These examples show the schema, not a complete Django rule set. Receiver and route strings use the portion before `=` as their source identity. The loader retains other typed array values, including nested tables, without interpreting them. Linker execution remains limited to the existing string rules; richer receiver and route execution is deferred to milestone 021. For the exact values a linker consumes, use the bundled files as reference and consult [Engine modularity](../architecture/modularity.md).

Project manifests are read and parsed while Forge collects the dependency registry for an index build or update. They are not compiled into the Forge binary. After adding or editing a manifest, rebuild or update the workspace index so its input digest and linker results include the change.

## Connected workspace rollout

Before validating a connected-workspace adapter for this integration, check for and remove any obsolete `pim` linked-workspace entry if present. This is a rollout prerequisite; it does not assume that a live PIM checkout or entry exists.

## Schema and validation

Every manifest must contain all four top-level arrays: `receivers`, `builtins`, `filters`, and `routes`. Optional `extend` and `replace` fields must be booleans and cannot both be true. The loader deserializes TOML and YAML into the same typed `FrameworkManifest`, rejects unknown top-level fields, and reports malformed syntax or values as build errors. TOML duplicate keys, duplicate tables, and key/table collisions fail through the TOML parser. A workspace may use only one file format for a given identifier; duplicate stems across files are rejected.

An invalid or unreadable manifest aborts the build before index publication. Forge also rejects a symlinked `.forge` directory, a symlinked `frameworks` directory or manifest file, and files larger than 4 MiB. If there is no `.forge/frameworks/` directory, the workspace has no project manifests and uses eligible bundled tables.

## Extending language support

Project manifests can supply data only to framework identifiers and rule values that an existing linker requests. To add a new framework integration, wire its identifier and rule lookups into the appropriate `LanguageLinker`; register its package mapping when a bundled table should activate from a declared dependency. Then add writer-seam tests. To add a new programming language, implement and register a `LanguageProfile` with its grammar and extraction behavior, add its Cargo `lang-*` feature and parser dependency as needed, and provide a language linker when it needs custom resolution. The build script generates the compiled profile registry from enabled features. These code changes require a new Forge build; a project manifest cannot add them at runtime.

## Language registry boundaries

`DependencyRegistry` lives in `src/engine/languages/dependency_registry.rs` and collects workspace dependencies together with framework rules. JavaScript package metadata and TypeScript path mappings use separate registries under `src/engine/languages/registries/`; Python package declarations are extracted in `src/engine/languages/python/dependencies.rs`. `LanguageProfile::manifest_filenames` names external package metadata inputs such as `package.json`, `pyproject.toml`, and `Cargo.toml`; it does not name framework rule files.

See [ADR 0013](../adr/0013-data-driven-framework-manifests.md) for the linker and grammar boundaries and [ADR 0017](../adr/0017-serde-framework-manifest-formats.md) for the accepted file formats and parser contract.
