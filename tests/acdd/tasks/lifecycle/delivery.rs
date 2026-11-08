use super::*;

#[test]
fn delivery_requires_terminal_subtasks_with_verification_evidence() {
    let root = acdd_workspace(DEFAULT_SEAM, "");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write(".gitignore", super::super::CANONICAL_FORGE_GITIGNORE);
    TaskHarness::commit_handoff_fixture_inputs(&root, &[".forge/acdd/profile.yaml"]);
    root.write("src/lib.rs", "pub fn delivered() -> bool { true }\n");
    let manifest = MilestoneFixtureBuilder::new("m-delivery-subtasks")
        .title("Delivery subtask guard")
        .task("deliver", &["src/lib.rs"])
        .task_target("Require completed subtask proof at delivery")
        .task_proof_policy("seam-test-first")
        .task_subtasks(&[contextunity_forge_mcp::core::tasks::SubtaskSpec {
            subtask_ref: "verify".into(),
            title: "Verify the delivered behavior".into(),
            status: "completed".into(),
            evidence: Some("verified".into()),
        }])
        .build();
    let milestone_ref = "docs/milestones/010-delivery-subtasks.md";
    root.write(milestone_ref, &manifest);
    let milestone = Milestone::parse(&manifest, "forge-mcp").unwrap();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let mut store = TasksStore::open(&root.root().join(".forge/tasks.sqlite")).unwrap();
    store.sync(&milestone, milestone_ref, root.root()).unwrap();
    drop(store);

    let mut h = TaskHarness::attach(&root, &task_id);
    h.advance_to("deliver").unwrap();
    h.claim_next().unwrap();

    for (status, evidence_value, expected_error) in [
        ("in_progress", None, "TASK_SUBTASKS_INCOMPLETE"),
        ("completed", None, "TASK_SUBTASK_EVIDENCE_MISSING"),
        ("completed", Some("   "), "TASK_SUBTASK_EVIDENCE_MISSING"),
    ] {
        let update_evidence = if status == "completed" && evidence_value.is_none() {
            Some("")
        } else {
            evidence_value
        };
        h.store
            .subtask_update(&task_id, "verify", status, update_evidence)
            .unwrap();
        let task = h.store.inspect(&task_id).unwrap();
        let error = tasks::submit(
            root.root(),
            TaskHarness::pass_request(&task_id, "deliver", &TaskHarness::default_evidence(&task)),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains(expected_error), "{status}: {error}");
        let unchanged = h.store.inspect(&task_id).unwrap();
        assert_eq!(unchanged.stage, "deliver", "{status}");
        assert_eq!(unchanged.status, "in_progress", "{status}");
        h.store
            .subtask_update(&task_id, "verify", "completed", Some("verified"))
            .unwrap();
    }
}

#[test]
fn create_rechecks_tombstone_after_waiting_for_writer() {
    let (error, empty_after_conflict) = check_blocked_sync_or_create("create").unwrap();
    assert!(error.contains("STORE_SYNC_FAILED"), "{error}");
    assert!(empty_after_conflict);
}

fn check_blocked_sync_or_create(mode: &str) -> anyhow::Result<(String, bool)> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Mutex, OnceLock};

    static RACE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    static BUSY_RETRIES: AtomicUsize = AtomicUsize::new(0);
    let _race = RACE_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    BUSY_RETRIES.store(0, Ordering::SeqCst);

    let (root, _, milestone) = fixture(DEFAULT_SEAM);
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let database = root.path(".forge/tasks.sqlite");
    let mut harness = TaskHarness::attach(&root, &task_id);
    let tx = harness.begin_competing_handoff_tombstone(&format!(
        "{}/{}/{}",
        milestone.repository, milestone.project, milestone.id
    ))?;
    let (started, observed_start) = mpsc::sync_channel(1);
    let root_path = root.root().to_owned();
    let path = database.clone();
    let mode = mode.to_owned();
    let worker = std::thread::spawn(move || -> anyhow::Result<(String, bool)> {
        let mut competing = TasksStore::open(&path)?;
        let milestone = Milestone::parse(SPEC, "forge-mcp")?;
        anyhow::ensure!(
            !competing.milestone_tombstoned(&milestone.id)?,
            "competing store must observe the pre-handoff state before blocking"
        );
        competing.connection.busy_handler(Some(|_: i32| {
            BUSY_RETRIES.fetch_add(1, Ordering::SeqCst);
            std::thread::yield_now();
            true
        }))?;
        started.send(()).expect("race caller is still waiting");
        let error = if mode == "create" {
            competing.create(&milestone, "docs/010-test.md", &root_path, "first")
        } else {
            competing.sync(&milestone, "docs/010-test.md", &root_path)
        }
        .unwrap_err()
        .to_string();
        let remaining_tasks = competing.list(None, "all", None)?.len();
        Ok((error, remaining_tasks == 0))
    });
    observed_start.recv_timeout(std::time::Duration::from_secs(10))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while BUSY_RETRIES.load(Ordering::SeqCst) == 0 {
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "writer did not reach SQLite busy handler"
        );
        std::thread::yield_now();
    }
    // The second connection has completed its stale read and is blocked on the writer lock.
    tx.commit()?;
    worker.join().expect("competing sync thread must not panic")
}

#[test]
fn sync_race_and_initialized_reader_use_disk_wal() {
    let (error, empty_after_conflict) = check_blocked_sync_or_create("sync").unwrap();
    assert!(error.contains("STORE_SYNC_FAILED"), "{error}");
    assert!(empty_after_conflict);

    let (root, _, milestone) = fixture(DEFAULT_SEAM);
    let task_id = "forge-mcp/m-test:first";
    let mut harness = TaskHarness::attach(&root, task_id);
    let tx = harness
        .begin_competing_handoff_tombstone(&format!(
            "{}/{}/{}",
            milestone.repository, milestone.project, milestone.id
        ))
        .unwrap();
    let reader = TasksStore::open(&root.root().join(".forge/tasks.sqlite"))
        .expect("initialized readers must open while another connection holds the writer lock");
    assert_eq!(reader.inspect(task_id).unwrap().task_id, task_id);
    tx.rollback().unwrap();
}

#[test]
fn in_memory_mutations_reject_archived_milestones_without_changes() {
    for action in ["sync", "create"] {
        let mut h = TaskHarness::memory();
        let root = h.root().to_owned();
        let milestone = Milestone::parse(SPEC, "forge-mcp").unwrap();
        assert!(!h.store.list(None, "all", None).unwrap().is_empty());
        // Match the committed task and tombstone effects of the handoff race.
        h.begin_competing_handoff_tombstone(&format!(
            "{}/{}/{}",
            milestone.repository, milestone.project, milestone.id
        ))
        .unwrap()
        .commit()
        .unwrap();
        assert!(h.store.list(None, "all", None).unwrap().is_empty());
        let result = if action == "create" {
            h.store
                .create(&milestone, "docs/010-test.md", &root, "first")
        } else {
            h.store.sync(&milestone, "docs/010-test.md", &root)
        };
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("STORE_SYNC_FAILED"),
            "{action}"
        );
        assert!(
            h.store.list(None, "all", None).unwrap().is_empty(),
            "{action}"
        );
        assert!(
            h.store.milestone_tombstoned(&milestone.id).unwrap(),
            "{action}"
        );
    }
}

#[test]
fn task_sync_many_payload_is_bounded_without_skipping_effects() {
    assert_task_mutation_payloads_for_actions(&["sync_many"]);
}

#[test]
fn task_mutation_payloads_are_bounded_without_skipping_effects() {
    assert_task_mutation_payloads_for_actions(&["sync", "next", "claim", "minimal", "context"]);
}

fn assert_task_mutation_payloads_for_actions(actions: &[&str]) {
    use contextunity_forge_mcp::{core::response::ResponsePolicy, mcp::response};

    let (root, mut store, _) = fixture(DEFAULT_SEAM);
    let large = "\\\"λ\n".repeat(1_200);
    const SYNC_MANY_TASK_TOTAL: usize = 63;
    let large_spec = actions
        .iter()
        .any(|&action| action != "sync_many")
        .then(|| {
            let mut large_spec = SPEC
        .replace(
            "invariants: [isolated]",
            &format!("invariants: [{}]", serde_json::to_string(&large).unwrap()),
        )
        .replace("task_ref: first", "task_ref: next")
        .replace("task_ref: second", "task_ref: sync")
        .replace(
            "task_ref: next\ntarget: Deliver first\nproof_policy: seam-test-first\nscope: [src/]",
            "task_ref: next\ntarget: Deliver first\nproof_policy: seam-test-first\nscope: [src/next.rs]",
        )
        .replace(
            "task_ref: sync\ntarget: Deliver second\nproof_policy: seam-test-first\nscope: [src/]",
            "task_ref: sync\ntarget: Deliver second\nproof_policy: seam-test-first\nscope: [src/sync.rs]",
        )
        .replace("depends_on: [first]\n", "");
    for task_ref in ["z-claim", "z-minimal", "z-context"] {
        large_spec.push_str(&format!("```yaml\ntask_ref: {task_ref}\ntarget: Payload check for {task_ref}\nproof_policy: seam-test-first\nscope: [src/{task_ref}.rs]\n```\n"));
    }
    large_spec
});
    let sync_many_spec = actions.contains(&"sync_many").then(|| {
        let mut sync_many_spec = SPEC.to_owned();
    for index in 0..(SYNC_MANY_TASK_TOTAL - 2) {
        sync_many_spec.push_str(&format!("```yaml\ntask_ref: extra-{index}\ntarget: Extra task\nproof_policy: seam-test-first\nscope: [src/extra-{index}.rs]\n```\n"));
    }
    sync_many_spec
});
    for &action in actions {
        if action == "sync_many" {
            root.write(
                "docs/010-test.md",
                sync_many_spec
                    .as_deref()
                    .expect("sync_many fixture is prepared"),
            );
        } else if action == "sync" {
            store
                .delete(
                    None,
                    Some("docs/010-test.md"),
                    true,
                    contextunity_forge_mcp::db::tasks_store::now(),
                )
                .unwrap();
            root.write(
                "docs/010-test.md",
                large_spec.as_deref().expect("large fixture is prepared"),
            );
        }
        let task_id = match action {
            "sync_many" => "forge-mcp/m-test:first",
            "next" => "forge-mcp/m-test:next",
            "sync" => "forge-mcp/m-test:sync",
            "claim" => "forge-mcp/m-test:z-claim",
            "minimal" => "forge-mcp/m-test:z-minimal",
            _ => "forge-mcp/m-test:z-context",
        };
        let value = if matches!(action, "claim" | "minimal") {
            tasks::claim(
                root.root(),
                tasks::Claim {
                    task_id: task_id.into(),
                    stage: "contract".into(),
                    worker_id: "builder".into(),
                    worktree: root.root().to_string_lossy().into_owned(),
                    bundle: Some(action != "minimal"),
                    session_fingerprint: Some(TaskHarness::session_for("builder")),
                },
            )
            .unwrap()
        } else {
            tasks::manage(
                root.root(),
                tasks::Manage {
                    action: match action {
                        "sync" | "sync_many" => tasks::ManageAction::Sync,
                        "next" => tasks::ManageAction::Next,
                        _ => tasks::ManageAction::Context,
                    },
                    milestone_ref: action
                        .starts_with("sync")
                        .then(|| "docs/010-test.md".into()),
                    task_id: (action == "context").then(|| task_id.into()),
                    worker_id: (action == "next").then(|| "builder".into()),
                    worktree: (action == "next")
                        .then(|| root.root().to_string_lossy().into_owned()),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let wire = response::result(Ok(value.clone()), &ResponsePolicy::default());
        assert_ne!(
            wire.is_error,
            Some(true),
            "{action}: mutation must return usable success: {wire:?}"
        );
        assert!(response::serialized_bytes(&wire) < 65536, "{action}");
        assert!(value.to_string().len() <= 24 * 1024, "{action}");
        if action == "sync_many" {
            assert_eq!(value["total"], SYNC_MANY_TASK_TOTAL);
            assert_eq!(value["byte_limited"], true);
            assert!(value["returned"].as_u64().unwrap() < SYNC_MANY_TASK_TOTAL as u64);
            let count = store.list(None, "all", None).unwrap().len();
            assert_eq!(
                count, SYNC_MANY_TASK_TOTAL,
                "response bounding must not skip sync effects"
            );
        } else if action != "sync" {
            assert_eq!(value["byte_limited"], true);
            assert!(!value["omitted_fields"].as_array().unwrap().is_empty());
        }
        let stored_invariants = store.inspect(task_id).unwrap().applicable_invariants;
        assert_eq!(stored_invariants.len(), 1, "{action}");
        let expected_invariant = if action == "sync_many" {
            "isolated"
        } else {
            large.as_str()
        };
        assert_eq!(
            stored_invariants[0].as_str(),
            expected_invariant,
            "{action}"
        );
        if matches!(action, "claim" | "minimal" | "next") {
            assert_eq!(value["task_id"], task_id);
            assert_eq!(
                value["claim_revision"],
                store.inspect(task_id).unwrap().claim_revision
            );
            assert!(value["workflow_guidance"].is_object());
        }
    }
}
