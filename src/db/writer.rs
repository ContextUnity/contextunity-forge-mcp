//! Database writer implementation, delegating to forge-index.

pub use crate::core::config::{policy, read_policy};
pub use crate::engine::pipeline::{
    build, delta, extract, validate_code_map_target, SourceSnapshotMismatch,
};
pub use forge_index::db::writer::*;
