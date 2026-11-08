//! Shared support for integration tests.

pub mod assertions;
pub mod cli;
pub mod mcp_client;
pub mod mcp_harness;
pub mod sqlite;
pub mod workspace;

pub use workspace::Workspace;
