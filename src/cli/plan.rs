use super::paths::{confined_file, ResolvedPaths};
use anyhow::{bail, ensure, Result};
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
        PlanCommand::Show { selector } | PlanCommand::Run { selector, .. } => selector,
    };
    let matches = paths
        .iter()
        .filter(|path| {
            path.to_str() == Some(selector.as_str())
                || path.file_stem().and_then(|name| name.to_str()) == Some(selector.as_str())
        })
        .collect::<Vec<_>>();
    let path = match matches.as_slice() {
        [path] => *path,
        [] => bail!("PLAN_NOT_FOUND: {selector}"),
        _ => bail!("PLAN_AMBIGUOUS: {selector}"),
    };
    let plan = read_plan(root, path)?;
    match command {
        PlanCommand::Show { .. } => Ok(serde_json::to_value(plan)?),
        PlanCommand::Run { .. } => {
            run_plan_workflow(root, plan, &settings.profile, settings.explicit_profile)
        }
        PlanCommand::List => unreachable!(),
    }
}
