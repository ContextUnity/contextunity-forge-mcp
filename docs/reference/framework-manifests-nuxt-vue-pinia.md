---
title: "Nuxt, Vue and Pinia manifest rules"
doc_type: guide
---

# Nuxt, Vue and Pinia manifest rules

The bundled `nuxt.toml` and `vue-runtime.toml` configure the existing finite
operations described in [framework manifests](framework-manifests.md).
Project overrides use those canonical filenames and the same merge or replacement
rules.

## Nuxt pages and composables

With a declared `nuxt` package, static Vue files under `pages/` define ordinary
`GET` routes owned by their page module. `pages/index.vue` maps to `/`,
`pages/catalog/detail.vue` to `/catalog/detail`, and
`pages/catalog/index.vue` to `/catalog`. Each segment admits identifier parts
separated by hyphens. Dynamic bracket paths and unsupported punctuation do not
produce routes. Existing HTTP request matching resolves only one exact route.

The component environment classifies `useFetch` and `useAsyncData` as Nuxt
autoimports in a declared Nuxt project. Local declarations retain ordinary
targets, and nested projects do not borrow another project's provenance.
Classification does not treat an AsyncData cache key as a URL or follow callback
execution.

The finite autoimport table also includes configured Nuxt helpers such as
`useRuntimeConfig`, `createError` and `defineEventHandler`. A separate Vue rule
classifies unshadowed `$fetch` with declared `nuxt` activation. These are external
classifications, not local handler edges. Module-specific helper rules retain
their declared-package activation; configured export and result contracts below
add exact persisted-module proof. The [Nuxt](../../skills/contextunity-forge/references/frameworks/nuxt.toml.example)
and [Vue/Pinia](../../skills/contextunity-forge/references/frameworks/vue-runtime.toml.example)
examples show partial extensions of the canonical bundles.

## Vue components and Pinia stores

Existing Vue SFC rules retain `<script setup>`, `defineProps`, `defineEmits`,
component options and runtime identities. Setup macro rules have empty activation;
Nuxt component discovery retains its project and configuration checks. Missing
components remain unresolved, and duplicate identities remain ambiguous without
an exact edge.

With a declared `pinia` package and verified imported `defineStore`, the options
object at argument 1 exposes original fields returned by `state` and methods
declared in `actions`. TypeScript and Vue consumers resolve those actual field
and method declarations. Dynamic options, missing members, shadowed or rebound
bindings, and duplicate imports produce no guessed exact target.

The default factory shape is `options`. The `setup_return` shape handles a
nonasync, nongenerator setup callback whose final literal object exposes actual
indexed callable declarations. Its local actions may be async. A public alias
retains the real callable's target identity; state or computed values without an
indexed declaration remain unresolved. Arbitrary nested property chains, getters,
plugins and `storeToRefs` do not gain inferred targets.

Non-returning `if` and `try` initialization may precede that sole final direct
return. Active alternate returns, abrupt exits, loops, unsupported control flow
and unsupported binding writes invalidate the configured setup summary. This
does not relax ordinary function-return inference or merge branch return values.

For an implicit `defineStore`, `configured_module = "@pinia/nuxt"` requires the
declared packages and that exact module in the persisted Nuxt configuration.
The environment's module descriptor selects named exports from its configured
`stores` directory with `.ts` extension. The indexed export and factory binding
prove `useUiStore`; its spelling is not a global allowlist or a filename guess.
Local declarations, parameters, imports, reassignments and duplicate providers
retain their normal nonexact guards. `components: false` controls component
discovery independently of configured module exports.

## External composable result members

The configured `@nuxtjs/i18n` module can supply
`call_results = { useI18n = ["t"] }`. A proven lexical
`const { t } = useI18n()` or renamed destructure retains external callable
provenance for that member. This creates no local `t` declaration or exact local
edge. An ordinary function parameter named `t` remains ordinary unresolved data;
local/imported composables continue to use their actual indexed return targets.
Package presence without the configured module does not establish the contract.

## Rule changes and cached providers

Persisted rule provenance, setup member target aliases, named exports and module
configuration participate in the existing canonical provider identity and delta
selection. Adding a previously missing store or removing a configured module
relinks unchanged consumers through cached provider facts. Provider/rule removal,
replacement and package changes preserve ordinary calls and imports. Verify warm
edits through reopened coverage and exact target IDs, then compare equal-final
cold and delta commitments. No source reread or full-workspace fallback establishes
an otherwise missing target.
