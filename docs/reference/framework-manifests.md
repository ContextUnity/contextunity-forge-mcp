---
title: "Framework manifests"
doc_type: guide
---

# Framework manifests

Framework manifests provide declarative rules to existing language extraction and linking hooks. They describe framework identities and data consumed by finite typed operations. A manifest does not install a dependency, add a parser, or register a new programming language.

## Bundled manifests

Bundled manifest tables live in `src/engine/languages/manifests/`. The build automatically embeds regular TOML, YAML and YML files from that directory in a sorted inventory, using each filename stem as its identifier. Duplicate identifiers and invalid UTF-8 identities fail the build. When the dependency registry is constructed, Forge parses the embedded text into typed `FrameworkManifest` values. The `gridviewspec` table recognizes the declared Python distribution `grid-view-spec` and maps the verified `GridViewSpec` receiver to `grid_view_spec.types.spec.GridViewSpec`.

To ship another bundled manifest with Forge, add a supported file in that directory and rebuild the binary. No Rust catalogue entry is needed. Cargo tracks the directory and individual files; relative filenames and file bytes contribute to the existing language-profile fingerprint, so additions, removals, renames and edits invalidate cached extraction. Typed sections execute through the existing extraction hooks. Adding a table does not introduce new extraction semantics.

The GridViewSpec hierarchy rules describe the verified public API of the specification, metadata, configuration, layout, block types, nested column/action/filter/card types, and Django host adapter. Concrete block rules include inherited `GridViewBlockBase` fields; dataclass fields support member reads, while the host adapter also exposes callable methods. Unknown members remain unresolved. See the [GridViewSpec example](../../skills/contextunity-forge/references/frameworks/gridviewspec.toml.example).

In the verified package API, `GridViewAction` is a union alias. The manifest registers its real `GridViewActionBase` and export, overlay, link, button, select, and menu action classes with their respective members.

The FastAPI pack links an imported `Depends` call to a unique indexed dependency
provider. Its HTTP decorator rules attach literal `response_model` symbols to
the decorated endpoint with `uses_model`; `exception_handler` and
`add_exception_handler` attach the actual handler to a unique indexed exception
class with `references`. These rules require a verified `FastAPI` or `APIRouter`
receiver and the declared `fastapi` package for bundled activation. Missing,
duplicate, dynamic, and shadowed identities produce no exact edge; missing and
duplicate targets retain unresolved and ambiguous coverage. Project overrides
can remove or rename these rules through the existing replacement mechanism.

The Starlette pack configures `Route`, `Mount` and `WebSocketRoute` registrations.
Literal `Route(methods=[...])` values select HTTP methods; handlerless mounts do
not create a `handles` edge. Response subclasses expose only their configured
external members, and `BaseHTTPMiddleware` exposes its configured constructor.
The Django pack also supplies verified Form/ModelForm inheritance members; it
does not infer an arbitrary object's type from a method such as `save`.

Annotated partial extensions are available for
[Starlette](../../skills/contextunity-forge/references/frameworks/starlette.toml.example),
[FastAPI](../../skills/contextunity-forge/references/frameworks/fastapi.toml.example),
[Nuxt](../../skills/contextunity-forge/references/frameworks/nuxt.toml.example),
[Vue/Pinia](../../skills/contextunity-forge/references/frameworks/vue-runtime.toml.example)
and [HTMX](../../skills/contextunity-forge/references/frameworks/htmx.toml.example).
Their canonical filenames extend the corresponding bundle; they are not full
replacement packs. See the [workspace verification runbook](../runbooks/framework-packs.md).
The existing [Django](../../skills/contextunity-forge/references/frameworks/django.toml.example),
[Alpine](../../skills/contextunity-forge/references/frameworks/alpinejs.toml.example)
and [GridViewSpec](../../skills/contextunity-forge/references/frameworks/gridviewspec.toml.example)
examples complete the eight-pack set. The generic TOML/YAML examples below show
illustrative APIs; the concrete pack examples use verified bundled identities.

## Project manifests

A project can provide or override framework manifest data under its workspace root:

```text
<workspace>/.forge/frameworks/<framework-id>.toml
<workspace>/.forge/frameworks/<framework-id>.yaml
<workspace>/.forge/frameworks/<framework-id>.yml
```

Forge uses the file stem as the framework identifier. Put the manifest in each workspace that needs it; linked workspaces have their own `.forge/frameworks/` directory. A project manifest activates for its workspace even when the package is not declared. Typed extraction rules use their declared activation, syntax, and canonical import checks to determine whether a rule applies.

Use the typed operations supported by the current language hooks. For example, a project can declare template symbols without framework-specific top-level lists:

```toml
[[syntax_rules]]
id = "django-template-symbols"
language = "html"
package_family = "python"
activation = [{ kind = "package", name = "django" }]
operation = { kind = "template_symbols", requires_loader = false, tags = ["csrf_token", "url", "load"], filters = ["safe"], libraries = ["static"], library_routes = { static = "static" } }
```

Project rules merge with the bundled table by default. `extend = true` selects the same merge behavior explicitly. Project typed rules take precedence over bundled rules with the same identity. Use `replace = true` to use only the project rules:

```toml
replace = true
[[syntax_rules]]
id = "django-template-symbols"
language = "html"
package_family = "python"
activation = [{ kind = "package", name = "django" }]
operation = { kind = "template_symbols", requires_loader = false, tags = ["csrf_token", "url", "load"], filters = ["safe"], libraries = ["static"], library_routes = { static = "static" } }
```

TOML, YAML and YML all deserialize into the same typed manifest. Unknown top-level fields, including the removed `receivers`, `builtins`, `filters`, and `routes` lists, fail validation. Use the bundled files and the generic [TOML](../../skills/contextunity-forge/references/frameworks/declarative-rules.toml.example) and [YAML](../../skills/contextunity-forge/references/frameworks/declarative-rules.yaml.example) examples as references.

Project manifests are read and parsed while Forge collects the dependency registry for an index build or update. They are not compiled into the Forge binary. After adding or editing a manifest, rebuild or update the workspace index so its input digest and linker results include the change.

## Dependency and configuration inputs

Package activation uses declarations parsed from admitted metadata, scoped to the
source file's project. It does not scan installed packages. The existing profiles
collect the following metadata inputs:

| Profile | Package declarations and locks | Other collected configuration |
| --- | --- | --- |
| Python | `pyproject.toml`, `requirements*.txt`, `setup.cfg`, `Pipfile` | None in this input list. |
| JavaScript/TypeScript | `package.json`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock` | `pnpm-workspace.yaml` and `package.json` workspace/export metadata; `tsconfig*.json` path aliases. |
| Rust | `Cargo.toml` | Own-library identity remains manifest scoped. |
| Go | `go.mod` | Enabled profile required. |
| Java/Kotlin | `pom.xml`, `build.gradle`, `build.gradle.kts` | Enabled profile required. |

Vue delegates metadata discovery to TypeScript; HTML delegates to JavaScript.
Workspace patterns and TypeScript alias files do not themselves declare a
framework dependency. A collected filename does not imply support for every
package-manager expression. Use the actual parsed declaration and nearest scope;
nested and linked projects cannot borrow another project's package activation.

Runtime rules are discovered only under each workspace's `.forge/frameworks/`.
Adding a new filename there requires an index refresh, without a Rust edit or
binary rebuild. Adding or changing an embedded pack under the package's manifest
directory requires rebuilding the binary; automatic embedding does not make an
already installed binary read that source directory at runtime.

## Typed extraction rules

Six optional typed sections execute through the existing language hooks:

| Section | Required fields | Meaning |
| --- | --- | --- |
| `class_hierarchy_receivers` | `bases`, `provider`, `members` | Recognize canonical imported base classes and expose the listed external members through verified inheritance. |
| `route_decorators` | `callee`, `method`, `path_argument` | Extract a route from a decorator and its zero-based path argument. |
| `route_calls` | `callee`, `method`, `path_argument` | Extract a route from a call; optional `handler_argument` selects the first zero-based handler position, including subsequent positional middleware handlers. |
| `di_calls` | `callee`, `argument` | Extract a dependency reference from the selected zero-based call argument. |
| `semantic_rules` | `id`, `namespace`, `scope`, `operation` | Project an exact imported call or class attribute to an actual file or symbol. |
| `syntax_rules` | `id`, `language`, `operation` | Apply one closed grammar operation with data-defined API, keyword, attribute, or directory identities. |

Python hierarchy rows optionally declare `returns = { member = { kind = "self" } }` or `{ kind = "receiver", base = "vendor.records.Selection" }`. Keys must be declared complete member paths; the explicit base identifies one effective row in the same pack through its ordered `bases` identity. Absence declares no result. `self` preserves a proven local subclass and overrides; an explicit result retains finite rule/import authority. Local overrides, unknown or conditional receivers and ambiguous targets cannot borrow a policy. Strict validation rejects unknown fields, invalid keys and targets, more than 256 policies per row or 128 policy-bearing effective rows. Bundled Django supplies `all`, `filter`, `exclude`, `order_by`, `select_related` and `prefetch_related` self policies to QuerySet, explicit QuerySet results to Manager, and the corresponding `objects.*` results to Model. Other declared members acquire no return policy implicitly.

JS/TS `receiver_type` rows optionally declare `argument_factories = [{ callee = { kind = "global_member", receiver = "document", member = "createElement" }, argument = 0, literals = ["input"] }]`. One clause per row selects the containing finite receiver authority; it does not invent a declaration. Only an unshadowed, unmodified configured global/member qualifies. Indices are 0–3, with at most 32 unique literals of 256 UTF-8 bytes per clause, 128 effective clauses and 4096 literal entries. Unknown/spread/fifth arguments, unsupported literal decoding and conflicting mappings cannot prove a subtype; independently proven generic receiver evidence remains usable. The bundled JS/TS rows expose input `value:string` and script `src:string` as properties. Direct and assigned property reads use the same bounded call evidence and retain the effective rule's provenance; property values can also supply ordinary string-member flow. Calling `value()` or `src()` does not turn a property into a method. These additions do not activate literal-factory inference for Vue or HTML.

A callee matches its qualified name; a leading dot selects a qualified member suffix, as in `.get`. Route rules optionally accept `receiver_names`, a list of admissible receiver basenames; its default empty list adds no receiver restriction. For restricted suffix rules, a receiver proven by the rule's configured constructor prefix provider is also admissible. Route paths must begin with `/` or `^` unless `allow_relative_path = true`; this flag defaults to false. Django rules enable it for empty and relative URL patterns. These checks keep ordinary mapping calls from becoming routes.

Route methods use uppercase letters, digits, or underscores; `ANY` represents an unspecified method. Route `prefixes` select constructor, class-decorator, and receiver-attachment operations. Their callee, keyword, child argument, and decorator argument are data. Static attachment chains compose their prefixes; dynamic prefixes, cycles, repeated attachments, and rewritten constructor bindings cannot establish an exact route. `methods_keyword` selects a literal method-list keyword. A registration can supply `name_keyword`, an `include` record (`callee`, `argument`, `namespace_keyword`), and a `registration_guard` naming a receiver-registration syntax rule in the same pack.

Omitting `handler_argument` still extracts a route from a matching call; it creates no `handles` edge. When an index is configured, a call without any valid positional handlers from that index does not register a route. Composed include routes retain the included module's handler lookup context while their `handles` edges originate at the composed route.

The complete [TOML example](../../skills/contextunity-forge/references/frameworks/declarative-rules.toml.example) and equivalent [YAML example](../../skills/contextunity-forge/references/frameworks/declarative-rules.yaml.example) include an imported-call semantic rule and configured setup/result operations. Copy one into `.forge/frameworks/project-rules.toml` or `.forge/frameworks/project-rules.yaml` and substitute verified package and API identities for the illustrative names. Use a bundled filename stem when intentionally extending that bundle.

Project typed rules take precedence over bundled rules with the same identity: ordered `bases` for class rules, `callee` plus `method` for route rules, and `callee` for DI rules. `replace = true` replaces the typed sections of the bundle. Unknown fields fail validation. Bases, providers, members, callees, and methods must be nonempty; callees cannot contain whitespace. Route argument positions are nonnegative integers below 32, and a route's handler argument must differ from its path argument. DI `argument` is a nonnegative positional index with no additional upper-bound validation. Syntax operations validate their own positional bounds.

## Connected workspace rollout

Before validating a connected-workspace adapter for this integration, check for and remove any obsolete `pim` linked-workspace entry if present. This is a rollout prerequisite; it does not assume that a live PIM checkout or entry exists.

## Schema and validation

Every manifest may omit rule sections and may provide any supported typed sections. Optional `extend` and `replace` fields must be booleans and cannot both be true. Optional `enabled` is strictly boolean and defaults to `true`. Set `enabled = false` in a workspace-local manifest to suppress the entire same-ID pack, including bundled rules and nonempty local typed rules. The disabled manifest remains a tombstone: merge or replacement cannot fall back to bundled defaults. Explicit `true` or removing the local file restores ordinary eligibility. Disabled manifests retain `enabled: false` when serialized; changes remain input-hashed and report removed eligible rule identities.

The loader deserializes TOML and YAML into the same typed `FrameworkManifest`, rejects unknown top-level fields, and reports malformed syntax or values as build errors. TOML duplicate keys, duplicate tables, and key/table collisions fail through the TOML parser. A workspace may use only one file format for a given identifier; duplicate stems across files are rejected.

An invalid or unreadable manifest aborts the build before index publication. Forge also rejects a symlinked `.forge` directory, a symlinked `frameworks` directory or manifest file, and files larger than 4 MiB. If there is no `.forge/frameworks/` directory, the workspace has no project manifests and uses eligible bundled tables.

## Compiled rules and coverage diagnostics

During dependency collection, Forge prepares immutable grammar-specific route and DI `RuleIR` tables once per workspace and shares them across the effective project-scope `Arc<LanguageContext>` values. Collected file and embedded-island extraction use these prepared contexts by lookup without recompiling rules or acquiring a rule-cache lock. Direct bundled/manual registry callers retain bounded thread-local context reuse, and custom grammars retain their compilation fallback. Extraction selects route and DI candidates with the parsed node's `kind_id`; the existing language hooks still prove the callee, literal arguments, lexical owner and target. Class-hierarchy receivers retain their borrowed linker rule lookup. Matching a grammar kind alone does not establish a semantic binding.

Preparation belongs to the effective dependency-registry generation and project scopes and distinguishes the actual Tree-sitter grammar. TypeScript and TSX cannot share a table merely because their profile identifier matches. Project merge, replacement and activation happen before preparation. Manifest changes recollect the registry and rebuild the prepared tables. JavaScript/TypeScript projection dispatch uses prepared callee/member/root candidate indices, preserving effective row order and distinct duplicate rules. Completed callback summaries are drained independently of whether a rule publishes a result. File-provider candidate indices select the target language; path and project checks remain authoritative.

Full build and delta reports contain `framework_rule_diagnostics`, including an empty list for clean coverage. Each diagnostic contains `framework`, `workspace`, `language`, `rule_domain`, `code` and `removed_eligible_rule_identities`. The code `eligible_rule_identity_removed` reports bundled identities absent from the effective rules for the applicable grammar. Equal counts can hide a removed identity. An addition or a changed payload under the same canonical identity does not itself remove that identity.

Diagnostic groups and identity lists are sorted and deduplicated. Rule domains use the existing section names. Class identities contain ordered `bases`; route identities contain `callee` and `method`; DI identities contain `callee`; semantic identities contain `id`. Ordinary delta checks the complete current source inventory, including unchanged languages. Manifest-change rebuilds recompute the report. These values are ephemeral: they do not become syntax errors, persisted records or commitment inputs.

## Extending language support

Typed project sections can add class receivers, route decorators, route calls, and DI calls without Rust changes when the existing language hooks support that syntax. New extraction semantics require code and writer-seam tests. To add a new programming language, implement and register a `LanguageProfile` with its grammar and extraction behavior, add its Cargo `lang-*` feature and parser dependency as needed, and provide a language linker when it needs custom resolution. The build script generates the compiled profile registry from enabled features. These code changes require a new Forge build; a project manifest cannot add them at runtime.

## Language registry boundaries

`DependencyRegistry` lives in `src/engine/languages/dependency_registry.rs` and collects workspace dependencies together with framework rules. JavaScript package metadata and TypeScript path mappings use separate registries under `src/engine/languages/registries/`; Python package declarations are extracted in `src/engine/languages/python/dependencies.rs`. `LanguageProfile::manifest_filenames` names external package metadata inputs such as `package.json`, `pyproject.toml`, and `Cargo.toml`; it does not name framework rule files.

See [ADR 0013](../adr/0013-data-driven-framework-manifests.md) for the linker and grammar boundaries and [ADR 0017](../adr/0017-serde-framework-manifest-formats.md) for the accepted file formats and parser contract.

## Imported calls to actual files

`semantic_rules` adds a bounded two-phase relation to the existing language owners. The Python parser proves a direct imported alias and a static positional literal; the shared linker joins it to one actual HTML module in the nearest Python project. The target projection matches the complete directory prefix plus literal path at a path-segment boundary. Multiple distinct targets across all matching rules and namespaces remain ambiguous. No basename fallback or synthetic target is used.

This complete TOML pack can be saved under any `.forge/frameworks/<name>.toml` filename:

```toml
[[semantic_rules]]
id = "render-template"
namespace = "templates"
scope = "project"
activation = [{ kind = "package", name = "django" }]
operation = { kind = "call_to_file", language = "python", callee = { kind = "imported", module = "django.shortcuts", export = "render" }, argument = 1, target = { kind = "project_path_suffix", prefix = "templates/", language = "html" }, relation = "renders_template" }
```

The equivalent complete YAML pack is accepted with `.yaml` or `.yml`:

```yaml
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

Activation is either empty or a list of package alternatives declared in the nearest dependency project for the operation language profile. Empty activation requires the same exact imported-callee proof. Package activation controls both caller and file-provider facts. Project rules replace bundled rules with the same ID; other bundled IDs remain unless `replace = true` replaces the whole pack. An empty replacement supplies no semantic rules. Use `enabled = false` to disable the whole effective pack.

Only already-normal relative literal paths are accepted: no absolute path, traversal, empty segment, escape-dependent or interpolated literal. Keyword arguments, argument expansion and unsupported binding contexts remain without semantic facts. A same-spelling local module cannot establish external API provenance. Facts retain the original call occurrence and actual target module; edits, deletions, renames and manifest changes invalidate both sides through their shared typed dependency identity.


## Closed syntax operations

Each syntax rule has a filename-derived pack identity, a pack-local `id`, a source
`language`, optional package alternatives in `activation`, and one `operation`.
`language` names an available language profile. Package activation uses the
profile's dependency family unless `package_family` selects another family.
Use this field when the syntax grammar and dependency metadata belong to
different profiles, such as HTML rules activated by a Python package. Package
activation requires that family to be available in the build and declared in
the nearest applicable project. Empty activation still requires the operation's
lexical and literal proofs.

| Operation kind | Configured policy | Existing semantic effect |
| --- | --- | --- |
| `render_context` | Provider package/module/export; target/context argument positions and exact argument count | Literal context keys propagate to actual template modules after external-import verification. |
| `named_route_call` | Imported module/export, argument and display expression prefix | Emit a typed static route key with actual pack/rule origin. |
| `receiver_registration` | Provider module/constructors, member, path/handler indices and name keyword | Prove a lexical receiver registration; route rules opt in with `registration_guard`. |
| `decorated_registration` | Provider modules/constructor/member, name keyword and provider identity | Register a statically verified decorated callable. |
| `template_loader` | Provider package/module/environment/loader and loader keyword | Record a static relative loader root with import proof. |
| `template_library` | Directory, provider module/constructor, registration member-to-tag/filter map and name keyword | Resolve registered template symbols within a dependency scope. |
| `object_factory` | Callee identities/suffixes and optional globals that must remain unshadowed | Associate object members with their exact factory object. |
| `implicit_receivers` | Explicit receiver names | Admit only the configured implicit receiver names. Package activation belongs to the syntax rule. |
| `implicit_builtins` | Names and source language | Classify configured runtime names without suppressing lexical shadows. |
| `receiver_type` | Nominal name, optional imported module/export, members, property types, callback callees and generic flag | Reuse existing nominal type, callback-parameter and property resolution. |
| `component_environment` | Package, config filename/callee/keys, component directory/extension, virtual module, autoimports, builtins and finite module descriptors | Resolve configured components, named exports and external call-result members from persisted configuration facts. |
| `component_options` | Imported wrapper module/export and registration-object key | Resolve existing explicit component registrations from bare or import-proven wrapped options objects. |
| `component_macros` | Distinct `props`, `emits`, `slots` callees and `markers` in the Vue language profile | Preserve existing setup input, callable and typed slot roles; ordinary local declarations shadow configured names. Rule-caused facts retain typed origin. |
| `route_object` | Collection name, path/handler keys and method | Extract a route from a literal object entry. |
| `http_client_call` | Direct callee/receiver names, method map/default, URL/method keys and accepted URL prefixes | Emit endpoint-call references from literal calls. |
| `template_named_route` | Tag name and display expression prefix | Emit a typed static template route key. |
| `template_symbols` | Loader requirement, tag/filter/library names, and library-to-route mappings | Admit configured template symbols and route libraries through the same generic operation. Package activation belongs to the syntax rule. |
| `template_dialect` | Directory, assignment, function, import/from/alias, load, include/extends and scope directive names | Parse bounded template declarations and references; normalize actual local targets during extraction. |
| `html_request_attributes` | Attribute prefixes, method map, context fields, optional `values` attribute and `value_dynamic_prefixes`, request/context node kinds, namespace and context label | Emit request and context facts from literal attributes; typed origin selects the request role. |
| `html_script_attributes` | Event prefixes, object attribute, event/object scope labels | Parse embedded JavaScript through the same effective rule context. |

Constructor and attachment proof follows the receiver's lexical binding; a
same-spelling parameter or local object cannot borrow a module receiver's proof.
Assignments, augmented assignments and updates to a configured macro producer
invalidate its rule-caused setup facts and derived callable/member/slot hints.
Template dialects use configured directory ownership and exact directive names;
conflicting candidates fail closed, and pack filename order selects no dialect.
Component relinking requests unchanged configuration facts from the persisted
cache when only a component provider changes. Template-library relinking requests
the loaded libraries' unchanged producer facts through the same hydration path,
using effective directory and project-scope data.

Each `component_environment.modules` entry requires `package` and `components`.
Optional `exports` defaults to an empty list of `{directory, extension}` records;
directories are normalized relative paths of at most 256 bytes and the supported
export extension is `.ts`. At most 32 export descriptors are accepted per module.
They select actual persisted named exports from immediate-directory indexed providers, rather than
deriving an export name from a filename. Optional `call_results` defaults to an
empty map from callee identifiers to nonempty, unique callable-member lists. The
map and each list are bounded at 128 entries. Unknown nested fields fail validation.

Both descriptors require the environment's declared package, the descriptor's
declared package and an exact configured module in the persisted configuration.
Installed packages alone do not establish that configuration. The component
`enabled_key` controls component discovery independently: `components = false`
does not disable configured module exports or call-result contracts. Lexical
locals, imports, parameters and writes retain precedence over implicit globals.

The optional strict `display` record contains `builtin`, `autoimport`, `library`
and `component` compatibility evidence labels. Only the corresponding symbol or
component operations admit these fields. Labels are bounded nonempty strings;
they do not control activation, targets, confidence or provenance. Evidence keeps
the actual filename-derived pack and rule ID separately. Named-route display
prefixes likewise remain independent of the typed route key and rule origin.

### Finite symbol, member and endpoint projections

The following operations use the same rule envelopes, activation and provenance:

| Envelope / operation | Required operation fields | Result |
| --- | --- | --- |
| Semantic `symbol_attachment` | `language="python"`, `callee`, `source`, `target`, `relation` | A real decorated or registered callable references a uniquely resolved class through `uses_model` or `references`. |
| Syntax `file_route` | `directory`, `extension`, `index`, `method="GET"`; language `vue` | A module-owned route from a static relative file path, matched by the existing route index. |
| Syntax `factory_members` | `callee`, `argument`, optional `shape` and `configured_module`; language JS/TS | Calling the uniquely bound factory exposes proven original declarations. `options` requires `state_member` and `actions_member`; `setup_return` projects callable declarations from a completed callback return. |
| Syntax `named_binding` | `domain`, `binding` | A project/domain/literal-key join to an actual callback, object or embedded listener declaration. |
| Semantic `endpoint_projection` | `language="python"`, imported `callee`, `attachment={callee,keyword}`, `url_keyword`, `method_keyword`, `default_method` | A direct nested constructor supplies a local URL and method to the existing endpoint matcher. |

A proven absent optional `symbol_attachment` target slot emits no attachment fact
or attachment coverage. A present dynamic value or uncertain keyword lookup stays
unresolved; an explicit identifier without a declaration stays unresolved, and
duplicate verified declarations stay ambiguous without an edge. Absence of an
optional field is different from a reference to a missing target.

HTML request attributes retain their extractor-owned URL classification. Only
`local_unverified` requests enter exact route matching. Dynamic or nonlocal
values remain unresolved; remote values retain external coverage and current-page
values retain resolved coverage without an endpoint edge. Missing or unknown
classifications fail closed.

When `values` is configured, `value_dynamic_prefixes` identifies expression
payloads. The extractor emits `{attribute}_status` and, for static JSON objects,
`{attribute}_key_count`, where `{attribute}` is the configured values attribute
name. It records the classification and key count without copying the payload.

Factory results are shared by ordinary JavaScript/TypeScript callers and Vue
setup/template consumers. `shape = "options"` is the default and preserves the
imported factory's existing state/action projection. Its distinct
`state_member` and `actions_member` values must be valid identifiers.
`shape = "setup_return"` accepts a nonasync, nongenerator callback at `argument`
with a final literal object return; omit the options fields or leave them empty.
The callback's contained callable declarations may be async. Public member aliases
bind only to a uniquely proven actual indexed callable, retaining its declaration
identity and position. Non-node state, unsupported RHS, computed/spread/duplicate
keys, conflicting returns and rewritten bindings produce no guessed exact target.
Configured setup callbacks may initialize through non-returning `if` and `try`
blocks before their sole final direct return. Alternate returns, abrupt exits,
loops, unsupported control flow and unsupported binding writes invalidate this
configured summary. Ordinary return inference keeps its existing stricter policy.
This does not create a general variable or nested-property graph.

Grammar extractors can retain ordinary property-read references for later
cross-file proof. Additional framework member facts, exact edges and coverage
require association with an effective configured factory result; declared
property reads require actual finite or local property proof. Raw syntax alone
does not admit an additional coverage observation: a fact-absent dynamic
provider adds no member coverage or exact edge, while an effective factory-result
fact with a missing member retains its existing unresolved outcome. A write-only
left-hand side is not a read. Ordinary import, enum, class and same-object member
resolution remains available under its existing admission rules, including
inside Vue expressions.

An imported API retains canonical import proof. A global factory API additionally
requires `configured_module`, naming one effective environment module descriptor
with package/configuration proof and an unshadowed lexical callee. The export
association preserves the provider's actual owner and binding; a directory never
creates a factory declaration. Vue template lookup
uses the completed setup bindings, including when the template precedes the
script, while lexical shadows, reassignment, import position and ambiguous
identities retain their existing guards.

A configured `call_results` member is different from a local declaration target.
For example, a lexical `const { t: translate } = useI18n()` can retain external
callable provenance for `translate` through a configured result contract. It
creates no local `t` node or exact local edge. Ordinary imported composables keep
their indexed return-member targets; a same-spelling local function or parameter
does not inherit the configured external contract. Persisted `Project(Call, member)`
and completed binding scope/position drive this proof through the ordinary
projection and alias reducers, including their uncertainty and join guards.

`Api` is one of `imported {module,export}`, Python-only
`constructed_member {module,constructors,member}`, JS/TS-only `global {name}` or
`global_member {receiver,member}`. Global roots must be unshadowed; a receiver may
use one unique, unreassigned local alias proved by the collected bindings.
`Slot` is `argument {index}`, Python `keyword {name}` or Python
`keyword_member {name,member}` for one literal dictionary member.
`AttachmentSource` is `decorated_callable {}` or `argument {index}`.

`NamedBindingForm` is `call_definition {callee,key,target,target_kind}`,
`call_reference {callee,key,provider_scopes?}`, Python
`assignment_definition {receiver_type,member,template_root}`, HTML
`template_reference {symbol_kind="filter"}`, JS/TS `member_reference {root}` or HTML
`attribute_definition {prefix,scope="event_handler"}`. Definition target kinds
are `object` and `callable`; attribute definitions retain the actual listener
scope. Remaining member chains use ordinary member resolution. Overlapping HTML
script-attribute rules share one embedded parse.

| Named binding form | Admitted languages |
| --- | --- |
| `call_definition` | JavaScript, TypeScript |
| `call_reference` | Python, JavaScript, TypeScript |
| `assignment_definition` | Python |
| `template_reference` | HTML |
| `member_reference` | JavaScript, TypeScript |
| `attribute_definition` | HTML |

`call_reference` defaults to the current project. Its optional `provider_scopes`
list adds at most 16 explicit `{workspace,project,package}` providers to that
lookup. An empty workspace names the primary workspace; a nonempty value names
an enabled linked-workspace adapter. The relative project path may be empty for
the workspace root. The package must match that collected project's own package,
not a dependency it imports. Unknown or disabled workspaces fail preparation;
missing project/package matches supply no provider. Local and admitted providers
remain one candidate set, including unknown and duplicate definitions. Configure
installation-specific adapters and package paths in local manifests; bundled
rules do not assume an installation alias.

`assignment_definition` recognizes `receiver.member["literal"] = callable` only
with canonical proof of its configured imported `receiver_type={module,export}`.
It records a closed `RegistryDefinition` with required receiver and target
operands in the existing named-binding vector. Uncertain operands stay unknown;
legacy definition records retain their shape. `template_root` is a confined
directory relative to the provider file. An HTML `template_reference` joins an
existing filter reference only within that root, nearest project/workspace and
matching domain. It does not infer an environment through helper calls or join
all same-named filters globally. Observed uncertain or duplicate registrations
remain unresolved or ambiguous rather than falling back to another definition.

### Explicit HTTP target context

`http_client_call` optionally accepts
`target_context={request_owners=[...],route_owners=[...]}`. Each list contains
1–128 unique, exact canonical file paths; linked paths use an enabled workspace
qualifier such as `[api]/src/routes.py`. Paths reject traversal and absolute
forms. This is an explicit application assertion whose correctness the author
checks against the actual client/application registration chain; Forge does not
infer an application graph or detect a valid path asserted for the wrong app.

Without the field, or outside `request_owners`, ordinary URL/method matching is
unchanged. For a matching request owner, only the configured route owners supply
candidates. Missing owners or method/path mismatches remain unresolved without
global fallback; multiple matching route identities remain ambiguous. This
context affects its producing HTTP-client rule only. Endpoint projections, HTML
request attributes and named-route matching keep their existing behavior.

All nested enums use `kind` tags and reject unknown fields. Domains and API
identifiers are bounded at 128 bytes, argument indices are below 32, constructor
alternatives are bounded at 32, and literal keys are nonempty, control-free and
at most 256 bytes. Factory objects contain at most 128 unique static members.
File routes reject dynamic, empty, dot and traversal segments. A dynamic method
does not use the default method; the default applies only when the argument is
absent. Missing, unknown or conflicting targets produce unresolved or ambiguous
coverage without an exact edge. Duplicate literal named keys remain ambiguous
even when one definition has an unknown target. Invalid payloads retain their
opaque evidence and cannot contribute exact semantic facts.

The four typed vectors `symbol_attachments`, `factory_results`, `named_bindings`
and `endpoint_requests` remain in compressed local facts and participate in the
canonical resolution identity. Each vector is bounded at 128 facts per scope;
exhaustion invalidates the scope's projection proof. Older payloads omit these
vectors. Named definitions and references also project compact internal
reference selectors for indexed delta invalidation and cached provider
hydration. They do not create graph reference kinds or expose callback payloads
in navigation details.

Before invalidating a canonically produced named-definition payload, the typed
store retains bounded uncertainty markers with its effective rule, domain and
key. A same-key competitor therefore cannot disappear and manufacture a unique
target. More than 128 distinct keys for one domain/rule origin widen that group
to an unknown key. Legitimate canonical exhaustion of the 128-group bound keeps
the containing flow invalid and persists an independent owner fault; it does
not abort indexing or reopen. Configured named authority remains unavailable
only in the affected project scope; ordinary calls and unrelated projects stay
eligible. Malformed, unknown-field or noncanonical raw marker payloads still
fail strict codec validation. Raw invalid flow payloads supply no trusted keys
or targets. Within the finite carrier bounds, unrelated keys and domains retain
their own admission checks. Markers and owner faults persist with full facts;
old and new fault-owner transitions reselect the same named project scopes for
delta handling.

All operations reject unknown fields. Arbitrary AST queries, executable snippets,
recursive configuration programs, and source reads from linking are unsupported.
Configured class attributes are identifiers; target file prefixes are normalized
relative directories ending in `/`, with complete path-segment matching.
`class_attribute_to_file` and `class_attribute_to_symbol` do not reserve framework
attribute spellings.

DI reference hints retain the actual pack and callee-based rule identity through
grammar-specific compilation, persistence and effective-rule admission.
Render contexts, component configurations and macro roles, factory object ownership and generated
route, loader, registration and embedded-script scopes retain their producing
pack/rule identity in typed value-flow facts. Configured reference rewrites retain
the identity in a typed reference hint. The parser transfers body facts and
owner-local projection vectors directly to FlowStore. Existing public/legacy
inputs hydrate at the TypedFacts boundary; serialization overlays the complete
public representation without a normal typed-to-JSON-to-typed roundtrip.
Linker consumers borrow typed facts.
Linking verifies current effective operations and uses stored facts, module
indexes, and exact import providers. A manifest/dependency digest change invokes
the existing build owner to reparse invalidated inputs; linking and route
composition do not open source files. Rule replacement, removal, and activation
changes therefore follow the same cold/delta invalidation boundary.

Setup return targets and public aliases, named exports and configured module
result contracts participate in that existing canonical resolution identity.
Old and new export/factory/callee names select unchanged consumers, including
missing-provider additions and module removal. Relinking selects only scoped
configured providers, hydrates each cached provider once through the existing
full-facts path and preserves ordinary edges. Verify warm source and TOML edits
against reopened SQL targets and equal-final cold commitments; cold coverage alone
does not prove delta behavior. These descriptors add no persisted fact vector or
source-read recovery path.

A project route rule supplies its own prefix/include/name policy. For example,
a custom decorator with a class prefix uses explicit data:

```toml
[[route_decorators]]
callee = "Expose"
method = "GET"
path_argument = 0
prefixes = [{ kind = "class_decorator", callee = "Mount", argument = 0 }]
```
