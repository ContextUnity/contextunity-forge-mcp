---
doc_type: api
title: Language support
---

# Language support

Language profiles are selected at compile time. A default build includes Python, Rust, TypeScript, JavaScript, Vue, Protocol Buffers, HTML, YAML, and TOML. Markdown and MDX documentation is indexed separately from code profiles.

```sh
cargo build --release --locked
cargo build --release --locked --features all-languages
cargo build --release --locked --no-default-features --features lang-rust
```

Use `contextunity-forge-mcp --help` to inspect the installed binary. Enabling a profile changes which files the scanner admits and which parser and linker rules are available. Rebuild the index after changing the binary's feature set.

## File profiles

| Cargo feature | Language | Extensions |
| --- | --- | --- |
| `lang-python` | Python | `.py`, `.pyi` |
| `lang-rust` | Rust | `.rs` |
| `lang-typescript` | TypeScript | `.ts`, `.tsx`, `.mts`, `.cts` |
| `lang-typescript` | JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` |
| `lang-vue` | Vue | `.vue` |
| `lang-proto` | Protocol Buffers | `.proto` |
| `lang-html` | HTML | `.html`, `.htm` |
| `lang-yaml` | YAML | `.yaml`, `.yml` |
| `lang-toml` | TOML | `.toml` |
| `lang-go` | Go | `.go` |
| `lang-java` | Java | `.java` |
| `lang-csharp` | C# | `.cs` |
| `lang-kotlin` | Kotlin | `.kt`, `.kts` |
| `lang-php` | PHP | `.php` |
| `lang-ruby` | Ruby | `.rb` |
| `lang-c` | C | `.c`, `.h` |
| `lang-cpp` | C++ | `.cpp`, `.cc`, `.cxx`, `.hpp`, `.hh`, `.hxx` |

`all-languages` enables every listed profile. Files outside the enabled profiles are excluded from code extraction. The scanner still admits Markdown and MDX in configured roots.

## What the graph means

The index records declarations, imports, references, and relationships recoverable from source syntax. A resolved edge means the linker found a candidate within its indexed scope. An unresolved or external edge remains evidence of a reference, not evidence that the dependency is absent. Confirm behavior in the build system and runtime where it matters.

| Profile group | Important static limit |
| --- | --- |
| Python and Ruby | Dynamic imports, metaprogramming, monkey patching, and runtime dispatch can escape syntax-level analysis. |
| TypeScript, JavaScript, and Vue | Compiler aliases, generated types, framework transforms, and inferred dispatch may require build context. |
| Rust | Macros, conditional compilation, generated code, and `include!` can change the compiled program. |
| Go | Build tags, generated code, CGo, and package selection affect the final build. |
| Protocol Buffers | Generated language bindings and import resolution depend on the compiler and include paths. |
| HTML and HTMX | Files are modules; individual tags are not graph symbols. Each HTMX request is an indexed source node contained by its HTML module. Local JavaScript/TypeScript script and link targets contribute references. Inline JavaScript imports require `lang-typescript`. Confirm runtime route ownership in server code. |
| YAML and TOML | Mapping keys and TOML tables are syntax symbols. Application-specific configuration meaning, YAML alias expansion, and schema validation are outside the graph. |
| Java, C#, Kotlin, and PHP | Classpath, reflection, dependency injection, autoloading, and generated code may add relationships. |
| C and C++ | Preprocessing, conditional compilation, templates, and linker configuration determine compiled relationships. |

Use [configuration](configuration.md) to control admitted paths and [indexing architecture](../architecture/indexing.md) to understand the extraction and linker boundary.

## Python type aliases

Python indexes module-level PEP 695 aliases, `TypeAlias` and `TypeAliasType` annotations, and assignments calling `TypeVar`, `NewType`, or `TypeAliasType` as `type` symbols. These symbols have module-qualified names and participate in import and annotation resolution. Module-level declarations inside `TYPE_CHECKING` blocks are included. Function-local and class-local aliases are excluded.

## Dependencies and typed receivers

Forge collects declared dependencies from workspace manifests before linking: Python `pyproject.toml`, `requirements*.txt`, `setup.cfg`, and `Pipfile`; JavaScript/TypeScript `package.json`; Rust `Cargo.toml`; Go `go.mod`; and Java/Kotlin `pom.xml`, `build.gradle`, and `build.gradle.kts`. Linked workspaces use the admitted adapter configuration. Standard-library imports and declared dependencies have distinct evidence. An absolute import with no indexed provider is classified as external even when no manifest declares it; relative imports and missing symbols in indexed modules retain their local resolution evidence.

The committed manifest digest covers manifests outside source roots as well as linked workspaces. A changed manifest triggers a full rebuild during delta admission or the next MCP inventory check, after the normal freshness TTL. Discovery excludes ignored directories and symlinks and reads manifests up to 4 MiB.

JavaScript and TypeScript recognize CommonJS `require` bindings, including destructuring, as import aliases. A locally bound `require` is treated as an ordinary callable.

Python functions retain explicit parameter annotations in `details.param_types`. Calls through an unmodified parameter can resolve to methods of an indexed receiver class with `inferred` confidence, or to external evidence when the receiver type comes from an external import. Inherited member lookup uses a cached Python C3 order and respects local overrides. Known unittest assertions, Django command output, and Django model manager operations receive external evidence only when their framework ancestry is established.

Python records receiver hints for string literals, constructor calls, and zero-argument `super()`. Known string methods and verified class members can resolve without interpreting arbitrary expressions. Ambiguous types, reassigned parameters, unknown return values, and unsupported computed receivers retain uncertainty.

Python import resolution follows explicit exports across ordinary modules and packages. Statically described PEP 562 exports are admitted only when a literal export name identifies an existing provider; dynamic export names and missing providers remain unverified. Incremental indexing hydrates the required import chain before linking. Linked workspaces retain both `src`-qualified and package-root module names.

## Python registrations

For an indexed module that imports `FastMCP` from `fastmcp` or `mcp.server.fastmcp`, Forge records `@server.tool` and `@server.tool(...)` on module-level functions when `server` is assigned a `FastMCP(...)` instance. Each confirmed tool has a `tool_registration` node, a `contains` edge from its module, and a `handles` edge to its function. A literal `name=` sets the advertised name; a dynamic name is marked unknown. Import aliases and module-qualified constructors are supported. Reassignment of the constructor before instance creation, or the server before registration, prevents a confirmed link.

The graph records registrations evident in source. Runtime-created tools, factory-returned servers, and imported server instances require runtime confirmation. Other decorator-based frameworks use their own registration rules; a method named `tool` alone does not identify FastMCP.

## HTML and configuration files

HTML and HTMX share one HTML Tree-sitter grammar and one language profile. HTMX attributes are read from the parsed HTML tags.

HTML supports file inspection, source snippets, syntax diagnostics, and nested AST search. Its module name retains the extension so `app.html` and `app.js` remain distinct. Local `script src` and `link href` references to JavaScript/TypeScript assets resolve when those targets are indexed. Inline scripts contribute imports, not JavaScript function declarations; inline content in scripts with `src` or non-JavaScript data types is skipped.

Each HTMX request in `.html` and `.htm` files is a separate `htmx_request` node. `code_map_search` returns its method, URL, file, and line with normal pagination; `code_map_inspect(..., detail="full")` adds the URL status and same-element trigger, target, swap, include, and `hx-vals` status. The profile recognizes `hx-get`, `hx-post`, `hx-put`, `hx-patch`, `hx-delete`, and standard `data-hx-` forms. Elements that declare only supporting HTMX attributes become `htmx_context` nodes. `local_unverified` identifies a local-looking runtime route whose handler can be confirmed in server code. Remote URLs, template expressions, and current-page requests have distinct statuses. Static `hx-vals` records the key count; dynamic or invalid values receive a status. Request nodes are connected to their source file by `contains` relationships. Check runtime inheritance, selector matching, and server dispatch where those behaviors matter. See the [HTMX attribute reference](https://htmx.org/reference/) for runtime behavior.

YAML exposes block and flow mapping keys, including nested mappings. TOML exposes tables and array tables as namespaces and key/value pairs as variables. Quoted key spelling is preserved; repeated keys or array-table entries have distinct symbol IDs. Use a path and line selector when a repeated name is ambiguous.

These profiles use the same index, selectors, snippets, and incremental update path as other languages. Diagnostics describe syntax accepted by the grammar, not complete HTML, YAML, or TOML specification validation.
