---
title: "GridViewSpec manifest bindings"
doc_type: guide
---

# GridViewSpec manifest bindings

The canonical `gridviewspec.toml` retains its 28 class-hierarchy receiver rules
and configures the existing finite operations described in
[framework manifests](framework-manifests.md). Workspace overrides use that
filename to extend the bundle or replace it with `replace = true`.

## Nested table datasources

The imported `grid_view_spec.types.table_v2.GridViewTable` accepts a directly
nested imported `GridViewDataSource` in its `datasource` keyword. A literal local
`endpoint` and static `method` resolve through the ordinary route matcher to one
actual backend route. An absent method defaults to `GET`; explicit `post` selects
`POST`. A dynamic method does not use the default.

Detached datasource constructors, shadowed imports, dynamic URLs, missing routes
and duplicate routes produce no guessed exact edge. Missing routes retain
unresolved coverage; duplicate routes retain ambiguous coverage.

## Renderer and action callbacks

An imported `GridViewColumn(renderer="stock_cart")` references the actual callback
registered by `window.GridView.registerRenderer("stock_cart", draw)`. The global
receiver may use a uniquely proved, unreassigned local alias such as
`var gv = window.GridView` followed by `gv.registerRenderer(...)`.

`GridViewColumn(extra={"action": "open_stock_cart"})` joins a separately configured
`gridviewspec_action` domain to the callback registered by `gv.registerAction(...)`.
Renderer keys use `gridviewspec_renderer`; neither domain borrows another pack's
definitions. These rules resolve
static callback identities; they do not execute callbacks or infer controller
APIs. Missing, dynamic, shadowed or reassigned identities stay nonexact, and
duplicate definitions retain ambiguous coverage.

The pack also admits the exact imported `registerRenderer` identity from
`../../grid-view/registry-api` for indexed runtime providers. Its finite runtime
builtin names classify configured `window.GridView` helpers as external behavior;
they do not create local declarations or make arbitrary runtime members exact.

For a renderer registered in another indexed project, a local `call_reference`
rule can add explicit `provider_scopes=[{workspace,project,package}]`. The adapter
must be enabled and the selected project must already have the named own-package
identity. This adds those providers to the local candidate set; it does not make
the domain global or choose between duplicate callbacks. Keep installation paths
and workspace aliases in local configuration. The bundle assumes no linked
frontend alias. Missing or uncertain providers produce no guessed exact edge.

## Persistence and rule causality

Typed provenance and cached provider facts preserve these joins across ordinary
provider and consumer edits. Rule removal or replacement removes their semantic
effects while retaining ordinary calls, imports and backend routes. Project
extensions retain the bundled receiver catalogue, and independent workspaces
keep their own effective rules. Literal HTMX requests continue to use the
separate bundled HTMX rules; interpolated URLs do not gain exact endpoint edges.
