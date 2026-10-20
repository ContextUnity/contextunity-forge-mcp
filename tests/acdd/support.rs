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

pub(crate) fn fixture_integration_evidence(
    root: &Path,
    selector: &str,
    command: &str,
    passed: u64,
    failed: u64,
) -> contextunity_forge_mcp::engine::milestones::IntegrationEvidence {
    use contextunity_forge_mcp::engine::{milestones, tasks::TaskSettings};
    let exclude = root.join(
        contextunity_forge_mcp::core::git::git_stdout(
            root,
            &["rev-parse", "--git-path", "info/exclude"],
            "fixture Git directory",
        )
        .unwrap(),
    );
    let mut ignored = std::fs::read_to_string(&exclude).unwrap_or_default();
    if !ignored.lines().any(|line| line == ".forge/") {
        ignored.push_str("\n.forge/\n");
    }
    std::fs::write(exclude, ignored).unwrap();
    let settings: TaskSettings = serde_yaml::from_str(
        &std::fs::read_to_string(root.join("forge-mcp.yaml")).unwrap_or_default(),
    )
    .unwrap();
    let view = milestones::show(root, selector, false).unwrap();
    let repository = view["frontmatter"]["repository"]
        .as_str()
        .unwrap_or(&settings.task_repository);
    let listed = tasks::list(
        root,
        tasks::List {
            repository: Some("all".into()),
            milestone_ref: Some(selector.into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .ok();
    let canonical_project = listed
        .as_ref()
        .and_then(|listed| listed["tasks"][0]["task_id"].as_str())
        .and_then(|id| id.split_once('/').map(|(project, _)| project));
    let declared_project = view["frontmatter"]["project"]
        .as_str()
        .or(settings.task_project.as_deref())
        .unwrap_or(repository);
    let project = canonical_project.unwrap_or_else(|| {
        settings
            .aliases
            .get(declared_project)
            .map(String::as_str)
            .unwrap_or(declared_project)
    });
    let store = TasksStore::open_project(&tasks::database_path(root).unwrap(), repository, project)
        .unwrap();
    let prefix = format!("{project}/{}:", view["id"].as_str().unwrap());
    let mut candidates = store.list(None, "all", None).unwrap();
    candidates.retain(|task| task.task_id.starts_with(&prefix));
    candidates.sort_by_key(|task| (task.wave_index, task.task_id.clone()));
    let target = candidates
        .first()
        .and_then(|task| {
            let details = store.inspect_details(&task.task_id).unwrap();
            details["gates"]
                .as_array()
                .unwrap()
                .iter()
                .find_map(|gate| {
                    if gate["stage"] != "build" {
                        return None;
                    }
                    let evidence: Value = serde_json::from_str(gate["evidence"].as_str()?).ok()?;
                    evidence["candidate_baseline_head"]
                        .as_str()
                        .map(str::to_owned)
                })
        })
        .unwrap_or_else(|| git_head(root));
    // Malformed/incomplete fixtures still reach the actual handoff rejection path.
    let binding = milestones::handoff_context(root, selector, &target).map(|context| context["binding"].clone()).unwrap_or_else(|_| json!({
        "version":1,"repository":repository,"project":project,"milestone":view["id"],"authority_digest":"unavailable","implementation_digest":"unavailable","target_base_digest":"unavailable","builders":["builder"]
    }));
    serde_json::from_value(json!({
        "binding":binding,"target_base":target,
        "review":{"worker_id":"integration-reviewer","decision":"pass","summary":"Fixture independently reviews the combined delta against admitted tasks","scope_findings":[],"contract_findings":[]},
        "verification":{"command":command,"exit_code":if failed == 0 {0} else {1},"tests_passed":passed,"tests_failed":failed}
    })).unwrap()
}

pub(crate) fn handoff_with_fixture_evidence(
    root: &Path,
    selector: &str,
    commit: Option<&str>,
    command: &str,
    passed: u64,
    failed: u64,
) -> anyhow::Result<Value> {
    contextunity_forge_mcp::engine::milestones::handoff(
        root,
        selector,
        commit,
        &fixture_integration_evidence(root, selector, command, passed, failed),
    )
}

pub(crate) fn fixture_evidence_file(
    root: &Path,
    selector: &str,
    command: &str,
    passed: u64,
    failed: u64,
) -> String {
    let path = root.join(".forge/integration-evidence.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        serde_json::to_vec(&fixture_integration_evidence(
            root, selector, command, passed, failed,
        ))
        .unwrap(),
    )
    .unwrap();
    path.to_string_lossy().into_owned()
}

pub(crate) fn commit_handoff_fixture_inputs(root: &ScopedWorkspace, paths: &[&str]) {
    let added = std::process::Command::new("git")
        .args(["add", "--"])
        .args(paths)
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(added.status.success(), "{added:?}");
    let committed = std::process::Command::new("git")
        .args(["commit", "-m", "admit handoff fixture inputs"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(committed.status.success(), "{committed:?}");
}

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
    let _ = std::process::Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(root)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "ACDD test"])
        .current_dir(root)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "acdd-test@example.invalid"])
        .current_dir(root)
        .output();
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
