use super::*;
use crate::acdd::tasks::harness::TaskHarness;

#[test]
fn provisional_snapshot_survives_gc_and_pending_publication() {
    let matrix = TaskHarness::gc_matrix();
    for output in &matrix.seed_outputs {
        assert!(output.status.success(), "{output:?}");
    }
    for pending in [false, true] {
        let (task, fault) = matrix.begin_case(pending).unwrap();
        let id = task.id.as_str();
        let output = fault.submit_build(id, &task.observed_evidence()).unwrap();
        let (gc, construction) = fault.observations().unwrap();
        assert!(construction.gc_attempted);
        assert_eq!(
            construction.exit, 128,
            "Git GC must honor construction exclusion before kept-pack publication"
        );
        assert!(gc.gc_attempted);
        assert!(
            gc.exists_after_gc,
            "actual new parentless candidate must survive real GC before commit-tree stdout"
        );
        if pending {
            assert!(!output.status.success());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("TASK_CHECKPOINT_PENDING"),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                fault.temporary_pack_present().unwrap(),
                "post-SQL failure retains actual provisional object home"
            );
            fault.release_publication_lock().unwrap();
            let resumed = fault.recover();
            assert!(
                resumed.status.success(),
                "{}",
                String::from_utf8_lossy(&resumed.stderr)
            );
            let stored = tasks::store(task.root()).unwrap().inspect(id).unwrap();
            assert_eq!(stored.stage, "review");
            assert_eq!(stored.status, "ready");
        } else {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        assert!(
            !fault.temporary_pack_present().unwrap(),
            "keeper durability releases only this task's temporary pack protection"
        );
        let candidate = gc.candidate.as_str();
        assert!(contextunity_forge_mcp::core::git::git_ancestor(
            task.root(),
            candidate,
            "refs/forge/state/forge-mcp/forge-mcp/m-gc/candidate"
        )
        .unwrap());
        let parents = TaskHarness::snapshot_parents(task.root(), candidate);
        assert!(parents.status.success(), "{parents:?}");
        let parents = std::str::from_utf8(&parents.stdout).unwrap();
        assert_eq!(
            parents.split_whitespace().count(),
            1,
            "candidate remains parentless"
        );
    }
}

#[test]
fn scoped_snapshot_delivery_and_handoff_controls() {
    let root = acdd_workspace(DEFAULT_SEAM, "");
    root.write("src/lib.rs", "pub fn baseline() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/milestones/010-control.md", "---\nid: m-control\ntitle: Control\ndoc_type: contract\nstatus: active\n---\n# Tasks\n```yaml\ntask_ref: first\ntarget: Deliver first\nproof_policy: seam-test-first\nscope: [src/]\n```\n");
    let added = TaskHarness::stage_changes(root.root(), &["src", "docs", "forge-mcp.yaml"]);
    assert!(added.status.success(), "{added:?}");
    let committed = TaskHarness::commit_fixture(root.root(), "control baseline");
    assert!(committed.status.success(), "{committed:?}");
    tasks::manage(
        root.root(),
        serde_json::from_value(
            json!({"action":"sync", "milestone_ref":"docs/milestones/010-control.md"}),
        )
        .unwrap(),
    )
    .unwrap();
    let task_id = "forge-mcp/m-control:first";
    let premature = contextunity_forge_mcp::engine::milestones::handoff(
        root.root(),
        "m-control",
        None,
        &unready_fixture_integration_evidence(root.root(), "m-control", "integration-reviewer"),
    );
    assert!(premature.is_err(), "unfinished task must reject handoff");
    assert_eq!(
        tasks::store(root.root())
            .unwrap()
            .inspect(task_id)
            .unwrap()
            .status,
        "ready"
    );
    root.write("src/new_feature.rs", "pub fn added() -> usize { 42 }\n");
    for (stage, worker) in [
        ("contract", "contract-author"),
        ("build", "builder"),
        ("review", "independent-reviewer"),
        ("deliver", "delivery-reviewer"),
    ] {
        TaskHarness::claim_at(root.root(), task_id, stage, worker).unwrap();
        let task = tasks::store(root.root()).unwrap().inspect(task_id).unwrap();
        let mut proof = TaskHarness::default_evidence(&task);
        proof.commit = None;
        if stage == "deliver" {
            root.write("src/new_feature.rs", "pub fn added() -> usize { 99 }\n");
            let rejected = tasks::submit(
                root.root(),
                TaskHarness::pass_request(task_id, stage, &proof),
            )
            .unwrap_err();
            assert!(
                rejected.to_string().contains("TASK_CANDIDATE_MISMATCH"),
                "{rejected}"
            );
            assert_eq!(
                tasks::store(root.root())
                    .unwrap()
                    .inspect(task_id)
                    .unwrap()
                    .status,
                "in_progress"
            );
            root.write("src/new_feature.rs", "pub fn added() -> usize { 42 }\n");
        }
        let result = tasks::submit(
            root.root(),
            TaskHarness::pass_request(task_id, stage, &proof),
        )
        .unwrap();
        assert_eq!(
            result["status"],
            if stage == "deliver" {
                "completed"
            } else {
                "ready"
            }
        );
    }
    let delivered = tasks::store(root.root()).unwrap().inspect(task_id).unwrap();
    let commit = delivered
        .receipt
        .as_ref()
        .and_then(|receipt| receipt.commit.as_deref())
        .unwrap()
        .to_owned();
    let content = TaskHarness::snapshot_content(root.root(), &commit, "src/new_feature.rs");
    assert!(content.status.success());
    assert_eq!(content.stdout, b"pub fn added() -> usize { 42 }\n");
    handoff_with_fixture_evidence(root.root(), "m-control", None, "cargo test", 1, 0).unwrap();
    let archived =
        std::fs::read_to_string(root.root().join("docs/milestones/archive/010-control.md"))
            .unwrap();
    let archived = Milestone::parse(&archived, "forge-mcp").unwrap();
    assert_eq!(
        archived.tasks[0]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        Some(commit.as_str())
    );
    assert!(tasks::store(root.root())
        .unwrap()
        .list(None, "all", None)
        .unwrap()
        .is_empty());
    let refs = TaskHarness::references(root.root(), "refs/forge/snapshots/");
    assert!(refs.status.success());
    assert!(refs.stdout.is_empty());
}

#[test]
fn scoped_snapshot_omits_deleted_exact_path_during_contract_submit() {
    let root = acdd_workspace(DEFAULT_SEAM, "");
    root.write("src/deleted.rs", "pub fn removed() {}\n");
    root.write("src/kept.rs", "pub fn kept() -> i32 { 1 }\n");
    root.write("src/unscoped.rs", "pub fn sibling() -> i32 { 2 }\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let spec = "---\nid: m-snapshot\ntitle: Snapshot Tasks\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\ninvariants: [isolated]\n---\n# Snapshot Tasks\n```yaml\ntask_ref: exact-path\ntarget: Capture a scoped candidate with a deleted exact path\nproof_policy: seam-test-first\nscope: [src/deleted.rs, src/kept.rs]\n```\n";
    root.write("docs/milestones/010-snapshot.md", spec);
    tasks::manage(
        root.root(),
        serde_json::from_value(
            json!({"action": "sync", "milestone_ref": "docs/milestones/010-snapshot.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    for output in TaskHarness::initial_fixture_commit(root.root()) {
        assert!(
            output.status.success(),
            "initial fixture commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let ready = tasks::store(root.root())
        .unwrap()
        .list(None, "ready", None)
        .unwrap();
    let task_id = ready[0].task_id.clone();
    TaskHarness::claim_at(root.root(), &task_id, "contract", "builder").unwrap();

    std::fs::remove_file(root.root().join("src/deleted.rs")).unwrap();
    root.write("src/kept.rs", "pub fn kept() -> i32 { 42 }\n");
    root.write("src/unscoped.rs", "pub fn sibling() -> i32 { 99 }\n");

    let stored = tasks::store(root.root())
        .unwrap()
        .inspect(&task_id)
        .unwrap();
    let mut proof = TaskHarness::default_evidence(&stored);
    proof.commit = None;
    tasks::submit(
        root.root(),
        TaskHarness::pass_request(&task_id, "contract", &proof),
    )
    .unwrap();

    let snapshot = pinned_candidate(root.root(), &task_id);
    let task_tree = TaskHarness::snapshot_paths(root.root(), &snapshot);
    assert!(task_tree.status.success());
    assert_eq!(
        String::from_utf8_lossy(&task_tree.stdout).trim(),
        "src/kept.rs"
    );
    let kept = TaskHarness::snapshot_content(root.root(), &snapshot, "src/kept.rs");
    assert!(kept.status.success());
    assert_eq!(
        String::from_utf8_lossy(&kept.stdout),
        "pub fn kept() -> i32 { 42 }\n"
    );
    let submitted = tasks::store(root.root())
        .unwrap()
        .inspect(&task_id)
        .unwrap();
    assert_eq!(submitted.gate, 1);
    assert_eq!(submitted.status, "ready");
}

#[test]
fn scoped_commit_commits_staged_deletion_and_preserves_unrelated_staged_work() {
    use contextunity_forge_mcp::core::git::git_scoped_commit;

    let root = TaskHarness::exact_tree_fixture(&[
        ("src/a.rs", "pub fn a() -> u32 { 1 }\n"),
        ("src/b.rs", "pub fn b() -> u32 { 2 }\n"),
    ])
    .unwrap();
    let root_path = root.root();
    let staged = TaskHarness::stage_unrelated_file(&root, "README.md", "# Unrelated work\n");
    assert!(staged.status.success(), "{staged:?}");
    let deleted = TaskHarness::stage_deletion(root_path, "src/a.rs");
    assert!(deleted.status.success(), "{deleted:?}");

    // Call git_scoped_commit with scope restricted strictly to src/a.rs
    let commit = git_scoped_commit(root_path, &["src/a.rs".into()], "remove a")
        .expect("git_scoped_commit should commit staged deletion of scoped file");

    // Verify commit tree: src/b.rs remains, src/a.rs is removed, README.md is not in commit
    let output = TaskHarness::snapshot_paths(root_path, &commit);
    assert!(output.status.success());
    let tree_files = String::from_utf8_lossy(&output.stdout);
    let files: Vec<&str> = tree_files.lines().collect();
    assert_eq!(files, vec!["src/b.rs"], "commit must contain only src/b.rs");

    // Verify that unrelated staged change README.md is still staged in .git/index
    let status_output = TaskHarness::porcelain_status(root_path);
    assert!(status_output.status.success());
    let status = String::from_utf8_lossy(&status_output.stdout);
    assert!(
        status.contains("A  README.md"),
        "real index must retain unrelated staged README.md: {status}"
    );
}
