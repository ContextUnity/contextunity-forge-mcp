# HTML and Template Relations Test Suite

Subtree tests for HTML syntax, template engines, masking, and cross-language relations.

## Scope and Structure

Keep test modules focused by logical template engine and relation domain.
Do not aggregate separate template engines into one file.

- `syntax.rs`: Pure HTML AST parsing, tags, attributes, and doctype.
- `django_templates.rs` and its children: Django template tags, render context, class views, and named URLs.
- `jinja_templates.rs` and its children: Jinja2 expressions, loaders, and delta behavior.
- `browser_templates.rs` and its children: generic browser assets, directives, and template expressions.
- `template_masking.rs`: Expression masking and parser boundary protection.
- `template_origins.rs`: Line-offset reconciliation between rendered and source templates.
- `classic_wire.rs`: Script and stylesheet inclusion tracking.
- `htmx_relations.rs`: HTML `hx-*` bindings linking to backend API endpoints.
- `alpine_relations.rs`: HTML Alpine directives linking to frontend stores.
- `manifest_relations.rs`: Configured generic manifest relations that do not belong to one HTML framework.
- `relation_support.rs`: Shared SQL and fixture helpers for the HTML relation modules.

The HTMX and Alpine persisted-relation contract shares one fixture and remains
in `htmx_relations.rs`; its Alpine cases are not copied into another test.

## Shared Helpers and Fixtures

- `tests/common/`: Shared temp workspace and SQLite connection factories.
- `tests/languages/support.rs`: `Workspace` helper and commitment assertions.
- `tests/languages/html/mod.rs`: `coverage_aggregate_rows` and common HTML test assertions.

## Verification Commands

Run full HTML language test suite:
```bash
cargo test --test languages html::
```

Run targeted template engine or relation module:
```bash
cargo test --test languages html::django_templates::
cargo test --test languages html::syntax::
cargo test --test languages html::htmx_relations::
cargo test --test languages html::manifest_relations::
```

## Rules and Invariants

- Verify cross-language links through real AST extractions and linker graph outputs.
- Test template syntax with realistic multi-line fixtures rather than trivial single tags.
- Split modules strictly along logical engine boundaries; do not duplicate an
  integrated persisted-fixture contract across files.
