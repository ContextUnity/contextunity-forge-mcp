pub(crate) use crate::common::Workspace;
pub(crate) use contextunity_forge_mcp::{
    core::tasks::{gates::Evidence, Milestone, Receipt, ReceiptRollup, ReviewSummary, GATES},
    db::{
        tasks_store::{TasksStore, RETENTION_SECONDS},
        writer,
    },
    engine::tasks,
};
pub(crate) use serde_json::{json, Value};
use std::path::PathBuf;

/// Compatibility wrapper for task fixtures; the shared Workspace owns setup and cleanup.
pub(crate) struct ScopedWorkspace(pub(crate) PathBuf, Workspace);

impl ScopedWorkspace {
    pub(crate) fn new(_prefix: &str) -> Self {
        let workspace = Workspace::new();
        Self(workspace.root().to_path_buf(), workspace)
    }

    pub(crate) fn write(&self, path: &str, content: &str) {
        self.1.write(path, content);
    }
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
    Evidence {
        task_id: task.task_id.clone(),
        stage: GATES[task.gate].into(),
        claim_revision: task.claim_revision,
        contract_revision: task.contract_revision,
        worker_id: task.worker_id.clone().unwrap(),
        worktree: task.worktree.clone().unwrap(),
        commit: Some("0123456789abcdef0123456789abcdef01234567".into()),
        proof: match task.gate {
            0 => {
                json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":101}})
            }
            1 => {
                json!({"test_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":1,"tests_failed":0}})
            }
            2 => passing_review_proof(),
            _ => json!({"stage":GATES[task.gate]}),
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
