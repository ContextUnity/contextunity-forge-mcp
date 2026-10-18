---
title: "Framework manifests"
doc_type: guide
---

# Framework manifests

Framework manifests provide declarative rules to existing language extraction and linking hooks. They describe framework-specific receivers, builtins, template filters, routes, and dependency-injection calls. A manifest does not install a dependency, add a parser, or register a new programming language.

## Bundled manifests

Bundled manifest tables live in `src/engine/languages/manifests/` as TOML files. The build embeds these files in the Forge binary with `include_str!`; when the dependency registry is constructed, Forge parses the embedded text into typed `FrameworkManifest` values. The bundled identifiers are `alpinejs`, `django`, `fastapi`, `gridviewspec`, `jinja2`, `nestjs`, `nuxt`, `pydantic`, and `unittest`. The `gridviewspec` table recognizes the declared Python distribution `grid-view-spec` and maps the verified `GridViewSpec` receiver to `grid_view_spec.types.spec.GridViewSpec`.

To ship another bundled manifest with Forge, add a TOML file in that directory and register its identifier in `load_standard_framework_manifests`. Rebuild the Forge binary to include the new table. Typed sections execute through the existing extraction hooks; legacy string rules need a caller in the relevant `LanguageLinker`. Adding a table does not introduce new extraction semantics.

The GridViewSpec hierarchy rules describe the verified public API of the specification, metadata, configuration, layout, block types, nested column/action/filter/card types, and Django host adapter. Concrete block rules include inherited `GridViewBlockBase` fields; dataclass fields support member reads, while the host adapter also exposes callable methods. Unknown members remain unresolved. See the [GridViewSpec example](../../skills/contextunity-forge/references/frameworks/gridviewspec.toml.example).

In the verified package API, `GridViewAction` is a union alias. The manifest registers its real `GridViewActionBase` and export, overlay, link, button, select, and menu action classes with their respective members.

## Project manifests

A project can provide or override framework manifest data under its workspace root:

```text
<workspace>/.forge/frameworks/<framework-id>.toml
<workspace>/.forge/frameworks/<framework-id>.yaml
<workspace>/.forge/frameworks/<framework-id>.yml
```

Forge uses the file stem as the framework identifier. Put the manifest in each workspace that needs it; linked workspaces have their own `.forge/frameworks/` directory. A project manifest activates for its workspace even when the package is not declared. Legacy bundled linker rules require a declared package in the source file's dependency scope. Typed extraction rules are available without a package declaration; their syntax and canonical import checks determine whether a rule applies. This preserves import-proven framework behavior in projects without package metadata.

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

These examples show the legacy string rules, not a complete Django rule set. Receiver and route strings use the portion before `=` as their source identity. The loader retains other values in those four arrays, including nested tables, without executing them generically. For the exact legacy values a linker consumes, use the bundled files as reference and consult [Engine modularity](../architecture/modularity.md).

Project manifests are read and parsed while Forge collects the dependency registry for an index build or update. They are not compiled into the Forge binary. After adding or editing a manifest, rebuild or update the workspace index so its input digest and linker results include the change.

## Typed extraction rules

Four optional typed sections execute through the existing language hooks:

| Section | Required fields | Meaning |
| --- | --- | --- |
| `class_hierarchy_receivers` | `bases`, `provider`, `members` | Recognize canonical imported base classes and expose the listed external members through verified inheritance. |
| `route_decorators` | `callee`, `method`, `path_argument` | Extract a route from a decorator and its zero-based path argument. |
| `route_calls` | `callee`, `method`, `path_argument` | Extract a route from a call; optional `handler_argument` selects the first zero-based handler position, including subsequent positional middleware handlers. |
| `di_calls` | `callee`, `argument` | Extract a dependency reference from the selected zero-based call argument. |

A callee matches its qualified name; a leading dot selects a qualified member suffix, as in `.get`. Route rules optionally accept `receiver_names`, a list of admissible receiver basenames; its default empty list adds no receiver restriction. For restricted suffix rules, a syntactically recognized `APIRouter` binding is also admissible. Route paths must begin with `/` or `^` unless `allow_relative_path = true`; this flag defaults to false. Django rules enable it for empty and relative URL patterns. These checks keep ordinary mapping calls from becoming routes.

Route methods use uppercase letters, digits, or underscores; `ANY` represents an unspecified method. Static route extraction combines FastAPI `APIRouter` and NestJS controller prefixes. Django include facts retain route names and namespaces and compose statically known nested URL prefixes before persistence. Dynamic paths retain their uncertainty.

Omitting `handler_argument` still extracts a route from a matching call; it creates no `handles` edge. When an index is configured, a call without any valid positional handlers from that index does not register a route. Composed Django routes retain the included module's handler lookup context while their `handles` edges originate at the composed route.

The complete [TOML example](../../skills/contextunity-forge/references/frameworks/declarative-rules.toml.example) and equivalent [YAML example](../../skills/contextunity-forge/references/frameworks/declarative-rules.yaml.example) include all four sections and a `semantic_rules` imported-call example. Copy one into `.forge/frameworks/django.toml` or `.forge/frameworks/django.yaml` to extend that bundle. Custom file stems work through the same typed hooks.

Project typed rules take precedence over bundled rules with the same identity: ordered `bases` for class rules, `callee` plus `method` for route rules, and `callee` for DI rules. `replace = true` replaces both legacy and typed sections of the bundle. Unknown fields in typed rules fail validation. Bases, providers, members, callees, and methods must be nonempty; callees cannot contain whitespace. Argument positions are nonnegative integers, and a route's handler argument must differ from its path argument.

## Connected workspace rollout

Before validating a connected-workspace adapter for this integration, check for and remove any obsolete `pim` linked-workspace entry if present. This is a rollout prerequisite; it does not assume that a live PIM checkout or entry exists.

## Schema and validation

Every manifest must contain all four top-level arrays: `receivers`, `builtins`, `filters`, and `routes`. Optional `extend` and `replace` fields must be booleans and cannot both be true. The loader deserializes TOML and YAML into the same typed `FrameworkManifest`, rejects unknown top-level fields, and reports malformed syntax or values as build errors. TOML duplicate keys, duplicate tables, and key/table collisions fail through the TOML parser. A workspace may use only one file format for a given identifier; duplicate stems across files are rejected.

An invalid or unreadable manifest aborts the build before index publication. Forge also rejects a symlinked `.forge` directory, a symlinked `frameworks` directory or manifest file, and files larger than 4 MiB. If there is no `.forge/frameworks/` directory, the workspace has no project manifests and uses eligible bundled tables.

## Compiled rules and coverage diagnostics

Forge compiles effective typed AST rules into grammar-specific `RuleIR` candidate tables. Extraction selects candidates with the parsed node's `kind_id`; the existing language hooks still prove the callee, literal arguments, lexical owner and target. Class-hierarchy receivers retain their borrowed linker rule lookup. Matching a grammar kind alone does not establish a semantic binding.

The cache belongs to a dependency-registry generation and distinguishes the actual Tree-sitter grammar, effective rule data and workspace. TypeScript and TSX cannot share a table merely because their profile identifier matches. Project merge and replacement happen before compilation. Manifest changes recollect the registry and invalidate its compiled results; semantic activation retains the source's project scope.

Full build and delta reports contain `framework_rule_diagnostics`, including an empty list for clean coverage. Each diagnostic contains `framework`, `workspace`, `language`, `rule_domain`, `code` and `removed_eligible_rule_identities`. The code `eligible_rule_identity_removed` reports bundled identities absent from the effective rules for the applicable grammar. Equal counts can hide a removed identity. An addition or a changed payload under the same canonical identity does not itself remove that identity.

Diagnostic groups and identity lists are sorted and deduplicated. Rule domains use the existing section names. Class identities contain ordered `bases`; route identities contain `callee` and `method`; DI identities contain `callee`; semantic identities contain `id`. Ordinary delta checks the complete current source inventory, including unchanged languages. Manifest-change rebuilds recompute the report. These values are ephemeral: they do not become syntax errors, persisted records or commitment inputs.

## Extending language support

Typed project sections can add class receivers, route decorators, route calls, and DI calls without Rust changes when the existing language hooks support that syntax. Legacy string rules still require a caller in the relevant `LanguageLinker`. New extraction semantics require code and writer-seam tests. To add a new programming language, implement and register a `LanguageProfile` with its grammar and extraction behavior, add its Cargo `lang-*` feature and parser dependency as needed, and provide a language linker when it needs custom resolution. The build script generates the compiled profile registry from enabled features. These code changes require a new Forge build; a project manifest cannot add them at runtime.

## Language registry boundaries

`DependencyRegistry` lives in `src/engine/languages/dependency_registry.rs` and collects workspace dependencies together with framework rules. JavaScript package metadata and TypeScript path mappings use separate registries under `src/engine/languages/registries/`; Python package declarations are extracted in `src/engine/languages/python/dependencies.rs`. `LanguageProfile::manifest_filenames` names external package metadata inputs such as `package.json`, `pyproject.toml`, and `Cargo.toml`; it does not name framework rule files.

See [ADR 0013](../adr/0013-data-driven-framework-manifests.md) for the linker and grammar boundaries and [ADR 0017](../adr/0017-serde-framework-manifest-formats.md) for the accepted file formats and parser contract.

## Imported calls to actual files

`semantic_rules` adds a bounded two-phase relation to the existing language owners. The Python parser proves a direct imported alias and a static positional literal; the shared linker joins it to one actual HTML module in the nearest Python project. The target projection matches the complete directory prefix plus literal path at a path-segment boundary. Multiple distinct targets across all matching rules and namespaces remain ambiguous. No basename fallback or synthetic target is used.

This complete TOML pack can be saved under any `.forge/frameworks/<name>.toml` filename:

```toml
receivers = []
builtins = []
filters = []
routes = []

[[semantic_rules]]
id = "render-template"
namespace = "templates"
scope = "project"
activation = [{ kind = "package", name = "django" }]
operation = { kind = "call_to_file", language = "python", callee = { kind = "imported", module = "django.shortcuts", export = "render" }, argument = 1, target = { kind = "project_path_suffix", prefix = "templates/", language = "html" }, relation = "renders_template" }
```

The equivalent complete YAML pack is accepted with `.yaml` or `.yml`:

```yaml
receivers: []
builtins: []
filters: []
routes: []
semantic_rules:
  - id: render-template
    namespace: templates
    scope: project
    activation:
      - kind: package
        name: django
    operation:
      kind: call_to_file
      language: python
      callee: {kind: imported, module: django.shortcuts, export: render}
      argument: 1
      target: {kind: project_path_suffix, prefix: templates/, language: html}
      relation: renders_template
```

`argument` is a zero-based positional index. Currently the closed operation accepts Python direct from-import callees and HTML file targets with `renders_template`. API module/export, index, namespace and directory prefix are data; a new pack needs no Rust registration. Unknown fields, unsupported languages/operations/relations, duplicate rule IDs and invalid path prefixes fail manifest loading with a file diagnostic. An invalid project override never falls back to bundled rules.

Activation is either empty or a list of package alternatives declared in the nearest Python project. Empty activation requires the same exact imported-callee proof. Package activation controls both caller and file-provider facts. Project rules replace bundled rules with the same ID; other bundled IDs remain unless `replace = true` replaces the whole pack. An empty replacement disables its semantic rules and activation.

Only already-normal relative literal paths are accepted: no absolute path, traversal, empty segment, escape-dependent or interpolated literal. Keyword arguments, argument expansion and unsupported binding contexts remain without semantic facts. A same-spelling local module cannot establish external API provenance. Facts retain the original call occurrence and actual target module; edits, deletions, renames and manifest changes invalidate both sides through their shared typed dependency identity.
