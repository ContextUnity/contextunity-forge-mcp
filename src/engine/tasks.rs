//! ACDD task engine re-exported from forge-tasks with automatic extractor registration.

use std::sync::Once;

static INIT_EXTRACTOR: Once = Once::new();

/// Ensures that the production AST extractor and symbol facts extractor are registered into forge_tasks.
pub fn ensure_extractors() {
    INIT_EXTRACTOR.call_once(|| {
        forge_tasks::set_ast_extractor(crate::engine::ast::extract_typed);
        forge_tasks::milestones::set_symbol_facts_extractor(|path, source| {
            let Some(language) = crate::engine::languages::for_path(std::path::Path::new(path)) else {
                return Ok(Vec::new());
            };
            let facts = crate::engine::ast::extract(path, language.id(), source)?;
            Ok(facts.nodes)
        });
    });
}

pub use forge_tasks::engine::*;

/// Claims a task gate after ensuring production extractors are wired.
pub fn claim(root: &std::path::Path, p: Claim) -> anyhow::Result<serde_json::Value> {
    ensure_extractors();
    forge_tasks::engine::claim(root, p)
}

/// Submits a task gate after ensuring production extractors are wired.
pub fn submit(root: &std::path::Path, p: Submit) -> anyhow::Result<serde_json::Value> {
    ensure_extractors();
    forge_tasks::engine::submit(root, p)
}

/// Submits an automated task gate without caller proof fields after ensuring production extractors are wired.
pub fn submit_zero_arg(
    root: &std::path::Path,
    task_id: &str,
    stage: &str,
    action: Action,
    findings: Option<serde_json::Value>,
) -> anyhow::Result<serde_json::Value> {
    ensure_extractors();
    forge_tasks::engine::submit_zero_arg(root, task_id, stage, action, findings)
}

/// Manages a task lifecycle after ensuring production extractors are wired.
pub fn manage(root: &std::path::Path, p: Manage) -> anyhow::Result<serde_json::Value> {
    ensure_extractors();
    forge_tasks::engine::manage(root, p)
}

/// Lists tasks after ensuring production extractors are wired.
pub fn list(root: &std::path::Path, p: List) -> anyhow::Result<serde_json::Value> {
    ensure_extractors();
    forge_tasks::engine::list(root, p)
}

/// Reopens a task after ensuring production extractors are wired.
pub fn reopen(root: &std::path::Path, task_id: &str) -> anyhow::Result<forge_tasks::store::Task> {
    ensure_extractors();
    forge_tasks::engine::reopen(root, task_id)
}

/// Resets a task after ensuring production extractors are wired.
pub fn reset(root: &std::path::Path, task_id: &str) -> anyhow::Result<forge_tasks::store::Task> {
    ensure_extractors();
    forge_tasks::engine::reset(root, task_id)
}

pub use forge_tasks::milestones;

/// Task engine compatibility alias.
#[allow(clippy::module_inception)]
pub mod tasks {
    pub use super::*;
}
