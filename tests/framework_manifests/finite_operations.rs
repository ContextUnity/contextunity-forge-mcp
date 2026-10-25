use super::*;

#[cfg(all(
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-vue",
    feature = "lang-html"
))]
mod support;

#[cfg(all(
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-vue",
    feature = "lang-html"
))]
use support::*;

#[cfg(all(
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-vue",
    feature = "lang-html"
))]
mod operations;

#[cfg(all(
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-vue",
    feature = "lang-html"
))]
mod callable_imports;

#[cfg(all(
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-vue",
    feature = "lang-html"
))]
mod target_context;

#[cfg(all(
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-vue",
    feature = "lang-html"
))]
#[test]
fn finite_framework_operations_preserve_canonical_targets_and_delta() {
    let mut failures = Vec::new();
    operations::run(&mut failures);
    callable_imports::run(&mut failures);
    target_context::run();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
