//! ContextUnity Forge ACDD task lifecycle and coordination.

/// Returns the forge-tasks crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
