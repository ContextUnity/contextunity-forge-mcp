use crate::acdd::{
    harness::{plans::PlanHarness, workflows::WorkflowHarness},
    support::{git_command, git_head},
};
use clap::Parser;
use contextunity_forge_mcp::{cli::Cli, forge_tasks::tasks::profile};
use serde_json::Value;
use std::fs;

mod harness;

#[test]
fn milestone_corrupt_root_route_fails_closed_without_reexecuting_observations() {
    use forge_tasks::engine::orchestrator::waves::run_milestone_waves;
    let mut h = harness::WaveHarness::new(true);
    h.pre_task_checks(true);
    assert!(run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).is_err());
    h.task.store.connection.execute("UPDATE gate_transitions SET proof_payload=json_set(proof_payload, '$.previous_observation', id) WHERE id=(SELECT max(id) FROM gate_transitions WHERE proof_kind='orchestrator')", []).unwrap();
    let error = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
    assert!(
        error.to_string().contains("MILESTONE_CHECKPOINT_INVALID"),
        "{error:#}"
    );
    assert_eq!(
        fs::read_to_string(h.root().join(".forge/preflight.log")).unwrap(),
        "seen"
    );
}

#[test]
fn milestone_partial_root_prefix_reuses_refreshed_suffix_on_unchanged_restart() {
    use forge_tasks::engine::{candidate_fingerprint, orchestrator::waves::run_milestone_waves};
    let mut h = harness::WaveHarness::new(true);
    h.partial_pre_task_checks(false);
    let original = fs::read(h.root().join("src/lib.rs")).unwrap();
    let initial = candidate_fingerprint(h.root()).unwrap();
    let first = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
    assert!(
        first
            .to_string()
            .contains("WORKFLOW_ATTEMPTS_EXHAUSTED: stop_before_tasks"),
        "{first:#}"
    );
    assert_ne!(candidate_fingerprint(h.root()).unwrap(), initial);
    fs::write(h.root().join("src/lib.rs"), &original).unwrap();
    assert_eq!(candidate_fingerprint(h.root()).unwrap(), initial);
    for _ in 0..2 {
        let resumed = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
        assert!(
            resumed
                .to_string()
                .contains("WORKFLOW_ATTEMPTS_EXHAUSTED: stop_before_tasks"),
            "{resumed:#}"
        );
        assert_eq!(candidate_fingerprint(h.root()).unwrap(), initial);
        assert_eq!(
            fs::read_to_string(h.root().join(".forge/preflight.log")).unwrap(),
            "seen"
        );
        assert_eq!(
            fs::read_to_string(h.root().join(".forge/second-preflight.log")).unwrap(),
            "seenseen"
        );
    }
}

#[test]
fn milestone_partial_root_prefix_preserves_failed_retry_route_on_restart() {
    use forge_tasks::engine::{candidate_fingerprint, orchestrator::waves::run_milestone_waves};
    let mut h = harness::WaveHarness::new(true);
    h.partial_pre_task_checks(true);
    let original = fs::read(h.root().join("src/lib.rs")).unwrap();
    let initial = candidate_fingerprint(h.root()).unwrap();
    let first = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
    assert!(
        first
            .to_string()
            .contains("WORKFLOW_ATTEMPTS_EXHAUSTED: observe_once"),
        "{first:#}"
    );
    assert_eq!(
        fs::read_to_string(h.root().join(".forge/second-preflight.log")).unwrap(),
        "seenseen"
    );
    fs::write(h.root().join("src/lib.rs"), &original).unwrap();
    fs::remove_file(h.root().join(".forge/preflight-retry-done")).unwrap();
    for _ in 0..2 {
        let resumed = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
        assert!(
            resumed
                .to_string()
                .contains("WORKFLOW_ATTEMPTS_EXHAUSTED: observe_once"),
            "{resumed:#}"
        );
        assert_eq!(candidate_fingerprint(h.root()).unwrap(), initial);
        assert_eq!(
            fs::read_to_string(h.root().join(".forge/preflight.log")).unwrap(),
            "seen"
        );
        assert_eq!(
            fs::read_to_string(h.root().join(".forge/second-preflight.log")).unwrap(),
            "seenseenseenseen"
        );
    }
}

#[test]
fn milestone_restart_fences_delivered_but_unintegrated_isolated_predecessor() {
    use forge_tasks::engine::orchestrator::{types::OrchestratorEvent, waves::run_milestone_waves};
    let mut h = harness::WaveHarness::new(false);
    let mut delivered = None;
    let first = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |event| {
        if let OrchestratorEvent::TaskDelivered { receipt_id, .. } = event {
            if delivered.is_none() {
                delivered = Some(receipt_id);
                fs::write(h.root().join("concurrent.txt"), "independent root writer\n").unwrap();
                for args in [
                    vec!["add", "concurrent.txt"],
                    vec!["commit", "-m", "concurrent root writer"],
                ] {
                    assert!(git_command(h.root(), &args).status().unwrap().success());
                }
            }
        }
    })
    .unwrap_err();
    assert!(
        format!("{first:#}").contains("Not possible to fast-forward"),
        "{first:#}"
    );
    let delivered = delivered.unwrap();
    assert!(!git_command(
        h.root(),
        &["merge-base", "--is-ancestor", &delivered, "HEAD"]
    )
    .status()
    .unwrap()
    .success());
    let accepted = h.task.store.inspect(&h.task.id).unwrap();
    assert_eq!(accepted.status, "completed");
    let receipt = serde_json::to_value(&accepted.receipt).unwrap();
    let owned = fs::read_dir(h.root().join(".worktrees"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(git_head(&owned), delivered);
    let head = git_head(h.root());
    for in_place in [false, true] {
        h.options.in_place = in_place;
        let mut claims = Vec::new();
        let restart = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |event| {
            if let OrchestratorEvent::TaskClaimed { task_id, .. } = event {
                claims.push(task_id);
            }
        });
        assert!(
            claims.is_empty(),
            "in_place={in_place}: unintegrated predecessor permitted claims: {claims:?}; {restart:#?}"
        );
        assert!(format!("{:#}", restart.unwrap_err()).contains("MILESTONE_DELIVERY_NOT_INTEGRATED"));
    }
    assert_eq!(git_head(h.root()), head);
    assert_eq!(git_head(&owned), delivered);
    assert_eq!(
        serde_json::to_value(h.task.store.inspect(&h.task.id).unwrap().receipt).unwrap(),
        receipt
    );
    let second = format!("{}:second", h.task.id.rsplit_once(':').unwrap().0);
    let count: i64 = h
        .task
        .store
        .connection
        .query_row(
            "SELECT count(*) FROM task_claims WHERE task_id=?1",
            [&second],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn milestone_pre_task_observations_resume_unchanged_content() {
    use forge_tasks::engine::orchestrator::waves::run_milestone_waves;
    let mut h = harness::WaveHarness::new(true);
    h.pre_task_checks(true);
    for _ in 0..2 {
        let error = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("WORKFLOW_ATTEMPTS_EXHAUSTED: stop_before_tasks"),
            "{error:#}"
        );
    }
    assert_eq!(
        fs::read_to_string(h.root().join(".forge/preflight.log")).unwrap(),
        "seen"
    );
}

#[test]
fn task_claim_fails_closed_until_pre_task_root_gates_pass() {
    let mut h = harness::WaveHarness::new(true);
    h.pre_task_checks(false);

    let error = h.task.claim_next().unwrap_err();
    let message = format!("{error:#}");
    assert!(
        message.contains("MILESTONE_PRE_TASK_GATE_PENDING"),
        "{message}"
    );
    assert!(message.contains("preflight"), "{message}");
    assert!(message.contains("gate_path"), "{message}");
    assert_eq!(h.task.store.inspect(&h.task.id).unwrap().status, "ready");
}

#[test]
fn task_next_surfaces_pre_task_root_gates_before_wave_tasks() {
    use forge_tasks::engine::{manage, Manage, ManageAction};

    let mut h = harness::WaveHarness::new(true);
    h.pre_task_checks(false);
    let action = manage(
        h.root(),
        Manage {
            action: ManageAction::Next,
            worker_id: Some("next-worker".into()),
            worktree: Some(h.root().to_string_lossy().into_owned()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(action["subject"], "milestone");
    assert_eq!(action["stage"], "preflight");
    assert_eq!(action["active_stage_id"], "preflight");
    assert_eq!(action["gate"]["command"], "preflight");
    let task = h.task.store.inspect(&h.task.id).unwrap();
    assert_eq!(task.status, "ready");
    assert!(
        task.worker_id.is_none(),
        "root gate dispatch claimed a wave task"
    );
    let claims: i64 = h
        .task
        .store
        .connection
        .query_row(
            "SELECT count(*) FROM task_claims WHERE task_id=?1",
            [&task.task_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(claims, 0, "root gate dispatch persisted a wave claim");
}

#[test]
fn task_next_keeps_the_barrier_closed_after_a_pre_task_gate_exhausts() {
    use forge_tasks::engine::orchestrator::waves::run_milestone_waves;
    use forge_tasks::engine::{manage, Manage, ManageAction};

    let mut h = harness::WaveHarness::new(true);
    h.pre_task_checks(true);
    let run_error = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
    assert!(
        format!("{run_error:#}").contains("WORKFLOW_ATTEMPTS_EXHAUSTED"),
        "{run_error:#}"
    );

    let before = h.task.store.inspect(&h.task.id).unwrap();
    let error = manage(
        h.root(),
        Manage {
            action: ManageAction::Next,
            worker_id: Some("next-worker".into()),
            worktree: Some(h.root().to_string_lossy().into_owned()),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("MILESTONE_PRE_TASK_GATE_PENDING"),
        "{error:#}"
    );

    let after = h.task.store.inspect(&h.task.id).unwrap();
    assert_eq!(after.status, "ready");
    assert_eq!(after.claim_revision, before.claim_revision);
    assert!(after.worker_id.is_none());
    let claims: i64 = h
        .task
        .store
        .connection
        .query_row(
            "SELECT count(*) FROM task_claims WHERE task_id=?1",
            [&h.task.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        claims, 0,
        "an exhausted root barrier persisted a task claim"
    );
}

#[test]
fn pre_task_root_transitions_are_persisted_for_the_milestone_subject() {
    use forge_tasks::engine::orchestrator::waves::run_milestone_waves;

    let mut h = harness::WaveHarness::new(true);
    h.pre_task_checks(true);
    let error = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
    assert!(
        format!("{error:#}").contains("WORKFLOW_ATTEMPTS_EXHAUSTED: stop_before_tasks"),
        "{error:#}"
    );

    h.task
        .store
        .connection
        .execute(
            "UPDATE gate_transitions SET proof_kind='blueprint' WHERE gate_path='preflight' AND action='pass'",
            [],
        )
        .unwrap();

    let claim_error = h.task.claim_next().unwrap_err();
    let claim_message = format!("{claim_error:#}");
    assert!(
        claim_message.contains("MILESTONE_PRE_TASK_GATE_PENDING"),
        "{claim_message}"
    );
    assert!(
        claim_message.contains("stop_before_tasks"),
        "{claim_message}"
    );

    let (milestone_id, subject): (String, String) = h
        .task
        .store
        .connection
        .query_row(
            "SELECT milestone_id, subject FROM gate_transitions WHERE gate_path='preflight' ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(!milestone_id.is_empty());
    assert_eq!(subject, "milestone");
}

#[test]
fn task_next_surfaces_post_task_root_gates_after_all_tasks_complete() {
    use forge_tasks::engine::{manage, Manage, ManageAction};

    let mut h = harness::WaveHarness::with_single_task(true);
    h.task.advance_to_completion().unwrap();

    let action = manage(
        h.root(),
        Manage {
            action: ManageAction::Next,
            worker_id: Some("next-worker".into()),
            worktree: Some(h.root().to_string_lossy().into_owned()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(action["subject"], "milestone");
    assert_eq!(action["stage"], "polish");
    assert_eq!(action["active_stage_id"], "polish");
    assert_eq!(action["role"], "wave_role");
    assert_eq!(action["workflow_guidance"]["role"], "wave_role");
    assert_eq!(
        action["workflow_guidance"]["role_guide"],
        "Exercise the real task gate protocol."
    );
    assert_eq!(
        h.task.store.inspect(&h.task.id).unwrap().status,
        "completed"
    );
}

fn install_post_task_route_profile(h: &harness::WaveHarness) {
    let path = h.root().join(".forge/acdd/profile.yaml");
    let mut profile =
        profile::validate_profile_document(&fs::read_to_string(&path).unwrap()).unwrap();
    let task_container = profile
        .gates
        .iter()
        .find(|gate| gate.tasks.is_some())
        .unwrap()
        .clone();
    let mut verify = profile
        .gates
        .iter()
        .find(|gate| gate.id == "verify")
        .unwrap()
        .clone();
    verify.on_success = Some("documentation".into());
    verify.on_fail = Some("polish".into());
    verify.max_attempts = Some(2);
    let mut polish = profile
        .gates
        .iter()
        .find(|gate| gate.id == "polish")
        .unwrap()
        .clone();
    polish.on_success = Some("milestone_review".into());
    let mut milestone_review = profile
        .gates
        .iter()
        .find(|gate| gate.id == "milestone_review")
        .unwrap()
        .clone();
    milestone_review.on_success = Some("verify".into());
    let documentation: profile::GateDef = serde_json::from_value(serde_json::json!({
        "id": "documentation",
        "role": "wave_role",
        "proof": "none",
        "pass": "handoff"
    }))
    .unwrap();
    let handoff = profile
        .gates
        .iter()
        .find(|gate| gate.id == "handoff")
        .unwrap()
        .clone();
    profile.gates = vec![
        task_container,
        verify,
        polish,
        milestone_review,
        documentation,
        handoff,
    ];
    profile.validate().unwrap();
    fs::write(
        path,
        format!(
            "inherit: none\n{}",
            serde_yaml::to_string(&profile).unwrap()
        ),
    )
    .unwrap();
}

fn record_milestone_root_transition(
    h: &harness::WaveHarness,
    gate_path: &str,
    action: &str,
    attempt: u32,
) {
    let manifest = fs::read_to_string(h.root().join(harness::MANIFEST)).unwrap();
    let milestone =
        contextunity_forge_mcp::forge_tasks::tasks::Milestone::parse(&manifest, "forge-mcp")
            .unwrap();
    let milestone_identity = format!(
        "{}/{}/{}",
        milestone.repository, milestone.project, milestone.id
    );
    let payload = serde_json::json!({"attempt":attempt,"findings":null});
    h.task
        .store
        .connection
        .execute(
            "INSERT INTO gate_transitions (
                milestone_id, subject, gate_path, action, proof_kind, proof_payload,
                git_tree_sha, worker_id
            ) VALUES (?1, 'milestone', ?2, ?3, 'orchestrator', ?4, ?5, 'root-route-test')",
            rusqlite::params![
                milestone_identity,
                gate_path,
                action,
                payload.to_string(),
                git_head(h.root())
            ],
        )
        .unwrap();
}

#[test]
fn task_next_follows_verify_pass_edge_to_documentation() {
    use forge_tasks::engine::{manage, Manage, ManageAction};

    let mut h = harness::WaveHarness::with_single_task(true);
    h.task.advance_to_completion().unwrap();
    install_post_task_route_profile(&h);
    record_milestone_root_transition(&h, "verify", "pass", 1);

    let action = manage(
        h.root(),
        Manage {
            action: ManageAction::Next,
            worker_id: Some("next-worker".into()),
            worktree: Some(h.root().to_string_lossy().into_owned()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(action["subject"], "milestone");
    assert_eq!(action["stage"], "documentation");
    assert_eq!(action["gate"]["id"], "documentation");
}

#[test]
fn task_next_follows_polish_pass_edge_after_failed_verification() {
    use forge_tasks::engine::{manage, Manage, ManageAction};

    let mut h = harness::WaveHarness::with_single_task(true);
    h.task.advance_to_completion().unwrap();
    install_post_task_route_profile(&h);
    record_milestone_root_transition(&h, "verify", "fail", 1);
    record_milestone_root_transition(&h, "polish", "pass", 1);

    let action = manage(
        h.root(),
        Manage {
            action: ManageAction::Next,
            worker_id: Some("next-worker".into()),
            worktree: Some(h.root().to_string_lossy().into_owned()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(action["subject"], "milestone");
    assert_eq!(action["stage"], "milestone_review");
    assert_eq!(action["gate"]["id"], "milestone_review");
}

#[test]
fn task_next_follows_the_latest_transition_after_a_repeated_polish_cycle() {
    use forge_tasks::engine::{manage, Manage, ManageAction};

    let mut h = harness::WaveHarness::with_single_task(true);
    h.task.advance_to_completion().unwrap();
    install_post_task_route_profile(&h);
    record_milestone_root_transition(&h, "verify", "fail", 1);
    record_milestone_root_transition(&h, "polish", "pass", 1);
    record_milestone_root_transition(&h, "milestone_review", "fail", 1);
    record_milestone_root_transition(&h, "polish", "pass", 2);

    let action = manage(
        h.root(),
        Manage {
            action: ManageAction::Next,
            worker_id: Some("next-worker".into()),
            worktree: Some(h.root().to_string_lossy().into_owned()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(action["subject"], "milestone");
    assert_eq!(action["stage"], "milestone_review");
    assert_eq!(action["gate"]["id"], "milestone_review");
}

#[test]
fn milestone_waves_select_the_applicable_conditional_task_container() {
    use forge_tasks::engine::orchestrator::waves::run_milestone_waves;

    let mut h = harness::WaveHarness::new(true);
    let mut profile = h.read_profile();
    let task_pos = profile
        .gates
        .iter()
        .position(|gate| gate.tasks.is_some())
        .unwrap();
    let mut code = profile.gates.remove(task_pos);
    code.id = "code".into();
    code.tasks.as_mut().unwrap().when = Some("code".into());
    let mut docs = code.clone();
    docs.id = "docs".into();
    docs.tasks.as_mut().unwrap().when = Some("doc".into());
    docs.tasks.as_mut().unwrap().gates = vec![serde_json::from_value(
        serde_json::json!({"id":"docs_only","role":"wave_role","proof":"none"}),
    )
    .unwrap()];
    profile.gates.insert(task_pos, docs);
    profile.gates.insert(task_pos, code);
    h.commit_profile(&profile, "conditional task tracks");
    h.pre_task_checks(true);
    let error = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
    assert!(
        format!("{error:#}").contains("WORKFLOW_ATTEMPTS_EXHAUSTED: stop_before_tasks"),
        "{error:#}"
    );
}

#[test]
fn invalid_pre_task_task_map_is_rejected_before_root_pass_is_recorded() {
    use forge_tasks::engine::orchestrator::waves::run_milestone_waves;

    let h = harness::WaveHarness::with_blueprint_gate(true);
    fs::write(h.root().join(".forge/invalid-task-proof-map"), "invalid").unwrap();
    let error = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
    assert!(
        format!("{error:#}").contains("WORKFLOW_ATTEMPTS_EXHAUSTED"),
        "{error:#}"
    );

    let (action, payload): (String, String) = h
        .task
        .store
        .connection
        .query_row(
            "SELECT action, proof_payload FROM gate_transitions
             WHERE subject='milestone' AND gate_path='preflight' ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(action, "fail");
    let payload: Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload["findings"]["code"], "MILESTONE_TASK_PROOF_INVALID");
    let accepted_passes: i64 = h
        .task
        .store
        .connection
        .query_row(
            "SELECT count(*) FROM gate_transitions
             WHERE subject='milestone' AND gate_path='preflight' AND action='pass'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(accepted_passes, 0);
    assert_eq!(h.task.store.inspect(&h.task.id).unwrap().status, "ready");
    let claims: i64 = h
        .task
        .store
        .connection
        .query_row("SELECT count(*) FROM task_claims", [], |row| row.get(0))
        .unwrap();
    assert_eq!(claims, 0);
}

#[test]
fn pre_task_blueprint_synthesis_container_barrier_and_per_task_proof_injection() {
    use forge_tasks::engine::orchestrator::waves::run_milestone_waves;

    let mut h = harness::WaveHarness::with_blueprint_gate(true);

    // 1. Direct claim before the preflight gate passes fails closed
    let claim_err = h.task.claim_next().unwrap_err();
    assert!(
        format!("{claim_err:#}").contains("MILESTONE_PRE_TASK_GATE_PENDING"),
        "{claim_err:#}"
    );

    // 2. Stop at milestone_review before handoff so state is not pruned
    let stop = h.root().join(".forge/stop-wave-review");
    fs::write(&stop, "").unwrap();
    let outcome = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {});
    assert!(outcome.is_err(), "{outcome:#?}");

    // 3. Verify gate_transitions contains the fanned-out task proof for subject = full_task_id
    let (action, proof_payload): (String, String) = h
        .task
        .store
        .connection
        .query_row(
            "SELECT action, proof_payload FROM gate_transitions
             WHERE subject=?1 AND gate_path='preflight' AND action='advance'",
            [&h.task.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("per-task proof transition should be recorded");
    assert_eq!(action, "advance");
    let payload: Value = serde_json::from_str(&proof_payload).unwrap();
    assert_eq!(payload["layer"], "fixture seam");
    assert_eq!(payload["summary"], "Exercise the pre-task proof boundary.");
    assert_eq!(payload["blackboard_decision_id"], "wave-blueprint");

    // 4. Verify that task completed successfully with proof receipt
    let inspected = h.task.store.inspect(&h.task.id).unwrap();
    assert_eq!(inspected.status, "completed");
    assert!(inspected.receipt.is_some());
}

#[test]
fn milestone_pre_task_observations_invalidate_changed_implementation() {
    use forge_tasks::engine::orchestrator::waves::run_milestone_waves;
    for committed in [false, true] {
        let mut h = harness::WaveHarness::new(true);
        h.pre_task_checks(true);
        assert!(run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).is_err());
        fs::write(
            h.root().join("src/lib.rs"),
            "pub fn changed_after_preflight() {}\n",
        )
        .unwrap();
        if committed {
            for args in [
                vec!["add", "src/lib.rs"],
                vec!["commit", "-m", "changed implementation"],
            ] {
                assert!(git_command(h.root(), &args).status().unwrap().success());
            }
        }
        for _ in 0..2 {
            let error =
                run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("WORKFLOW_ATTEMPTS_EXHAUSTED: stop_before_tasks"),
                "{error:#}"
            );
            assert_eq!(
                fs::read_to_string(h.root().join(".forge/preflight.log")).unwrap(),
                "seenseen",
                "committed={committed}"
            );
        }
    }
}

#[test]
fn milestone_pre_task_refresh_after_delivery_resumes_without_new_task_claims() {
    use forge_tasks::engine::orchestrator::{types::OrchestratorEvent, waves::run_milestone_waves};
    let mut h = harness::WaveHarness::new(true);
    h.pre_task_checks(false);
    let stop = h.root().join(".forge/stop-wave-review");
    fs::write(&stop, "").unwrap();
    assert!(run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).is_err());
    assert_eq!(
        h.task.store.inspect(&h.task.id).unwrap().status,
        "completed"
    );
    fs::remove_file(stop).unwrap();
    let mut claims = Vec::new();
    let resumed = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |event| {
        if let OrchestratorEvent::TaskClaimed { task_id, .. } = event {
            claims.push(task_id);
        }
    });
    assert!(resumed.is_ok(), "{resumed:#?}");
    assert!(claims.is_empty(), "{claims:?}");
    assert_eq!(
        fs::read_to_string(h.root().join(".forge/preflight.log")).unwrap(),
        "seenseen"
    );
}

#[test]
fn selected_server_worktree_rebases_config_profile_and_index_without_leaking_root() {
    use crate::common::mcp_harness::McpTestHarness;
    let plan = PlanHarness::linear();
    let primary = plan.materialize(plan.path);
    let selected = plan.materialize(plan.path);
    selected.write("src/lib.rs", "pub fn worktree_selected() {}\n");
    let settings = harness::selected_settings(
        primary.root(),
        "configs/custom.yaml",
        "profiles/plan.yaml",
        ".forge/selected-index.sqlite",
    );
    let mut client = settings.run(|| {
        McpTestHarness::with_root_and_db(primary.root(), &settings.code_db, "worktree-server")
            .unwrap()
    });
    let found = client.payload("code_map_search", serde_json::json!({"pattern":"worktree_selected","exact":true,"worktree":selected.root()})).unwrap();
    assert_eq!(found["nodes"]["total"], 1);
    assert!(selected.path(".forge/selected-index.sqlite").exists());
    assert!(!settings.code_db.exists());
    client
        .payload("task_list", serde_json::json!({"worktree":selected.root()}))
        .unwrap();
    assert!(selected.path(".forge/custom-tasks.sqlite").exists());
    assert!(!primary.path(".forge/custom-tasks.sqlite").exists());
    let primary_result = client
        .payload(
            "code_map_search",
            serde_json::json!({"pattern":"worktree_selected","exact":true}),
        )
        .unwrap();
    assert_eq!(primary_result["nodes"]["total"], 0);
    assert!(settings.code_db.exists());
    client.finish().unwrap();
}

#[test]
fn milestone_preflight_enforces_selected_module_policy() {
    use crate::acdd::support::MilestoneFixtureBuilder;
    let plan = PlanHarness::linear();
    let root = plan.materialize(plan.path);
    root.write(
        "docs/milestones/001-selected.md",
        &MilestoneFixtureBuilder::new("m-selected")
            .task("one", &["src/first.rs"])
            .build(),
    );
    root.write("configs/minimal.yaml", "modules: {profile: minimal}\n");
    let error = run_cli(
        root.root(),
        &[
            "--config",
            "configs/minimal.yaml",
            "--profile",
            "profiles/plan.yaml",
            "milestone",
            "init",
            "docs/milestones/001-selected.md",
            "--in-place",
        ],
    )
    .unwrap_err();
    assert!(error.to_string().contains("FEATURE_DISABLED"), "{error:#}");
    assert!(!root.path(".forge/tasks.sqlite").exists());
    let result = run_cli(
        root.root(),
        &[
            "--config",
            "configs/custom.yaml",
            "--profile",
            "profiles/plan.yaml",
            "milestone",
            "init",
            "docs/milestones/001-selected.md",
            "--in-place",
        ],
    )
    .unwrap();
    assert_eq!(result["milestone_id"], "m-selected");
    assert!(root.path(".forge/custom-tasks.sqlite").exists());
    assert!(!root.path(".worktrees").exists());
}

#[test]
fn selected_config_reaches_cli_and_mcp_guide_validation() {
    use crate::common::mcp_harness::McpTestHarness;
    let plan = PlanHarness::linear();
    let root = plan.materialize(plan.path);
    root.write("configs/index.yaml", "roots: [selected]\n");
    root.write("selected/lib.rs", "pub fn selected_guide() {}\n");
    let result = run_cli(
        root.root(),
        &["--config", "configs/index.yaml", "guide", "validate"],
    )
    .unwrap();
    assert_eq!(
        result["roots"],
        serde_json::json!([root.path("selected").canonicalize().unwrap()])
    );
    let selection = harness::selected_settings(
        root.root(),
        "configs/index.yaml",
        "profiles/plan.yaml",
        ".forge/server-index.sqlite",
    );
    let mut client = selection.run(|| {
        McpTestHarness::with_root_and_db(root.root(), &selection.code_db, "guide-server").unwrap()
    });
    let mcp = client
        .payload("forge_guide", serde_json::json!({"topic":"validate"}))
        .unwrap();
    assert_eq!(mcp["roots"], result["roots"]);
    assert_eq!(mcp["files"], result["files"]);
    assert!(!selection.code_db.exists());
    client.finish().unwrap();
}

#[test]
fn validation_attributes_the_explicit_selected_profile() {
    let plan = PlanHarness::linear();
    let root = plan.materialize(plan.path);
    let result = run_cli(
        root.root(),
        &[
            "--config",
            "configs/custom.yaml",
            "--profile",
            "profiles/plan.yaml",
            "validate",
        ],
    )
    .unwrap();
    assert!(result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["code"] == "PROFILE_VALID"
            && diagnostic["path"] == "profiles/plan.yaml"));
}

#[test]
fn selected_adapter_resolves_linked_sources_in_cli_and_mcp_ast_queries() {
    use crate::common::mcp_harness::McpTestHarness;
    let plan = PlanHarness::linear();
    let root = plan.materialize(plan.path);
    root.write(
        "configs/linked.yaml",
        "roots: [src]\nlinked_workspaces:\n  - name: extra\n    path: linked\n",
    );
    root.write("linked/lib.rs", "pub fn linked_selected() {}\n");
    let cli = run_cli(
        root.root(),
        &[
            "--db",
            ".forge/linked-index.sqlite",
            "--config",
            "configs/linked.yaml",
            "ast",
            "grep",
            "pub fn $NAME() {}",
            "--language",
            "rust",
            "--path",
            "[extra]/lib.rs",
        ],
    )
    .unwrap();
    assert_eq!(
        cli["matches"]["items"][0]["path"], "[extra]/lib.rs",
        "{cli}"
    );
    let selection = harness::selected_settings(
        root.root(),
        "configs/linked.yaml",
        "profiles/plan.yaml",
        ".forge/linked-index.sqlite",
    );
    let mut client = selection.run(|| {
        McpTestHarness::with_root_and_db(root.root(), &selection.code_db, "linked-server").unwrap()
    });
    let mcp = client.payload("ast_grep_search", serde_json::json!({"pattern":"pub fn $NAME() {}","language":"rust","path":"[extra]/lib.rs"})).unwrap();
    assert_eq!(mcp["matches"]["items"][0]["path"], "[extra]/lib.rs");
    client.finish().unwrap();
}

#[test]
fn server_claim_uses_retained_explicit_profile_over_configured_profile() {
    use crate::acdd::support::MilestoneFixtureBuilder;
    use crate::common::mcp_harness::McpTestHarness;
    let plan = PlanHarness::linear();
    let root = plan.materialize(plan.path);
    root.write(
        "docs/milestones/001-selected.md",
        &MilestoneFixtureBuilder::new("m-selected")
            .task("one", &["src/first.rs"])
            .build(),
    );
    let selection = harness::selected_settings(
        root.root(),
        "configs/custom.yaml",
        "profiles/plan.yaml",
        ".forge/server-index.sqlite",
    );
    let mut client = selection.run(|| {
        McpTestHarness::with_root_and_db(root.root(), &selection.code_db, "profile-server").unwrap()
    });
    // Configuration points to rejected.yaml; the explicitly selected, already
    // validated profile remains bound even when its source changes afterward.
    root.write(
        "profiles/plan.yaml",
        include_str!("../fixtures/plans/rejected-profile.yaml"),
    );
    client
        .payload(
            "task_manage",
            serde_json::json!({"action":"sync","milestone_ref":"docs/milestones/001-selected.md"}),
        )
        .unwrap();
    let result = client.payload("task_claim", serde_json::json!({"task_id":"forge-mcp/m-selected:one","stage":"record","worker_id":"selected-profile-worker","worktree":root.root()})).unwrap();
    assert_eq!(result["stage"], "record");
    assert!(root.path(".forge/custom-tasks.sqlite").is_file());
    assert!(!root.path(".forge/tasks.sqlite").exists());
    assert!(!selection.code_db.exists());
    client.finish().unwrap();
}

#[test]
fn server_selection_survives_transport_lifetime_and_refreshes() {
    use crate::common::{mcp_client::invalidate_mcp_connection_cache, mcp_harness::McpTestHarness};
    use forge_tasks::tasks::configuration::current;
    let plan = PlanHarness::linear();
    let root = plan.materialize(plan.path);
    root.write(
        "configs/index.yaml",
        "roots: [selected]\ntasks_db: .forge/server-tasks.sqlite\n",
    );
    root.write("selected/lib.rs", "pub fn before_refresh() {}\n");
    let selection = harness::selected_settings(
        root.root(),
        "configs/index.yaml",
        "profiles/plan.yaml",
        ".forge/server-index.sqlite",
    );
    let db = root.path(".forge/server-index.sqlite");
    let mut client =
        selection.run(|| McpTestHarness::spawn(root.root(), &db, "selected-server").unwrap());
    assert!(current(root.root()).is_none());
    let initialized = client.request_value("initialize", serde_json::json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"selection-proof","version":"1"}})).unwrap();
    assert!(initialized.get("error").is_none(), "{initialized}");
    client
        .notify("notifications/initialized", serde_json::json!({}))
        .unwrap();
    let before = client
        .payload(
            "code_map_search",
            serde_json::json!({"pattern":"before_refresh","exact":true}),
        )
        .unwrap();
    assert_eq!(before["nodes"]["total"], 1);
    root.write("selected/lib.rs", "pub fn after_refresh() {}\n");
    invalidate_mcp_connection_cache(&db).unwrap();
    let after = client
        .payload(
            "code_map_search",
            serde_json::json!({"pattern":"after_refresh","exact":true}),
        )
        .unwrap();
    assert_eq!(after["nodes"]["total"], 1);
    client.payload("task_list", serde_json::json!({})).unwrap();
    assert!(root.path(".forge/server-tasks.sqlite").is_file());
    assert!(!root.path(".forge/tasks.sqlite").exists());
    assert!(current(root.root()).is_none());
    client.finish().unwrap();
    let conn = contextunity_forge_mcp::db::reader::open(&db, root.root()).unwrap();
    assert_eq!(
        contextunity_forge_mcp::core::config::read_policy(root.root(), &conn)
            .unwrap()
            .adapter_path,
        Some(root.path("configs/index.yaml").canonicalize().unwrap())
    );
}

#[test]
fn simultaneous_servers_keep_selected_module_and_task_policies_independent() {
    use crate::common::mcp_harness::McpTestHarness;
    let plan = PlanHarness::linear();
    let root = plan.materialize(plan.path);
    root.write("configs/minimal.yaml", "modules: {profile: minimal}\n");
    let minimal = harness::selected_settings(
        root.root(),
        "configs/minimal.yaml",
        "profiles/plan.yaml",
        ".forge/minimal-index.sqlite",
    );
    let full = harness::selected_settings(
        root.root(),
        "configs/custom.yaml",
        "profiles/plan.yaml",
        ".forge/full-index.sqlite",
    );
    let parent = (
        std::env::var_os("FORGE_CONFIG"),
        std::env::var_os("FORGE_PROFILE"),
    );
    std::thread::scope(|threads| {
        let first = threads.spawn(|| {
            let mut client = minimal.run(|| {
                McpTestHarness::with_root_and_db(root.root(), &minimal.code_db, "minimal-server")
                    .unwrap()
            });
            let listed = client
                .request_value("tools/list", serde_json::json!({}))
                .unwrap();
            assert!(!listed["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "task_list"));
            let error = client
                .payload("task_list", serde_json::json!({}))
                .unwrap_err();
            assert!(error.to_string().contains("FEATURE_DISABLED"), "{error}");
            client.finish().unwrap();
        });
        let second = threads.spawn(|| {
            let mut client = full.run(|| {
                McpTestHarness::with_root_and_db(root.root(), &full.code_db, "full-server").unwrap()
            });
            let listed = client
                .request_value("tools/list", serde_json::json!({}))
                .unwrap();
            assert!(listed["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "task_list"));
            client.payload("task_list", serde_json::json!({})).unwrap();
            client.finish().unwrap();
        });
        first.join().unwrap();
        second.join().unwrap();
    });
    assert!(root.path(".forge/custom-tasks.sqlite").exists());
    assert!(!root.path(".forge/tasks.sqlite").exists());
    assert_eq!(
        (
            std::env::var_os("FORGE_CONFIG"),
            std::env::var_os("FORGE_PROFILE")
        ),
        parent
    );
}

#[test]
fn cli_query_rebuilds_existing_index_with_the_selected_policy() {
    let plan = PlanHarness::linear();
    let root = plan.materialize(plan.path);
    root.write("configs/index.yaml", "roots: [selected]\n");
    root.write("selected/lib.rs", "pub fn selected_query() {}\n");
    run_cli(root.root(), &["--db", ".forge/query-index.sqlite", "build"]).unwrap();
    run_cli(
        root.root(),
        &[
            "--db",
            ".forge/query-index.sqlite",
            "--config",
            "configs/index.yaml",
            "query",
            "overview",
        ],
    )
    .unwrap();
    let conn = contextunity_forge_mcp::db::reader::open(
        &root.path(".forge/query-index.sqlite"),
        root.root(),
    )
    .unwrap();
    let inventory = contextunity_forge_mcp::db::reader::inventory_snapshot(&conn).unwrap();
    assert!(inventory
        .iter()
        .any(|entry| entry.path == "selected/lib.rs"));
    assert!(!inventory.iter().any(|entry| entry.path == "src/first.rs"));
    assert_eq!(
        contextunity_forge_mcp::core::config::read_policy(root.root(), &conn)
            .unwrap()
            .adapter_path,
        Some(root.path("configs/index.yaml").canonicalize().unwrap())
    );
}

#[test]
fn cli_delta_selects_policy_and_preserves_legacy_persisted_selection() {
    let harness = PlanHarness::linear();
    let root = harness.materialize(harness.path);
    root.write("configs/index.yaml", "roots: [selected]\n");
    root.write("selected/lib.rs", "pub fn selected() {}\n");
    let db = root.path(".forge/selected-index.sqlite");
    run_cli(root.root(), &["--db", db.to_str().unwrap(), "build"]).unwrap();
    run_cli(
        root.root(),
        &[
            "--db",
            db.to_str().unwrap(),
            "--config",
            "configs/index.yaml",
            "delta",
            "--modified",
            "selected/lib.rs",
        ],
    )
    .unwrap();
    let conn = contextunity_forge_mcp::db::reader::open(&db, root.root()).unwrap();
    let selected = contextunity_forge_mcp::core::config::read_policy(root.root(), &conn).unwrap();
    assert_eq!(
        selected.adapter_path.as_deref(),
        Some(
            root.path("configs/index.yaml")
                .canonicalize()
                .unwrap()
                .as_path()
        )
    );
    assert_eq!(
        contextunity_forge_mcp::db::reader::inventory_snapshot(&conn)
            .unwrap()
            .iter()
            .filter(|entry| !entry.is_doc)
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        ["selected/lib.rs"]
    );
    drop(conn);
    root.write("selected/lib.rs", "pub fn selected_after_delta() {}\n");
    run_cli(
        root.root(),
        &[
            "--db",
            db.to_str().unwrap(),
            "delta",
            "--modified",
            "selected/lib.rs",
        ],
    )
    .unwrap();
    let conn = contextunity_forge_mcp::db::reader::open(&db, root.root()).unwrap();
    assert_eq!(
        contextunity_forge_mcp::core::config::read_policy(root.root(), &conn)
            .unwrap()
            .adapter_path,
        selected.adapter_path
    );
    assert!(!root.path(".forge/code-map.sqlite").exists());
}

#[test]
fn cli_missing_or_malformed_policy_never_changes_delta_generation() {
    let harness = PlanHarness::linear();
    let root = harness.materialize(harness.path);
    let db = root.path(".forge/selected-index.sqlite");
    run_cli(root.root(), &["--db", db.to_str().unwrap(), "build"]).unwrap();
    root.write("configs/broken.yaml", "roots: [\n");
    let before = fs::read(&db).unwrap();
    for config in ["configs/absent.yaml", "configs/broken.yaml"] {
        for command in ["serve", "delta"] {
            let mut args = vec!["--db", db.to_str().unwrap(), "--config", config, command];
            if command == "delta" {
                args.extend(["--modified", "src/first.rs"]);
            }
            assert!(run_cli(root.root(), &args).is_err());
            assert_eq!(fs::read(&db).unwrap(), before);
        }
    }
}

#[test]
fn milestone_cancelled_dependency_blocks_dispatch_without_completion_credit() {
    use crate::acdd::support::MilestoneFixtureBuilder;
    use forge_tasks::engine::orchestrator::waves::run_milestone_waves;
    let h = harness::WaveHarness::new(true);
    let source = MilestoneFixtureBuilder::new("m-wave")
        .task("first", &["src/lib.rs"])
        .task_status("cancelled")
        .task_with_deps("second", &["src/second.rs"], &["first"])
        .build();
    fs::write(h.root().join(harness::MANIFEST), source).unwrap();
    let error = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
    assert!(
        error.to_string().contains("TASK_DEPENDENCY_INCOMPLETE"),
        "{error:#}"
    );
    let cancelled = h.task.store.inspect(&h.task.id).unwrap();
    assert_eq!(cancelled.status, "cancelled");
    assert!(cancelled.receipt.is_none());
    assert!(!h.root().join(".forge/lifecycle/roles.jsonl").exists());
    assert!(h.root().join(harness::MANIFEST).exists());
}

#[test]
fn plan_immutable_gate_rejects_drift_without_completion_or_index_mutation() {
    let harness = PlanHarness::linear();
    let root = harness.materialize(harness.path);
    let source = fs::read(root.path(harness.path)).unwrap();
    let head = git_head(root.root());
    let index = git_command(root.root(), &["ls-files", "--stage"])
        .output()
        .unwrap()
        .stdout;
    let error = run_cli(
        root.root(),
        &[
            "--profile",
            "profiles/immutable-drift.yaml",
            "plan",
            "run",
            "linear",
        ],
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("TASK_CANDIDATE_MISMATCH"),
        "{error:#}"
    );
    assert_eq!(fs::read(root.path(harness.path)).unwrap(), source);
    assert_eq!(git_head(root.root()), head);
    assert_eq!(
        git_command(root.root(), &["ls-files", "--stage"])
            .output()
            .unwrap()
            .stdout,
        index
    );
    assert_eq!(
        fs::read_to_string(root.path("src/first.rs")).unwrap(),
        "changed\n"
    );
}

#[test]
fn explicit_missing_configuration_or_profile_fails_before_plan_execution() {
    let harness = PlanHarness::linear();
    let root = harness.materialize(harness.path);
    for flag in ["--config", "--profile"] {
        let error = run_cli(
            root.root(),
            &[flag, "missing.yaml", "plan", "run", "linear"],
        )
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("CLI_PATH_MISSING"),
            "{error:#}"
        );
        assert!(!root.path("plan-execution.log").exists());
    }
}

#[test]
fn milestone_changed_content_invalidates_root_observation_reuse() {
    use forge_tasks::engine::orchestrator::waves::run_milestone_waves;
    let h = harness::WaveHarness::new(true);
    let stop = h.root().join(".forge/stop-wave-review");
    fs::write(&stop, "").unwrap();
    assert!(run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).is_err());
    fs::write(
        h.root().join("src/lib.rs"),
        "pub fn example() { /* polish */ }\n",
    )
    .unwrap();
    assert!(git_command(h.root(), &["add", "src/lib.rs"])
        .status()
        .unwrap()
        .success());
    assert!(git_command(h.root(), &["commit", "-m", "fixture polish"])
        .status()
        .unwrap()
        .success());
    fs::remove_file(stop).unwrap();
    let result = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {});
    assert!(result.is_ok(), "{result:#?}");
    let contexts = fs::read_to_string(h.root().join(".forge/wave-roles.jsonl")).unwrap();
    let polish = contexts
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|c| c["gate"] == "polish")
        .collect::<Vec<_>>();
    assert_eq!(polish.len(), 2);
    assert_ne!(
        polish[0]["subject_context"]["binding"],
        polish[1]["subject_context"]["binding"]
    );
}

#[test]
fn milestone_handoff_refuses_a_profile_route_without_independent_review() {
    use forge_tasks::engine::orchestrator::waves::run_milestone_waves;
    let mut h = harness::WaveHarness::new(true);
    let profile_path = h.root().join(".forge/acdd/profile.yaml");
    let mut profile =
        profile::validate_profile_document(&fs::read_to_string(&profile_path).unwrap()).unwrap();
    profile.gates.retain(|gate| gate.id != "milestone_review");
    let profile_yaml = format!(
        "inherit: none\n{}",
        serde_yaml::to_string(&profile).unwrap()
    );
    fs::write(&profile_path, profile_yaml).unwrap();
    assert!(
        git_command(h.root(), &["add", "-f", ".forge/acdd/profile.yaml"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_command(h.root(), &["commit", "-m", "fixture gate profile"])
            .status()
            .unwrap()
            .success()
    );
    h.options.target_base = git_head(h.root());
    let error = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
    assert!(
        error.to_string().contains("HANDOFF_REVIEW_REQUIRED"),
        "{error:#}; task role records: {}",
        fs::read_to_string(h.root().join(".forge/lifecycle/roles.jsonl")).unwrap_or_default()
    );
    assert!(h.root().join(harness::MANIFEST).exists());
}

#[test]
fn milestone_failed_verification_cannot_archive_passing_tasks() {
    use forge_tasks::engine::orchestrator::waves::run_milestone_waves;
    let mut h = harness::WaveHarness::new(true);
    let profile_path = h.root().join(".forge/acdd/profile.yaml");
    let mut profile =
        profile::validate_profile_document(&fs::read_to_string(&profile_path).unwrap()).unwrap();
    profile.commands.insert(
        "check_full_repo".into(),
        profile::CommandDef::Script("exit 19".into()),
    );
    let profile_yaml = format!(
        "inherit: none\n{}",
        serde_yaml::to_string(&profile).unwrap()
    );
    fs::write(&profile_path, profile_yaml).unwrap();
    assert!(
        git_command(h.root(), &["add", "-f", ".forge/acdd/profile.yaml"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_command(h.root(), &["commit", "-m", "fixture gate profile"])
            .status()
            .unwrap()
            .success()
    );
    h.options.target_base = git_head(h.root());
    let error = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
    assert!(error.to_string().contains("WORKFLOW_"), "{error:#}");
    let exit: i32 = h.task.store.connection.query_row("SELECT exit_code FROM gate_execution_traces WHERE subject='milestone' ORDER BY id DESC LIMIT 1", [], |row| row.get(0)).unwrap();
    assert_eq!(exit, 19);
    assert!(h.root().join(harness::MANIFEST).exists());
    assert!(!h
        .root()
        .join("docs/milestones/archive/011-wave.md")
        .exists());
}

#[test]
fn milestone_restart_reuses_bound_root_observations_and_delivered_tasks() {
    use forge_tasks::engine::orchestrator::waves::run_milestone_waves;
    let h = harness::WaveHarness::new(true);
    let stop = h.root().join(".forge/stop-wave-review");
    fs::write(&stop, "").unwrap();
    let error = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
    assert!(
        error.to_string().contains("WORKFLOW_RUNNER_FAILED"),
        "{error:#}"
    );
    let claims: u64 = h
        .task
        .store
        .connection
        .query_row("SELECT count(*) FROM task_claims", [], |row| row.get(0))
        .unwrap();
    fs::remove_file(stop).unwrap();
    let result = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {});
    assert!(result.is_ok(), "{result:#?}");
    let contexts = fs::read_to_string(h.root().join(".forge/wave-roles.jsonl")).unwrap();
    let gates = contexts
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap()["gate"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(gates, ["polish", "milestone_review", "milestone_review"]);
    assert!(claims > 0);
    // Successful handoff prunes live claims only after accepting all existing receipts.
    assert!(h
        .root()
        .join("docs/milestones/archive/011-wave.md")
        .exists());
}

#[test]
fn milestone_waves_integrate_owned_isolated_worktrees_before_handoff() {
    use forge_tasks::engine::orchestrator::waves::run_milestone_waves;
    let h = harness::WaveHarness::new(false);
    let result = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {});
    assert!(result.is_ok(), "{result:#?}");
    assert!(h
        .root()
        .join("docs/milestones/archive/011-wave.md")
        .exists());
    assert_eq!(
        fs::read_dir(h.root().join(".worktrees")).unwrap().count(),
        0
    );
}

#[test]
fn milestone_isolation_failure_preserves_owned_worktree_and_foreign_paths() {
    use forge_tasks::engine::orchestrator::waves::run_milestone_waves;
    let h = harness::WaveHarness::new(false);
    fs::create_dir_all(h.root().join(".worktrees/foreign")).unwrap();
    fs::write(h.root().join(".worktrees/foreign/keep"), "user data").unwrap();
    fs::write(h.root().join(".forge/fail-isolated-role"), "").unwrap();
    let head = git_head(h.root());
    let error = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
    assert!(
        format!("{error:#}").contains("recoverable worktree"),
        "{error:#}"
    );
    assert_eq!(git_head(h.root()), head);
    assert_eq!(
        fs::read_to_string(h.root().join(".worktrees/foreign/keep")).unwrap(),
        "user data"
    );
    let entries = fs::read_dir(h.root().join(".worktrees"))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert!(h.root().join(harness::MANIFEST).exists());
}

#[test]
fn milestone_waves_execute_dependencies_and_content_bound_handoff_in_place() {
    use forge_tasks::engine::orchestrator::{types::OrchestratorEvent, waves::run_milestone_waves};
    let h = harness::WaveHarness::new(true);
    let mut events = Vec::new();
    let result = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |event| {
        events.push(event)
    });
    assert!(result.is_ok(), "{result:#?}");
    let starts = events
        .iter()
        .filter_map(|event| match event {
            OrchestratorEvent::WaveStarted { wave_index, .. } => Some(*wave_index),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(starts, vec![0, 1]);
    assert!(h
        .root()
        .join("docs/milestones/archive/011-wave.md")
        .exists());
    assert_eq!(
        fs::read_to_string(&h.verification_marker).unwrap(),
        "verified"
    );
    assert!(!h.root().join(".worktrees").exists());
}

#[test]
fn milestone_waves_reject_unobserved_or_stale_role_decisions() {
    use forge_tasks::engine::orchestrator::waves::run_milestone_waves;
    for (control, expected) in [
        ("no-wave-decision", "WORKFLOW_DECISION_REQUIRED"),
        ("stale-wave-review", "HANDOFF_EVIDENCE_STALE"),
    ] {
        let h = harness::WaveHarness::new(true);
        fs::write(h.root().join(".forge").join(control), "").unwrap();
        let error = run_milestone_waves(h.root(), "m-wave", &h.options, &mut |_| {}).unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{error:#}");
        assert!(h.root().join(harness::MANIFEST).exists());
        assert!(!h
            .root()
            .join("docs/milestones/archive/011-wave.md")
            .exists());
    }
}

#[test]
fn plan_delivery_reuses_subtask_completion_and_evidence_guard() {
    use crate::acdd::harness::plans::PlanFixture;
    for (fixture, expected) in [
        (
            PlanFixture::PendingSubtask,
            Some("TASK_SUBTASKS_INCOMPLETE"),
        ),
        (
            PlanFixture::MissingSubtaskEvidence,
            Some("TASK_SUBTASK_EVIDENCE_MISSING"),
        ),
        (PlanFixture::CompletedSubtask, None),
    ] {
        let harness = PlanHarness::fixture(fixture);
        let root = harness.materialize(harness.path);
        let before = fs::read(root.path(harness.path)).unwrap();
        let result = run_cli(root.root(), &["plan", "run", "invalid"]);
        if let Some(expected) = expected {
            let error = result.unwrap_err();
            assert!(error.to_string().contains(expected), "{error:#}");
            assert_eq!(fs::read(root.path(harness.path)).unwrap(), before);
        } else {
            assert_eq!(result.unwrap()["status"], "completed");
        }
    }
}

#[test]
fn global_config_selects_existing_task_store_without_using_index_db() {
    let harness = PlanHarness::linear();
    let root = harness.materialize(harness.path);
    run_cli(
        root.root(),
        &[
            "--config",
            "configs/custom.yaml",
            "--db",
            ".forge/selected-index.sqlite",
            "task",
            "list",
        ],
    )
    .unwrap();
    assert!(root.path(".forge/custom-tasks.sqlite").exists());
    assert!(!root.path(".forge/tasks.sqlite").exists());
    assert!(!root.path(".forge/selected-index.sqlite").exists());
}

#[test]
fn global_settings_survive_interleaved_future_polls_without_leaking() {
    use contextunity_forge_mcp::forge_tasks::{
        tasks::configuration::{current, InvocationSettings},
        TaskSettings,
    };
    let harness = PlanHarness::linear();
    let root = harness.materialize(harness.path);
    let profile = std::sync::Arc::new(
        profile::validate_profile_document(include_str!("../fixtures/plans/snapshot.yaml"))
            .unwrap(),
    );
    let default = InvocationSettings::new(
        root.root().to_owned(),
        Some(root.path("forge-mcp.yaml")),
        profile.clone(),
        None,
        root.path(".forge/first-index.sqlite"),
    );
    let custom = InvocationSettings::new(
        root.root().to_owned(),
        Some(root.path("configs/custom.yaml")),
        profile,
        None,
        root.path(".forge/second-index.sqlite"),
    );
    let workspace_root = root.root();
    let check = |expected: &'static str| async move {
        for _ in 0..3 {
            assert_eq!(
                TaskSettings::load(workspace_root)
                    .unwrap()
                    .tasks_db
                    .to_str(),
                Some(expected)
            );
            tokio::task::yield_now().await;
        }
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tokio::join!(
                default.clone().scope(check(".forge/tasks.sqlite")),
                custom.scope(check(".forge/custom-tasks.sqlite"))
            );
        });
    assert!(current(root.root()).is_none());
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        default.run(|| panic!("restore scoped settings"))
    }));
    assert!(current(root.root()).is_none());
    assert_eq!(
        TaskSettings::load(root.root()).unwrap().tasks_db.to_str(),
        Some(".forge/tasks.sqlite")
    );
}

#[test]
fn global_selections_reach_role_children_without_parent_environment_mutation() {
    let harness = PlanHarness::linear();
    let root = harness.materialize(harness.path);
    WorkflowHarness::agents().write_to(&root);
    let original = (
        std::env::var_os("FORGE_CONFIG"),
        std::env::var_os("FORGE_PROFILE"),
        std::env::var_os("FORGE_DB"),
    );
    run_cli(
        root.root(),
        &[
            "--config",
            "configs/custom.yaml",
            "--profile",
            "profiles/plan.yaml",
            "--db",
            ".forge/selected-index.sqlite",
            "workflow",
            "run",
            "agents",
        ],
    )
    .unwrap();
    let canonical_root = root.root().canonicalize().unwrap();
    let records = fs::read_to_string(root.path("workflow-role-sessions.jsonl")).unwrap();
    for record in records
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
    {
        assert_eq!(
            record["config"],
            canonical_root.join("configs/custom.yaml").to_str().unwrap()
        );
        assert_eq!(
            record["profile"],
            canonical_root.join("profiles/plan.yaml").to_str().unwrap()
        );
        assert_eq!(
            record["db"],
            canonical_root
                .join(".forge/selected-index.sqlite")
                .to_str()
                .unwrap()
        );
    }
    assert_eq!(
        (
            std::env::var_os("FORGE_CONFIG"),
            std::env::var_os("FORGE_PROFILE"),
            std::env::var_os("FORGE_DB")
        ),
        original
    );
}

#[test]
fn workflow_role_rejection_resumes_builder_and_preserves_independence() {
    let root = WorkflowHarness::agents().materialize();
    let result = run_cli(root.root(), &["workflow", "run", "agents"]).unwrap();
    assert_eq!(result["status"], "completed");
    let records = fs::read_to_string(root.path("workflow-role-sessions.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 4);
    assert_eq!(records[0]["gate"], "build");
    assert_eq!(records[1]["gate"], "review");
    assert_eq!(records[0]["session"], records[2]["session"]);
    assert_ne!(records[0]["session"], records[1]["session"]);
    assert_eq!(
        records[2]["findings"]["scope_findings"][0],
        "Apply the required correction"
    );
    assert!(!root.path(".forge").exists());
    assert!(!root.path(".git").exists());
}

#[test]
fn workflow_role_exit_success_without_decision_cannot_pass() {
    let root = WorkflowHarness::agents().materialize();
    fs::write(root.path("missing-decision"), "").unwrap();
    let error = run_cli(root.root(), &["workflow", "run", "agents"]).unwrap_err();
    assert!(
        error.to_string().contains("WORKFLOW_DECISION_REQUIRED"),
        "{error:#}"
    );
    assert!(!root.path(".forge").exists());
}

#[test]
fn plan_dependency_validation_precedes_execution() {
    use crate::acdd::harness::plans::PlanFixture;
    for (fixture, expected) in [
        (PlanFixture::UnknownDependency, "PLAN_DEPENDENCY_UNKNOWN"),
        (PlanFixture::Cycle, "PLAN_DEPENDENCY_CYCLE"),
        (
            PlanFixture::CancelledDependency,
            "PLAN_DEPENDENCY_INCOMPLETE",
        ),
    ] {
        let harness = PlanHarness::fixture(fixture);
        let root = harness.materialize(harness.path);
        let before = fs::read(root.path(harness.path)).unwrap();
        let error = run_cli(root.root(), &["plan", "run", "invalid", "--in-place"]).unwrap_err();
        assert!(error.to_string().contains(expected), "{error:#}");
        assert!(!root.path("plan-execution.log").exists());
        assert_eq!(fs::read(root.path(harness.path)).unwrap(), before);
    }
}

#[test]
fn plan_scope_validation_precedes_execution() {
    use crate::acdd::harness::plans::PlanFixture;
    let harness = PlanHarness::fixture(PlanFixture::UnsafeScope);
    let root = harness.materialize(harness.path);
    let error = run_cli(root.root(), &["plan", "run", "invalid"]).unwrap_err();
    assert!(
        error.to_string().contains("PLAN_SCOPE_INVALID"),
        "{error:#}"
    );
    assert!(!root.path("plan-execution.log").exists());
}

#[test]
fn plan_completed_source_prevents_command_replay() {
    let harness = PlanHarness::linear();
    let root = harness.materialize(harness.path);
    let first = run_cli(root.root(), &["plan", "run", "linear"]).unwrap();
    assert_eq!(first["snapshots"].as_array().unwrap().len(), 2);
    let source = fs::read(root.path(harness.path)).unwrap();
    let snapshots = harness.snapshots(&root);
    let second = run_cli(root.root(), &["plan", "run", "linear"]).unwrap();
    assert_eq!(second["status"], "completed");
    assert!(second["snapshots"].as_array().unwrap().is_empty());
    assert_eq!(fs::read(root.path(harness.path)).unwrap(), source);
    assert_eq!(harness.snapshots(&root), snapshots);
    assert_eq!(
        fs::read_to_string(root.path("plan-execution.log")).unwrap(),
        "step\nstep\n"
    );
}

#[test]
fn plan_explicit_commit_profile_is_rejected_before_effects() {
    let harness = PlanHarness::linear();
    let root = harness.materialize(harness.path);
    let head = git_head(root.root());
    let error = run_cli(
        root.root(),
        &["--profile", "profiles/commit.yaml", "plan", "run", "linear"],
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("PLAN_PROFILE_COMMIT_FORBIDDEN"),
        "{error:#}"
    );
    assert!(!root.path("forbidden-plan-command").exists());
    assert_eq!(git_head(root.root()), head);
}

fn run_cli(root: &std::path::Path, arguments: &[&str]) -> anyhow::Result<Value> {
    let cli = Cli::try_parse_from(
        [
            "contextunity-forge-mcp",
            "--root",
            root.to_str().unwrap(),
            "--format",
            "json",
        ]
        .into_iter()
        .chain(arguments.iter().copied()),
    )?;
    let mut output = Vec::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(cli.run_with_writer(&mut output))?;
    Ok(serde_json::from_slice(&output)?)
}

#[test]
fn workflow_failure_routes_retry_without_creating_task_state() {
    let root = WorkflowHarness::retry().materialize();
    let result = run_cli(root.root(), &["workflow", "run", "retry"]).unwrap();
    assert_eq!(result["status"], "completed");
    assert_eq!(result["outcomes"].as_array().unwrap().len(), 2);
    assert_eq!(
        fs::read_to_string(root.path("retry-result")).unwrap(),
        "retried\n"
    );
    assert!(!root.path(".forge").exists());
}

#[test]
fn workflow_failure_exhaustion_prevents_successor_execution() {
    let root = WorkflowHarness::failing().materialize();
    let error = run_cli(root.root(), &["workflow", "run", "failing"]).unwrap_err();
    assert!(
        error.to_string().contains("WORKFLOW_ATTEMPTS_EXHAUSTED"),
        "{error:#}"
    );
    assert!(!root.path("must-not-run").exists());
    assert!(!root.path(".forge").exists());
}

#[test]
fn workflow_timeout_reaps_the_owned_process_group() {
    let root = WorkflowHarness::timeout().materialize();
    let started = std::time::Instant::now();
    let error = run_cli(root.root(), &["workflow", "run", "timeout"]).unwrap_err();
    assert!(
        error.to_string().contains("WORKFLOW_ATTEMPTS_EXHAUSTED"),
        "{error:#}"
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    let pid = fs::read_to_string(root.path("timeout.pid")).unwrap();
    assert!(!std::path::Path::new("/proc").join(pid.trim()).exists());
    assert!(!root.path(".forge").exists());
}

#[test]
fn workflow_agent_gate_requires_a_structured_decision_runner() {
    let root = WorkflowHarness::rejection_routing().materialize();
    let error = run_cli(root.root(), &["workflow", "run", "rejection_routing"]).unwrap_err();
    assert!(
        error.to_string().contains("WORKFLOW_RUNNER_REQUIRED"),
        "{error:#}"
    );
    assert!(!root.path(".forge").exists());
}

#[cfg(unix)]
#[test]
fn workflow_discovery_rejects_escaping_symlinks() {
    use std::os::unix::fs::symlink;
    let root = WorkflowHarness::ordered().materialize();
    let outside = WorkflowHarness::ordered().materialize();
    symlink(
        outside.path("docs/workflows/ordered.yaml"),
        root.path("docs/workflows/escape.yaml"),
    )
    .unwrap();
    let error = run_cli(root.root(), &["workflow", "list"]).unwrap_err();
    assert!(
        error.to_string().contains("WORKFLOW_PATH_INVALID"),
        "{error:#}"
    );
    assert!(!root.path("workflow-order.log").exists());
}

#[test]
fn workflow_run_executes_root_gates_without_task_state() {
    let harness = WorkflowHarness::ordered();
    let root = harness.materialize();
    let parsed = Cli::try_parse_from([
        "contextunity-forge-mcp",
        "--root",
        root.root().to_str().unwrap(),
        "--format",
        "json",
        "workflow",
        "run",
        "ordered",
    ]);
    assert!(
        parsed.is_ok(),
        "assertion failed: workflow run must enter production CLI dispatch: {:?}",
        parsed.as_ref().err()
    );
    let mut output = Vec::new();
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(Cli::run_with_writer(parsed.unwrap(), &mut output));
    assert!(result.is_ok(), "taskless root execution: {result:?}");
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["status"], "completed");
    assert_eq!(
        fs::read_to_string(root.path("workflow-order.log")).unwrap(),
        "first\nsecond\n"
    );
    assert!(!root.path(".git").exists());
    assert!(
        !root.path(".forge").exists(),
        "save:none must remain ephemeral"
    );
    assert_eq!(
        fs::read_to_string(root.path("docs/workflows/ordered.yaml")).unwrap(),
        include_str!("../fixtures/workflows/ordered.yaml")
    );
}

#[test]
fn workflow_discovery_does_not_execute_gates() {
    let harness = WorkflowHarness::ordered();
    let root = harness.materialize();
    for action in [
        vec!["workflow", "list"],
        vec!["workflow", "show", "ordered"],
    ] {
        let parsed = Cli::try_parse_from(
            [
                "contextunity-forge-mcp",
                "--root",
                root.root().to_str().unwrap(),
                "--format",
                "json",
            ]
            .into_iter()
            .chain(action),
        );
        assert!(
            parsed.is_ok(),
            "workflow discovery dispatch: {:?}",
            parsed.as_ref().err()
        );
        let mut output = Vec::new();
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(Cli::run_with_writer(parsed.unwrap(), &mut output));
        assert!(result.is_ok(), "workflow discovery: {result:?}");
        let value: Value = serde_json::from_slice(&output).unwrap();
        assert!(value.to_string().contains("ordered"));
        assert!(!root.path("workflow-order.log").exists());
        assert!(!root.path(".forge").exists());
        assert!(!root.path(".git").exists());
    }
}

#[test]
fn plan_run_executes_steps_in_place_without_committing() {
    let harness = PlanHarness::linear();
    let root = harness.materialize(harness.path);
    let head = git_head(root.root());
    let snapshots = harness.snapshots(&root);
    let plan_profile =
        profile::validate_profile_document(include_str!("../fixtures/plans/snapshot.yaml"));
    assert!(
        plan_profile.is_ok(),
        "snapshot-only plan profile: {plan_profile:?}"
    );
    let parsed = Cli::try_parse_from([
        "contextunity-forge-mcp",
        "--root",
        root.root().to_str().unwrap(),
        "--format",
        "json",
        "plan",
        "run",
        "linear",
        "--in-place",
    ]);
    assert!(
        parsed.is_ok(),
        "plan run dispatch: {:?}",
        parsed.as_ref().err()
    );
    let mut output = Vec::new();
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(Cli::run_with_writer(parsed.unwrap(), &mut output));
    assert!(result.is_ok(), "in-place plan execution: {result:?}");
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["status"], "completed");
    assert_eq!(
        fs::read_to_string(root.path("plan-execution.log")).unwrap(),
        "step\nstep\n"
    );
    assert_eq!(
        git_head(root.root()),
        head,
        "plan snapshots must not commit HEAD"
    );
    assert!(
        harness
            .snapshots(&root)
            .iter()
            .any(|snapshot| snapshot.starts_with("commit ") && !snapshots.contains(snapshot)),
        "plan execution must retain a new Git snapshot commit"
    );
    assert!(!root.path(".worktrees").exists());
    assert!(!root.path("rejected-profile.log").exists());
}

#[test]
fn plan_discovery_preserves_canonical_step_dependencies() {
    let harness = PlanHarness::dag();
    let root = harness.materialize(harness.path);
    let head = git_head(root.root());
    let source = fs::read(root.path(harness.path)).unwrap();
    for action in [vec!["plan", "list"], vec!["plan", "show", "dag"]] {
        let show = action[1] == "show";
        let parsed = Cli::try_parse_from(
            [
                "contextunity-forge-mcp",
                "--root",
                root.root().to_str().unwrap(),
                "--format",
                "json",
            ]
            .into_iter()
            .chain(action),
        );
        assert!(
            parsed.is_ok(),
            "plan discovery dispatch: {:?}",
            parsed.as_ref().err()
        );
        let mut output = Vec::new();
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(Cli::run_with_writer(parsed.unwrap(), &mut output));
        assert!(result.is_ok(), "plan discovery: {result:?}");
        let value: Value = serde_json::from_slice(&output).unwrap();
        assert!(value.to_string().contains("dag"));
        if show {
            let steps = value["steps"]
                .as_array()
                .expect("plan show exposes structured steps");
            for title in ["Step B: Extension Beta", "Step C: Extension Gamma"] {
                let step = steps
                    .iter()
                    .find(|step| {
                        step["title"]
                            .as_str()
                            .is_some_and(|name| name.contains(title))
                    })
                    .expect("canonical dependent step");
                assert!(
                    step["depends_on"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|dependency| dependency
                            .as_str()
                            .is_some_and(|name| name.contains("Step A: Independent Core"))),
                    "{title} must retain its dependency"
                );
            }
        }
        assert_eq!(fs::read(root.path(harness.path)).unwrap(), source);
        assert_eq!(git_head(root.root()), head);
        assert!(!root.path("plan-execution.log").exists());
    }
}

#[test]
fn plan_run_honors_explicit_config_and_profile_paths() {
    let harness = PlanHarness::linear();
    let root = harness.materialize("custom-plans/linear.md");
    let head = git_head(root.root());
    let snapshots = harness.snapshots(&root);
    let parsed = Cli::try_parse_from([
        "contextunity-forge-mcp",
        "--root",
        root.root().to_str().unwrap(),
        "--db",
        ".forge/selected-index.sqlite",
        "--config",
        "configs/custom.yaml",
        "--profile",
        "profiles/plan.yaml",
        "--format",
        "json",
        "plan",
        "run",
        "linear",
        "--in-place",
    ]);
    assert!(
        parsed.is_ok(),
        "global path dispatch: {:?}",
        parsed.as_ref().err()
    );
    let mut output = Vec::new();
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(Cli::run_with_writer(parsed.unwrap(), &mut output));
    assert!(
        result.is_ok(),
        "explicit config/profile execution: {result:?}"
    );
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["status"], "completed");
    assert_eq!(value["path"], "custom-plans/linear.md");
    assert_eq!(
        fs::read_to_string(root.path("plan-execution.log")).unwrap(),
        "step\nstep\n"
    );
    assert!(
        !root.path("rejected-profile.log").exists(),
        "explicit profile must override selected config"
    );
    assert!(!root.path(".forge/tasks.sqlite").exists());
    assert!(!root.path(".forge/custom-tasks.sqlite").exists());
    assert_eq!(git_head(root.root()), head);
    assert!(harness
        .snapshots(&root)
        .iter()
        .any(|snapshot| snapshot.starts_with("commit ") && !snapshots.contains(snapshot)));
}

#[test]
fn milestone_root_gate_claim_and_submit_enforce_barriers_independence_and_fanout() {
    use contextunity_forge_mcp::forge_tasks::engine::{
        claim, manage, submit, Action, Claim, Manage, ManageAction, Submit,
    };

    let mut h = harness::WaveHarness::with_single_task(true);
    h.with_blueprint_synthesis_root_gates();

    let worktree = h.root().to_string_lossy().into_owned();
    let revisions_before_next: i64 = h
        .task
        .store
        .connection
        .query_row("SELECT count(*) FROM task_revisions", [], |row| row.get(0))
        .unwrap();
    let transitions_before_next: i64 = h
        .task
        .store
        .connection
        .query_row("SELECT count(*) FROM gate_transitions", [], |row| {
            row.get(0)
        })
        .unwrap();
    let next_action = manage(
        h.root(),
        Manage {
            action: ManageAction::Next,
            worker_id: Some("architect-1".into()),
            worktree: Some(worktree.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(next_action["subject"], "milestone");
    assert_eq!(next_action["stage"], "blueprint_synthesis.synthesize");
    let revisions_after_next: i64 = h
        .task
        .store
        .connection
        .query_row("SELECT count(*) FROM task_revisions", [], |row| row.get(0))
        .unwrap();
    let transitions_after_next: i64 = h
        .task
        .store
        .connection
        .query_row("SELECT count(*) FROM gate_transitions", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        revisions_after_next, revisions_before_next,
        "pre-task next dispatch must not enroll task revisions"
    );
    assert_eq!(
        transitions_after_next, transitions_before_next,
        "pre-task next dispatch must not mutate gate_transitions"
    );

    let pre_claim = claim(
        h.root(),
        Claim {
            task_id: "m-wave".into(),
            stage: String::new(),
            worker_id: "architect-1".into(),
            worktree: worktree.clone(),
            bundle: Some(true),
            session_fingerprint: None,
        },
    )
    .unwrap();
    assert_eq!(pre_claim["subject"], "milestone");
    assert_eq!(pre_claim["stage"], "blueprint_synthesis.synthesize");
    let milestone_id = pre_claim["milestone_id"].as_str().unwrap().to_owned();

    // Post-task gate cannot be claimed while pre-task barrier or tasks are incomplete.
    let barrier_err = claim(
        h.root(),
        Claim {
            task_id: milestone_id.clone(),
            stage: "polish".into(),
            worker_id: "architect-1".into(),
            worktree: worktree.clone(),
            bundle: None,
            session_fingerprint: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        barrier_err.contains("MILESTONE_PRE_TASK_GATE_PENDING"),
        "{barrier_err}"
    );

    let task_ref = h.task.store.inspect(&h.task.id).unwrap().spec.task_ref;
    submit(
        h.root(),
        Submit {
            task_id: milestone_id.clone(),
            stage: "blueprint_synthesis.synthesize".into(),
            evidence: serde_json::json!({
                "worker_id": "architect-1",
                "worktree": worktree,
                "proof": {
                    "scheme_proof": {
                        "tasks": {
                            task_ref: {
                                "goal": "root-fanout-ready"
                            }
                        }
                    }
                }
            }),
            action: Action::Pass,
            findings: None,
            session_fingerprint: None,
        },
    )
    .unwrap();

    // Independent reviewer cannot reuse architect-1 on either claim or submit.
    let claim_not_independent = claim(
        h.root(),
        Claim {
            task_id: milestone_id.clone(),
            stage: "blueprint_synthesis.blueprint_review".into(),
            worker_id: "architect-1".into(),
            worktree: worktree.clone(),
            bundle: None,
            session_fingerprint: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        claim_not_independent.contains("TASK_REVIEW_NOT_INDEPENDENT"),
        "{claim_not_independent}"
    );

    let not_independent = submit(
        h.root(),
        Submit {
            task_id: milestone_id.clone(),
            stage: "blueprint_synthesis.blueprint_review".into(),
            evidence: serde_json::json!({
                "worker_id": "architect-1",
                "worktree": worktree,
                "proof": {"none_proof": {}}
            }),
            action: Action::Pass,
            findings: None,
            session_fingerprint: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        not_independent.contains("TASK_REVIEW_NOT_INDEPENDENT"),
        "{not_independent}"
    );

    // Passing final pre-task gate eagerly fans out task slices before any context call.
    submit(
        h.root(),
        Submit {
            task_id: milestone_id.clone(),
            stage: "blueprint_synthesis.blueprint_review".into(),
            evidence: serde_json::json!({
                "worker_id": "reviewer-2",
                "worktree": worktree,
                "proof": {"none_proof": {}}
            }),
            action: Action::Pass,
            findings: None,
            session_fingerprint: None,
        },
    )
    .unwrap();

    let fanned_out_rows: i64 = h
        .task
        .store
        .connection
        .query_row(
            "SELECT count(*) FROM gate_transitions WHERE milestone_id=?1 AND subject=?2 AND action='advance'",
            rusqlite::params![milestone_id, h.task.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(fanned_out_rows, 1);

    let total_before_context: i64 = h
        .task
        .store
        .connection
        .query_row("SELECT count(*) FROM gate_transitions", [], |row| {
            row.get(0)
        })
        .unwrap();
    let ctx = manage(
        h.root(),
        Manage {
            action: ManageAction::Context,
            task_id: Some(h.task.id.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let total_after_context: i64 = h
        .task
        .store
        .connection
        .query_row("SELECT count(*) FROM gate_transitions", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(total_before_context, total_after_context);
    assert_eq!(
        ctx["context_bundle"]["inherited_proofs"][0]["proof"]["goal"],
        "root-fanout-ready"
    );

    // Barrier 2 rejects post-task root claim until wave task completes, then allows it.
    let barrier2_err = claim(
        h.root(),
        Claim {
            task_id: milestone_id.clone(),
            stage: "polish".into(),
            worker_id: "architect-1".into(),
            worktree: worktree.clone(),
            bundle: None,
            session_fingerprint: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        barrier2_err.contains("MILESTONE_POST_TASK_BARRIER_OPEN"),
        "{barrier2_err}"
    );

    h.task.advance_to_completion().unwrap();
    let post_claim = claim(
        h.root(),
        Claim {
            task_id: milestone_id.clone(),
            stage: "polish".into(),
            worker_id: "architect-1".into(),
            worktree: worktree.clone(),
            bundle: None,
            session_fingerprint: None,
        },
    )
    .unwrap();
    assert_eq!(post_claim["stage"], "polish");
    let post_submit = submit(
        h.root(),
        Submit {
            task_id: milestone_id,
            stage: "polish".into(),
            evidence: serde_json::json!({
                "worker_id": "architect-1",
                "worktree": worktree,
                "proof": {"none_proof": {}}
            }),
            action: Action::Pass,
            findings: None,
            session_fingerprint: None,
        },
    )
    .unwrap();
    assert_eq!(post_submit["action"], "pass");
}

#[test]
fn root_command_verify_blocks_unobserved_pass_and_bypassed_polish_allows_independent_milestone_review(
) {
    use crate::acdd::support::passing_review_proof;
    use contextunity_forge_mcp::forge_tasks::engine::{
        claim, manage, submit, Action, Claim, Manage, ManageAction, Submit,
    };

    let mut h = harness::WaveHarness::with_single_task(true);
    let mut profile = h.read_profile();
    profile.commands.insert(
        "root-verify-pass".into(),
        serde_json::from_value(serde_json::json!({
            "command": "true",
            "output": { "require_counts": false }
        }))
        .unwrap(),
    );
    let task_container = profile
        .gates
        .iter()
        .find(|gate| gate.tasks.is_some())
        .unwrap()
        .clone();
    let verify: profile::GateDef = serde_json::from_value(serde_json::json!({
        "id": "verify",
        "command": "root-verify-pass",
        "proof": "command",
        "pass": "milestone_review",
        "fail": "polish"
    }))
    .unwrap();
    let polish: profile::GateDef = serde_json::from_value(serde_json::json!({
        "id": "polish",
        "role": "wave_role",
        "proof": "none",
        "pass": "milestone_review"
    }))
    .unwrap();
    let milestone_review: profile::GateDef = serde_json::from_value(serde_json::json!({
        "id": "milestone_review",
        "role": "wave_role",
        "proof": "review",
        "independent_from": "polish"
    }))
    .unwrap();
    let handoff = profile
        .gates
        .iter()
        .find(|gate| gate.id == "handoff")
        .unwrap()
        .clone();
    profile.gates = vec![task_container, verify, polish, milestone_review, handoff];
    h.commit_profile(&profile, "verified review root gate profile");
    h.task.advance_to_completion().unwrap();
    let milestone_id = "m-wave".to_owned();
    let worktree = h.root().to_string_lossy().into_owned();

    let verify_claim = claim(
        h.root(),
        Claim {
            task_id: milestone_id.clone(),
            stage: "verify".into(),
            worker_id: "architect-1".into(),
            worktree: worktree.clone(),
            bundle: None,
            session_fingerprint: None,
        },
    )
    .unwrap();
    assert_eq!(verify_claim["stage"], "verify");

    let unobserved_err = submit(
        h.root(),
        Submit {
            task_id: milestone_id.clone(),
            stage: "verify".into(),
            evidence: serde_json::json!({
                "worker_id": "architect-1",
                "worktree": worktree,
                "proof": {
                    "command_proof": {
                        "command": "root-verify-pass",
                        "exit_code": 0
                    }
                }
            }),
            action: Action::Pass,
            findings: None,
            session_fingerprint: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        unobserved_err.contains("TASK_PROOF_UNOBSERVED"),
        "{unobserved_err}"
    );

    let verified = manage(
        h.root(),
        Manage {
            action: ManageAction::Verify,
            task_id: Some(milestone_id.clone()),
            worktree: Some(worktree.clone()),
            worker_id: Some("architect-1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(verified["passed"], true);

    let verify_submit = submit(
        h.root(),
        Submit {
            task_id: milestone_id.clone(),
            stage: "verify".into(),
            evidence: serde_json::json!({
                "worker_id": "architect-1",
                "worktree": worktree,
                "proof": verified["proof"].clone()
            }),
            action: Action::Pass,
            findings: None,
            session_fingerprint: None,
        },
    )
    .unwrap();
    assert_eq!(verify_submit["action"], "pass");

    // Passing `verify` routes directly to `milestone_review`, bypassing `polish`,
    // so `independent_from: polish` must still allow claiming `milestone_review`.
    let review_claim = claim(
        h.root(),
        Claim {
            task_id: milestone_id.clone(),
            stage: "milestone_review".into(),
            worker_id: "architect-1".into(),
            worktree: worktree.clone(),
            bundle: None,
            session_fingerprint: None,
        },
    )
    .unwrap();
    assert_eq!(review_claim["stage"], "milestone_review");

    let review_submit = submit(
        h.root(),
        Submit {
            task_id: milestone_id,
            stage: "milestone_review".into(),
            evidence: serde_json::json!({
                "worker_id": "architect-1",
                "worktree": worktree,
                "proof": passing_review_proof()
            }),
            action: Action::Pass,
            findings: None,
            session_fingerprint: None,
        },
    )
    .unwrap();
    assert_eq!(review_submit["action"], "pass");
}

#[test]
fn root_immutable_gate_rejects_modified_candidate_after_claim() {
    use contextunity_forge_mcp::forge_tasks::engine::{claim, submit, Action, Claim, Submit};

    let mut h = harness::WaveHarness::with_single_task(true);
    let mut profile = h.read_profile();
    let task_container = profile
        .gates
        .iter()
        .find(|gate| gate.tasks.is_some())
        .unwrap()
        .clone();
    let preparation: profile::GateDef = serde_json::from_value(serde_json::json!({
        "id": "preparation",
        "role": "wave_role",
        "proof": "none",
        "save": { "pre": "snapshot", "post": "snapshot" }
    }))
    .unwrap();
    let preparation_review: profile::GateDef = serde_json::from_value(serde_json::json!({
        "id": "preparation_review",
        "role": "wave_role",
        "proof": "none",
        "independent_from": "preparation",
        "save": { "immutable": true }
    }))
    .unwrap();
    let handoff = profile
        .gates
        .iter()
        .find(|gate| gate.id == "handoff")
        .unwrap()
        .clone();
    profile.gates = vec![preparation, preparation_review, task_container, handoff];
    h.commit_profile(&profile, "immutable root gate profile");

    let worktree = h.root().to_string_lossy().into_owned();
    let milestone_id = "m-wave".to_owned();
    claim(
        h.root(),
        Claim {
            task_id: milestone_id.clone(),
            stage: "preparation".into(),
            worker_id: "worker-a".into(),
            worktree: worktree.clone(),
            bundle: None,
            session_fingerprint: None,
        },
    )
    .unwrap();
    submit(
        h.root(),
        Submit {
            task_id: milestone_id.clone(),
            stage: "preparation".into(),
            evidence: serde_json::json!({
                "worker_id": "worker-a",
                "worktree": worktree,
                "proof": { "none_proof": {} }
            }),
            action: Action::Pass,
            findings: None,
            session_fingerprint: None,
        },
    )
    .unwrap();

    claim(
        h.root(),
        Claim {
            task_id: milestone_id.clone(),
            stage: "preparation_review".into(),
            worker_id: "reviewer-b".into(),
            worktree: worktree.clone(),
            bundle: None,
            session_fingerprint: None,
        },
    )
    .unwrap();

    fs::write(
        h.root().join("src/lib.rs"),
        "pub fn modified() -> bool { false }\n",
    )
    .unwrap();

    let err = submit(
        h.root(),
        Submit {
            task_id: milestone_id,
            stage: "preparation_review".into(),
            evidence: serde_json::json!({
                "worker_id": "reviewer-b",
                "worktree": worktree,
                "proof": { "none_proof": {} }
            }),
            action: Action::Pass,
            findings: None,
            session_fingerprint: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("TASK_CANDIDATE_MISMATCH"), "{err}");
}

#[test]
fn claim_response_overflow_releases_claim_and_keeps_task_ready() {
    use contextunity_forge_mcp::forge_tasks::engine::{claim, submit, Action, Claim, Submit};

    let mut h = harness::WaveHarness::with_single_task(true);
    h.with_blueprint_synthesis_root_gates();
    let worktree = h.root().to_string_lossy().into_owned();
    let milestone_id = "m-wave".to_owned();

    claim(
        h.root(),
        Claim {
            task_id: milestone_id.clone(),
            stage: "blueprint_synthesis.synthesize".into(),
            worker_id: "architect-1".into(),
            worktree: worktree.clone(),
            bundle: None,
            session_fingerprint: None,
        },
    )
    .unwrap();
    submit(
        h.root(),
        Submit {
            task_id: milestone_id.clone(),
            stage: "blueprint_synthesis.synthesize".into(),
            evidence: serde_json::json!({
                "worker_id": "architect-1",
                "worktree": worktree,
                "proof": {
                    "scheme_proof": {
                        "tasks": {
                            "first": { "padding": "A".repeat(2048) }
                        }
                    }
                }
            }),
            action: Action::Pass,
            findings: None,
            session_fingerprint: None,
        },
    )
    .unwrap();
    claim(
        h.root(),
        Claim {
            task_id: milestone_id.clone(),
            stage: "blueprint_synthesis.blueprint_review".into(),
            worker_id: "reviewer-1".into(),
            worktree: worktree.clone(),
            bundle: None,
            session_fingerprint: None,
        },
    )
    .unwrap();
    submit(
        h.root(),
        Submit {
            task_id: milestone_id,
            stage: "blueprint_synthesis.blueprint_review".into(),
            evidence: serde_json::json!({
                "worker_id": "reviewer-1",
                "worktree": worktree,
                "proof": { "none_proof": {} }
            }),
            action: Action::Pass,
            findings: None,
            session_fingerprint: None,
        },
    )
    .unwrap();

    // Lower configured response budget below the inherited proof size to force TASK_RESPONSE_OVERFLOW.
    let cfg_path = h.root().join("forge-mcp.yaml");
    let mut cfg = fs::read_to_string(&cfg_path).unwrap();
    cfg.push_str("\nresponse:\n  max_output_bytes: 1024\n");
    fs::write(&cfg_path, cfg).unwrap();

    let first_err = claim(
        h.root(),
        Claim {
            task_id: h.task.id.clone(),
            stage: "contract".into(),
            worker_id: "worker-1".into(),
            worktree: worktree.clone(),
            bundle: Some(true),
            session_fingerprint: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(first_err.contains("TASK_RESPONSE_OVERFLOW"), "{first_err}");

    let task = h.task.store.inspect(&h.task.id).unwrap();
    assert_eq!(task.status, "ready");
    assert!(task.worker_id.is_none());

    let second_err = claim(
        h.root(),
        Claim {
            task_id: h.task.id.clone(),
            stage: "contract".into(),
            worker_id: "worker-2".into(),
            worktree,
            bundle: Some(true),
            session_fingerprint: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        second_err.contains("TASK_RESPONSE_OVERFLOW"),
        "{second_err}"
    );
}

#[test]
fn root_immutable_gate_enforces_prior_snapshot_across_intervening_ordinary_pass() {
    use contextunity_forge_mcp::forge_tasks::engine::{claim, submit, Action, Claim, Submit};

    let mut h = harness::WaveHarness::with_single_task(true);
    let mut profile = h.read_profile();
    let task_container = profile
        .gates
        .iter()
        .find(|gate| gate.tasks.is_some())
        .unwrap()
        .clone();
    let preparation: profile::GateDef = serde_json::from_value(serde_json::json!({
        "id": "preparation",
        "role": "wave_role",
        "proof": "none",
        "save": { "pre": "snapshot", "post": "snapshot" }
    }))
    .unwrap();
    let ordinary_gate: profile::GateDef = serde_json::from_value(serde_json::json!({
        "id": "ordinary_gate",
        "role": "wave_role",
        "proof": "none"
    }))
    .unwrap();
    let preparation_review: profile::GateDef = serde_json::from_value(serde_json::json!({
        "id": "preparation_review",
        "role": "wave_role",
        "proof": "none",
        "independent_from": "preparation",
        "save": { "immutable": true }
    }))
    .unwrap();
    let handoff = profile
        .gates
        .iter()
        .find(|gate| gate.id == "handoff")
        .unwrap()
        .clone();
    profile.gates = vec![
        preparation,
        ordinary_gate,
        preparation_review,
        task_container,
        handoff,
    ];
    h.commit_profile(&profile, "intervening pass before immutable root gate");

    let worktree = h.root().to_string_lossy().into_owned();
    let milestone_id = "m-wave".to_owned();
    for (stage, worker) in [("preparation", "worker-a"), ("ordinary_gate", "worker-b")] {
        claim(
            h.root(),
            Claim {
                task_id: milestone_id.clone(),
                stage: stage.into(),
                worker_id: worker.into(),
                worktree: worktree.clone(),
                bundle: None,
                session_fingerprint: None,
            },
        )
        .unwrap();
        submit(
            h.root(),
            Submit {
                task_id: milestone_id.clone(),
                stage: stage.into(),
                evidence: serde_json::json!({
                    "worker_id": worker,
                    "worktree": worktree,
                    "proof": { "none_proof": {} }
                }),
                action: Action::Pass,
                findings: None,
                session_fingerprint: None,
            },
        )
        .unwrap();
    }

    fs::write(
        h.root().join("src/lib.rs"),
        "pub fn altered() -> bool { false }\n",
    )
    .unwrap();

    let err = claim(
        h.root(),
        Claim {
            task_id: milestone_id,
            stage: "preparation_review".into(),
            worker_id: "reviewer-c".into(),
            worktree,
            bundle: None,
            session_fingerprint: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("TASK_CANDIDATE_MISMATCH"), "{err}");
}

#[test]
fn task_pinned_inherit_none_profile_cannot_bypass_milestone_root_barriers() {
    use contextunity_forge_mcp::forge_tasks::engine::{
        claim, manage, submit, Action, Claim, Manage, ManageAction, Submit,
    };
    use sha2::Digest;

    let h = harness::WaveHarness::with_single_task(true);
    let milestone_yaml = "\
inherit: none
version: 2
roles:
  actor: {mode: subagent}
gates:
  - id: preparation
    role: actor
    proof:
      scheme:
        type: object
        required: [tasks]
        properties:
          tasks:
            type: object
  - tasks:
      gates:
        - id: build
          role: actor
          proof: none
        - id: deliver
          role: actor
          proof: delivery
          receipt: true
          auto_commit: false
  - id: handoff
    receipt: true
    proof: delivery
";
    let task_yaml = "\
inherit: none
version: 2
roles:
  actor: {mode: subagent}
gates:
  - tasks:
      gates:
        - id: build
          role: actor
          proof: none
        - id: deliver
          role: actor
          proof: delivery
          receipt: true
          auto_commit: false
";
    let pin = |path: &str, yaml: &str| {
        let digest = format!("{:x}", sha2::Sha256::digest(yaml.as_bytes()));
        format!("{path}:{}", &digest[..12])
    };
    let milestone_pin = pin("profiles/milestone.yaml", milestone_yaml);
    let task_pin = pin("profiles/task.yaml", task_yaml);
    fs::create_dir_all(h.root().join("profiles")).unwrap();
    fs::write(h.root().join("profiles/milestone.yaml"), milestone_yaml).unwrap();
    fs::write(h.root().join("profiles/task.yaml"), task_yaml).unwrap();

    let manifest_rel = "docs/milestones/001-pinned-barrier.md";
    let spec = format!(
        "---\nid: m-pinned-barrier\ntitle: Pinned Barrier\ndoc_type: contract\nstatus: active\nacdd_profile: \"{milestone_pin}\"\n---\n# Pinned Barrier\n```yaml\ntask_ref: t1\ntarget: Task with inherit none pin\nacdd_profile: \"{task_pin}\"\nproof_policy: seam-test-first\nscope: [src/]\n```\n"
    );
    fs::write(h.root().join(manifest_rel), spec).unwrap();
    fs::remove_file(h.root().join(harness::MANIFEST)).unwrap();
    fs::remove_file(h.root().join(".forge/tasks.sqlite")).unwrap();
    let cfg = fs::read_to_string(h.root().join("forge-mcp.yaml")).unwrap();
    let cleaned_cfg = cfg
        .lines()
        .filter(|line| !line.starts_with("acdd_profile:"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(h.root().join("forge-mcp.yaml"), format!("{cleaned_cfg}\n")).unwrap();
    for args in [
        vec!["rm", harness::MANIFEST],
        vec!["add", "forge-mcp.yaml", "profiles", manifest_rel],
        vec!["commit", "-m", "pinned milestone and task profiles"],
    ] {
        assert!(git_command(h.root(), &args).status().unwrap().success());
    }

    let synced = manage(
        h.root(),
        Manage {
            action: ManageAction::Sync,
            milestone_ref: Some(manifest_rel.into()),
            ..Default::default()
        },
    )
    .unwrap();
    let task_id = synced["tasks"][0]["task_id"].as_str().unwrap().to_owned();
    let worktree = h.root().to_string_lossy().into_owned();

    let next_before = manage(
        h.root(),
        Manage {
            action: ManageAction::Next,
            worker_id: Some("coord-1".into()),
            worktree: Some(worktree.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(next_before["subject"], "milestone");
    assert_eq!(next_before["stage"], "preparation");

    let early_claim_err = claim(
        h.root(),
        Claim {
            task_id: task_id.clone(),
            stage: "build".into(),
            worker_id: "worker-1".into(),
            worktree: worktree.clone(),
            bundle: None,
            session_fingerprint: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        early_claim_err.contains("MILESTONE_PRE_TASK_GATE_PENDING"),
        "{early_claim_err}"
    );

    claim(
        h.root(),
        Claim {
            task_id: "m-pinned-barrier".into(),
            stage: "preparation".into(),
            worker_id: "coord-1".into(),
            worktree: worktree.clone(),
            bundle: None,
            session_fingerprint: None,
        },
    )
    .unwrap();
    submit(
        h.root(),
        Submit {
            task_id: "m-pinned-barrier".into(),
            stage: "preparation".into(),
            evidence: serde_json::json!({
                "worker_id": "coord-1",
                "worktree": worktree,
                "proof": {
                    "scheme_proof": {
                        "tasks": {
                            "t1": { "summary": "pinned blueprint slice" }
                        }
                    }
                }
            }),
            action: Action::Pass,
            findings: None,
            session_fingerprint: None,
        },
    )
    .unwrap();

    let allowed = claim(
        h.root(),
        Claim {
            task_id,
            stage: "build".into(),
            worker_id: "worker-1".into(),
            worktree,
            bundle: Some(true),
            session_fingerprint: None,
        },
    )
    .unwrap();
    assert_eq!(allowed["stage"], "build");
    let inherited = allowed["context_bundle"]["inherited_proofs"]
        .as_array()
        .unwrap();
    assert_eq!(inherited.len(), 1, "{allowed:?}");
    assert_eq!(inherited[0]["gate_path"], "preparation");
    assert_eq!(inherited[0]["proof"]["summary"], "pinned blueprint slice");
}

#[test]
fn post_task_immutable_review_ignores_pre_task_snapshot_and_cli_verifies_root_gate() {
    use contextunity_forge_mcp::forge_tasks::engine::{claim, submit, Action, Claim, Submit};

    let mut h = harness::WaveHarness::with_single_task(true);
    let mut profile = h.read_profile();
    let task_container = profile
        .gates
        .iter()
        .find(|gate| gate.tasks.is_some())
        .unwrap()
        .clone();
    let blueprint: profile::GateDef = serde_json::from_value(serde_json::json!({
        "id": "blueprint",
        "role": "wave_role",
        "proof": "none",
        "save": { "pre": "snapshot", "post": "snapshot" }
    }))
    .unwrap();
    let mut verify = profile.find_gate("verify").unwrap().clone();
    verify.on_success = Some("milestone_review".into());
    verify.on_fail = None;
    let mut milestone_review = profile.find_gate("milestone_review").unwrap().clone();
    milestone_review.independent_from = None;
    milestone_review.on_fail = None;
    milestone_review.persistence.immutable = true;
    let handoff = profile.find_gate("handoff").unwrap().clone();
    profile.gates = vec![blueprint, task_container, verify, milestone_review, handoff];
    h.commit_profile(&profile, "blueprint snapshot to post-task review profile");

    let worktree = h.root().to_string_lossy().into_owned();
    let milestone_id = "m-wave".to_owned();
    claim(
        h.root(),
        Claim {
            task_id: milestone_id.clone(),
            stage: "blueprint".into(),
            worker_id: "architect-1".into(),
            worktree: worktree.clone(),
            bundle: None,
            session_fingerprint: None,
        },
    )
    .unwrap();
    submit(
        h.root(),
        Submit {
            task_id: milestone_id.clone(),
            stage: "blueprint".into(),
            evidence: serde_json::json!({
                "worker_id": "architect-1",
                "worktree": worktree,
                "proof": { "none_proof": {} }
            }),
            action: Action::Pass,
            findings: None,
            session_fingerprint: None,
        },
    )
    .unwrap();

    h.task.advance_to_completion().unwrap();

    let verified = run_cli(h.root(), &["task", "verify", "m-wave"]).unwrap();
    assert!(
        verified["milestone"].as_str().unwrap().ends_with("/m-wave"),
        "{verified:?}"
    );
    assert_eq!(verified["stage"], "verify");
    assert_eq!(verified["passed"], true);

    claim(
        h.root(),
        Claim {
            task_id: milestone_id.clone(),
            stage: "verify".into(),
            worker_id: "coord-2".into(),
            worktree: worktree.clone(),
            bundle: None,
            session_fingerprint: None,
        },
    )
    .unwrap();
    submit(
        h.root(),
        Submit {
            task_id: milestone_id.clone(),
            stage: "verify".into(),
            evidence: serde_json::json!({
                "worker_id": "coord-2",
                "worktree": worktree,
                "proof": {
                    "command_proof": {
                        "command": "check_full_repo",
                        "exit_code": 0,
                        "tests_passed": 1,
                        "tests_failed": 0
                    }
                }
            }),
            action: Action::Pass,
            findings: None,
            session_fingerprint: None,
        },
    )
    .unwrap();

    claim(
        h.root(),
        Claim {
            task_id: milestone_id.clone(),
            stage: "milestone_review".into(),
            worker_id: "reviewer-9".into(),
            worktree: worktree.clone(),
            bundle: None,
            session_fingerprint: None,
        },
    )
    .unwrap();
    let review_submit = submit(
        h.root(),
        Submit {
            task_id: milestone_id,
            stage: "milestone_review".into(),
            evidence: serde_json::json!({
                "worker_id": "reviewer-9",
                "worktree": worktree,
                "proof": {
                    "scheme_proof": {
                        "decision": "pass",
                        "summary": "verified post-task candidate without pre-task snapshot collision"
                    }
                }
            }),
            action: Action::Pass,
            findings: None,
            session_fingerprint: None,
        },
    )
    .unwrap();
    assert_eq!(review_submit["action"], "pass");
}
