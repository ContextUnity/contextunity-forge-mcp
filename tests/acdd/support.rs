pub(crate) use crate::common::Workspace;
pub(crate) use contextunity_forge_mcp::{
    core::tasks::{gates::Evidence, Milestone, Receipt, ReceiptRollup, ReviewSummary},
    db::{
        tasks_store::{TasksStore, RETENTION_SECONDS},
        writer,
    },
    engine::tasks,
};
pub(crate) use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(crate) const GATES: [&str; 4] = ["contract", "build", "review", "deliver"];

/// Compatibility wrapper for task fixtures; the shared Workspace owns setup and cleanup.
pub(crate) struct ScopedWorkspace(pub(crate) PathBuf, Workspace);

impl ScopedWorkspace {
    pub(crate) fn new(_prefix: &str) -> Self {
        let workspace = Workspace::new();
        init_git(workspace.root());
        Self(workspace.root().to_path_buf(), workspace)
    }

    pub(crate) fn write(&self, path: &str, content: &str) {
        self.1.write(path, content);
    }
}

pub(crate) fn init_git(root: &Path) {
    let initialized = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        initialized.status.success(),
        "could not initialize ACDD fixture repository: {}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    let initial = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            "initial fixture baseline",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        initial.status.success(),
        "could not create ACDD fixture baseline: {}",
        String::from_utf8_lossy(&initial.stderr)
    );
}

pub(crate) fn git_head(root: &Path) -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "could not read ACDD fixture HEAD: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub(crate) fn review_proof(decision: &str) -> serde_json::Value {
    let contours: serde_json::Map<String, serde_json::Value> =
        contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS
            .iter()
            .map(|name| {
                (
                    (*name).into(),
                    json!({"applicable":true,"evidence":"public-seam verification"}),
                )
            })
            .collect();
    json!({"review_proof":{"decision":decision,"contours":contours}})
}
pub(crate) fn passing_review_proof() -> serde_json::Value {
    review_proof("pass")
}
pub(crate) fn evidence(task: &contextunity_forge_mcp::db::tasks_store::Task) -> Evidence {
    let stage = task.stage.as_str();
    Evidence {
        task_id: task.task_id.clone(),
        stage: task.stage.clone(),
        claim_revision: task.claim_revision,
        contract_revision: task.contract_revision,
        worker_id: task.worker_id.clone().unwrap(),
        worktree: task.worktree.clone().unwrap(),
        // Snapshot and baseline hashes are engine-owned acceptance data, never test fixture input.
        commit: None,
        candidate_baseline_head: None,
        proof: match stage {
            "contract" => {
                json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":101}})
            }
            "build" => {
                json!({"command_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":1,"tests_failed":0}})
            }
            "review" => passing_review_proof(),
            "deliver" => json!({"delivery_proof":{}}),
            _ => json!({"none_proof":{}}),
        },
    }
}
pub(crate) fn pinned_candidate(root: &std::path::Path, task_id: &str) -> String {
    let reference = format!("refs/forge/snapshots/{}", task_id.replace(':', "/"));
    let output = std::process::Command::new("git")
        .args(["rev-parse", &reference])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
pub(crate) const SPEC: &str = "---\nid: m-test\ntitle: Tasks\ndoc_type: contract\nstatus: active\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: first\ntarget: Deliver first\nproof_policy: seam-test-first\nscope: [src/]\nscope_roots: [src/, tests/]\n```\n```yaml\ntask_ref: second\ntarget: Deliver second\nproof_policy: seam-test-first\nscope: [src/]\ndepends_on: [first]\n```\n";
pub(crate) fn fixture() -> (ScopedWorkspace, TasksStore, Milestone) {
    let root = ScopedWorkspace::new("forge_tasks");
    root.write("src/lib.rs", "pub fn example() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-test.md", SPEC);
    let milestone = Milestone::parse(SPEC, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store.sync(&milestone, "docs/010-test.md", &root.0).unwrap();
    (root, store, milestone)
}
