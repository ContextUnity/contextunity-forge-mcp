---
doc_type: architecture
title: Indexing architecture
---

# Indexing architecture

Forge builds a local SQLite graph from admitted source and Markdown. The CLI creates or updates the index; the MCP server checks freshness before reads. Readers resolve selectors and return bounded graph or document results.

```text
CLI build / scan / delta             MCP request
          |                               |
          +---- scanner and adapter ------+
                       |
             language and Markdown extraction
                       |
                    linker
                       |
                SQLite index
                       |
             CLI and MCP readers
```

## Ownership and data flow

> [!IMPORTANT]
> Invariant: Every capability exposed through the MCP JSON-RPC protocol maintains
> equivalent CLI command coverage under `contextunity-forge-mcp`. Both surfaces
> share the same underlying storage, analysis, and execution engines (see [ADR 0001](../adr/0001-dual-surface-parity.md)).

- `src/cli/` owns command parsing, build commands, and command output.
- `src/mcp/` owns the 20 tool schemas, request validation, freshness checks, and bounded responses.
- `src/engine/scanner/` owns admitted file inventory, roots, ignores, linked workspaces, and scan limits.
- `src/engine/languages/` and `src/engine/ast/` extract syntax facts for enabled language profiles. A `LanguageProfile` owns parsing and import normalization; a `LanguageLinker` owns language-specific import, receiver, and incremental-linking rules. The shared linker builds namespace indexes and graph edges through those contracts. `src/engine/docs.rs` extracts Markdown documents and heading sections.
- `src/engine/linker/semantic_context.rs` exposes borrowed, read-only access to prebuilt language member indexes through `LanguageMemberLookup`. `src/engine/linker/framework_relations.rs` composes cross-language route and template relationships after ordinary reference resolution; its graph output joins through the common deterministic sort.
- Each linking pass builds one borrowed [`LinkerInterner`](../../src/engine/linker/interner.rs). Prepared semantic tables use `PathId`; common member keys and semantic target sets use `SymbolId`. Graph emission restores the existing string identities and ordering.
- The linker connects facts using indexed files and configured workspace boundaries. An edge may remain unresolved or external when its target cannot be established statically. During cold builds, the linker operates over a lightweight `SymbolCatalog` (`src/core/semantic.rs`, `src/engine/linker/compact.rs`), which decouples reference linking from heavy AST details and avoids workspace-wide fact maps.
- Unmodified files reuse extracted facts across worktrees via a content-addressed binary `ExtractionCache` (`src/db/writer/cache.rs`), keyed by source digest, grammar ID, semantics version, and effective framework ruleset hash.
- Cache encoding borrows record fields directly through binary codecs. Typed sidecars retain their existing V3 JSON-value overlays; V2 loading, checksums and malformed-payload rejection remain part of cache admission. Each extraction worker reuses its source buffer and validates length, source metadata and digest for every file.
- [`NodeSemanticEntry`](../../src/core/typed_facts.rs) stores admitted bindings, rebindings and parameter types natively. Public node serialization overlays the original JSON fields, preserving parameter order and malformed raw values. [`ValueFlowFacts`](../../src/core/semantic.rs) allocates its boxed framework/template facts only for scopes that need them; serialization retains the existing flat fields.
- `src/db/` stores generations, nodes, edges, documents, and source metadata; its readers implement selectors, traversal, search, and analysis. Local syntax facts use Zstandard-compressed JSON, reverse dependency lookups use the target index on `dependencies`, and repeated resolution evidence uses a shared dictionary.

## Documentation references and verification

The scanner selects configured Markdown and MDX files, and the documentation
extractor splits them into heading sections and records links and code claims.
The linker resolves code claims within the eligible workspace or an explicitly
linked source file. Only a unique declaration in the first non-empty candidate
tier creates a verified documentation edge; ambiguous candidates remain
available for navigation and do not count as verified coverage. A same-line
`INV-*` claim with an explicit source link can create a verified `governs` edge
when that declaration resolves uniquely. A GitHub alert callout's
`is_invariant` section flag is a separate discovery signal.

For the current implementation, follow [`collect_documentation_paths`](../../src/engine/scanner/mod.rs#L622-L685),
[`extract_with_details`](../../src/engine/docs.rs#L24-L69), and
[`document_symbol_candidates`](../../src/engine/linker/resolution/mod.rs#L105-L143).
The incremental parity test
[`source_delta_relinks_affected_docs_with_bare_suffix_references`](../../tests/incremental/doc_parity.rs#L70-L160)
checks documentation links, invariant claims, diagnostics, and cold/delta
parity. The [MCP documentation query contract](../reference/mcp-tools.md#documentation-queries)
defines the candidate and verified-edge response distinction.

A root workspace owns the index under its `.forge` directory by default. Linked workspaces add files to that same index when enabled and available. Each linked workspace has its own roots and ignore settings. A source root is an inventory boundary; it does not by itself declare an import prefix. Use the actual package and module structure when interpreting imports.

`build` creates a full index. `delta` updates it from changed and deleted files. `scan` reports admitted files without creating an index. The MCP server checks indexed ownership, source inventory, adapter configuration, and file changes before serving database-backed requests. When the indexed state is stale, it updates affected files or rebuilds as needed. CLI readers use the index already on disk; run `build` or `delta` after editing source before relying on CLI results.

> [!IMPORTANT]
> Invariant: `forge-owner-sha256-v2` commits sorted FTS5 instance postings for `node_search` and `doc_search`, plus each index's configuration rows. Verification derives each active document's per-column token counts from those postings and checks them against FTS5 document-size rows, which preserves ranking-length integrity without depending on shadow segment layout. Cold builds and deltas over identical admitted inputs retain the bit-identical root required by [ADR 0004](../adr/0004-deterministic-merkle-commitments.md); reindexing and vacuuming do not change it.

A full build validates a staged database before publication. Forge readers hold a shared database lock during each query; the builder takes an exclusive lock before replacing the index. The MCP server reuses its SQLite connection between requests, starts a read transaction for each request, and discards the connection when the database identity changes. It reuses a successful source-inventory check for up to five seconds when the adapter digest and database identity remain valid. Adapter loading and database identity checks still occur during admission; see [performance guidance](performance.md) for the current boundary.

## Resolving a query

`code_map_search` ranks exact names and direct name prefixes first, exact qualified names next, and natural-language terms with SQLite FTS5 BM25. Graph-connected nodes and module exports receive a ranking boost. Markdown nodes are excluded unless `include_docs` is true; `group_by_file` groups the returned page by path. `code_map_inspect` resolves a selector to one node and its immediate evidence. `code_map_explain`, `code_map_impact`, and related tools traverse indexed edges with depth and size limits. `code_map_tests` bounds graph traversal to four steps and uses indexed test-file names when inbound graph links produce no tests. `ast_grep_search` uses indexed declaration-name selection where the index covers the query; general structural patterns scan the bounded indexed file set because `node_search` does not contain arbitrary body literals. Each candidate's source digest is checked before parsing. `search_docs` and `get_doc` read indexed Markdown, including sections extracted from headings. `code_map_analyze` executes a bounded read-only query over the index.

Selectors must identify one target. For example, `package/settings.py:load_config` names a function in a file, while `package/settings.py#L12` resolves the narrowest indexed symbol at that line. If a bare name matches more than one target, the resolver reports ambiguity and requires a path or exact symbol identifier. See the [MCP tool reference](../reference/mcp-tools.md) for selector syntax and response bounds.

## Declarative framework extraction

`DependencyRegistry` loads and validates bundled and workspace-local framework manifests before parallel extraction. It prepares merged typed rules for the effective project scopes and shares an immutable `Arc<LanguageContext>` across files; AST extraction uses `extract_typed_with_registry` in both the cold writer and delta paths. Profiles receive those rules through the existing extraction hooks, together with a per-file `RouteContext` for static router and controller prefixes. Extraction does not reread manifest files for each syntax node.

Grammar-bound syntax classification and route/DI dispatch use numeric kind IDs in tables sized to the actual grammar. [`with_warm_parser`](../../src/engine/languages/mod.rs) reuses thread-local parsers for ordinary files, Vue script islands and HTML JavaScript fragments, resetting and selecting the actual grammar before parsing. Embedded facts retain their original source offsets.

Class-hierarchy rules feed canonical Python receiver lookup through the inheritance index. Import provenance, method shadowing, and unknown external bases still bound resolution. Route and DI rules produce ordinary syntax facts and references. Configured route facts retain include modules, route names, and namespaces; static include-prefix composition runs before persistence, including after incremental fact hydration. Manifest digest changes invalidate the indexed rule snapshot so an update cannot retain extraction from an old rule set.

Closed syntax operations select provider APIs, keyword and attribute names, object factories, template dialects, component configuration, receiver types, and directory conventions from validated data. Profiles receive the effective rules during preparation and embedded-language extraction. Typed render contexts and component configurations retain pack/rule origin; loader and registration facts retain the same identity. Context indexes use project scope and rule provenance. No framework identity selects a private cache or linker branch.

Finite symbol attachments, factory results, named definitions/references and
endpoint requests use four bounded typed vectors. The existing canonical
resolution projection includes their complete operands and uncertainty states,
including facts owned by functions. Named joins prepare one borrowed index by
effective project, domain and key. Compact internal reference selectors feed
indexed delta invalidation for old and new provider keys, including missing and
unknown targets. Selected providers hydrate from compressed local facts through
the existing typed dispatcher. Each newly hydrated frontier contributes its own
named selectors until the same convergence completes; linking does not recover
provider source. HTML embedded definitions publish selectors after their typed
finalizer. Known endpoint URL/method pairs feed ordinary request references and
the existing route index; uncertain inputs retain typed unresolved evidence.

`RouteContext` completes lexical binding and Python write summaries in one
prepass before resolving buffered route candidates. Assignments, prefix
providers and route receiver queries reuse those summaries. Existing non-route
compatibility callers retain their recursive collectors.

Dependency collection prepares effective contexts for the collected workspace/package-scope boundaries before file workers run. Route and DI rules compile once per workspace into immutable `RuleIR` tables keyed by the actual Tree-sitter grammar and shared across project contexts. Normal file and embedded-island binding uses lookup and `Arc` sharing without a rule-cache lock or recompilation; direct bundled/manual registry callers retain bounded thread-local context reuse, and custom grammars retain the legacy compilation fallback. AST hooks select candidates by `kind_id` and retain their existing semantic checks. JavaScript/TypeScript projection hooks merge prepared callee/member/root index slices in effective rule order, without scanning unrelated syntax rules. Their callback-summary cleanup remains independent of publication. Active file-provider indices select the target language before existing project/path proof. Full build and ordinary delta expose sorted, typed `framework_rule_diagnostics` from the complete current inventory; manifest-change fallback rebuilds recompute them. These ephemeral reports compare eligible identities, including equal-count losses, without writing parser errors or changing commitment domains.

The [framework manifest reference](../reference/framework-manifests.md) owns fields, merge behavior and diagnostic identities. New rule data uses existing language hooks; a manifest does not register a grammar or introduce arbitrary extraction code.

## Extending a language profile

Language extraction belongs to its profile and the shared AST layer. Add a profile through the language registry and compile-time feature configuration, then add a `LanguageLinker` only when that language needs import, receiver, or incremental-linking behavior beyond the generic implementation. Python package initializers, paired declaration files, re-exports, and external import aliases live in the Python linker. Verify facts against parser fixtures and linker tests. A profile does not change the selector or database contracts. Keep unresolved references visible instead of guessing a target from a name alone.

Rust import resolution follows named bindings from existing `use` references and public export facts. A bounded forwarding index is prepared with the namespace indexes and shared by import resolution and Rust member lookup. Private parent bindings are available only to descendants; public `use` bindings participate in package exports. Ambiguous targets, cycles, and exhausted alias walks stay unresolved. The root Cargo package name, or an explicit `[lib].name`, maps to the indexed conventional `src/lib.rs` only within the same manifest scope; custom library paths and foreign same-name packages do not acquire that mapping. Existing Rust member lookup resolves associated methods after the canonical type is selected.

## Static value flow

The shared `SinglePassExtractor` accumulates Python, Rust and TypeScript body bindings and typed value-flow facts through language hooks, then finalizes each owner at scope exit. Public profile defaults remain compatible; body summaries do not rerun the legacy recursive collectors. The shared schema in `src/core/semantic.rs` records source positions, assignments, annotations, fields, aliases, return contracts, and branch-join alternatives. Branch joins use a separate transfer-fact array, so binding facts stay one-to-one with source writes. The internal `TypedFacts` envelope stores these facts separately from dynamic node details. The linker borrows them directly. Serialization views preserve the public `details.value_flow` representation, property order, compressed local facts, and commitment bytes. Legacy hydration retains malformed metadata and original encoding. Embedded JavaScript uses the same typed AST extraction contract with source offsets and explicit owners.

`src/engine/linker/value_flow.rs` reduces facts against canonical import and symbol admission in `semantic_context.rs`. It builds receiver and computed-call caches before parallel reference linking. Language member indexes supply Python inheritance, Rust inherent and visible trait implementations, and TypeScript class/interface inheritance through `LanguageMemberLookup`. Inferred edges always point to actual indexed declarations; external evidence retains its import provenance.

Optional typed call-site evidence retains the inner source position, bounded nested receiver and up to four positional literal/Unknown slots inside existing compressed value-flow facts. Direct computed hints carry the same typed call value. Legacy calls without evidence retain their prior behavior. Finite hierarchy/nominal receiver authority and exact literal mappings are prepared from effective manifest rules; reference linking does not reread source or decode per-reference JSON. Existing contracts retain resolution-relevant call discriminators, and the existing engine-source/profile fingerprint invalidates incompatible cached facts on rebuild. Generic fallback is retained only when independently proven; configured subtype properties are checked before generic platform normalization. JavaScript/TypeScript read references can select finite property types after ordinary local-member proof. Call resolution does not use that property lookup, so a string-valued property does not become callable.

Rust projects the first payload of standard `Option<T>`, `Result<T, E>`, and `anyhow::Result<T>` only after an explicit `?`, `.unwrap()`, or `.expect()` operation. The same typed projection supports initializer bindings and computed receivers; ordinary wrapper values retain container identity. TypeScript and JavaScript await bindings expose an annotated Promise parameter or local alias payload, or a bounded async constructor return. Unawaited wrapper values do not expose the payload, and writes or conditional bindings fence prior Promise proof. Python parameters and local bindings project one non-None payload from `typing.Optional[T]`, `typing.Union[T, None]`, and PEP 604 binary union expressions (`T | None` or `None | T`), resolving member calls to the inner type. Multi-nonnull unions (`A | B`), unions with `NoneType`, and malformed wrapper arities retain unresolved semantics.

Rust trait-object calls retain their unresolved dispatch gap and link to the indexed trait method with inferred confidence. Up to eight indexed concrete implementations contribute separate `possible_calls` edges; overflow emits no concrete candidates. Python abstract methods (`abc.ABC`) and interface methods similarly link to the base declaration with inferred confidence and emit up to eight `possible_calls` edges to concrete subclass overrides in the workspace. Macro transcription tokens contribute macro-owned references to statically named calls and types. Invocation links to the macro declaration and retains the opaque expansion gap; extraction does not expand or execute the macro.

Python joins direct identifier assignments from complete local `if`/`elif`/`else` paths. A missing `else` contributes the incoming value, and unknown alternatives make the result incomplete. The reducer retains at most eight distinct local declaration identities; overflow widens the whole result and emits no truncated prefix. Candidate alternatives live in compressed typed value-flow facts, not `nodes.details` or FTS text. Candidate members are persisted as separate `possible_calls` edges; they never create exact `calls` edges or certify removal safety. Candidate coverage is ambiguous when multiple complete call targets remain and unresolved for one target, unknown alternatives, or overflow. Inbound impact and removal assessment include possible edges as dependencies; exact call counts continue to use only `calls`. The CLI and MCP share the same stored graph contract.

### Universal principles for language coverage expansion

When extending semantic resolution or coverage across existing and new language profiles, implementations must adhere to three universal invariants:

1. **Explicit Unwrapping**: Every language explicitly un-nests its container/wrapped types without implicit or guessed resolutions (`Result`/`Option` in Rust, `Promise` in TS/JS, `Optional`/`Union` in Python). Negative cases (un-unwrapped calls) must remain unresolved to prevent phantom edges.
2. **Bounded Polymorphic Dispatch**: When resolving calls on abstract interfaces / traits / base classes (`dyn Trait` in Rust, `ABC`/Protocol/subclasses in Python, interfaces in TS), the engine emits an inferred `calls` edge to the declaration and bounded `possible_calls` ($K \le 8$) to concrete implementations in the workspace, retaining uncertainty in `analysis_gaps`. Overflows beyond $K=8$ widen to Unknown/Incomplete and emit zero concrete candidates.
3. **Parser-Owned Structured Facts for Delta**: Language syntax distinctions (such as module vs named imports and re-exports) belong in language parsers (`src/engine/languages/`). The existing build/parser owner reads changed or invalidated inputs. `src/db/delta.rs` relinking and dependency-frontier logic consume stored facts and indexes; they do not open source to recover missing framework semantics (`INV-DELTA-NO-SOURCE-IO`). Manifest/dependency digest changes use the existing full-build boundary.

Assignments take effect after their right-hand expression completes. Unknown writes and ambiguous declarations block stale proofs. Explicit return annotations take precedence over bounded summaries of complete straight-line returns. Inference records dependencies on return, binding, and field summaries and reevaluates expressions when those summaries change, preserving owner order and the bounded iteration count. Alias cycles, unsupported dynamic values, and exhausted inference limits retain uncertainty. Reference linking uses cached receivers; it does not parse JSON or consult disk or SQL for each method call.

The internal graph uses static tags for known edge kinds, confidence values, and coverage statuses, with owned strings for arbitrary values. Public graph adapters retain the existing string fields. SQLite persistence uses bounded multi-value batches, shared key ownership, and reusable search buffers; serialization and graph ordering use the same string values across both representations.

Canonical HTML classic scopes preserve their empty `bindings` and `fields` JSON slots as null and decode those slots as empty sections on both public and cached paths. Other scopes retain strict value-flow decoding. Coverage commitment leaves preload dictionaries and stream borrowed canonical rows; noncanonical SQLite storage retains the generic encoding path. Partial sealing loads dictionaries for the selected owners.

Incremental dependency keys include value-flow types, constructors, aliases, return expressions, computed-call hints, and export facts. A changed provider expands the consumer frontier before the language linker hydrates required facts. Full and incremental indexing share the same reducer and symbol admission rules.

Delta compares resolution identities for non-component nodes as a sorted set of 16-byte SHA-256 digests. Each digest covers the admitted node identity tuple and `Contracts::details`, so source positions do not trigger resolution invalidation while normalized contracts do. Direct importers of a changed provider are always refreshed. Later hops expand only when indexed exports or return contracts show that the importer exposes the changed provider; missing or ambiguous facts expand conservatively. Import syntax comes from extracted facts, and delta reads no source files. Rust importers use the same frontier, with changed trait implementation shapes also refreshing persisted trait-object dispatch candidates.

Body edits refresh the changed owner's facts, call edges, coverage, and commitments. Consumer invalidation compares semantic return and field contracts, signatures, exports, and import identities rather than body-local positions. Changes to inferred factory types, scoped imports, or inference validity limits expand the consumer frontier; equivalent internal call changes retain the existing consumer links.

`FORGE_PROFILE_LINKER=1` writes phase timings to stderr for namespace/export indexing, inheritance, semantic admission, value flow, reference linking, and graph sorting. It leaves graph facts and commitments unchanged.

Manifest `call_to_file` rules reuse the Python lexical import/literal owners and the shared linker composition pass. Core-owned semantic DTOs remain native in the owner-indexed `SemanticStore` during typed extraction and linking. Serialization overlays the existing `details.semantic_facts` slot; public extraction materializes that same representation. Compact navigation metadata omits parser-only flow and semantic payloads while full-facts serialization retains them. Legacy payloads hydrate at the compatibility boundary, retaining original values and invalid raw evidence. Actual module providers and call consumers share a typed project/path/projection identity, whose opaque dependency key feeds existing delta owner invalidation. Exact promotion aggregates every matching rule and namespace and requires one actual target plus canonical external-import proof.
