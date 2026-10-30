//! ContextUnity Forge core domain models, configuration, and commitments.

/// Returns the forge-core crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
