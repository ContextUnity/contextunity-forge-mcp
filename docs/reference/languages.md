---
doc_type: api
title: Language support
---

# Language support

Language profiles are selected at compile time. A default build includes Python, Rust, TypeScript, JavaScript, Vue, and Protocol Buffers. Markdown and MDX documentation is indexed separately from code profiles.

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
| Java, C#, Kotlin, and PHP | Classpath, reflection, dependency injection, autoloading, and generated code may add relationships. |
| C and C++ | Preprocessing, conditional compilation, templates, and linker configuration determine compiled relationships. |

Use [configuration](configuration.md) to control admitted paths and [indexing architecture](../architecture/indexing.md) to understand the extraction and linker boundary.
