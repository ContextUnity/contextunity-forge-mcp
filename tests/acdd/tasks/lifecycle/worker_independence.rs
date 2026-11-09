use super::*;
use crate::common::mcp_harness::McpTestHarness;
use contextunity_forge_mcp::db::tasks_store::UNKNOWN_SESSION_FINGERPRINT;
use forge_tasks::engine as lifecycle_engine;
use serde_json::{json, Value};
use std::process::Command;

fn independent_fixture() -> TaskHarness {
    let mut profile = fixture_profile();
    profile.gates = serde_yaml::from_str(
        "- id: author\n  role: builder\n  proof: none\n- id: audit\n  role: builder\n  proof: none\n  independent_from: author\n- id: inspection\n  role: independent_reviewer\n  proof: review\n  independent_from: author\n  contours: guidance.contours\n- id: finish\n  role: builder\n  proof: delivery\n  review_sources: [inspection]\n  auto_commit: false\n",
    ).unwrap();
    independent_profile_fixture(profile)
}

fn independent_profile_fixture(
    profile: contextunity_forge_mcp::core::tasks::profile::GateProfile,
) -> TaskHarness {
    let manifest = MilestoneFixtureBuilder::new("m-worker-independence")
        .task("independent", &["src/"])
        .task_proof_policy("direct-proof")
        .build();
    profile.validate().unwrap();
    TaskHarness::disk_with_profile(&manifest, "docs/010-worker-independence.md", profile)
}

fn cli_command(harness: &TaskHarness, session: Option<&str>, legacy_hosts: bool) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"));
    command
        .arg("--root")
        .arg(harness.root())
        .args(["--format", "json", "task"]);
    for key in [
        "FORGE_AGENT_SESSION_ID",
        "CODEX_THREAD_ID",
        "AGY_CONVERSATION_ID",
        "CLAUDE_SESSION_ID",
        "CURSOR_SESSION_ID",
    ] {
        command.env_remove(key);
    }
    if let Some(session) = session {
        command.env("FORGE_AGENT_SESSION_ID", session);
    }
    if legacy_hosts {
        for key in [
            "CODEX_THREAD_ID",
            "AGY_CONVERSATION_ID",
            "CLAUDE_SESSION_ID",
            "CURSOR_SESSION_ID",
        ] {
            command.env(key, "shared-session");
        }
    }
    command.current_dir(harness.root());
    command
}

#[test]
fn mcp_independent_gates_use_worker_identity_without_session_proof() {
    for session in [
        UNKNOWN_SESSION_FINGERPRINT,
        "shared-session",
        "different-session",
    ] {
        let mut harness = independent_fixture();
        let author = harness
            .claim_gate_in_session("author", "author-worker", Some("shared-session"))
            .unwrap();
        let evidence = harness.evidence_for_gate(&author).unwrap();
        harness
            .submit_evidence_in_session(&evidence, Some("shared-session"))
            .unwrap();
        let mut client = McpTestHarness::with_root_and_db(
            harness.root(),
            &harness.root().join(".forge/code-map.sqlite"),
            session,
        )
        .unwrap();
        let claim = client.payload("task_claim", json!({"task_id":harness.id,"stage":"audit","worker_id":"audit-worker","worktree":harness.root(),"bundle":false}));
        eprintln!("MCP independent claim with {session}: {claim:?}");
        assert!(claim.is_ok());
        let claimed = harness.store.inspect(&harness.id).unwrap();
        assert_eq!(claimed.worker_id.as_deref(), Some("audit-worker"));
        assert_eq!(
            harness
                .persisted_claim_session_optional(claimed.claim_revision)
                .as_deref(),
            Some(session)
        );
        let evidence = harness.evidence_for_gate(&claimed).unwrap();
        let submitted = client.payload(
            "task_submit",
            json!({"task_id":harness.id,"stage":"audit","action":"pass","evidence":evidence}),
        );
        assert!(submitted.is_ok());
        let submitted = submitted.unwrap();
        assert_eq!(submitted["dispatch"]["gate"], "inspection");
        assert_eq!(submitted["dispatch"]["agent"], "independent_reviewer");
        assert_eq!(
            submitted["dispatch"]["independent_from_worker_id"],
            "author-worker"
        );
        let ready = harness.store.inspect(&harness.id).unwrap();
        assert_eq!(
            (ready.status.as_str(), ready.stage.as_str()),
            ("ready", "inspection")
        );
        assert_eq!(
            harness.persisted_gate_evidence("audit")["worker_id"],
            "audit-worker"
        );
        assert_eq!(
            harness.persisted_gate_evidence("author")["worker_id"],
            "author-worker"
        );
        client.finish().unwrap();
    }
}

#[test]
fn mcp_independent_submission_ignores_transport_fingerprint_changes() {
    for submitting_session in [UNKNOWN_SESSION_FINGERPRINT, "shared-session"] {
        let mut harness = independent_fixture();
        let author = harness
            .claim_gate_in_session("author", "author-worker", Some("shared-session"))
            .unwrap();
        let evidence = harness.evidence_for_gate(&author).unwrap();
        harness
            .submit_evidence_in_session(&evidence, Some("shared-session"))
            .unwrap();
        let audit = harness
            .claim_gate_in_session("audit", "audit-worker", Some("audit-session"))
            .unwrap();
        let evidence = harness.evidence_for_gate(&audit).unwrap();
        let mut client = McpTestHarness::with_root_and_db(
            harness.root(),
            &harness.root().join(".forge/code-map.sqlite"),
            submitting_session,
        )
        .unwrap();
        let submitted = client.payload(
            "task_submit",
            json!({"task_id":harness.id,"stage":"audit","action":"pass","evidence":evidence}),
        );
        eprintln!("MCP independent submission with {submitting_session}: {submitted:?}");
        assert!(submitted.is_ok());
        let ready = harness.store.inspect(&harness.id).unwrap();
        assert_eq!(
            (ready.status.as_str(), ready.stage.as_str()),
            ("ready", "inspection")
        );
        assert_eq!(
            harness
                .persisted_claim_session_optional(audit.claim_revision)
                .as_deref(),
            Some("audit-session")
        );
        assert_eq!(
            harness.persisted_gate_evidence("audit")["worker_id"],
            "audit-worker"
        );
        client.finish().unwrap();
    }
}

#[test]
fn cli_independent_gates_advance_without_ambient_session_scraping() {
    for (session, legacy_hosts) in [
        (None, false),
        (None, true),
        (Some("shared-session"), true),
        (Some("different-session"), true),
    ] {
        let mut harness = independent_fixture();
        let author = harness
            .claim_gate_in_session("author", "author-worker", Some("shared-session"))
            .unwrap();
        let evidence = harness.evidence_for_gate(&author).unwrap();
        harness
            .submit_evidence_in_session(&evidence, Some("shared-session"))
            .unwrap();
        let claim = cli_command(&harness, session, legacy_hosts)
            .args([
                "claim",
                &harness.id,
                "--stage",
                "audit",
                "--worker",
                "audit-worker",
                "--worktree",
            ])
            .arg(harness.root())
            .output()
            .unwrap();
        eprintln!(
            "CLI claim stderr: {}",
            String::from_utf8_lossy(&claim.stderr)
        );
        assert!(claim.status.success());
        let claimed: Value = serde_json::from_slice(&claim.stdout).unwrap();
        assert_eq!(claimed["worker_id"], "audit-worker");
        let audit = harness.store.inspect(&harness.id).unwrap();
        assert_eq!(
            harness
                .persisted_claim_session_optional(audit.claim_revision)
                .as_deref(),
            session
        );
        let evidence = serde_json::to_string(&harness.evidence_for_gate(&audit).unwrap()).unwrap();
        let submitted = cli_command(&harness, session, legacy_hosts)
            .args([
                "submit",
                &harness.id,
                "--stage",
                "audit",
                "--action",
                "pass",
                "--evidence",
                &evidence,
            ])
            .output()
            .unwrap();
        assert!(
            submitted.status.success(),
            "{}",
            String::from_utf8_lossy(&submitted.stderr)
        );
        let response: Value = serde_json::from_slice(&submitted.stdout).unwrap();
        assert_eq!(response["dispatch"]["gate"], "inspection");
        assert_eq!(response["dispatch"]["agent"], "independent_reviewer");
        assert_eq!(
            response["dispatch"]["suggested_worker_id"],
            "independent-inspection"
        );
        let ready = harness.store.inspect(&harness.id).unwrap();
        assert_eq!(
            (ready.status.as_str(), ready.stage.as_str()),
            ("ready", "inspection")
        );
        assert_eq!(
            harness.persisted_gate_evidence("audit")["worker_id"],
            "audit-worker"
        );
    }
}

#[test]
fn independent_gate_rejects_reused_worker_despite_distinct_session() {
    let mut harness = independent_fixture();
    let author = harness
        .claim_gate_in_session("author", "author-worker", Some("author-session"))
        .unwrap();
    let evidence = harness.evidence_for_gate(&author).unwrap();
    harness
        .submit_evidence_in_session(&evidence, Some("author-session"))
        .unwrap();
    let before = serde_json::to_value(harness.store.inspect(&harness.id).unwrap()).unwrap();
    let mut client = McpTestHarness::with_root_and_db(
        harness.root(),
        &harness.root().join(".forge/code-map.sqlite"),
        "fresh-session",
    )
    .unwrap();
    let rejected = client.payload("task_claim", json!({"task_id":harness.id,"stage":"audit","worker_id":"author-worker","worktree":harness.root(),"bundle":false}));
    assert!(rejected
        .unwrap_err()
        .to_string()
        .contains("TASK_REVIEW_NOT_INDEPENDENT"));
    assert_eq!(
        serde_json::to_value(harness.store.inspect(&harness.id).unwrap()).unwrap(),
        before
    );
    assert_eq!(
        harness.persisted_gate_evidence("author")["worker_id"],
        "author-worker"
    );
    client.finish().unwrap();
}

#[test]
fn independent_submission_keeps_active_worker_binding() {
    let mut harness = independent_fixture();
    let author = harness
        .claim_gate_in_session("author", "author-worker", Some("author-session"))
        .unwrap();
    let evidence = harness.evidence_for_gate(&author).unwrap();
    harness
        .submit_evidence_in_session(&evidence, Some("author-session"))
        .unwrap();
    let audit = harness
        .claim_gate_in_session("audit", "audit-worker", Some("audit-session"))
        .unwrap();
    let mut evidence = harness.evidence_for_gate(&audit).unwrap();
    evidence.worker_id = "unclaimed-worker".into();
    let before = serde_json::to_value(&audit).unwrap();
    let mut client = McpTestHarness::with_root_and_db(
        harness.root(),
        &harness.root().join(".forge/code-map.sqlite"),
        "audit-session",
    )
    .unwrap();
    let rejected = client.payload(
        "task_submit",
        json!({"task_id":harness.id,"stage":"audit","action":"pass","evidence":evidence}),
    );
    assert!(rejected
        .unwrap_err()
        .to_string()
        .contains("TASK_STALE_SUBMISSION"));
    assert_eq!(
        serde_json::to_value(harness.store.inspect(&harness.id).unwrap()).unwrap(),
        before
    );
    client.finish().unwrap();
}

#[test]
fn independent_gate_dispatch_is_shared_by_submit_and_guidance() {
    let mut harness = independent_fixture();
    let mut client = McpTestHarness::with_root_and_db(
        harness.root(),
        &harness.root().join(".forge/code-map.sqlite"),
        "observer-session",
    )
    .unwrap();
    for (stage, next, agent, worker) in [
        ("author", "audit", "builder", "author-worker"),
        (
            "audit",
            "inspection",
            "independent_reviewer",
            "audit-worker",
        ),
    ] {
        let session = format!("explicit-{worker}");
        let claim = harness
            .claim_gate_in_session(stage, worker, Some(&session))
            .unwrap();
        let evidence = harness.evidence_for_gate(&claim).unwrap();
        let submitted = lifecycle_engine::submit(
            harness.root(),
            TaskHarness::pass_request_in_session(&harness.id, stage, &evidence, Some(&session)),
        )
        .unwrap();
        let ready = harness.store.inspect(&harness.id).unwrap();
        assert_eq!(
            (ready.status.as_str(), ready.stage.as_str()),
            ("ready", next)
        );
        assert_eq!(harness.persisted_gate_evidence(stage)["worker_id"], worker);
        eprintln!("actual submission response: {submitted}");
        assert!(submitted["dispatch"].is_object());
        let suggested = format!("independent-{next}");
        let directive = format!("[INDEPENDENT GATE DISPATCH REQUIRED] Next gate is '{next}' (agent: '{agent}'). Rule: Must be independent from 'author' worker 'author-worker'. Directs coordinator to spawn an independent subagent with worker_id '{suggested}'.");
        let expected = json!({"gate":next,"agent":agent,"independent_from":"author","independent_from_worker_id":"author-worker","suggested_worker_id":suggested,"directive":directive});
        assert_eq!(submitted["dispatch"], expected);
        let inspected = cli_command(&harness, None, false)
            .args(["inspect", &harness.id])
            .output()
            .unwrap();
        assert!(inspected.status.success());
        let cli: Value = serde_json::from_slice(&inspected.stdout).unwrap();
        let mcp = client
            .payload(
                "task_manage",
                json!({"action":"inspect","task_id":harness.id}),
            )
            .unwrap();
        for value in [&cli, &mcp] {
            let guidance = &value["workflow_guidance"];
            assert_eq!(guidance["dispatch"], expected);
            assert_eq!(guidance["independence_rule"], directive);
            assert!(guidance["steps"]
                .as_array()
                .unwrap()
                .contains(&json!(directive)));
            assert_eq!(guidance["independent_from_worker_id"], "author-worker");
        }
    }
    client.finish().unwrap();
}

#[test]
fn dispatch_follows_profile_routes_and_conditions_with_configured_roles() {
    for explicit_route in [false, true] {
        let mut profile = fixture_profile();
        profile.roles.insert(
            "safety_auditor".into(),
            profile.roles["independent_reviewer"].clone(),
        );
        profile.gates = serde_yaml::from_str(
            "- id: author\n  role: builder\n  proof: none\n- id: documentation_only\n  when: doc\n  role: builder\n  proof: none\n- id: safety\n  role: safety_auditor\n  proof: none\n  independent_from: author\n- id: finish\n  role: builder\n  proof: delivery\n  auto_commit: false\n",
        ).unwrap();
        if explicit_route {
            profile.gates[0].on_success = Some("safety".into());
        }
        let mut harness = independent_profile_fixture(profile);
        let author = harness
            .claim_gate_in_session("author", "author-worker", Some("shared"))
            .unwrap();
        let evidence = harness.evidence_for_gate(&author).unwrap();
        let submitted = lifecycle_engine::submit(
            harness.root(),
            TaskHarness::pass_request_in_session(&harness.id, "author", &evidence, Some("shared")),
        )
        .unwrap();
        assert_eq!(submitted["stage"], "safety");
        assert_eq!(submitted["dispatch"]["gate"], "safety");
        assert_eq!(submitted["dispatch"]["agent"], "safety_auditor");
        assert_eq!(
            submitted["dispatch"]["independent_from_worker_id"],
            "author-worker"
        );
        assert_eq!(
            submitted["dispatch"]["suggested_worker_id"],
            "independent-safety"
        );
        assert!(submitted["dispatch"]["directive"]
            .as_str()
            .unwrap()
            .contains("(agent: 'safety_auditor')"));
        let before = harness.store.inspect_details(&harness.id).unwrap();
        let mut client = McpTestHarness::with_root_and_db(
            harness.root(),
            &harness.root().join(".forge/code-map.sqlite"),
            "observer",
        )
        .unwrap();
        for _ in 0..2 {
            let context = client
                .payload(
                    "task_manage",
                    json!({"action":"context","task_id":harness.id}),
                )
                .unwrap();
            assert_eq!(
                context["workflow_guidance"]["dispatch"],
                submitted["dispatch"]
            );
            assert!(context["workflow_guidance"]["actionable_steps"]
                .as_array()
                .unwrap()
                .contains(&submitted["dispatch"]["directive"]));
        }
        assert_eq!(harness.store.inspect_details(&harness.id).unwrap(), before);
        let safety = harness
            .claim_gate_in_session("safety", "safety-worker", Some("shared"))
            .unwrap();
        assert_eq!(safety.worker_id.as_deref(), Some("safety-worker"));
        client.finish().unwrap();
    }
}

#[test]
fn independent_dispatch_is_absent_for_a_non_independent_next_gate() {
    let mut harness = independent_fixture();
    harness.advance_to("inspection").unwrap();
    let claim = harness
        .claim_gate_in_session(
            "inspection",
            "inspection-worker",
            Some("inspection-session"),
        )
        .unwrap();
    let evidence = harness.evidence_for_gate(&claim).unwrap();
    let submitted = tasks::submit(
        harness.root(),
        TaskHarness::pass_request_in_session(
            &harness.id,
            "inspection",
            &evidence,
            Some("inspection-session"),
        ),
    )
    .unwrap();
    assert_eq!(submitted["stage"], "finish");
    assert!(submitted.get("dispatch").is_none());
    let mut client = McpTestHarness::with_root_and_db(
        harness.root(),
        &harness.root().join(".forge/code-map.sqlite"),
        "observer-session",
    )
    .unwrap();
    let inspected = client
        .payload(
            "task_manage",
            json!({"action":"inspect","task_id":harness.id}),
        )
        .unwrap();
    assert!(inspected["workflow_guidance"].get("dispatch").is_none());
    client.finish().unwrap();
}
