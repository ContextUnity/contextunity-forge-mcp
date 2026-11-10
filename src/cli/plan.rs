use super::paths::{confined_file, verify_no_symlink_traversal, ResolvedPaths};
use anyhow::{bail, ensure, Context, Result};
use clap::Subcommand;
use forge_tasks::engine::orchestrator::plan::{run_plan_workflow, PlanDocument};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Structured documentation plan operations.
#[derive(Debug, Subcommand)]
pub enum PlanCommand {
    /// List structured plan documents.
    List,
    /// Show validated steps and dependencies.
    Show {
        /// Plan name or discovered root-relative path.
        selector: String,
    },
    /// Execute a plan without creating a worktree or branch commit.
    Run {
        /// Plan name or discovered root-relative path.
        selector: String,
        /// Explicitly select execution in the current workspace.
        #[arg(long)]
        in_place: bool,
    },
    /// Clear working snapshots and traces for a plan without moving its manifest.
    Clear {
        /// Plan name or discovered root-relative path.
        selector: String,
    },
    /// Move a plan document to docs/plans/archive/ and prune its snapshot references.
    Archive {
        /// Plan name or discovered root-relative path.
        selector: String,
    },
}

fn read_plan(root: &Path, path: &Path) -> Result<PlanDocument> {
    let file = confined_file(root, path)?;
    let source = fs::read_to_string(file)?;
    let sections =
        crate::engine::docs::extract_with_details(&path.to_string_lossy(), &source, 0.0)?
            .into_iter()
            .map(|(section, _)| section)
            .collect::<Vec<_>>();
    PlanDocument::from_sections(path, source, &sections)
}

fn prune_plan_snapshots(root: &Path, path: &Path, _slug: &str) -> Result<usize> {
    let worktree = root.canonicalize()?;
    let namespace = format!(
        "{:x}",
        <sha2::Sha256 as sha2::Digest>::digest(worktree.to_string_lossy().as_bytes())
    );
    let plan_rel = path.strip_prefix(root).unwrap_or(path);
    let plan_tag = format!(
        "{:x}",
        <sha2::Sha256 as sha2::Digest>::digest(plan_rel.to_string_lossy().as_bytes())
    );
    let tag_prefix = &plan_tag[..16];

    let plan_prefix = format!("refs/forge/snapshots/{namespace}/plan-{tag_prefix}-");
    let gate_prefix = format!("refs/forge/snapshots/{namespace}/plan-gate-{tag_prefix}-");

    let output = forge_core::git::git_command(
        root,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/forge/snapshots/",
        ],
    )?;
    anyhow::ensure!(
        output.status.success(),
        "PLAN_CLEAR_FAILED: git for-each-ref failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut refs_to_delete = Vec::new();
    for reference in String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
    {
        if reference.is_empty() {
            continue;
        }
        let matches = reference.starts_with(&plan_prefix) || reference.starts_with(&gate_prefix);
        if matches {
            refs_to_delete.push(reference.to_owned());
        }
    }
    if !refs_to_delete.is_empty() {
        let mut stdin_data = String::new();
        for ref_name in &refs_to_delete {
            stdin_data.push_str(&format!("delete {ref_name}\n"));
        }
        forge_tasks::git::git_input(root, &["update-ref", "--stdin"], stdin_data.as_bytes())?;
    }
    Ok(refs_to_delete.len())
}

fn prune_plan_traces(root: &Path, path: &Path, slug: &str) -> Result<usize> {
    let db_path = forge_tasks::database_path(root)?;
    if !db_path.exists() {
        return Ok(0);
    }
    let conn = rusqlite::Connection::open(&db_path)?;
    let path_str = path.to_string_lossy().into_owned();
    let path_prefix = format!("{path_str}#");
    let slug_prefix = format!("{slug}#");
    let count = conn.execute(
        "DELETE FROM gate_execution_traces WHERE substr(subject, 1, length(?1))=?1 OR substr(subject, 1, length(?2))=?2",
        rusqlite::params![path_prefix, slug_prefix],
    )?;
    Ok(count)
}

fn discover(root: &Path, settings: &ResolvedPaths) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for directory in &settings.plans {
        let directory = forge_tasks::tasks::confined_path(root, directory)?;
        if !directory.exists() {
            continue;
        }
        let mut pending = vec![directory];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory)? {
                let entry = entry?;
                let path = entry.path();
                ensure!(
                    !entry.file_type()?.is_symlink(),
                    "PLAN_PATH_INVALID: symlink in plan discovery"
                );
                if path.is_dir() {
                    if path.file_name().is_some_and(|name| name != "archive") {
                        pending.push(path);
                    }
                } else if path.extension().is_some_and(|extension| extension == "md") {
                    let relative = path.strip_prefix(root)?.to_owned();
                    let source = fs::read_to_string(&path)?;
                    let sections = crate::engine::docs::extract_with_details(
                        &relative.to_string_lossy(),
                        &source,
                        0.0,
                    )?;
                    if sections
                        .iter()
                        .any(|(section, _)| section.doc_type == "plan")
                    {
                        paths.push(relative);
                    }
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

pub(super) fn run(root: &Path, command: PlanCommand, settings: &ResolvedPaths) -> Result<Value> {
    let paths = discover(root, settings)?;
    let selector = match &command {
        PlanCommand::List => return Ok(json!({"plans":paths})),
        PlanCommand::Show { selector }
        | PlanCommand::Run { selector, .. }
        | PlanCommand::Clear { selector }
        | PlanCommand::Archive { selector } => selector,
    };
    let archive_path_buf: PathBuf;
    let path = if let Some(exact) = paths.iter().find(|p| p.to_str() == Some(selector.as_str())) {
        exact
    } else {
        let matches = paths
            .iter()
            .filter(|path| {
                path.file_stem().and_then(|name| name.to_str()) == Some(selector.as_str())
                    || path.file_name().and_then(|name| name.to_str()) == Some(selector.as_str())
            })
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [path] => *path,
            [] => {
                let candidate1 = PathBuf::from("docs/plans/archive").join(format!("{selector}.md"));
                let candidate2 = PathBuf::from("docs/plans/archive").join(selector.as_str());
                if matches!(command, PlanCommand::Clear { .. }) && root.join(&candidate1).exists() {
                    archive_path_buf = candidate1;
                    &archive_path_buf
                } else if matches!(command, PlanCommand::Clear { .. })
                    && root.join(&candidate2).exists()
                {
                    archive_path_buf = candidate2;
                    &archive_path_buf
                } else {
                    bail!("PLAN_NOT_FOUND: {selector}")
                }
            }
            _ => bail!("PLAN_AMBIGUOUS: {selector}"),
        }
    };
    match command {
        PlanCommand::Clear { .. } => {
            let slug = path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or(selector.as_str());
            let refs_pruned = prune_plan_snapshots(root, path, slug)?;
            let traces_deleted = prune_plan_traces(root, path, slug)?;
            Ok(json!({
                "plan": slug,
                "path": path,
                "cleared": true,
                "refs_pruned": refs_pruned,
                "traces_deleted": traces_deleted,
            }))
        }
        PlanCommand::Archive { .. } => {
            let slug = path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or(selector.as_str());
            ensure!(
                !path.components().any(|c| c.as_os_str() == "archive"),
                "PLAN_ALREADY_ARCHIVED: {}",
                path.display()
            );
            let filename = path
                .file_name()
                .context("PLAN_PATH_INVALID: missing file name")?;
            let dest_relative = PathBuf::from("docs/plans/archive").join(filename);

            // ADR 0005: Strict confinement and symlink traversal defense
            let dest_path = verify_no_symlink_traversal(root, &dest_relative, "PLAN_PATH_INVALID")?;
            if dest_path.exists() {
                bail!(
                    "PLAN_ARCHIVE_EXISTS: destination archive file already exists: {}",
                    dest_relative.display()
                );
            }

            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
                verify_no_symlink_traversal(
                    root,
                    Path::new("docs/plans/archive"),
                    "PLAN_PATH_INVALID",
                )?;
            }

            let refs_pruned = prune_plan_snapshots(root, path, slug)?;
            let traces_deleted = prune_plan_traces(root, path, slug)?;
            let src_path = root.join(path);
            fs::rename(&src_path, &dest_path)?;
            Ok(json!({
                "plan": slug,
                "archived": true,
                "from": path,
                "to": dest_relative,
                "refs_pruned": refs_pruned,
                "traces_deleted": traces_deleted,
            }))
        }
        PlanCommand::Show { .. } => {
            let plan = read_plan(root, path)?;
            Ok(serde_json::to_value(plan)?)
        }
        PlanCommand::Run { .. } => {
            let plan = read_plan(root, path)?;
            run_plan_workflow(root, plan, &settings.profile, settings.explicit_profile)
        }
        PlanCommand::List => unreachable!(),
    }
}
