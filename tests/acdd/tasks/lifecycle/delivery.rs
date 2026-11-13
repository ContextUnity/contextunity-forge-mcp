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
    let large = "\\\"λ\n".repeat(12_000);
    const SYNC_MANY_TASK_TOTAL: usize = 180;
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
        assert!(
            response::serialized_bytes(&wire) < forge_core::response::MAX_OUTPUT_BYTES,
            "{action}"
        );
        assert!(
            value.to_string().len() <= (forge_core::response::MAX_OUTPUT_BYTES * 5) / 8,
            "{action}"
        );
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

#[test]
fn nested_when_seam_candidate_snapshot_resolves_cleanly_at_delivery() {
    let profile_yaml = r#"inherit: none
version: 2
roles:
  worker: {mode: subagent}
guidance:
  contour.paths: {description: Paths, criteria: [Review path changes]}
  contour.claims: {description: Claims, criteria: [Review claim behavior]}
  contour.concurrency: {description: Concurrency, criteria: [Review concurrent updates]}
  contour.project_isolation: {description: Project isolation, criteria: [Review project boundaries]}
  contour.administration: {description: Administration, criteria: [Review administrative behavior]}
gates:
  - tasks:
      gates:
        - id: build
          role: worker
          proof: none
          save: snapshot
        - id: review
          role: worker
          proof: review
          contours: guidance.contours
        - id: seam
          when: seam
          gates:
            - id: integrate
              role: worker
              proof: none
              save: snapshot
            - id: review
              role: worker
              proof: review
              contours: guidance.contours
        - id: deliver
          role: worker
          proof: delivery
          receipt: true
          need: [review]
          auto_commit: false
"#;
    let profile =
        contextunity_forge_mcp::core::tasks::profile::validate_profile_document(profile_yaml)
            .unwrap();
    let manifest = MilestoneFixtureBuilder::new("m-nested-seam-delivery")
        .title("Nested seam delivery")
        .task("first", &["src/lib.rs"])
        .task_target("Deliver candidate after seam integration")
        .task_proof_policy("seam-test-first")
        .task_concurrency_mode("parallel")
        .build();
    let mut h = TaskHarness::disk_with_profile(&manifest, "docs/010-nested-seam.md", profile);
    h.advance_to("seam.integrate").unwrap();
    std::fs::write(
        h.root().join("src/lib.rs"),
        "pub fn integrated_seam_logic() {}\n",
    )
    .unwrap();
    let completed = h.advance_to("completed").unwrap();
    assert_eq!(completed.status, "completed");
    assert!(completed.receipt.is_some());
}

#[test]
fn response_limit_from_config_bounds_context_while_preserving_inherited_proofs() {
    use contextunity_forge_mcp::{core::response::ResponsePolicy, mcp::response};

    let profile_yaml = r#"inherit: none
version: 2
roles:
  architect: {mode: subagent}
  worker: {mode: subagent}
guidance:
  contour.paths: {description: Paths, criteria: [Review path changes]}
  contour.claims: {description: Claims, criteria: [Review claim behavior]}
  contour.concurrency: {description: Concurrency, criteria: [Review concurrent updates]}
  contour.project_isolation: {description: Project isolation, criteria: [Review project boundaries]}
  contour.administration: {description: Administration, criteria: [Review administrative behavior]}
gates:
  - id: blueprint_synthesis
    role: architect
    proof: review
    contours: guidance.contours
  - tasks:
      gates:
        - id: contract
          role: worker
          proof: none
        - id: review
          role: worker
          proof: review
          contours: guidance.contours
        - id: deliver
          role: worker
          proof: delivery
          need: [review]
          auto_commit: false
"#;
    let root = acdd_workspace(DEFAULT_SEAM, "");
    root.write(".forge/acdd/profile.yaml", profile_yaml);
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nresponse:\n  max_output_bytes: 8192\n",
    );
    root.write(".gitignore", super::super::CANONICAL_FORGE_GITIGNORE);
    TaskHarness::commit_handoff_fixture_inputs(&root, &[".forge/acdd/profile.yaml"]);
    root.write("src/lib.rs", "pub fn delivered() -> bool { true }\n");

    let manifest = MilestoneFixtureBuilder::new("m-bounded-proofs")
        .title("Bounded context preserves inherited proofs")
        .task("bounded-task", &["src/lib.rs"])
        .task_target("Verify configured response limit preserves inherited proofs")
        .task_proof_policy("seam-test-first")
        .build();
    let milestone_ref = "docs/milestones/010-bounded-proofs.md";
    root.write(milestone_ref, &manifest);
    let (repo, project) = tasks::project_identity(root.root()).unwrap();
    let milestone = Milestone::parse(&manifest, &repo).unwrap();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let mut store =
        TasksStore::open_project(&root.root().join(".forge/tasks.sqlite"), &repo, &project)
            .unwrap();
    store.sync(&milestone, milestone_ref, root.root()).unwrap();

    let milestone_identity = format!("{repo}/{project}/{}", milestone.id);
    let inherited_slice = serde_json::json!({"goal": "keep-me"});
    let proof_payload = serde_json::json!({"tasks": {"bounded-task": inherited_slice.clone()}});
    let head_sha = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root.root())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap();
    store
        .connection
        .execute(
            "INSERT INTO gate_transitions (
                milestone_id, subject, gate_path, action, proof_kind, proof_payload,
                git_tree_sha, worker_id
            ) VALUES (?1, 'milestone', 'blueprint_synthesis', 'pass', 'blueprint', ?2, ?3, 'architect')",
            rusqlite::params![milestone_identity, proof_payload.to_string(), head_sha],
        )
        .unwrap();
    store
        .blackboard_post(
            &task_id,
            "architect",
            "notes",
            &"context padding ".repeat(400),
        )
        .unwrap();
    drop(store);

    let claimed = tasks::claim(
        root.root(),
        tasks::Claim {
            task_id: task_id.clone(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            worktree: root.root().to_string_lossy().into_owned(),
            bundle: Some(true),
            session_fingerprint: Some(TaskHarness::session_for("builder")),
        },
    )
    .unwrap();
    let transitions_before_context: i64 = {
        let store =
            TasksStore::open_project(&root.root().join(".forge/tasks.sqlite"), &repo, &project)
                .unwrap();
        store
            .connection
            .query_row("SELECT count(*) FROM gate_transitions", [], |row| {
                row.get(0)
            })
            .unwrap()
    };
    let context = tasks::manage(
        root.root(),
        tasks::Manage {
            action: tasks::ManageAction::Context,
            task_id: Some(task_id.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let transitions_after_context: i64 = {
        let store =
            TasksStore::open_project(&root.root().join(".forge/tasks.sqlite"), &repo, &project)
                .unwrap();
        store
            .connection
            .query_row("SELECT count(*) FROM gate_transitions", [], |row| {
                row.get(0)
            })
            .unwrap()
    };
    assert_eq!(
        transitions_before_context, transitions_after_context,
        "task context must remain strictly read-only on gate_transitions"
    );

    let custom_config_path = root.root().join("configs/override.yaml");
    root.write(
        "configs/override.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nresponse:\n  max_output_bytes: 16384\nlimits:\n  max_metadata_bytes: 512\n",
    );
    let selected_settings =
        contextunity_forge_mcp::core::tasks::configuration::InvocationSettings::new(
            root.root().canonicalize().unwrap(),
            Some(custom_config_path),
            std::sync::Arc::new(
                contextunity_forge_mcp::core::tasks::profile::validate_profile_document(
                    profile_yaml,
                )
                .unwrap(),
            ),
            None,
            root.root().join(".forge/code.sqlite"),
        );
    let selected_context = selected_settings
        .run(|| {
            tasks::manage(
                root.root(),
                tasks::Manage {
                    action: tasks::ManageAction::Context,
                    task_id: Some(task_id),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    assert!(selected_context.to_string().len() <= (16384 * 5) / 8);
    assert!(selected_context.to_string().len() > (8192 * 5) / 8);

    for (label, value) in [("claim", claimed), ("context", context)] {
        assert_eq!(value["byte_limited"], true, "{label}");
        assert!(value.to_string().len() <= (8192 * 5) / 8, "{label}");
        let wire = response::result(
            Ok(value.clone()),
            &ResponsePolicy {
                max_output_bytes: 8192,
                ..Default::default()
            },
        );
        assert_ne!(wire.is_error, Some(true), "{label}");
        assert!(response::serialized_bytes(&wire) <= 8192, "{label}");
        let proofs = value
            .pointer("/context_bundle/inherited_proofs")
            .and_then(|v| v.as_array())
            .expect("inherited_proofs must survive bounding");
        assert_eq!(proofs.len(), 1, "{label}");
        assert_eq!(proofs[0]["proof"], inherited_slice, "{label}");
    }
}

#[test]
fn metadata_limit_from_config_validates_bounds_and_regulates_profile() {
    use contextunity_forge_mcp::core::config::{load_adapter, MAX_METADATA_BYTES};
    use contextunity_forge_mcp::core::tasks::{
        configuration::InvocationSettings, profile::load_for_workspace,
    };
    use contextunity_forge_mcp::forge_tasks::config::load_workspace_adapter;

    let root = acdd_workspace(DEFAULT_SEAM, "");

    for invalid in ["0", &format!("{}", MAX_METADATA_BYTES + 1), "-1", "\"abc\""] {
        root.write(
            "forge-mcp.yaml",
            &format!("roots: [src]\ndocs: [docs]\nlimits:\n  max_metadata_bytes: {invalid}\n"),
        );
        assert!(load_adapter(root.root(), None).is_err(), "{invalid}");
        assert!(load_workspace_adapter(root.root()).is_err(), "{invalid}");
        let profile_err = load_for_workspace(root.root()).unwrap_err().to_string();
        assert!(
            profile_err.contains("ACDD_PROFILE_SCHEMA_INVALID"),
            "unexpected error for {invalid}: {profile_err}"
        );
    }

    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\nlimits:\n  max_metadata_bytes: 256\n",
    );
    let adapter = load_adapter(root.root(), None).unwrap();
    assert_eq!(adapter.limits.max_metadata_bytes, 256);
    assert_eq!(
        load_workspace_adapter(root.root())
            .unwrap()
            .limits
            .max_metadata_bytes,
        256
    );
    let profile = load_for_workspace(root.root()).unwrap();
    assert_eq!(profile.max_metadata_bytes(), 256);

    let alt_config = root.root().join("configs/alt-meta.yaml");
    root.write(
        "configs/alt-meta.yaml",
        "roots: [src]\ndocs: [docs]\nlimits:\n  max_metadata_bytes: 512\n",
    );
    let selected = InvocationSettings::new(
        root.root().canonicalize().unwrap(),
        Some(alt_config),
        profile,
        None,
        root.root().join(".forge/code.sqlite"),
    );
    selected.run(|| {
        assert_eq!(
            load_workspace_adapter(root.root())
                .unwrap()
                .limits
                .max_metadata_bytes,
            512
        );
        assert_eq!(
            load_for_workspace(root.root())
                .unwrap()
                .max_metadata_bytes(),
            512
        );
    });
}

#[test]
fn renamed_seam_test_gate_exposes_seam_partners_without_hardcoded_stage_name() {
    let profile_yaml = r#"inherit: none
version: 2
roles:
  worker: {mode: subagent}
commands:
  check_seam:
    command: "true"
    output:
      require_counts: false
      seam_ref_pattern: '--seam-ref (\\S+)'
gates:
  - id: blueprint_synthesis
    role: worker
    proof: none
  - tasks:
      gates:
        - id: build
          role: worker
          proof: none
          save: snapshot
        - id: custom_integration
          when: seam
          gates:
            - id: verify_cross_seam
              role: worker
              proof: seam_test
              command: check_seam
              save: snapshot
        - id: deliver
          role: worker
          proof: delivery
          receipt: true
          auto_commit: false
"#;
    let profile =
        contextunity_forge_mcp::core::tasks::profile::validate_profile_document(profile_yaml)
            .unwrap();
    let manifest = r#"---
id: m-renamed-seam
title: Renamed seam_test gate context
doc_type: contract
status: active
---
```yaml
task_ref: provider
target: Provide shared seam symbol
proof_policy: direct-proof
scope: [src/provider.rs]
symbols: [fn:shared_seam]
```
```yaml
task_ref: consumer
target: Integrate against provider seam
proof_policy: seam-test-first
scope: [src/consumer.rs]
depends_on: [provider]
concurrency:
  mode: parallel
```
"#;
    let root = acdd_workspace(DEFAULT_SEAM, "");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write(".gitignore", super::super::CANONICAL_FORGE_GITIGNORE);
    root.write(
        ".forge/acdd/profile.yaml",
        &format!(
            "inherit: none\n{}",
            serde_yaml::to_string(&profile).unwrap()
        ),
    );
    root.write("src/provider.rs", "pub fn shared_seam() -> bool { true }\n");
    root.write("src/consumer.rs", "pub fn consumer() -> bool { true }\n");
    let milestone_ref = "docs/milestones/010-renamed-seam.md";
    root.write(milestone_ref, manifest);
    TaskHarness::commit_handoff_fixture_inputs(
        &root,
        &[
            ".gitignore",
            "forge-mcp.yaml",
            ".forge/acdd/profile.yaml",
            "src/provider.rs",
            "src/consumer.rs",
            milestone_ref,
        ],
    );

    let (repo, project) = tasks::project_identity(root.root()).unwrap();
    let milestone = Milestone::parse(manifest, &repo).unwrap();
    let provider_id = milestone.task_id(&milestone.tasks[0]);
    let consumer_id = milestone.task_id(&milestone.tasks[1]);
    let mut store = tasks::store_for_task(root.root(), &provider_id).unwrap();
    store.sync(&milestone, milestone_ref, root.root()).unwrap();
    let milestone_identity = format!("{repo}/{project}/{}", milestone.id);
    store
        .connection
        .execute(
            "INSERT INTO gate_transitions (
                milestone_id, subject, gate_path, action, proof_kind, proof_payload,
                git_tree_sha, worker_id
            ) VALUES (?1, 'milestone', 'blueprint_synthesis', 'pass', 'none', '{}', 'architect-tree', 'architect')",
            rusqlite::params![milestone_identity],
        )
        .unwrap();
    store
        .connection
        .execute(
            "INSERT INTO gate_transitions (
                milestone_id, subject, gate_path, action, proof_kind, proof_payload,
                git_tree_sha, worker_id
            ) VALUES (?1, ?2, 'blueprint_synthesis', 'advance', 'blueprint', ?3, 'architect-tree', 'architect')",
            rusqlite::params![
                milestone_identity,
                consumer_id,
                serde_json::json!({
                    "goal": "integrate provider seam",
                    "incoming_seams": [{
                        "source_task": "provider",
                        "contract_type": "seam_dependency",
                        "contract_ref": "public-api"
                    }]
                })
                .to_string(),
            ],
        )
        .unwrap();
    drop(store);

    let mut provider = TaskHarness::attach(&root, &provider_id);
    provider.advance_to("completed").unwrap();

    let mut consumer = TaskHarness::attach(&root, &consumer_id);
    consumer
        .advance_to("custom_integration.verify_cross_seam")
        .unwrap();

    let context = tasks::manage(
        root.root(),
        tasks::Manage {
            action: tasks::ManageAction::Context,
            task_id: Some(consumer_id),
            ..Default::default()
        },
    )
    .unwrap();
    let seam_partners = context
        .pointer("/context_bundle/seam_partners")
        .and_then(|v| v.as_array())
        .expect("renamed seam_test gate must include seam_partners");
    assert!(
        seam_partners
            .iter()
            .any(|partner| partner.to_string().contains("provider")),
        "expected provider in seam_partners, got {seam_partners:?}"
    );
}
