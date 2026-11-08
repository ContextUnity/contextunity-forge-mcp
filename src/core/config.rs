//! Workspace adapter configuration, scanner limits, and path resolution re-exported from forge-core.

use anyhow::Result;
pub use forge_core::config::*;
use std::path::Path;

/// Reads adapter policy from index SQLite metadata table.
pub fn read_policy(root: &Path, conn: &rusqlite::Connection) -> Result<Adapter> {
    let raw: String =
        conn.query_row("SELECT value FROM metadata WHERE key='adapter'", [], |r| {
            r.get(0)
        })?;
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    forge_core::config::read_policy_from_value(root, &value)
}
