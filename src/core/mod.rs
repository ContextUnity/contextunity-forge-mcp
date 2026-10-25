/// Implements adapter support.
pub mod adapter;
/// Implements commitments support.
pub mod commitments;
/// Implements workspace configuration and limits support.
pub mod config;
/// Implements debug log support.
pub mod debug_log;
/// Shared operation presentation and metadata compaction.
pub mod format;
/// Implements fs support.
pub mod fs;
/// Implements Git command support.
pub mod git;
/// Implements models support.
pub mod models;
/// Implements response support.
pub mod response;
/// Implements schema support.
pub mod schema;
pub mod semantic;
/// Implements host and cgroup system support.
pub mod sys;
/// Implements tasks support.
pub mod tasks;
pub(crate) mod typed_facts;
