//! ContextUnity Forge MCP code graph, persistence, language analysis, and tool APIs.
#![warn(missing_docs)]

/// Implements cli support.
pub mod cli;
/// Implements core support.
pub mod core;
/// Implements db support.
pub mod db;
/// Implements engine support.
pub mod engine;
/// Implements mcp support.
pub mod mcp;

/// Re-exported forge-core crate.
pub use forge_core;
/// Re-exported forge-index crate.
pub use forge_index;
/// Re-exported forge-tasks crate.
pub use forge_tasks;
