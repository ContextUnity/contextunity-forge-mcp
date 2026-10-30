//! ContextUnity Forge AST extraction, linking, and index persistence.

/// Returns the forge-index crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
