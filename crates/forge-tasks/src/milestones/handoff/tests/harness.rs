use super::*;
use crate::milestones::integration::{IntegrationReview, IntegrationVerification};
use crate::tasks::gates::Evidence;
use crate::tasks::profile::ProofKind;

pub(super) const ACTIVE: &str = "docs/milestones/010-native-shared.md";
pub(super) const ARCHIVE: &str = "docs/milestones/archive/010-native-shared.md";
const IDENTITY: &str = "forge-mcp/forge-mcp/m-native-shared";
const SOURCE: &str = "---\nid: m-native-shared\ntitle: Reviewed shared handoff\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\n---\n# Reviewed shared handoff\n```yaml\ntask_ref: first\ntarget: First version\nproof_policy: direct-proof\nscope: [src/shared.rs]\n```\n```yaml\ntask_ref: second\ntarget: Reviewed next version\nproof_policy: direct-proof\nscope: [src/shared.rs]\ndepends_on: [first]\n```\n";
const PROFILE: &str =         "commands:\n  test:\n    command: ': --seam-ref tests/acdd/tasks.rs::test_shared && rustc --test tests/acdd/tasks.rs -o .forge/native-tests && .forge/native-tests'\n    output:\n      seam_ref_pattern: '--seam-ref (\\S+)'\n  rust-check-and-test:\n    command: 'rustc --test tests/acdd/tasks.rs -o .forge/native-tests && .forge/native-tests'\n  check_full_repo:\n    command: 'rustc --test tests/acdd/tasks.rs -o .forge/native-tests && .forge/native-tests'\n    output:\n      counts: {passed: '(\\d+) passed', failed: '(\\d+) failed'}\n";
const SEAM: &str =         "#[path=\"../../src/shared.rs\"] mod shared;\n#[test]\nfn test_shared() {\n    assert!(shared::shared() > 0);\n}\n";

pub(super) struct NativeHandoffHarness {
    directory: tempfile::TempDir,
    pub milestone: crate::core::tasks::Milestone,
    pub base: String,
}

pub(super) struct FailedCheck {
    pub exit_code: Option<i32>,
    pub passed: bool,
}

impl NativeHandoffHarness {
    pub fn new() -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("forge-native-handoff-")
            .tempdir()?;
        let root = directory.path();
        for path in ["src", "docs/milestones", "tests/acdd"] {
            fs::create_dir_all(root.join(path))?;
        }
        Self::git(root, &["init", "--quiet"])?;
        fs::write(root.join(".git/info/exclude"), ".forge/\n")?;
        for (key, value) in [
            ("user.name", "ACDD test"),
            ("user.email", "acdd-test@example.invalid"),
            ("commit.gpgsign", "false"),
        ] {
            Self::git(root, &["config", key, value])?;
        }
        for (path, value) in [
            ("profile.yaml", PROFILE),
            ("forge-mcp.yaml", "roots: [src]\ndocs: [docs]\nacdd_profile: profile.yaml\ntasks_db: .forge/tasks.sqlite\n"),
            (ACTIVE, SOURCE),
            ("src/shared.rs", "pub fn shared() -> u8 { 0 }\n"),
            ("tests/acdd/tasks.rs", SEAM),
        ] { fs::write(root.join(path), value)?; }
        Self::git(
            root,
            &[
                "add",
                "forge-mcp.yaml",
                "docs",
                "src",
                "tests",
                "profile.yaml",
            ],
        )?;
        Self::git(root, &["commit", "-m", "admit native shared handoff"])?;
        let base = Self::git(root, &["rev-parse", "HEAD"])?;
        let milestone = crate::core::tasks::Milestone::parse(SOURCE, "forge-mcp")?;
        let mut store = TasksStore::open(&root.join(".forge/tasks.sqlite"))?;
        store.sync(&milestone, ACTIVE, root)?;
        drop(store);
        Ok(Self {
            directory,
            milestone,
            base,
        })
    }

    fn git(root: &Path, args: &[&str]) -> Result<String> {
        let output = crate::core::git::git_command(root, args)?;
        anyhow::ensure!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }
    pub fn root(&self) -> &Path {
        self.directory.path()
    }
    pub fn store(&self) -> Result<TasksStore> {
        TasksStore::open(&self.root().join(".forge/tasks.sqlite"))
    }
    pub fn write_shared_value(&self, value: usize) -> Result<()> {
        Ok(fs::write(
            self.root().join("src/shared.rs"),
            format!("pub fn shared() -> u8 {{ {value} }}\n"),
        )?)
    }
    pub fn file_bytes(&self, path: &str) -> Result<Vec<u8>> {
        Ok(fs::read(self.root().join(path))?)
    }
    pub fn document(&self, path: &str) -> Result<String> {
        Ok(fs::read_to_string(self.root().join(path))?)
    }
    pub fn exists(&self, path: &str) -> bool {
        self.root().join(path).exists()
    }
    pub fn head(&self) -> Result<String> {
        Self::git(self.root(), &["rev-parse", "HEAD"])
    }
    pub fn commit_shared_change(&self, message: &str) -> Result<()> {
        Self::git(self.root(), &["add", "src/shared.rs"])?;
        Self::git(self.root(), &["commit", "-m", message])?;
        Ok(())
    }

    pub fn complete_task(&self, id: &str) -> Result<()> {
        let store = self.store()?;
        loop {
            let before = store.inspect(id)?;
            if before.status == "completed" {
                return Ok(());
            }
            let profile = store.profile_for_task(&before)?;
            let gate = profile
                .by_stage(&before.stage)
                .context("active task gate absent from profile")?;
            let worker = format!(
                "{}-{}",
                gate.role.as_deref().unwrap_or("worker"),
                before.stage
            );
            let session = Some(format!("test-session-{worker}"));
            tasks::claim(
                self.root(),
                tasks::Claim {
                    task_id: id.into(),
                    stage: before.stage.clone(),
                    worker_id: worker.clone(),
                    worktree: self.root().to_string_lossy().into_owned(),
                    session_fingerprint: session.clone(),
                    ..Default::default()
                },
            )?;
            let task = store.inspect(id)?;
            let evidence: Evidence = match gate.proof.kind() {
                Some(ProofKind::Contract | ProofKind::Command | ProofKind::Delivery) => {
                    let verified = tasks::verify_task(self.root(), id, None)?;
                    serde_json::from_value(if gate.proof.kind() == Some(ProofKind::Delivery) {
                        verified
                    } else {
                        verified["evidence"].clone()
                    })?
                }
                Some(ProofKind::Review | ProofKind::None) => {
                    let proof = if gate.proof.kind() == Some(ProofKind::Review) {
                        let contours: serde_json::Map<String,Value> = crate::core::tasks::gates::REVIEW_CONTOURS.iter().map(|name| ((*name).into(), json!({"applicable":true,"evidence":"real Git and SQLite handoff seam"}))).collect();
                        json!({"review_proof":{"decision":"pass","contours":contours}})
                    } else {
                        json!({"none_proof":{}})
                    };
                    Evidence {
                        task_id: id.into(),
                        stage: task.stage.clone(),
                        claim_revision: task.claim_revision,
                        contract_revision: task.contract_revision,
                        worker_id: worker,
                        worktree: self.root().to_string_lossy().into_owned(),
                        commit: None,
                        candidate_baseline_head: None,
                        proof,
                    }
                }
                other => {
                    anyhow::bail!("native fixture has no production proof provider for {other:?}")
                }
            };
            tasks::submit(
                self.root(),
                tasks::Submit {
                    task_id: id.into(),
                    stage: task.stage,
                    action: tasks::Action::Pass,
                    evidence: serde_json::to_value(evidence)?,
                    findings: None,
                    session_fingerprint: session,
                },
            )?;
            let after = store.inspect(id)?;
            anyhow::ensure!(
                after.status == "completed" || after.stage != before.stage,
                "native task traversal stalled"
            );
        }
    }

    pub fn unobserved_verification(command: &str, command_id: &str) -> IntegrationVerification {
        IntegrationVerification {
            command: command.into(),
            exit_code: 0,
            tests_passed: 1,
            tests_failed: 0,
            lint_command: None,
            project_command: Some(command_id.into()),
        }
    }
    pub fn observed_verification(check: &Value) -> Result<IntegrationVerification> {
        Ok(IntegrationVerification {
            command: serde_json::from_value(check["command"].clone())?,
            exit_code: serde_json::from_value(check["exit_code"].clone())?,
            tests_passed: serde_json::from_value(check["tests_passed"].clone())?,
            tests_failed: serde_json::from_value(check["tests_failed"].clone())?,
            lint_command: None,
            project_command: Some(serde_json::from_value(check["command_id"].clone())?),
        })
    }
    pub fn evidence(
        context: &Value,
        base: &str,
        verification: IntegrationVerification,
    ) -> Result<IntegrationEvidence> {
        Ok(IntegrationEvidence {
            binding: serde_json::from_value(context["binding"].clone())?,
            target_base: base.into(),
            review: IntegrationReview {
                worker_id: "integration-reviewer".into(),
                decision: "pass".into(),
                summary: "Reviewed combined delta and task authority".into(),
                scope_findings: Vec::new(),
                contract_findings: Vec::new(),
            },
            verification,
        })
    }
    pub fn fail_verification(&self, observed: &Value) -> Result<FailedCheck> {
        let store = self.store()?;
        let (failed, _) = super::super::super::integration::run_milestone_check(
            self.root(),
            &store,
            IDENTITY,
            2,
            observed["gate"].as_str().context("verification gate")?,
            observed["verification"]["command_id"]
                .as_str()
                .context("verification command")?,
            &crate::tasks::profile::CommandDef::Script("exit 1".into()),
        )?;
        Ok(FailedCheck {
            exit_code: failed.exit_code,
            passed: failed.passed(),
        })
    }
    pub fn telemetry_rows(&self, table: &str) -> Result<i64> {
        anyhow::ensure!(
            [
                "gate_checkpoints",
                "gate_execution_traces",
                "gate_transitions"
            ]
            .contains(&table),
            "unsupported milestone telemetry table"
        );
        Ok(self.store()?.connection.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE milestone_id=?1"),
            [IDENTITY],
            |row| row.get(0),
        )?)
    }
}
