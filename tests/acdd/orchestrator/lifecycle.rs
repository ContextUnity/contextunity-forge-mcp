#![cfg(target_os = "linux")]

use crate::acdd::{support::*, tasks::harness::TaskHarness};
use forge_tasks::{
    engine::orchestrator::{
        spawner::{
            monitor_runner_process, render_role_prompt, spawn_role_runner, RunnerSessionPolicy,
        },
        task_runner::run_task_lifecycle,
        types::{OrchestratorEvent, RunnerInvocationContext},
    },
    tasks::profile::{GateDriver, ProofKind, RunnerConfig},
};
use std::{fs, process::Command, time::Duration};

mod harness;
use harness::{LifecycleHarness, MANIFEST};

#[test]
fn claimed_role_process_submits_through_real_task_cli() {
    let mut h = LifecycleHarness::new();
    let mut starts = 0;
    loop {
        let gate = h
            .task
            .store
            .resolve_gate_for_task(&h.task.id, h.root().to_str().unwrap(), None)
            .unwrap();
        if gate.driver() != Some(GateDriver::Agent)
            || gate.proof.kind() == Some(ProofKind::Delivery)
        {
            break;
        }
        let claimed = h.task.claim_next().unwrap();
        let context =
            TaskHarness::manage_task(h.root(), &h.task.id, tasks::ManageAction::Context).unwrap();
        let profile = h.task.profile().unwrap();
        let role_id = gate.role.as_ref().unwrap();
        let role = &profile.roles[role_id];
        let invocation = RunnerInvocationContext {
            task_id: h.task.id.clone(),
            stage: claimed.stage.clone(),
            role: role_id.clone(),
            model: role
                .models
                .first()
                .map(|model| model.model.clone())
                .unwrap_or_default(),
            reasoning: role
                .models
                .first()
                .and_then(|model| model.reasoning.clone())
                .unwrap_or_default(),
            session_id: TaskHarness::session_for(claimed.worker_id.as_deref().unwrap()),
            prompt: render_role_prompt(&context)
                .unwrap_or_else(|error| panic!("role context: {error}: {context}")),
            worktree: h.root().to_owned(),
        };
        let config = RunnerConfig::from_role(role).unwrap();
        let process = spawn_role_runner(&config, &invocation, RunnerSessionPolicy::Resume).unwrap();
        let mut output = Vec::new();
        let outcome = monitor_runner_process(process, Duration::from_secs(20), &mut |_, bytes| {
            output.extend_from_slice(bytes);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            outcome.exit_code,
            Some(0),
            "{}: {}",
            claimed.stage,
            String::from_utf8_lossy(&output)
        );
        assert!(!outcome.timed_out);
        let after = h.task.store.inspect(&h.task.id).unwrap();
        assert_ne!(after.stage, claimed.stage);
        assert_eq!(after.status, "ready");
        assert!(!h.task.persisted_gate_evidence(&claimed.stage)["proof"].is_null());
        starts += 1;
    }
    assert!(starts > 0);
    assert_eq!(h.records().len(), starts);
}

#[test]
fn task_run_advances_profile_gates_and_records_delivery() {
    let h = LifecycleHarness::new();
    let mut events = Vec::new();
    let result = run_task_lifecycle(h.root(), &h.task.id, &h.options, &mut |event| {
        events.push(event)
    });
    assert!(
        result.is_ok(),
        "assertion failed: configured task lifecycle must reach delivery: {result:?}"
    );
    let result = result.unwrap();
    let accepted = h.task.store.inspect(&h.task.id).unwrap();
    assert_eq!(accepted.status, "completed");
    assert_eq!(result["status"], "completed");
    let receipt = accepted.receipt.unwrap();
    let delivered = receipt.commit.unwrap();
    assert_eq!(git_head(h.root()), delivered);
    assert!(delivered.starts_with(result["receipt"]["commit"].as_str().unwrap()));
    let (candidate, _) = h
        .task
        .store
        .build_snapshot_candidate(&h.task.id)
        .unwrap()
        .unwrap();
    let source = |commit: &str| {
        git_command(h.root(), &["show", &format!("{commit}:src/lib.rs")])
            .output()
            .unwrap()
    };
    let candidate_source = source(&candidate);
    let delivered_source = source(&delivered);
    assert!(candidate_source.status.success() && delivered_source.status.success());
    assert_eq!(candidate_source.stdout, delivered_source.stdout);
    let manifest = Milestone::parse(
        &fs::read_to_string(h.root().join(MANIFEST)).unwrap(),
        "forge-mcp",
    )
    .unwrap();
    assert_eq!(manifest.tasks[0].status.as_deref(), Some("completed"));
    assert_eq!(manifest.tasks[0].receipt.as_ref().unwrap().decision, "pass");
    assert!(!h.task.persisted_gate_proof("runtime-check")["command_proof"].is_null());
    let profile = h.task.profile().unwrap();
    let records = h.records();
    assert!(records.iter().any(|record| {
        let gate = profile.by_stage(record["stage"].as_str().unwrap()).unwrap();
        gate.command.is_some() && gate.driver() == Some(GateDriver::Agent)
    }));
    for record in &records {
        let gate = profile.by_stage(record["stage"].as_str().unwrap()).unwrap();
        assert_ne!(gate.proof.kind(), Some(ProofKind::Delivery));
        assert_eq!(record["session"], record["environment_session"]);
        assert_eq!(
            h.task
                .persisted_claim_session(record["claim_revision"].as_u64().unwrap()),
            record["session"].as_str().unwrap()
        );
    }
    let builder = records
        .iter()
        .find(|record| record["stage"] == "build")
        .unwrap();
    let reviewer = records
        .iter()
        .find(|record| record["stage"] == "review")
        .unwrap();
    assert_ne!(builder["session"], reviewer["session"]);
    assert!(events.iter().any(|event| matches!(event, OrchestratorEvent::TaskDelivered { task_id, .. } if task_id == &h.task.id)));
}

#[test]
fn task_run_resumes_persisted_cursor_without_replaying_accepted_gates() {
    let mut h = LifecycleHarness::new();
    h.task.advance_to("build").unwrap();
    let prior = h.task.persisted_gate_evidence("contract");
    let result = run_task_lifecycle(h.root(), &h.task.id, &h.options, &mut |_| {});
    assert!(result.is_ok(), "persisted cursor must resume: {result:?}");
    assert_eq!(h.task.persisted_gate_evidence("contract"), prior);
    assert!(h
        .records()
        .iter()
        .all(|record| record["stage"] != "contract"));
    let completed = h.task.store.inspect(&h.task.id).unwrap();
    let head = git_head(h.root());
    let records = h.records();
    let mut events = Vec::new();
    let repeated = run_task_lifecycle(h.root(), &h.task.id, &h.options, &mut |event| {
        events.push(event)
    });
    assert!(
        repeated.is_ok(),
        "completed task must be idempotent: {repeated:?}"
    );
    assert_eq!(
        h.task.store.inspect(&h.task.id).unwrap().claim_revision,
        completed.claim_revision
    );
    assert_eq!(git_head(h.root()), head);
    assert_eq!(h.records(), records);
    assert!(events.is_empty());
}

#[test]
fn task_run_routes_structured_rejection_to_retained_stage_session() {
    for (kind, target) in [("scope", "build"), ("symbols", "contract")] {
        let h = LifecycleHarness::new();
        let findings = json!({"kind":kind,"details":[{"message":"retain structured feedback {model}","path":"src/lib.rs"}]});
        h.write_control("reject.json", &findings);
        let mut events = Vec::new();
        let result = run_task_lifecycle(h.root(), &h.task.id, &h.options, &mut |event| {
            events.push(event)
        });
        assert!(
            result.is_ok(),
            "{kind} rejection must repair and deliver: {result:?}; role records: {:?}",
            h.records()
        );
        let records = h.records();
        let target_runs: Vec<_> = records
            .iter()
            .filter(|record| record["stage"] == target)
            .collect();
        assert_eq!(target_runs.len(), 2, "{kind}: {records:?}");
        assert_eq!(target_runs[0]["worker_id"], target_runs[1]["worker_id"]);
        assert_eq!(target_runs[0]["session"], target_runs[1]["session"]);
        assert_eq!(target_runs[1]["rejection"]["findings"], findings);
        assert_eq!(target_runs[1]["rejection"]["rejected_from"], "review");
        let review_runs: Vec<_> = records
            .iter()
            .filter(|record| record["stage"] == "review")
            .collect();
        assert_eq!(review_runs.len(), 2);
        assert_ne!(review_runs[0]["session"], target_runs[1]["session"]);
        assert!(events.iter().any(|event| matches!(event, OrchestratorEvent::GateRejected { findings: observed, .. } if observed == &findings)));
        assert_eq!(
            h.task.store.inspect(&h.task.id).unwrap().status,
            "completed"
        );
    }
}

#[test]
fn task_run_recovers_rejection_handoff_after_restart() {
    let mut h = LifecycleHarness::new();
    h.task.advance_to("review").unwrap();
    let builder = h.task.persisted_gate_evidence("build");
    let builder_session = h
        .task
        .persisted_claim_session(builder["claim_revision"].as_u64().unwrap());
    let review = h.task.claim_next().unwrap();
    let mut evidence = h.task.evidence_for_gate(&review).unwrap();
    evidence.proof = review_proof("reject");
    let findings = json!({"kind":"scope","scope_findings":[{"path":"src/lib.rs","message":"repair before delivery"}]});
    let reviewer_session = TaskHarness::session_for(&evidence.worker_id);
    h.task
        .submit_rejection_in_session(&evidence, Some(&reviewer_session), Some(&findings))
        .unwrap();
    let result = run_task_lifecycle(h.root(), &h.task.id, &h.options, &mut |_| {});
    assert!(
        result.is_ok(),
        "persisted rejection must resume: {result:?}"
    );
    let records = h.records();
    assert_eq!(records[0]["stage"], "build");
    assert_eq!(records[0]["worker_id"], builder["worker_id"]);
    assert_eq!(records[0]["session"], builder_session);
    assert_eq!(records[0]["rejection"]["findings"], findings);
    assert_eq!(
        h.task.store.inspect(&h.task.id).unwrap().status,
        "completed"
    );
}

#[test]
fn task_run_does_not_accept_process_success_without_gate_submission() {
    let h = LifecycleHarness::new();
    h.write_control("mode.json", &json!("no-submit"));
    let result = run_task_lifecycle(h.root(), &h.task.id, &h.options, &mut |_| {});
    assert!(
        result.is_err(),
        "an exit code is not accepted gate evidence"
    );
    let task = h.task.store.inspect(&h.task.id).unwrap();
    assert_eq!(task.status, "in_progress");
    assert_eq!(h.records().len(), 1);
    assert!(task.receipt.is_none());
    assert!(h.task.store.inspect_details(&h.task.id).unwrap()["gates"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn task_run_preserves_foreign_active_claim() {
    let mut h = LifecycleHarness::new();
    let foreign = h.task.claim_next().unwrap();
    let result = run_task_lifecycle(h.root(), &h.task.id, &h.options, &mut |_| {});
    let error = result.unwrap_err().to_string();
    assert!(error.contains("TASK_ALREADY_CLAIMED"), "{error}");
    let retained = h.task.store.inspect(&h.task.id).unwrap();
    assert_eq!(retained.claim_revision, foreign.claim_revision);
    assert_eq!(retained.worker_id, foreign.worker_id);
    assert_eq!(retained.worktree, foreign.worktree);
    assert_eq!(retained.status, "in_progress");
    assert!(h.records().is_empty());
}

#[test]
fn task_run_stops_on_runner_failure_or_timeout_without_delivery() {
    for (mode, code, timed_out) in [("exit-17", Some(17), false), ("sleep", None, true)] {
        let mut h = LifecycleHarness::new();
        h.options.runner_timeout = Duration::from_secs(1);
        h.write_control("mode.json", &json!(mode));
        let mut events = Vec::new();
        let result = run_task_lifecycle(h.root(), &h.task.id, &h.options, &mut |event| {
            events.push(event)
        });
        assert!(result.is_err(), "{mode} must stop execution");
        assert!(events.iter().any(|event| matches!(event, OrchestratorEvent::RunnerExited { exit_code, timed_out: observed_timeout, .. } if *exit_code == code && *observed_timeout == timed_out)));
        assert_ne!(
            h.task.store.inspect(&h.task.id).unwrap().status,
            "completed"
        );
        assert!(h.task.store.inspect(&h.task.id).unwrap().receipt.is_none());
    }
}

#[test]
fn task_run_delivery_preserves_subtask_guard() {
    let mut h = LifecycleHarness::new();
    h.task.advance_to("deliver").unwrap();
    TaskHarness::claim_in_worktree(
        h.root(),
        &h.task.id,
        "",
        &h.options.worker_id,
        h.root(),
        Some(&h.options.session_fingerprint),
    )
    .unwrap();
    let head = git_head(h.root());
    let manifest = fs::read(h.root().join(MANIFEST)).unwrap();
    let candidate = h.task.store.build_snapshot_candidate(&h.task.id).unwrap();
    let revision = h.task.store.inspect(&h.task.id).unwrap().claim_revision;
    for (status, evidence, expected) in [
        ("ready", Some(""), "TASK_SUBTASKS_INCOMPLETE"),
        ("in_progress", Some(""), "TASK_SUBTASKS_INCOMPLETE"),
        ("blocked", Some(""), "TASK_SUBTASKS_INCOMPLETE"),
        ("completed", None, "TASK_SUBTASK_EVIDENCE_MISSING"),
        ("completed", Some(""), "TASK_SUBTASK_EVIDENCE_MISSING"),
        ("completed", Some("   "), "TASK_SUBTASK_EVIDENCE_MISSING"),
        ("pending", Some(""), "TASK_SUBTASKS_INCOMPLETE"),
    ] {
        if status == "pending" {
            h.task.set_all_subtasks_legacy_pending().unwrap();
            h.task
                .store
                .subtask_update(&h.task.id, "optional", "cancelled", None)
                .unwrap();
        } else {
            h.task
                .store
                .subtask_update(&h.task.id, "verify", status, evidence)
                .unwrap();
        }
        let result = run_task_lifecycle(h.root(), &h.task.id, &h.options, &mut |_| {});
        let error = result.unwrap_err().to_string();
        assert!(error.contains(expected), "{status}/{evidence:?}: {error}");
        let task = h.task.store.inspect(&h.task.id).unwrap();
        assert_eq!(task.status, "in_progress");
        assert_eq!(task.claim_revision, revision);
        assert!(task.receipt.is_none());
        assert_eq!(git_head(h.root()), head);
        assert_eq!(fs::read(h.root().join(MANIFEST)).unwrap(), manifest);
        assert_eq!(
            h.task.store.build_snapshot_candidate(&h.task.id).unwrap(),
            candidate
        );
    }
}

#[test]
fn task_run_delivery_rejects_candidate_drift() {
    let mut h = LifecycleHarness::new();
    h.task.advance_to("deliver").unwrap();
    let head = git_head(h.root());
    let candidate = h.task.store.build_snapshot_candidate(&h.task.id).unwrap();
    let manifest = fs::read(h.root().join(MANIFEST)).unwrap();
    let changed = "pub fn example() -> bool { false }\n";
    fs::write(h.root().join("src/lib.rs"), changed).unwrap();
    let result = run_task_lifecycle(h.root(), &h.task.id, &h.options, &mut |_| {});
    let error = result.unwrap_err().to_string();
    assert!(error.contains("TASK_CANDIDATE_MISMATCH"), "{error}");
    assert!(h.task.store.inspect(&h.task.id).unwrap().receipt.is_none());
    assert_eq!(git_head(h.root()), head);
    assert_eq!(
        h.task.store.build_snapshot_candidate(&h.task.id).unwrap(),
        candidate
    );
    assert_eq!(fs::read(h.root().join(MANIFEST)).unwrap(), manifest);
    assert_eq!(
        fs::read_to_string(h.root().join("src/lib.rs")).unwrap(),
        changed
    );
}

#[test]
fn task_run_cli_accepts_in_place_and_uses_lifecycle() {
    let h = LifecycleHarness::new();
    let output = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            h.root().to_str().unwrap(),
            "--format",
            "json",
            "task",
            "run",
            &h.task.id,
            "--in-place",
        ])
        .env("FORGE_AGENT_SESSION_ID", "fixture-cli-coordinator")
        .current_dir(h.root())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "task run --in-place: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "completed");
    assert_eq!(
        h.task.store.inspect(&h.task.id).unwrap().status,
        "completed"
    );
    assert!(!h.records().is_empty());
}
