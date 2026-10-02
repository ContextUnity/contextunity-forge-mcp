use crate::{
    core::tasks::{confined_path, Milestone},
    engine::tasks,
};
use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Subcommand)]
pub enum MigrateCommand {
    Preview { milestone_ref: Option<String> },
    Apply { milestone_ref: Option<String> },
    Verify { milestone_ref: Option<String> },
}

pub fn run(root: &Path, command: MigrateCommand) -> Result<Value> {
    let (reference, mode) = match command {
        MigrateCommand::Preview { milestone_ref } => (milestone_ref, "preview"),
        MigrateCommand::Apply { milestone_ref } => (milestone_ref, "apply"),
        MigrateCommand::Verify { milestone_ref } => (milestone_ref, "verify"),
    };
    if let Some(reference) = reference {
        return reconcile(root, &reference, mode);
    }
    let mut pending = vec![root.join("docs/milestones")];
    let mut references = Vec::new();
    while let Some(directory) = pending.pop() {
        if !directory.exists() {
            continue;
        }
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file()
                && entry.file_name().to_str().is_some_and(|name| {
                    name.as_bytes().first().is_some_and(u8::is_ascii_digit) && name.ends_with(".md")
                })
            {
                references.push(
                    entry
                        .path()
                        .strip_prefix(root)?
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    references.sort();
    let reports = references
        .iter()
        .map(|reference| reconcile(root, reference, mode))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"mode":mode,"manifests":reports}))
}

fn reconcile(root: &Path, reference: &str, mode: &str) -> Result<Value> {
    let (repository, project) = tasks::project_identity(root)?;
    let milestone = Milestone::parse_with_identity(
        &std::fs::read_to_string(confined_path(root, reference)?)?,
        &repository,
        &project,
    )?;
    if milestone.repository != repository || milestone.project != project {
        bail!("TASK_PROJECT_MISMATCH");
    }
    for spec in &milestone.tasks {
        if spec.status.as_deref() == Some("completed") {
            crate::core::tasks::gates::validate_durable_receipt(
                spec.receipt
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("TASK_RECEIPT_INVALID"))?,
                spec.contract_revision,
            )?;
        }
    }
    if mode == "apply" {
        return Ok(
            json!({"mode":mode,"tasks":tasks::store(root)?.sync(&milestone,reference,root)?}),
        );
    }
    let path = tasks::database_path(root)?;
    let connection = path
        .exists()
        .then(|| {
            rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        })
        .transpose()?;
    if let Some(connection) = &connection {
        connection.execute_batch("PRAGMA busy_timeout=5000; PRAGMA synchronous=NORMAL; PRAGMA mmap_size=268435456; BEGIN DEFERRED")?;
    }
    let mut conflicts = Vec::new();
    for spec in &milestone.tasks {
        let id = milestone.task_id(spec);
        let existing = connection
            .as_ref()
            .map(|c| crate::db::tasks_store::load(c, &id))
            .transpose()?
            .flatten();
        match existing {
            Some(task)
                if task.digest == milestone.digest(spec)?
                    && (spec.status.as_deref() != Some("completed")
                        || task.receipt == spec.receipt) => {}
            None if spec.status.as_deref() == Some("completed") => {}
            None => conflicts.push(json!({"task_id":id,"reason":"not_imported"})),
            Some(_) => {
                conflicts.push(json!({"task_id":id,"reason":"specification_or_receipt_mismatch"}))
            }
        }
    }
    if mode == "verify" && !conflicts.is_empty() {
        bail!(
            "TASK_MIGRATION_CONFLICT: {}",
            serde_json::to_string(&conflicts)?
        );
    }
    Ok(json!({"mode":mode,"conflicts":conflicts,"tasks":milestone.tasks.len()}))
}
