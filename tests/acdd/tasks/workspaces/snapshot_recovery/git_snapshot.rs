use super::*;
#[test]
fn scoped_git_snapshot_captures_untracked_and_modified_files_and_cleans_up_on_handoff() {
    let (root, task_id, target, contract_snap) = TaskHarness::recovery_fixture().unwrap();
    let task_id = &task_id;
    let stored = tasks::store(root.root()).unwrap().inspect(task_id).unwrap();
    let checkpoint_ref = format!("refs/forge/state/forge-mcp/{}", task_id.replace(':', "/"));
    // Only the real keeper retains the parentless contract candidate during isolated GC.
    let snapshot_ref = format!("refs/forge/snapshots/{}", task_id.replace(':', "/"));
    for output in TaskHarness::collect_unreachable_snapshot(root.root(), &snapshot_ref) {
        assert!(output.status.success(), "{output:?}");
    }
    assert!(TaskHarness::object_exists(root.root(), &contract_snap)
        .status
        .success());
    let recover = || {
        TaskHarness::run_cli_for_root(
            root.root(),
            &["--format", "json", "milestone", "db-recover"],
        )
    };
    macro_rules! submit {
        ($stage:expr, $evidence:expr) => {
            tasks::submit(
                root.root(),
                TaskHarness::pass_request(task_id, $stage, $evidence),
            )
        };
    }
    let mut ev = TaskHarness::default_evidence(&stored);
    ev.commit = Some("caller-selected-commit".into());
    let scoped_snapshot_error = submit!("build", &ev).unwrap_err().to_string();
    assert_eq!(
        scoped_snapshot_error,
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}'. supplied commit caller-selected-commit != scoped snapshot {contract_snap}. Do: resubmit with commit {contract_snap}."
        )
    );
    ev.commit = None;
    let build_res = submit!("build", &ev).unwrap();
    let build_snap = TaskHarness::verified_revision_text(root.root(), &snapshot_ref).unwrap();
    assert_eq!(build_snap.len(), 40);
    assert_eq!(build_res["snapshot"]["commit"], build_snap);
    assert_eq!(
        build_snap, contract_snap,
        "an unrelated sibling commit must not change the scoped candidate SHA"
    );
    let show_output = TaskHarness::snapshot_diff(root.root(), &build_snap);
    let show_text = String::from_utf8_lossy(&show_output.stdout);
    assert!(show_text.contains("new_feature.rs"));
    assert!(show_text.contains("pub fn new_feature() -> i32 { 42 }"));
    assert!(
        !show_text.contains("README.md"),
        "snapshot must contain only scoped files"
    );
    let parents = TaskHarness::snapshot_parents(root.root(), &build_snap);
    assert!(parents.status.success());
    assert_eq!(
        String::from_utf8_lossy(&parents.stdout)
            .split_whitespace()
            .count(),
        1,
        "root snapshots must not include a branch parent"
    );
    let same_worker_error =
        TaskHarness::claim_at(root.root(), task_id, "review", "builder").unwrap_err();
    assert!(same_worker_error
        .to_string()
        .contains("TASK_REVIEW_NOT_INDEPENDENT"));
    TaskHarness::claim_at(root.root(), task_id, "review", "reviewer").unwrap();
    let mut review_store = tasks::store(root.root()).unwrap();
    let review_task = review_store.inspect(task_id).unwrap();
    let mut wrong_review_commit = TaskHarness::default_evidence(&review_task);
    wrong_review_commit.commit = Some("caller-selected-commit".into());
    assert_eq!(
        review_store
            .submit(
                task_id,
                "review",
                &wrong_review_commit,
                "pass",
                None,
                Some(&TaskHarness::session_for(&wrong_review_commit.worker_id))
            )
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_CANDIDATE_MISMATCH: task '{task_id}'. supplied commit caller-selected-commit != candidate snapshot {build_snap}. Do: resubmit with commit {build_snap}."
        )
    );
    let explicit_review_commit_error = tasks::submit(
        root.root(),
        TaskHarness::pass_request(task_id, "review", &wrong_review_commit),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        explicit_review_commit_error,
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}'. supplied commit caller-selected-commit != candidate snapshot {build_snap}. Do: resubmit with commit {build_snap}."
        )
    );
    root.write("src/new_feature.rs", "pub fn new_feature() -> i32 { 99 }\n");
    let stored = tasks::store(root.root()).unwrap().inspect(task_id).unwrap();
    let mut ev = TaskHarness::default_evidence(&stored);
    ev.commit = None;
    let mismatch_err = submit!("review", &ev).unwrap_err();
    assert_eq!(
        mismatch_err.to_string(),
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}' diverged from snapshot {}. D src/new_feature.rs, ?? src/new_feature.rs. Do: restore those paths to snapshot {}.",
            &build_snap[..7],
            &build_snap[..7]
        )
    );
    root.write("src/new_feature.rs", "pub fn new_feature() -> i32 { 42 }\n");
    drop(review_store);
    // A post-validation ref lock forces publication to fail after SQL commit without replaying the gate.
    let keeper_lock = root
        .root()
        .join(".git")
        .join(format!("{checkpoint_ref}.lock"));
    let mut committed = ev.clone();
    committed.commit = Some(build_snap.clone());
    committed.candidate_baseline_head =
        Some(TaskHarness::verified_revision_text(root.root(), "HEAD").unwrap());
    let review_session = TaskHarness::session_for(&committed.worker_id);
    let mut pending_store = tasks::store(root.root()).unwrap();
    let error = pending_store
        .submit_with_pre_persist(
            task_id,
            "review",
            &mut committed,
            "pass",
            None,
            Some(&review_session),
            |_| {
                std::fs::write(&keeper_lock, b"isolated publication rendezvous")?;
                Ok(None)
            },
        )
        .unwrap_err();
    assert!(
        contextunity_forge_mcp::db::tasks_store::recovery::is_pending(&error),
        "{error:#}"
    );
    assert!(pending_store
        .inspect(task_id)
        .unwrap_err()
        .to_string()
        .contains("TASK_CHECKPOINT_PENDING"));
    let committed_counts =
        TaskHarness::persisted_submission_counts(&pending_store, task_id).unwrap();
    let failed_retry = recover();
    assert!(!failed_retry.status.success());
    assert!(String::from_utf8_lossy(&failed_retry.stderr).contains("TASK_CHECKPOINT_PENDING"));
    assert_eq!(
        TaskHarness::persisted_submission_counts(&pending_store, task_id).unwrap(),
        committed_counts
    );
    std::fs::remove_file(&keeper_lock).unwrap();
    drop(pending_store);
    let reconciled = recover();
    assert!(
        reconciled.status.success(),
        "{}",
        String::from_utf8_lossy(&reconciled.stderr)
    );
    let reconciled: Value = serde_json::from_slice(&reconciled.stdout).unwrap();
    assert_eq!(reconciled["checkpoint_reconciled"], true);
    let mut published = tasks::store(root.root()).unwrap();
    assert_eq!(published.inspect(task_id).unwrap().stage, "deliver");
    assert_eq!(
        TaskHarness::persisted_submission_counts(&published, task_id).unwrap(),
        committed_counts
    );
    published
        .submit_with_pre_persist(
            task_id,
            "review",
            &mut committed,
            "pass",
            None,
            Some(&review_session),
            |_| panic!("accepted submission retry must not replay pre-persist side effects"),
        )
        .unwrap();
    assert_eq!(
        TaskHarness::persisted_submission_counts(&published, task_id).unwrap(),
        committed_counts
    );
    drop(published);
    let review_res = submit!("review", &ev).unwrap();
    assert_eq!(review_res["snapshot"]["commit"], build_snap);
    TaskHarness::claim_at(root.root(), task_id, "deliver", "delivery").unwrap();
    let mut delivery_store = tasks::store(root.root()).unwrap();
    let delivery_task = delivery_store.inspect(task_id).unwrap();
    let missing_delivery_commit = TaskHarness::default_evidence(&delivery_task);
    let (candidate_snapshot, _) = delivery_store
        .build_snapshot_candidate(task_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        delivery_store
            .submit(
                task_id,
                "deliver",
                &missing_delivery_commit,
                "pass",
                None,
                Some(&TaskHarness::session_for(
                    &missing_delivery_commit.worker_id
                ))
            )
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_EVIDENCE_INVALID: task '{task_id}' for stage 'deliver'. delivery evidence missing required commit. Do: set commit to candidate snapshot {candidate_snapshot}."
        )
    );
    let review_evidence_before =
        TaskHarness::gate_evidence_json(&delivery_store, task_id, "review").unwrap();
    TaskHarness::set_gate_evidence_commit(
        &delivery_store,
        task_id,
        "review",
        "tampered-review-commit",
    )
    .unwrap();
    let review_source_error = tasks::submit(
        root.root(),
        TaskHarness::pass_request(task_id, "deliver", &missing_delivery_commit),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        review_source_error,
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}'. review source 'review' commit != candidate snapshot {candidate_snapshot}. Do: resubmit review evidence with commit {candidate_snapshot}."
        )
    );
    TaskHarness::replace_gate_evidence(&delivery_store, task_id, "review", &review_evidence_before)
        .unwrap();
    delivery_store
        .release_failed_claim(task_id, delivery_task.claim_revision)
        .unwrap();
    drop(delivery_store);
    TaskHarness::claim_at(root.root(), task_id, "deliver", "delivery").unwrap();
    root.write(
        "src/new_feature.rs",
        "pub fn new_feature() -> i32 { 999 }\n",
    );
    let stored = tasks::store(root.root()).unwrap().inspect(task_id).unwrap();
    let mut ev = TaskHarness::default_evidence(&stored);
    ev.commit = None;
    let deliver_err = submit!("deliver", &ev).unwrap_err();
    assert_eq!(
        deliver_err.to_string(),
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}' diverged from snapshot {}. D src/new_feature.rs, ?? src/new_feature.rs. Do: restore those paths to snapshot {}.",
            &build_snap[..7],
            &build_snap[..7]
        )
    );

    // Restore worktree to match candidate
    root.write("src/new_feature.rs", "pub fn new_feature() -> i32 { 42 }\n");
    let delivery_baseline = TaskHarness::verified_revision_text(root.root(), "HEAD").unwrap();
    let milestone_path = root.root().join("docs/milestones/010-test.md");
    let milestone_before_delivery = std::fs::read_to_string(&milestone_path).unwrap();

    // Keep the unrelated staged edit while delivery uses its isolated tree; preserve it and the committed README.
    root.write("README.md", "# unrelated staged edit\n");
    assert!(TaskHarness::stage_changes(root.root(), &["README.md"])
        .status
        .success());
    let unrelated_staged_before = TaskHarness::staged_entries(root.root(), &["README.md"]);
    assert!(unrelated_staged_before.status.success());
    let unrelated_staged_before = unrelated_staged_before.stdout;
    let index_before_delivery = TaskHarness::staged_entries(root.root(), &[]);
    assert!(index_before_delivery.status.success());
    let index_before_delivery = index_before_delivery.stdout;

    // A rejected hook must leave both the branch and task receipt untouched.
    let hook = TaskHarness::rejecting_pre_commit_hook(root.root()).unwrap();
    let hook_path = hook.parent().unwrap().to_string_lossy().to_string();
    assert!(TaskHarness::configure_hooks(root.root(), Some(&hook_path))
        .status
        .success());

    // Kill the real delivery process at its provisional receipt via FIFO rendezvous, before SQL commit.
    use std::{
        os::unix::process::CommandExt,
        process::{Command, Stdio},
        sync::mpsc,
        time::Duration,
    };
    let ready = root.root().join(".forge/crash-ready");
    let resume = root.root().join(".forge/crash-resume");
    let fifo_status = Command::new("mkfifo")
        .args([&ready, &resume])
        .status()
        .unwrap();
    assert!(fifo_status.success());
    let gates_before = tasks::store(root.root())
        .unwrap()
        .inspect_details(task_id)
        .unwrap()["gates"]
        .clone();
    std::fs::write(
        &hook,
        "#!/bin/sh\nprintf ready > \"$FORGE_CRASH_READY\"\nread answer < \"$FORGE_CRASH_RESUME\"\n",
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            root.root().to_str().unwrap(),
            "task",
            "submit",
            task_id,
            "--stage",
            "deliver",
            "--action",
            "pass",
            "--evidence",
            &serde_json::to_string(&ev).unwrap(),
        ])
        .env("FORGE_CRASH_READY", &ready)
        .env("FORGE_CRASH_RESUME", &resume)
        .env(
            "FORGE_AGENT_SESSION_ID",
            TaskHarness::session_for(&ev.worker_id),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let (sent, received) = mpsc::channel();
    let ready_path = ready.clone();
    let rendezvous = std::thread::spawn(move || {
        let _ = sent.send(std::fs::read_to_string(ready_path));
    });
    let signal = received.recv_timeout(Duration::from_secs(30));
    let kill_status = Command::new("kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .status()
        .unwrap();
    let terminated = child.wait().unwrap();
    if signal.is_ok() {
        rendezvous
            .join()
            .expect("crash rendezvous reader should finish");
    }
    assert!(kill_status.success());
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(terminated.signal(), Some(9));
    assert_eq!(signal.unwrap().unwrap(), "ready");
    assert_ne!(
        std::fs::read_to_string(&milestone_path).unwrap(),
        milestone_before_delivery
    );
    assert_eq!(
        TaskHarness::verified_revision_text(root.root(), "HEAD").unwrap(),
        delivery_baseline
    );
    assert_eq!(
        tasks::store(root.root())
            .unwrap()
            .inspect(task_id)
            .unwrap()
            .status,
        "in_progress"
    );
    assert!(
        contextunity_forge_mcp::engine::milestones::show(root.root(), "m-test", true)
            .unwrap_err()
            .to_string()
            .contains("TASK_CHECKPOINT_PENDING")
    );
    #[cfg(unix)]
    let released_before_rewind = {
        let (real_git, failed_rewind) =
            TaskHarness::recover_with_rewind_fault(root.root(), &checkpoint_ref).unwrap();
        assert!(real_git.status.success());
        assert!(!failed_rewind.status.success());
        assert!(
            String::from_utf8_lossy(&failed_rewind.stderr).contains("cannot rewind"),
            "{}",
            String::from_utf8_lossy(&failed_rewind.stderr)
        );
        let descriptor = TaskHarness::read_only_descriptor(root.root(), task_id).unwrap();
        assert_eq!(descriptor["status"], "ready");
        assert!(descriptor["worker_id"].is_null() && descriptor["worktree"].is_null());
        descriptor["claim_revision"].as_u64()
    };
    #[cfg(not(unix))]
    let released_before_rewind: Option<u64> = None;
    let recovered = recover();
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    let released = tasks::store(root.root()).unwrap().inspect(task_id).unwrap();
    assert_eq!(released.status, "ready");
    assert_eq!(released.stage, "deliver");
    assert!(released.worker_id.is_none() && released.worktree.is_none());
    assert!(released.claim_revision > ev.claim_revision);
    if let Some(revision) = released_before_rewind {
        assert_eq!(
            released.claim_revision, revision,
            "retry must not release the same claim twice"
        );
    }
    assert!(
        released.receipt.is_none(),
        "recovery must not accept the provisional receipt"
    );
    assert_eq!(
        tasks::store(root.root())
            .unwrap()
            .inspect_details(task_id)
            .unwrap()["gates"],
        gates_before
    );
    assert!(recover().status.success());
    assert_eq!(
        tasks::store(root.root())
            .unwrap()
            .inspect(task_id)
            .unwrap()
            .claim_revision,
        released.claim_revision
    );
    std::fs::write(&milestone_path, &milestone_before_delivery).unwrap();
    tasks::claim(
        root.root(),
        tasks::Claim {
            task_id: task_id.into(),
            stage: "deliver".into(),
            worker_id: ev.worker_id.clone(),
            session_fingerprint: Some(TaskHarness::session_for(&ev.worker_id)),
            worktree: root.root().to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    ev = TaskHarness::default_evidence(
        &tasks::store(root.root()).unwrap().inspect(task_id).unwrap(),
    );
    TaskHarness::reject_pre_commit_hook(&hook).unwrap();
    let hook_error = submit!("deliver", &ev).unwrap_err().to_string();
    assert!(
        hook_error.contains("TASK_DELIVERY_HOOK_REJECTED"),
        "{hook_error}"
    );
    assert_eq!(
        std::fs::read_to_string(&milestone_path).unwrap(),
        milestone_before_delivery
    );
    let hook_head = TaskHarness::revision(root.root(), "HEAD");
    assert_eq!(
        String::from_utf8_lossy(&hook_head.stdout).trim(),
        delivery_baseline
    );
    let hook_index = TaskHarness::staged_entries(root.root(), &[]);
    assert_eq!(hook_index.stdout, index_before_delivery);
    let after_hook_failure = tasks::store(root.root()).unwrap().inspect(task_id).unwrap();
    assert_eq!(after_hook_failure.status, "in_progress");
    assert_eq!(after_hook_failure.stage, "deliver");
    assert!(TaskHarness::configure_hooks(root.root(), None)
        .status
        .success());

    // A SQLite statement failure must roll back both the candidate branch and milestone file.
    TaskHarness::install_delivery_sql_abort(root.root()).unwrap();
    let sqlite_error = submit!("deliver", &ev).unwrap_err().to_string();
    assert!(
        sqlite_error.contains("forced delivery sqlite failure"),
        "{sqlite_error}"
    );
    TaskHarness::remove_delivery_sql_abort(root.root()).unwrap();
    assert_eq!(
        std::fs::read_to_string(&milestone_path).unwrap(),
        milestone_before_delivery
    );
    let sqlite_rollback_head = TaskHarness::revision(root.root(), "HEAD");
    assert_eq!(
        String::from_utf8_lossy(&sqlite_rollback_head.stdout).trim(),
        delivery_baseline
    );
    let sqlite_rollback_index = TaskHarness::staged_entries(root.root(), &[]);
    assert_eq!(sqlite_rollback_index.stdout, index_before_delivery);
    let after_sqlite_failure = tasks::store(root.root()).unwrap().inspect(task_id).unwrap();
    assert_eq!(after_sqlite_failure.status, "in_progress");
    assert_eq!(after_sqlite_failure.stage, "deliver");
    let deliver_res = submit!("deliver", &ev).unwrap();
    let delivered_task = tasks::store(root.root()).unwrap().inspect(task_id).unwrap();
    let delivery_commit = delivered_task
        .receipt
        .as_ref()
        .and_then(|receipt| receipt.commit.as_deref())
        .unwrap()
        .to_owned();
    ev.commit = Some("caller-selected-commit".into());
    assert_eq!(
        tasks::submit(
            root.root(),
            TaskHarness::pass_request(task_id, "deliver", &ev),
        )
        .unwrap_err()
        .to_string(),
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}'. supplied commit caller-selected-commit != accepted receipt commit {delivery_commit}. Do: resubmit with commit {delivery_commit}."
        )
    );
    assert_eq!(deliver_res["status"], "completed");
    assert_eq!(delivery_commit.len(), 40);
    assert_ne!(delivery_commit, build_snap);
    assert_eq!(deliver_res["snapshot"]["commit"], delivery_commit);
    assert_eq!(
        deliver_res["receipt"]["commit"], delivery_commit,
        "the returned receipt must carry the delivery commit"
    );
    let delivery_message = TaskHarness::commit_message(root.root());
    assert!(delivery_message.status.success());
    let delivery_message = String::from_utf8(delivery_message.stdout).unwrap();
    let target_present = delivery_message.contains(&target);
    let completed_subtask_present =
        delivery_message.contains("Publish release notes and notify maintainers");
    let pending_subtask_excluded = !delivery_message.contains("Optional cleanup follow-up");
    let subtask_title_is_single_line =
        completed_subtask_present && !delivery_message.contains("\nand notify maintainers");
    assert!(
        target_present
            && completed_subtask_present
            && pending_subtask_excluded
            && subtask_title_is_single_line,
        "delivery commit must include task target/completed subtasks, exclude pending subtasks, and normalize title newlines; target_present={target_present}, completed_subtask_present={completed_subtask_present}, pending_subtask_excluded={pending_subtask_excluded}, subtask_title_is_single_line={subtask_title_is_single_line}, message={delivery_message:?}"
    );
    let unrelated_staged_after = TaskHarness::staged_entries(root.root(), &["README.md"]);
    assert_eq!(unrelated_staged_after.stdout, unrelated_staged_before);
    let clean_task_paths = TaskHarness::tracked_status(
        root.root(),
        &["src/new_feature.rs", "docs/milestones/010-test.md"],
    );
    assert!(clean_task_paths.status.success());
    assert!(
        clean_task_paths.stdout.is_empty(),
        "delivery task paths must be clean in the real index and worktree: {}",
        String::from_utf8_lossy(&clean_task_paths.stdout)
    );
    let committed_milestone =
        TaskHarness::snapshot_content(root.root(), &delivery_commit, "docs/milestones/010-test.md");
    assert!(committed_milestone.status.success());
    let committed_text = String::from_utf8(committed_milestone.stdout).unwrap();
    let committed_spec = Milestone::parse(&committed_text, "forge-mcp").unwrap();
    assert_eq!(committed_spec.tasks[0].status.as_deref(), Some("completed"));
    assert_eq!(
        committed_spec.tasks[0]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        None,
        "delivery commit must not embed its own SHA in the milestone receipt"
    );
    let committed_readme =
        TaskHarness::snapshot_content(root.root(), &delivery_commit, "README.md");
    assert!(committed_readme.status.success());
    assert_eq!(committed_readme.stdout, b"# sibling change\n");
    assert!(TaskHarness::restore_head_paths(root.root(), &["README.md"])
        .status
        .success());
    let written = std::fs::read_to_string(root.root().join("docs/milestones/010-test.md")).unwrap();
    let milestone = Milestone::parse(&written, "forge-mcp").unwrap();
    assert_eq!(
        milestone.tasks[0]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        None,
        "the active milestone receipt stays commitless until handoff"
    );
    let details = tasks::store(root.root())
        .unwrap()
        .inspect_details(task_id)
        .unwrap();
    assert_eq!(details["receipt"]["commit"], delivery_commit);
    assert_eq!(details["latest_snapshot"]["commit"], build_snap);
    assert_eq!(
        details["latest_snapshot"]["inspect_cmd"],
        format!("git show {}", &build_snap[..7])
    );
    for gate in details["gates"].as_array().unwrap() {
        let evidence: serde_json::Value =
            serde_json::from_str(gate["evidence"].as_str().unwrap()).unwrap();
        let expected = if gate["stage"] == "deliver" {
            delivery_commit.as_str()
        } else {
            build_snap.as_str()
        };
        assert_eq!(gate["commit"], expected);
        assert_eq!(evidence["commit"], expected);
        if gate["stage"] == "build" {
            assert_eq!(evidence["candidate_baseline_head"], delivery_baseline);
        }
        if gate["stage"] == "deliver" {
            assert_eq!(evidence["candidate_baseline_head"], delivery_baseline);
        }
    }
    let sqlite_store = tasks::store(root.root()).unwrap();
    let build_evidence_json =
        TaskHarness::gate_evidence_json(&sqlite_store, task_id, "build").unwrap();
    let build_evidence: serde_json::Value = serde_json::from_str(&build_evidence_json).unwrap();
    assert_eq!(build_evidence["commit"], build_snap);
    assert_eq!(build_evidence["candidate_baseline_head"], delivery_baseline);
    let delivery_evidence_json =
        TaskHarness::gate_evidence_json(&sqlite_store, task_id, "deliver").unwrap();
    let delivery_evidence: serde_json::Value =
        serde_json::from_str(&delivery_evidence_json).unwrap();
    assert_eq!(delivery_evidence["commit"], delivery_commit);
    assert_eq!(
        delivery_evidence["candidate_baseline_head"],
        delivery_baseline
    );
    let refs_out = TaskHarness::references(root.root(), "refs/forge/snapshots/");
    let refs_text = String::from_utf8_lossy(&refs_out.stdout);
    assert!(refs_text.contains("refs/forge/snapshots/"));
    let ready_tasks = tasks::store(root.root())
        .unwrap()
        .list(None, "ready", None)
        .unwrap();
    let task_id_2 = ready_tasks[0].task_id.clone();
    let milestone_branch = TaskHarness::branch_name(root.root());
    let milestone_branch = String::from_utf8_lossy(&milestone_branch.stdout)
        .trim()
        .to_owned();
    assert!(
        TaskHarness::create_branch(root.root(), "task-second-delivery")
            .status
            .success()
    );
    let mut second_task = TaskHarness::attach(&root, &task_id_2);
    let completed_second_task = second_task
        .advance_to_completion()
        .expect("second task should pass its profile-driven gates");
    assert_eq!(completed_second_task.status, "completed");
    let second_delivery_commit = completed_second_task
        .receipt
        .and_then(|receipt| receipt.commit)
        .unwrap();
    assert!(TaskHarness::checkout_branch(root.root(), &milestone_branch)
        .status
        .success());
    assert!(
        TaskHarness::merge_branch(root.root(), "task-second-delivery", "merge second task")
            .status
            .success()
    );
    let merge_commit = TaskHarness::revision(root.root(), "HEAD");
    let merge_commit = String::from_utf8_lossy(&merge_commit.stdout)
        .trim()
        .to_owned();
    for delivered in [&delivery_commit, &second_delivery_commit] {
        assert!(
            TaskHarness::is_ancestor(root.root(), delivered, &merge_commit)
                .status
                .success()
        );
    }
    let merge_parent = TaskHarness::revision(root.root(), "HEAD^1");
    assert_eq!(
        String::from_utf8_lossy(&merge_parent.stdout).trim(),
        delivery_commit,
        "the merge landing's first parent is the fast-forwarded first delivery"
    );
    handoff_with_fixture_evidence(root.root(), "m-test", None, "cargo test", 2, 0).unwrap();
    let archive_path = root.root().join("docs/milestones/archive/010-test.md");
    let archived_text = std::fs::read_to_string(&archive_path).unwrap();
    let archived = Milestone::parse(&archived_text, "forge-mcp").unwrap();
    assert_eq!(
        archived.tasks[0]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        Some(delivery_commit.as_str()),
        "first task keeps its reachable accepted delivery pointer"
    );
    assert_eq!(
        archived.tasks[1]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        Some(second_delivery_commit.as_str()),
        "second task keeps its reachable accepted delivery pointer across merge"
    );
    assert_ne!(delivery_commit, merge_commit);
    assert!(tasks::store(root.root())
        .unwrap()
        .list(None, "all", None)
        .unwrap()
        .is_empty());
    let refs_after = TaskHarness::references(root.root(), "refs/forge/snapshots/");
    let refs_after_text = String::from_utf8_lossy(&refs_after.stdout)
        .trim()
        .to_string();
    assert!(
        refs_after_text.is_empty(),
        "Expected all snapshot refs to be pruned on handoff, got: {refs_after_text}"
    );
}
