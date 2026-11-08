# Boundary Fixture Mechanics

- Keep shared Git history construction and typed fixture observations in `harness.rs`, extending `TaskHarness`, only when at least two boundary tests call them.
- Keep scenarios, mutation data, production actions, and every contract assertion in the parent `boundaries.rs` tests.
- Do not add assertion helpers or one-caller scenario runners here.
