use super::paths::verify_no_symlink_traversal;
use crate::forge_tasks::engine::orchestrator::workflow;
use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Subcommand)]
/// Standalone workflow operations.
pub enum WorkflowCommand {
    /// List standalone workflow documents without executing them.
    List,
    /// Show one validated root-gate workflow.
    Show {
        /// Workflow name or discovered root-relative path.
        selector: String,
    },
    /// Execute a standalone workflow in the current workspace.
    Run {
        /// Workflow name or discovered root-relative path.
        selector: String,
    },
    /// Clear standalone workflow execution run logs in .forge/runs/ and traces.
    Clear {
        /// Workflow name or discovered root-relative path.
        selector: String,
    },
}

/// Dispatch a workflow operation without task-store initialization.
pub fn run(root: &Path, command: WorkflowCommand) -> Result<Value> {
    match command {
        WorkflowCommand::List => Ok(json!({"workflows": workflow::list_workflows(root)?})),
        WorkflowCommand::Show { selector } => {
            let path = workflow::resolve_workflow(root, &selector)?;
            Ok(json!({"path":path, "profile":workflow::read_workflow(root, &path)?}))
        }
        WorkflowCommand::Run { selector } => {
            let path = workflow::resolve_workflow(root, &selector)?;
            workflow::run_standalone_workflow(root, &path)
        }
        WorkflowCommand::Clear { selector } => {
            let (workflow_path, slug) = match workflow::resolve_workflow(root, &selector) {
                Ok(path) => {
                    let stem = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&selector)
                        .to_owned();
                    (Some(path), stem)
                }
                Err(err) => {
                    let err_str = err.to_string();
                    if err_str.contains("WORKFLOW_AMBIGUOUS") {
                        return Err(err);
                    }
                    if err_str.contains("WORKFLOW_NOT_FOUND") {
                        anyhow::ensure!(
                            !selector.contains('/')
                                && !selector.contains('\\')
                                && !selector.contains(".."),
                            "WORKFLOW_NOT_FOUND: {}",
                            selector
                        );
                        (None, selector.clone())
                    } else {
                        return Err(err);
                    }
                }
            };

            let other_workflows: Vec<(PathBuf, String)> = match workflow::list_workflows(root) {
                Ok(paths) => paths
                    .into_iter()
                    .filter(|p| workflow_path.as_ref() != Some(p))
                    .filter_map(|p| {
                        let stem = p.file_stem().and_then(|s| s.to_str())?.to_owned();
                        Some((p, stem))
                    })
                    .collect(),
                Err(_) => Vec::new(),
            };

            for (other_path, other_slug) in &other_workflows {
                if other_slug == &slug {
                    let selected_name = workflow_path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| slug.clone());
                    anyhow::bail!(
                        "WORKFLOW_AMBIGUOUS: workflow '{}' has ambiguous artifact ownership with '{}'",
                        selected_name,
                        other_path.display()
                    );
                }
            }

            let runs_dir = verify_no_symlink_traversal(
                root,
                Path::new(".forge/runs"),
                "WORKFLOW_PATH_INVALID",
            )?;
            let mut to_delete = Vec::new();
            if runs_dir.exists() {
                'entry_loop: for entry in std::fs::read_dir(&runs_dir)? {
                    let entry = entry?;
                    let entry_path = entry.path();
                    let rel = entry_path.strip_prefix(root).unwrap_or(&entry_path);
                    verify_no_symlink_traversal(root, rel, "WORKFLOW_PATH_INVALID")?;
                    let file_type = entry.file_type()?;
                    let file_name = entry.file_name();
                    let file_name_str = file_name.to_string_lossy();
                    if !file_type.is_file() {
                        continue;
                    }

                    if let Some(target_match) = classify_workflow_log(&file_name_str, &slug) {
                        for (other_path, other_slug) in &other_workflows {
                            if let Some(other_match) =
                                classify_workflow_log(&file_name_str, other_slug)
                            {
                                match (target_match, other_match) {
                                    (WorkflowLogMatch::Exact, WorkflowLogMatch::Timestamp) => {}
                                    (WorkflowLogMatch::Timestamp, WorkflowLogMatch::Exact) => {
                                        continue 'entry_loop;
                                    }
                                    _ => {
                                        let selected_name = workflow_path
                                            .as_ref()
                                            .map(|p| p.display().to_string())
                                            .unwrap_or_else(|| slug.clone());
                                        anyhow::bail!(
                                            "WORKFLOW_LOG_AMBIGUOUS: log file '{}' has ambiguous workflow ownership between '{}' and '{}'",
                                            file_name_str,
                                            selected_name,
                                            other_path.display()
                                        );
                                    }
                                }
                            }
                        }
                        to_delete.push(entry.path());
                    }
                }
            }

            let mut files_deleted = 0;
            for path in to_delete {
                std::fs::remove_file(path)?;
                files_deleted += 1;
            }

            let mut traces_deleted = 0;
            let db_path = forge_tasks::database_path(root)?;
            if db_path.exists() {
                let conn = rusqlite::Connection::open(&db_path)?;
                let prefix = format!("{slug}#");
                traces_deleted = conn.execute(
                    "DELETE FROM gate_execution_traces WHERE substr(subject, 1, length(?1))=?1",
                    rusqlite::params![prefix],
                )?;
            }
            Ok(json!({
                "workflow": selector,
                "cleared": true,
                "files_deleted": files_deleted,
                "traces_deleted": traces_deleted,
            }))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkflowLogMatch {
    Exact,
    Timestamp,
}

fn classify_workflow_log(file_name: &str, slug: &str) -> Option<WorkflowLogMatch> {
    if file_name == format!("{slug}.log") {
        return Some(WorkflowLogMatch::Exact);
    }
    let rest = file_name
        .strip_prefix(&format!("{slug}-"))
        .or_else(|| file_name.strip_prefix(&format!("{slug}_")))?;
    let ts = rest.strip_suffix(".log")?;
    if is_valid_timestamp_suffix(ts) {
        Some(WorkflowLogMatch::Timestamp)
    } else {
        None
    }
}

fn is_valid_timestamp_suffix(ts: &str) -> bool {
    if ts.is_empty() || !ts.is_ascii() {
        return false;
    }

    if ts.len() >= 9 && ts.len() <= 19 && ts.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }

    if chrono::NaiveDate::parse_from_str(ts, "%Y-%m-%d").is_ok()
        || chrono::NaiveDate::parse_from_str(ts, "%Y_%m_%d").is_ok()
        || chrono::NaiveDate::parse_from_str(ts, "%Y%m%d").is_ok()
    {
        return true;
    }

    let formats = [
        "%Y-%m-%d-%H-%M-%S",
        "%Y-%m-%d_%H-%M-%S",
        "%Y-%m-%d-%H%M%S",
        "%Y-%m-%d_%H%M%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H%M%S",
        "%Y%m%d-%H%M%S",
        "%Y%m%d_%H%M%S",
        "%Y%m%dT%H%M%S",
    ];
    for fmt in formats {
        if chrono::NaiveDateTime::parse_from_str(ts, fmt).is_ok() {
            return true;
        }
    }

    chrono::DateTime::parse_from_rfc3339(ts).is_ok()
}
