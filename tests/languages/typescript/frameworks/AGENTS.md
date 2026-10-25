# TypeScript Frameworks Test Suite

Subtree tests for TypeScript and JavaScript framework rules, projections, and decorators.

## Scope and Structure

Split framework-specific rules into dedicated modules and smaller children where
the test cases have natural boundaries:

- `nestjs.rs`: NestJS route-decorator coverage.
- `nuxt.rs` and its children: Nuxt auto-imports, project scope, components, and the persisted Nuxt/Vue/Pinia contract.
- `vue_runtime.rs` and its children: Vue setup provenance, template bindings, component bridges, iteration/slots, and composable members.
- `dom.rs`: Web APIs, standard browser globals, and DOM event listener resolution.
- Parser-selection coverage lives in the parent `grammar.rs` suite.

## Shared Helpers and Fixtures

- `tests/common/`: Shared workspace RAII harnesses.
- `tests/languages/support.rs`: Workspace runner, TS/JS syntax helpers, and edge matchers.

## Verification Commands

Run full TypeScript frameworks suite:
```bash
cargo test --test languages typescript::frameworks::
```

Run targeted framework modules:
```bash
cargo test --test languages typescript::frameworks::nestjs::
cargo test --test languages typescript::frameworks::nuxt::
cargo test --test languages typescript::frameworks::vue_runtime::
```

## Rules and Invariants

- Validate both ESM (`import`) and CommonJS (`require`) export semantics where applicable.
- Prove symbol definitions link to concrete callees without falling back to lexical heuristics.
- Group test modules strictly by framework domain (NestJS, Nuxt, Vue runtime).
  The persisted Nuxt/Vue/Pinia case and composable-return case remain intact
  single-contract modules rather than being split artificially.
