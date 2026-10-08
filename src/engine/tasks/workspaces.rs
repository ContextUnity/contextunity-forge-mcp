use super::TaskSettings;
use crate::{
    core::tasks::{confined_path, shorten_task_response_commits, valid_identity},
    db::tasks_store::TasksStore,
};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
pub(super) struct LinkedConfig {
    name: String,
    path: PathBuf,
    enabled: Option<bool>,
    tasks: Option<LinkedTasks>,
}

#[derive(Deserialize)]
struct LinkedTasks {
    #[serde(default)]
    enabled: bool,
    #[serde(default = "default_milestones")]
    milestones_dir: String,
    #[serde(default = "default_guidance")]
    agents_guidance: String,
}
fn default_milestones() -> String {
    "docs/milestones".into()
}
fn default_guidance() -> String {
    "AGENTS.md".into()
}

#[derive(Default, Deserialize)]
struct LinkedIdentity {
    task_repository: Option<String>,
    task_project: Option<String>,
}

fn declared_projects(directory: &Path) -> Result<BTreeSet<String>> {
    let mut projects = BTreeSet::new();
    for dir in [directory.to_path_buf(), directory.join("archive")] {
        if !dir.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file()
                || entry.path().extension().is_none_or(|ext| ext != "md")
            {
                continue;
            }
            let text = std::fs::read_to_string(entry.path())?;
            let text = text.replace("\r\n", "\n");
            let Some(header) = text
                .strip_prefix("---\n")
                .and_then(|body| body.split_once("\n---\n"))
                .map(|(header, _)| header)
            else {
                continue;
            };
            let meta: serde_yaml::Value = serde_yaml::from_str(header)?;
            if let Some(project) = meta["project"].as_str() {
                valid_identity(project)?;
                projects.insert(project.to_owned());
            }
        }
    }
    Ok(projects)
}

pub(super) struct Workspace {
    pub name: String,
    pub root: PathBuf,
    pub repository: String,
    pub project: String,
    milestones: Vec<PathBuf>,
    guidance: PathBuf,
}
impl Workspace {
    pub fn open(&self, database: &Path) -> Result<TasksStore> {
        TasksStore::open_project(database, &self.repository, &self.project)
    }
    pub fn envelope(&self, mut value: Value) -> Result<Value> {
        let object = value
            .as_object_mut()
            .context("task envelope must be an object")?;
        object.insert("workspace_root".into(), json!(self.root));
        object.insert("agents_guidance".into(), json!(self.guidance));
        object.insert("workspace".into(), json!(self.name));
        shorten_task_response_commits(&mut value);
        Ok(value)
    }
    pub fn guidance_envelope(&self, value: Value) -> Result<Value> {
        let mut value = self.envelope(value)?;
        let stage = if value["status"] == "completed" {
            "completed"
        } else {
            value["stage"].as_str().context("task stage missing")?
        };
        let agent_type = value["spec"]["agent_type"].as_str().unwrap_or("worker");
        let (subagent_role, actions): (&str, &[&str]) = match stage {
            "contract/v1" => (
                "contract_author",
                &[
                    "Author or verify the public-seam test proof (or direct-proof exit code 0 for pre-existing code).",
                    "Submit contract proof.",
                ],
            ),
            "build/v1" => (
                "builder",
                &[
                    "Implement the approved contract inside the allowed write scope.",
                    "Run the owning repository's tests and lint checks, then submit passing test proof.",
                ],
            ),
            "review/v1" => (
                "independent_reviewer",
                &[
                    "Inspect candidate diff using inspect_cmd.",
                    "Verify the 5 review contours: paths, claims, concurrency, project_isolation, administration.",
                    "Accept legitimate adjacent defect fixes within scope and submit review proof.",
                ],
            ),
            "deliver/v1" => (
                "delivery_reviewer",
                &[
                    "Submit delivery proof first; Forge writes the durable receipt into the milestone document.",
                    "After delivery, commit scoped files, tests, and the milestone receipt under the repository's Git permissions.",
                ],
            ),
            "completed" => (
                "completed",
                &["Read the durable task receipt for completed work."],
            ),
            _ => bail!("TASK_STAGE_INVALID"),
        };
        let guidance_exists = self.guidance.is_file();
        let mut steps = if guidance_exists {
            vec![format!(
                "Read the task instructions at {}.",
                self.guidance.display()
            )]
        } else {
            Vec::new()
        };
        steps.extend(actions.iter().map(|action| (*action).to_owned()));
        let builder = value["gates"].as_array().and_then(|gates| {
            gates.iter().rev().find_map(|gate| {
                if gate["stage"] != "build/v1" || gate["state"] != "passed" {
                    return None;
                }
                let evidence = gate["evidence"].as_str()?;
                let parsed: Value = serde_json::from_str(evidence).ok()?;
                parsed["worker_id"].as_str().map(str::to_owned)
            })
        });
        let needs_independent_reviewer = matches!(stage, "review/v1" | "deliver/v1");
        let warning = if guidance_exists {
            Value::Null
        } else {
            steps.extend(
                [
                    "Use the current stage and task scope to continue.",
                    "Record a verifiable proof in the task store.",
                    "Request independent review before delivery.",
                ]
                .map(str::to_owned),
            );
            json!({
                "code": "TASK_GUIDANCE_MISSING",
                "message": "Task guidance file is missing; use the inline steps and scaffold repository instructions.",
                "path": self.guidance,
            })
        };
        let review_policy = if needs_independent_reviewer {
            Some("Reviewers must accept legitimate defect fixes registered via extend-scope or reopened tasks and verify changes against the declared scope (preventing uncontracted scope creep or overengineering).")
        } else {
            None
        };
        let mut workflow_guidance = json!({
            "active_stage": stage,
            "agent_type": agent_type,
            "subagent_role": subagent_role,
            "steps": steps,
            "instructions_path": self.guidance,
            "independence_rule": needs_independent_reviewer.then_some("Use a reviewer worker_id different from the accepted build worker_id."),
            "independent_from_worker_id": if needs_independent_reviewer { builder } else { None },
            "warning": warning,
            "documentation_url": (!guidance_exists).then_some("https://github.com/ContextUnity/contextunity-forge-mcp/blob/main/docs/reference/acdd.md"),
        });
        if let Some(policy) = review_policy {
            workflow_guidance["review_policy"] = json!(policy);
        }
        value
            .as_object_mut()
            .context("task envelope must be an object")?
            .insert("workflow_guidance".into(), workflow_guidance);
        Ok(value)
    }
    pub fn manifests(&self) -> Result<Vec<String>> {
        let mut directories = self.milestones.clone();
        let mut manifests = Vec::new();
        while let Some(directory) = directories.pop() {
            if !directory.exists() {
                continue;
            }
            for entry in std::fs::read_dir(directory)? {
                let entry = entry?;
                let kind = entry.file_type()?;
                if kind.is_dir() {
                    directories.push(entry.path());
                } else if kind.is_file()
                    && entry.file_name().to_str().is_some_and(|name| {
                        name.as_bytes().first().is_some_and(u8::is_ascii_digit)
                            && name.ends_with(".md")
                    })
                {
                    manifests.push(
                        entry
                            .path()
                            .strip_prefix(&self.root)?
                            .to_string_lossy()
                            .into_owned(),
                    );
                }
            }
        }
        manifests.sort();
        Ok(manifests)
    }
    pub fn resolve_manifest(&self, reference: &str) -> Result<Option<String>> {
        let reference = reference.trim();
        if reference.is_empty() {
            bail!("milestone reference cannot be empty");
        }
        let mut reference_components = Path::new(reference).components();
        let is_path_reference = !matches!(
            reference_components.next(),
            Some(std::path::Component::Normal(_))
        ) || reference_components.next().is_some();
        if is_path_reference {
            let direct = confined_path(&self.root, reference)?;
            if !direct.is_file() {
                return Ok(None);
            }
            let relative = direct
                .strip_prefix(&self.root)
                .context("resolved milestone path escaped workspace root")?;
            let selected = relative.to_string_lossy().into_owned();
            if direct
                .extension()
                .is_some_and(|extension| extension == "md")
            {
                // Explicit paths are validated as milestone manifests by the
                // registry after resolution. Keep path references compatible
                // with workspaces whose manifests live outside the numbered
                // directories used by implicit discovery.
                return Ok(Some(selected));
            }
            bail!(
                "milestone path '{}' is not a Markdown manifest in workspace '{}'",
                reference,
                self.name
            );
        }
        let manifests = self.manifests()?;
        if manifests.is_empty() {
            return Ok(None);
        }

        let mut exact_matches = Vec::new();
        let mut fuzzy_matches = Vec::new();

        let numeric_ref = if !reference.is_empty() && reference.bytes().all(|b| b.is_ascii_digit())
        {
            reference.parse::<u64>().ok()
        } else {
            None
        };

        for manifest in &manifests {
            let path = self.root.join(manifest);
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
            let (prefix_str, slug) = stem.split_once('-').unwrap_or(("", stem));
            let prefix_num = prefix_str.parse::<u64>().ok();
            let id = crate::engine::milestones::file_id(&path)
                .with_context(|| format!("failed to read milestone id from '{manifest}'"))?;

            if id.as_deref() == Some(reference) || stem == reference || file_name == reference {
                exact_matches.push(manifest.clone());
                continue;
            }

            if let Some(target_num) = numeric_ref {
                if prefix_num == Some(target_num) || prefix_str == reference {
                    exact_matches.push(manifest.clone());
                    continue;
                }
            }

            if slug == reference
                || reference.strip_prefix("m-").is_some_and(|r| r == slug)
                || id
                    .as_deref()
                    .and_then(|i| i.strip_prefix("m-"))
                    .is_some_and(|i| i == reference)
            {
                fuzzy_matches.push(manifest.clone());
            }
        }

        let is_archived_manifest = |m: &str| {
            Path::new(m)
                .components()
                .any(|c| c.as_os_str() == "archive")
        };
        let select_preferred = |matches: Vec<String>| -> Result<Option<String>> {
            if matches.is_empty() {
                return Ok(None);
            }
            if matches.len() == 1 {
                return Ok(Some(matches.into_iter().next().unwrap()));
            }
            let archive_scoped_reference = Path::new(reference)
                .components()
                .any(|component| component.as_os_str() == "archive");
            if !archive_scoped_reference {
                let active: Vec<String> = matches
                    .iter()
                    .filter(|m| !is_archived_manifest(m))
                    .cloned()
                    .collect();
                if active.len() == 1 {
                    return Ok(Some(active.into_iter().next().unwrap()));
                }
                if active.len() > 1 {
                    bail!(
                        "ambiguous milestone reference '{}' matches multiple active documents in workspace '{}': {:?}",
                        reference,
                        self.name,
                        active
                    );
                }
            }
            bail!(
                "ambiguous milestone reference '{}' matches multiple documents in workspace '{}': {:?}",
                reference,
                self.name,
                matches
            );
        };

        if let Some(matched) = select_preferred(exact_matches)? {
            return Ok(Some(matched));
        }
        if let Some(matched) = select_preferred(fuzzy_matches)? {
            return Ok(Some(matched));
        }

        Ok(None)
    }
}

pub(super) struct Registry {
    pub database: PathBuf,
    workspaces: Vec<Workspace>,
}
impl Registry {
    pub fn load(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let settings = TaskSettings::load(&root)?;
        let database = settings.path(&root)?;
        let guidance = settings.agents_guidance.unwrap_or_else(default_guidance);
        let repository = settings.task_repository;
        let project = settings.task_project.unwrap_or_else(|| repository.clone());
        let primary_milestone_dirs = crate::engine::milestones::configured_milestone_dirs(&root);
        let mut primary_milestones = Vec::new();
        let mut subproject_milestones: std::collections::BTreeMap<String, Vec<PathBuf>> =
            std::collections::BTreeMap::new();
        for dir in primary_milestone_dirs {
            let rel = dir.strip_prefix(&root).unwrap_or(&dir);
            let inferred = crate::core::tasks::infer_project_from_path(rel);
            if let Some(proj) = inferred {
                subproject_milestones
                    .entry(proj)
                    .or_default()
                    .push(dir.clone());
            } else {
                primary_milestones.push(dir.clone());
            }
            for declared in declared_projects(&dir)? {
                if declared == project {
                    primary_milestones.push(dir.clone());
                } else {
                    subproject_milestones
                        .entry(declared)
                        .or_default()
                        .push(dir.clone());
                }
            }
        }
        primary_milestones.sort();
        primary_milestones.dedup();
        if primary_milestones.is_empty() {
            primary_milestones.push(confined_path(&root, &default_milestones())?);
        }
        let guidance_path = confined_path(&root, &guidance)?;
        let mut workspaces = vec![Workspace {
            name: repository.clone(),
            repository: repository.clone(),
            project: project.clone(),
            milestones: primary_milestones,
            guidance: guidance_path.clone(),
            root: root.clone(),
        }];
        let mut names = BTreeSet::from([workspaces[0].name.clone()]);
        let mut namespaces = BTreeSet::from([(
            workspaces[0].repository.clone(),
            workspaces[0].project.clone(),
        )]);
        for (sub_name, sub_dirs) in subproject_milestones {
            if sub_name == repository || sub_name == project {
                continue;
            }
            if names.insert(sub_name.clone())
                && namespaces.insert((repository.clone(), sub_name.clone()))
            {
                workspaces.push(Workspace {
                    name: sub_name.clone(),
                    repository: repository.clone(),
                    project: sub_name,
                    milestones: {
                        let mut dirs = sub_dirs;
                        dirs.sort();
                        dirs.dedup();
                        dirs
                    },
                    guidance: guidance_path.clone(),
                    root: root.clone(),
                });
            }
        }
        for linked in settings.linked_workspaces {
            let Some(tasks) = linked
                .tasks
                .filter(|tasks| tasks.enabled && linked.enabled != Some(false))
            else {
                continue;
            };
            valid_identity(&linked.name)?;
            if linked.name == "all" || !names.insert(linked.name.clone()) {
                bail!("TASK_WORKSPACE_INVALID: duplicate or reserved workspace name");
            }
            let path = root.join(&linked.path);
            if !path.exists() {
                continue;
            }
            let path = path.canonicalize()?;
            if !path.is_dir() {
                bail!("TASK_WORKSPACE_INVALID: expected directory");
            }
            if workspaces.iter().any(|workspace| workspace.root == path) {
                bail!("TASK_WORKSPACE_INVALID: duplicate workspace root");
            }
            let identity = match std::fs::read_to_string(path.join("forge-mcp.yaml")) {
                Ok(config) => serde_yaml::from_str::<LinkedIdentity>(&config)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    LinkedIdentity::default()
                }
                Err(error) => return Err(error.into()),
            };
            let repository = identity
                .task_repository
                .unwrap_or_else(|| linked.name.clone());
            let project = identity.task_project.unwrap_or_else(|| repository.clone());
            valid_identity(&repository)?;
            valid_identity(&project)?;
            if !namespaces.insert((repository.clone(), project.clone())) {
                bail!("TASK_WORKSPACE_INVALID: duplicate task namespace");
            }
            workspaces.push(Workspace {
                name: linked.name,
                repository,
                project,
                milestones: vec![confined_path(&path, &tasks.milestones_dir)?],
                guidance: confined_path(&path, &tasks.agents_guidance)?,
                root: path,
            });
        }
        Ok(Self {
            database,
            workspaces,
        })
    }
    pub fn select(&self, name: Option<&str>) -> Result<&Workspace> {
        match name {
            None => Ok(&self.workspaces[0]),
            Some(name) => self
                .workspaces
                .iter()
                .find(|workspace| workspace.name == name || workspace.project == name)
                .context("TASK_WORKSPACE_NOT_FOUND"),
        }
    }
    pub fn selected(&self, repository: Option<&str>) -> Result<Vec<&Workspace>> {
        if repository == Some("all") {
            Ok(self.workspaces.iter().collect())
        } else {
            Ok(vec![self.select(repository)?])
        }
    }
    pub fn owner(&self, id: &str) -> Result<&Workspace> {
        let mut components = id.split('/');
        let repository = components.next();
        let project = components.next();
        self.workspaces
            .iter()
            .find(|workspace| {
                Some(workspace.repository.as_str()) == repository
                    && Some(workspace.project.as_str()) == project
            })
            .context("TASK_PROJECT_MISMATCH")
    }
    pub fn check_worktree(&self, owner: &Workspace, worktree: &Path) -> Result<()> {
        if worktree.starts_with(&owner.root) {
            return Ok(());
        }
        let foreign = self
            .workspaces
            .iter()
            .filter(|workspace| worktree.starts_with(&workspace.root))
            .max_by_key(|workspace| workspace.root.components().count());
        if foreign.is_some_and(|workspace| workspace.root != owner.root) {
            bail!("TASK_SCOPE_INVALID: worktree belongs to another repository");
        }
        Ok(())
    }
    pub fn check_scope(
        &self,
        owner: &Workspace,
        paths: &[String],
        worktree: Option<&Path>,
    ) -> Result<()> {
        for root in [&owner.root, worktree.unwrap_or(&owner.root)] {
            for path in paths {
                let resolved = confined_path(root, path)?;
                let directory = path.ends_with('/') || resolved.is_dir();
                for foreign in &self.workspaces {
                    if foreign.name == owner.name
                        || foreign.root == owner.root
                        || owner.root.starts_with(&foreign.root)
                    {
                        continue;
                    }
                    if resolved.starts_with(&foreign.root)
                        || (directory && foreign.root.starts_with(&resolved))
                    {
                        bail!("TASK_SCOPE_INVALID: path includes another configured repository");
                    }
                }
            }
        }
        Ok(())
    }
    pub fn resolve_milestone(
        &self,
        workspace: Option<&str>,
        reference: &str,
    ) -> Result<(&Workspace, String)> {
        let primary = self.select(None)?;
        let mut found = match workspace {
            Some("all") => {
                let mut matches = Vec::new();
                for candidate in &self.workspaces {
                    if let Some(path) = candidate.resolve_manifest(reference)? {
                        matches.push((candidate, path));
                    }
                }
                matches
            }
            Some(name) => {
                let candidate = self.select(Some(name))?;
                candidate
                    .resolve_manifest(reference)?
                    .map(|path| vec![(candidate, path)])
                    .unwrap_or_default()
            }
            None => primary
                .resolve_manifest(reference)?
                .map(|path| vec![(primary, path)])
                .unwrap_or_default(),
        };

        if workspace.is_none() && found.is_empty() {
            for candidate in &self.workspaces {
                if candidate.name == primary.name {
                    continue;
                }
                if let Some(path) = candidate.resolve_manifest(reference)? {
                    found.push((candidate, path));
                }
            }
        }

        let (resolved_ws, path) = match found.len() {
            0 => {
                if let Some(name) = workspace.filter(|name| *name != "all") {
                    bail!(
                        "milestone reference '{}' not found in workspace '{}'",
                        reference,
                        name
                    );
                }
                bail!(
                    "milestone reference '{}' not found in workspace selection",
                    reference
                );
            }
            1 => found.pop().context("resolved milestone missing")?,
            _ => {
                let matched_ws: Vec<&str> = found.iter().map(|(w, _)| w.name.as_str()).collect();
                bail!(
                    "ambiguous milestone reference '{}' found across multiple workspaces: {:?}",
                    reference,
                    matched_ws
                );
            }
        };

        let inferred = crate::core::tasks::infer_project_from_path(Path::new(&path));
        let full_path = confined_path(&resolved_ws.root, &path)?;
        let text = std::fs::read_to_string(full_path)?;
        let parsed = crate::core::tasks::Milestone::parse_with_identity(
            &text,
            &resolved_ws.repository,
            inferred.as_deref().unwrap_or(&resolved_ws.project),
        )
        .with_context(|| format!("invalid milestone manifest '{path}'"))?;
        let owner = self.owner(&format!("{}/{}", parsed.repository, parsed.project))?;
        Ok((owner, path))
    }
}
