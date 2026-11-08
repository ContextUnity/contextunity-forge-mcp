//! ACDD task engine re-exported from forge-tasks with automatic extractor registration.

use std::sync::Once;

static INIT_EXTRACTOR: Once = Once::new();

/// Ensures that the production AST extractor and symbol facts extractor are registered into forge_tasks.
pub fn ensure_extractors() {
    INIT_EXTRACTOR.call_once(|| {
        forge_tasks::set_ast_extractor(|path, source| {
            let Some(language) = crate::engine::languages::for_path(std::path::Path::new(path))
            else {
                return Ok(None);
            };
            crate::engine::ast::extract_typed(path, language.id(), source).map(Some)
        });
        forge_tasks::milestones::set_symbol_facts_extractor(|path, source| {
            let Some(language) = crate::engine::languages::for_path(std::path::Path::new(path))
            else {
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
    session_fingerprint: Option<&str>,
) -> anyhow::Result<serde_json::Value> {
    ensure_extractors();
    forge_tasks::engine::submit_zero_arg(
        root,
        task_id,
        stage,
        action,
        findings,
        session_fingerprint,
    )
}

/// Verifies a claimed gate without advancing its lifecycle after wiring extractors.
pub fn verify_task(
    root: &std::path::Path,
    task_id: &str,
    timeout_seconds: Option<u64>,
) -> anyhow::Result<serde_json::Value> {
    ensure_extractors();
    forge_tasks::engine::verify_task(root, task_id, timeout_seconds)
}

/// Submits an automated gate with a bounded runtime timeout override.
pub fn submit_zero_arg_with_timeout(
    root: &std::path::Path,
    task_id: &str,
    stage: &str,
    action: Action,
    findings: Option<serde_json::Value>,
    timeout_seconds: Option<u64>,
    session_fingerprint: Option<&str>,
) -> anyhow::Result<serde_json::Value> {
    ensure_extractors();
    forge_tasks::engine::submit_zero_arg_with_timeout(
        root,
        task_id,
        stage,
        action,
        findings,
        timeout_seconds,
        session_fingerprint,
    )
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
