use crate::acdd::{support::*, tasks::harness::TaskHarness};
use forge_tasks::{
    engine::orchestrator::task_runner::TaskRunOptions,
    tasks::profile::{GateDef, ProofKind},
};
use std::{fs, path::Path, time::Duration};

pub(super) const MANIFEST: &str = "docs/milestones/010-lifecycle.md";

pub(super) struct LifecycleHarness {
    pub task: TaskHarness,
    pub options: TaskRunOptions,
}

impl LifecycleHarness {
    pub fn new() -> Self {
        let mut profile = runner_profile(DEFAULT_SEAM, "");
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/acdd/orchestrator/lifecycle/role_fixture.py"
        );
        let executable = env!("CARGO_BIN_EXE_contextunity-forge-mcp");
        let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
        for role in profile.roles.values_mut() {
            role.guidance =
                Some("Execute the external fixture protocol using real task operations.".into());
            role.runner = Some(format!(
                "python3 {} {} {{session_id}} '{{prompt}}'",
                quote(fixture),
                quote(executable)
            ));
            role.prompt_template =
                Some("Execute {role} for {task_id} at {stage} in {worktree}. {goal}".into());
        }
        let block = profile
            .gates
            .iter_mut()
            .find_map(|gate| gate.tasks.as_mut())
            .unwrap();
        for gate in &mut block.gates {
            gate.guidance.clear();
        }
        let delivery = block
            .gates
            .iter()
            .position(|gate| gate.proof.kind() == Some(ProofKind::Delivery))
            .unwrap();
        let check: GateDef = serde_json::from_value(json!({
            "id": "runtime-check",
            "command": "rust-check-and-test",
            "proof": "command"
        }))
        .unwrap();
        block.gates.insert(delivery, check);
        let source = MilestoneFixtureBuilder::new("m-headless-lifecycle")
            .title("Task lifecycle execution")
            .task("run", &["src/lib.rs"])
            .task_target("Execute the configured task lifecycle")
            .task_proof_policy("seam-test-first")
            .task_subtasks(&[
                contextunity_forge_mcp::core::tasks::SubtaskSpec {
                    subtask_ref: "verify".into(),
                    title: "Verify task behavior".into(),
                    status: "completed".into(),
                    evidence: Some("canonical task seam verified".into()),
                },
                contextunity_forge_mcp::core::tasks::SubtaskSpec {
                    subtask_ref: "optional".into(),
                    title: "Explicitly cancelled work".into(),
                    status: "cancelled".into(),
                    evidence: None,
                },
            ])
            .build();
        let task = TaskHarness::disk_with_profile(&source, MANIFEST, profile);
        fs::create_dir_all(task.root().join(".forge/lifecycle")).unwrap();
        fs::write(
            task.root().join(".forge/lifecycle/review-proof.json"),
            serde_json::to_vec(&passing_review_proof()).unwrap(),
        )
        .unwrap();
        fs::write(
            task.root().join("src/lib.rs"),
            "pub fn example() -> bool { true }\n",
        )
        .unwrap();
        let options = TaskRunOptions {
            worktree: task.root().to_owned(),
            worker_id: "lifecycle-coordinator".into(),
            session_fingerprint: "fixture-coordinator-session".into(),
            runner_timeout: Duration::from_secs(20),
        };
        Self { task, options }
    }

    pub fn records(&self) -> Vec<Value> {
        fs::read_to_string(self.task.root().join(".forge/lifecycle/roles.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    pub fn write_control(&self, name: &str, value: &Value) {
        fs::write(
            self.task.root().join(".forge/lifecycle").join(name),
            serde_json::to_vec(value).unwrap(),
        )
        .unwrap();
    }

    pub fn root(&self) -> &Path {
        self.task.root()
    }
}
