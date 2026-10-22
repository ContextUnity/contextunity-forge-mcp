---
title: "HTMX and Alpine framework manifests"
doc_type: api
---

# HTMX and Alpine framework manifests

The bundled `htmx.toml` and `alpinejs.toml` packs configure the finite typed
operations described in [framework manifests](framework-manifests.md). Their
explicit request, store and event rules have empty activation and work without
`package.json` or `pyproject.toml`.

## HTMX requests

`hx-get`, `hx-post`, `hx-put`, `hx-patch` and `hx-delete`, including their
`data-hx-` forms, supply a URL and HTTP method. A literal local URL produces a
`calls_endpoint` relation only when it selects one matching route, including
routes supplied by the Django and FastAPI packs. Missing routes retain
unresolved coverage; duplicate routes retain ambiguous coverage without an edge.

Remote, dynamic, empty and current-page values produce no endpoint edge.
`hx-target` and `hx-swap` remain request context. Context-only elements do not
create endpoint identities. The pack also retains configured `trigger`,
`include` and `vals` context.

## Alpine stores and events

In JavaScript, an unshadowed `Alpine.store('cart', { checkout() {} })` registers
the actual object under its literal key. `$store.cart.checkout()` resolves to
that object's original `checkout` declaration through ordinary member resolution
and produces a `calls` relation.

An unshadowed JavaScript `$dispatch('cart-open')` produces a `references`
relation to the actual embedded handler scope of a unique HTML
`x-on:cart-open` listener. Listener extraction shares the existing script-attribute
parse; it does not create a second handler declaration.

Store and event joins require one statically verified target in their project
and domain. Missing targets retain unresolved coverage and duplicate targets
retain ambiguous coverage; neither creates a guessed edge. Shadowed roots and
unsupported dynamic identities do not establish exact targets. These static
joins do not model browser event propagation, runtime registrations or DOM scope.

The pack's finite implicit-receiver table also classifies configured Alpine magic
members such as `$root.querySelector` and `$el.addEventListener` as external
runtime behavior when eligible. This classification supplies no local callback
target. Ordinary local bindings and unsupported magic members retain their normal
resolution guards. The [HTMX example](../../skills/contextunity-forge/references/frameworks/htmx.toml.example)
shows the complete request operation without duplicating the Alpine rules.

## Overrides and incremental indexing

Use `.forge/frameworks/htmx.toml` or `.forge/frameworks/alpinejs.toml` for a
workspace-local override. The filename stem identifies the pack. Follow the
[manifest merge and validation rules](framework-manifests.md) when extending or
replacing rules. Removing an effective rule removes its semantic relations;
ordinary language calls and imports remain available.

The existing typed facts carry rule provenance through persistence and delta
indexing. Provider, listener, consumer and effective-rule changes relink affected
facts. Equivalent final sources and rules produce equivalent cold and delta
graphs, coverage and commitments. API names, attribute prefixes and literal
keys are manifest data and can be renamed together with their sources.
