use super::TaskSettings;
use crate::{
    core::tasks::{confined_path, valid_identity},
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

pub(super) struct Workspace {
    pub name: String,
    pub root: PathBuf,
    pub repository: String,
    pub project: String,
    milestones: PathBuf,
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
            "design/v1" => (
                "planner",
                &["Inspect the task target, scope, and invariants.", "Record the design proof before advancing the gate."],
            ),
            "contract/v1" => (
                "contract_author",
                &["Write and run a failing public-seam test.", "Have an independent reviewer verify the contract before submitting the proof."],
            ),
            "build/v1" => (
                "builder",
                &["Implement the approved contract inside the allowed write scope.", "Run the seam test and clippy, then submit passing JSON test proof."],
            ),
            "review/v1" => (
                "independent_reviewer",
                &["Review the candidate against the contract and five review contours.", "Run the required verification and submit inline JSON review proof."],
            ),
            "deliver/v1" => (
                "delivery_reviewer",
                &[
                    "Verify accepted build and review proofs against the current milestone specification.",
                    "Submit delivery proof for the reviewed candidate commit SHA; delivery writes the task receipt in the milestone document.",
                    "Amend the candidate commit with the generated receipt so the task has one commit.",
                ],
            ),
            "completed" => ("completed", &["Read the durable task receipt for completed work."]),
            _ => bail!("TASK_STAGE_INVALID"),
        };
        let guidance_exists = self.guidance.is_file();
        let mut steps = if guidance_exists {
            vec![format!("Read the task instructions at {}.", self.guidance.display())]
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
            steps.extend([
                "Use the current stage and task scope to continue.",
                "Record a verifiable proof in the task store.",
                "Request independent review before delivery.",
            ].map(str::to_owned));
            json!({
                "code": "TASK_GUIDANCE_MISSING",
                "message": "Task guidance file is missing; use the inline steps and scaffold repository instructions.",
                "path": self.guidance,
            })
        };
        let workflow_guidance = json!({
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
        value
            .as_object_mut()
            .context("task envelope must be an object")?
            .insert("workflow_guidance".into(), workflow_guidance);
        Ok(value)
    }
    pub fn manifests(&self) -> Result<Vec<String>> {
        let mut directories = vec![self.milestones.clone()];
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
        let mut workspaces = vec![Workspace {
            name: repository.clone(),
            repository,
            project,
            milestones: confined_path(&root, &default_milestones())?,
            guidance: confined_path(&root, &guidance)?,
            root: root.clone(),
        }];
        let mut names = BTreeSet::from([workspaces[0].name.clone()]);
        let mut namespaces = BTreeSet::from([(
            workspaces[0].repository.clone(),
            workspaces[0].project.clone(),
        )]);
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
                milestones: confined_path(&path, &tasks.milestones_dir)?,
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
                .find(|workspace| workspace.name == name)
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
        let foreign = self
            .workspaces
            .iter()
            .filter(|workspace| worktree.starts_with(&workspace.root))
            .max_by_key(|workspace| workspace.root.components().count());
        if foreign.is_some_and(|workspace| workspace.name != owner.name) {
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
                    if foreign.name == owner.name || owner.root.starts_with(&foreign.root) {
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
}
