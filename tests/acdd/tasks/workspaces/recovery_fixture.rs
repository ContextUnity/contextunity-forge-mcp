use crate::acdd::support::{acdd_workspace, Workspace, DEFAULT_SEAM};
use crate::acdd::tasks::harness::TaskHarness;
use contextunity_forge_mcp::engine::tasks;
use serde_json::{json, Value};

pub(crate) struct RecoveryContractFixture {
    pub root: Workspace,
    pub id: String,
    pub target: String,
}

impl TaskHarness {
    pub fn recovery_contract_fixture() -> anyhow::Result<RecoveryContractFixture> {
        let root = acdd_workspace(DEFAULT_SEAM, "");
        root.write("src/lib.rs", "pub fn example() {}\n");
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        root.write("docs/milestones/010-test.md", "---\nid: m-test\ntitle: Tasks\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: first\ntarget: Deliver first\nproof_policy: seam-test-first\nscope: [src/]\n```\n```yaml\ntask_ref: second\ntarget: Deliver second\nproof_policy: seam-test-first\nscope: [src/]\ndepends_on: [first]\n```\n");
        tasks::manage(
            root.root(),
            serde_json::from_value(
                json!({"action":"sync","milestone_ref":"docs/milestones/010-test.md"}),
            )?,
        )?;
        for output in [
            Self::stage_changes(root.root(), &["src", "docs", "forge-mcp.yaml"]),
            Self::commit_fixture(root.root(), "initial"),
        ] {
            anyhow::ensure!(
                output.status.success(),
                "Recovery seed Git setup failed: {output:?}"
            );
        }
        let ready = tasks::store(root.root())?.list(None, "ready", None)?;
        let id = ready[0].task_id.clone();
        let target = ready[0].spec.target.clone();
        let subtasks = [
            (
                "release-notes",
                "Publish release notes\nand notify maintainers",
                "completed",
                "Release notes reviewed",
            ),
            (
                "cleanup-followup",
                "Optional cleanup follow-up",
                "cancelled",
                "Optional cleanup follow-up deferred beyond this task",
            ),
        ];
        for (reference, title, _, _) in subtasks {
            tasks::manage(
                root.root(),
                tasks::Manage {
                    action: tasks::ManageAction::SubtaskAdd,
                    task_id: Some(id.clone()),
                    subtask_ref: Some(reference.into()),
                    title: Some(title.into()),
                    ..Default::default()
                },
            )?;
        }
        for (reference, _, status, note) in subtasks {
            tasks::manage(
                root.root(),
                tasks::Manage {
                    action: tasks::ManageAction::SubtaskUpdate,
                    task_id: Some(id.clone()),
                    subtask_ref: Some(reference.into()),
                    subtask_status: Some(status.into()),
                    evidence: Some(note.into()),
                    ..Default::default()
                },
            )?;
        }
        root.write("src/new_feature.rs", "pub fn new_feature() -> i32 { 42 }\n");
        Self::claim_at(root.root(), &id, "contract", "builder")?;
        Ok(RecoveryContractFixture { root, id, target })
    }

    pub fn recovery_fixture() -> anyhow::Result<(Workspace, String, String, String)> {
        let fixture = Self::recovery_contract_fixture()?;
        let proof = fixture.contract_evidence()?;
        let (_, snapshot) = fixture.submit_contract(&proof)?;
        fixture.begin_build()?;
        Ok((fixture.root, fixture.id, fixture.target, snapshot))
    }
}

impl RecoveryContractFixture {
    pub fn contract_evidence(
        &self,
    ) -> anyhow::Result<contextunity_forge_mcp::core::tasks::gates::Evidence> {
        let task = tasks::store(self.root.root())?.inspect(&self.id)?;
        let mut proof = TaskHarness::default_evidence(&task);
        proof.commit = None;
        Ok(proof)
    }

    pub fn submit_contract(
        &self,
        proof: &contextunity_forge_mcp::core::tasks::gates::Evidence,
    ) -> anyhow::Result<(Value, String)> {
        let receipt = tasks::submit(
            self.root.root(),
            TaskHarness::pass_request(&self.id, "contract", proof),
        )?;
        let snapshot_ref = format!("refs/forge/snapshots/{}", self.id.replace(':', "/"));
        let snapshot = TaskHarness::verified_revision_text(self.root.root(), &snapshot_ref)?;
        Ok((receipt, snapshot))
    }

    pub fn begin_build(&self) -> anyhow::Result<()> {
        self.root.write("README.md", "# sibling change\n");
        for output in [
            TaskHarness::stage_changes(self.root.root(), &["README.md"]),
            TaskHarness::commit_fixture(self.root.root(), "sibling README change"),
        ] {
            anyhow::ensure!(
                output.status.success(),
                "Recovery sibling commit failed: {output:?}"
            );
        }
        TaskHarness::claim_at(self.root.root(), &self.id, "build", "builder")?;
        Ok(())
    }
}
