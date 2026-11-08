use super::*;
use crate::acdd::snapshot::{self, FixtureSnapshot};

pub(crate) struct DeliveredBaseline {
    pub root: MilestoneHarness,
    pub base: String,
    pub branch: String,
    pub milestone: Milestone,
    pub deliveries: Vec<String>,
    pub before: Vec<Value>,
    pub evidence: milestone_engine::IntegrationEvidence,
    pub snapshot: FixtureSnapshot,
    pub sqlite: Option<Vec<u8>>,
}

pub(crate) enum DeliveredCompletion<'a> {
    Default,
    Builder(&'a str),
}

pub(crate) enum DeliveredEvidence<'a> {
    Default {
        command: &'a str,
        passed: u64,
        failed: u64,
    },
    Observed {
        reviewer: &'a str,
        summary: &'a str,
    },
}

pub(crate) struct DeliveredTask<'a> {
    pub task_ref: &'a str,
    pub updates: Vec<(&'a str, String)>,
    pub completion: DeliveredCompletion<'a>,
}

pub(crate) struct DeliveredProfile<'a> {
    pub milestone_id: &'a str,
    pub active_path: &'a str,
    pub source: &'a str,
    pub config: &'a str,
    pub files: Vec<(&'a str, String)>,
    pub seed_paths: &'a [&'a str],
    pub commit_message: &'a str,
    pub disable_git_maintenance: bool,
    pub tasks: Vec<DeliveredTask<'a>>,
    pub evidence: DeliveredEvidence<'a>,
    pub capture_sqlite: bool,
}

pub(crate) fn delivered(profile: DeliveredProfile<'_>) -> DeliveredBaseline {
    snapshot::assert_safe_git_environment();
    let root = MilestoneHarness::workspace(DEFAULT_SEAM, "");
    if profile.disable_git_maintenance {
        MilestoneHarness::git_at(root.root(), &["config", "maintenance.auto", "false"]).unwrap();
        MilestoneHarness::git_at(root.root(), &["config", "gc.auto", "0"]).unwrap();
    }
    root.write("forge-mcp.yaml", profile.config);
    for (path, contents) in profile.files {
        root.write(path, &contents);
    }
    root.write(profile.active_path, profile.source);
    root.stage_paths(profile.seed_paths);
    root.commit_staged(profile.commit_message);
    let base = root.head();
    let branch = root.current_branch();
    let (milestone, store) = root.sync_document(profile.active_path, profile.source);
    let mut deliveries = Vec::with_capacity(profile.tasks.len());
    for task in profile.tasks {
        for (path, contents) in task.updates {
            root.write(path, &contents);
        }
        let spec = milestone
            .tasks
            .iter()
            .find(|spec| spec.task_ref == task.task_ref)
            .expect("delivered profile names an admitted task");
        let task_id = milestone.task_id(spec);
        let mode = match task.completion {
            DeliveredCompletion::Default => CompletionMode::Store {
                store: &store,
                task_id: &task_id,
            },
            DeliveredCompletion::Builder(builder) => CompletionMode::Builder {
                store: &store,
                task_id: &task_id,
                builder,
            },
        };
        root.complete_task(mode).unwrap();
        deliveries.push(root.head());
    }
    let before = milestone
        .tasks
        .iter()
        .map(|spec| store.inspect_details(&milestone.task_id(spec)).unwrap())
        .collect();
    let evidence: milestone_engine::IntegrationEvidence = match profile.evidence {
        DeliveredEvidence::Default {
            command,
            passed,
            failed,
        } => root.default_fixture_evidence(profile.milestone_id, command, passed, failed),
        DeliveredEvidence::Observed { reviewer, summary } => {
            let context =
                milestone_engine::handoff_context(root.root(), profile.milestone_id, &base)
                    .unwrap();
            let observed = milestone_engine::execute_milestone_verification(
                root.root(),
                profile.milestone_id,
                &base,
            )
            .unwrap();
            let check = &observed["verification"];
            let verification: milestone_engine::IntegrationVerification =
                serde_json::from_value(json!({
                    "command": check["command"], "exit_code": check["exit_code"],
                    "tests_passed": check["tests_passed"],
                    "tests_failed": check["tests_failed"],
                    "project_command": check["command_id"]
                }))
                .unwrap();
            serde_json::from_value(json!({
                "binding": context["binding"], "target_base": context["target_base"],
                "review": {"worker_id": reviewer, "decision": "pass", "summary": summary,
                    "scope_findings": [], "contract_findings": []},
                "verification": verification
            }))
            .unwrap()
        }
    };
    drop(store);
    let snapshot = root.checkpoint();
    let sqlite = profile
        .capture_sqlite
        .then(|| std::fs::read(root.path(".forge/tasks.sqlite")).unwrap());
    DeliveredBaseline {
        root,
        base,
        branch,
        milestone,
        deliveries,
        before,
        evidence,
        snapshot,
        sqlite,
    }
}

impl MilestoneHarness {
    pub(crate) fn open_task_store(&self) -> anyhow::Result<TasksStore> {
        TasksStore::open(&self.path(".forge/tasks.sqlite"))
    }

    pub(crate) fn head(&self) -> String {
        git_head(self.root())
    }

    pub(crate) fn default_fixture_evidence(
        &self,
        selector: &str,
        command: &str,
        passed: u64,
        failed: u64,
    ) -> milestone_engine::IntegrationEvidence {
        fixture_integration_evidence(self.root(), selector, command, passed, failed)
    }

    pub(crate) fn handoff_at(
        &self,
        id: &str,
        head: Option<&str>,
        evidence: &milestone_engine::IntegrationEvidence,
    ) -> anyhow::Result<Value> {
        milestone_engine::handoff(self.root(), id, head, evidence)
    }
}
