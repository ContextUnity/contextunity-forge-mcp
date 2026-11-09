use forge_tasks::{
    engine::orchestrator::types::{OrchestratorEvent, OrchestratorState, RunnerInvocationContext},
    tasks::profile::{parse_profile, validate_profile_document, AgentDef, GateDef, RunnerConfig},
};
use serde_json::json;

#[test]
fn profile_accepts_native_agents_declarations() {
    let document = serde_yaml::to_string(&json!({
        "agents": {
            "builder": {
                "models": ["test-model:high"],
                "runner": "runner {prompt}"
            }
        }
    }))
    .unwrap();

    let parsed = validate_profile_document(&document);
    assert!(
        parsed.is_ok(),
        "assertion failed: native agents declarations must be accepted"
    );
}

#[test]
fn agent_and_role_profile_keys_share_the_same_validated_model() {
    for key in ["agents", "roles"] {
        let mut document = serde_json::Map::new();
        document.insert(key.into(), json!({"custom": {"runner": "agent {prompt}"}}));
        let profile =
            validate_profile_document(&serde_yaml::to_string(&document).unwrap()).unwrap();
        let agent: &AgentDef = &profile.roles["custom"];
        assert_eq!(agent.runner.as_deref(), Some("agent {prompt}"));
        let serialized = serde_json::to_value(&profile).unwrap();
        assert!(serialized.get("agents").is_some());
        assert!(serialized.get("roles").is_none());
    }
}

#[test]
fn gate_agent_and_legacy_role_keys_select_the_same_driver() {
    for key in ["agent", "role"] {
        let mut gate = serde_json::Map::new();
        gate.insert("id".into(), json!("build"));
        gate.insert(key.into(), json!("builder"));
        let document = serde_yaml::to_string(&json!({"gates": [gate]})).unwrap();
        let profile = parse_profile(&document).unwrap();
        assert_eq!(profile.gates[0].role.as_deref(), Some("builder"));
        let serialized = serde_json::to_value(&profile.gates[0]).unwrap();
        assert_eq!(serialized["agent"], "builder");
        assert!(serialized.get("role").is_none());
    }
}

#[test]
fn ambiguous_agent_and_role_declarations_are_rejected() {
    let profile = serde_yaml::to_string(&json!({"agents": {}, "roles": {}})).unwrap();
    assert!(parse_profile(&profile).is_err());

    let gate = json!({"id": "build", "agent": "builder", "role": "reviewer"});
    assert!(serde_json::from_value::<GateDef>(gate).is_err());
}

#[test]
fn runner_configuration_interpolates_runtime_context() {
    let template = "{model}|{reasoning}|{session_id}|{prompt}|{worktree}";
    let profile = validate_profile_document(
        &serde_yaml::to_string(&serde_json::json!({
            "roles": {
                "builder": {
                    "models": ["test-model:high"],
                    "runner": template,
                    "prompt_template": "Perform the claimed gate."
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let role = &profile.roles["builder"];
    let config = RunnerConfig::from_role(role).expect("configured role has a runner");
    assert_eq!(config.runner, template);
    assert_eq!(
        config.prompt_template.as_deref(),
        Some("Perform the claimed gate.")
    );
    let serialized_role = serde_json::to_value(role).unwrap();
    assert_eq!(serialized_role["runner"], template);
    assert_eq!(
        serialized_role["prompt_template"],
        "Perform the claimed gate."
    );

    for (model, reasoning, session_id, prompt, worktree, expected) in [
        (
            "test-model",
            "high",
            "builder-session",
            "Implement the claimed scope.",
            "/tmp/forge-runner",
            "test-model|high|builder-session|Implement the claimed scope.|/tmp/forge-runner",
        ),
        (
            "another-model",
            "medium",
            "resumed-session",
            "Keep {model} and {session_id} literal: \"quoted\"\nnext line",
            "/tmp/forge runner",
            "another-model|medium|resumed-session|Keep {model} and {session_id} literal: \"quoted\"\nnext line|/tmp/forge runner",
        ),
    ] {
        let context = RunnerInvocationContext {
            task_id: "fixture/milestone:runner".into(),
            stage: "build".into(),
            role: "builder".into(),
            model: model.into(),
            reasoning: reasoning.into(),
            session_id: session_id.into(),
            prompt: prompt.into(),
            worktree: worktree.into(),
        };
        assert_eq!(
            context.render_runner(&config),
            expected,
            "runner configuration must interpolate runtime context for {session_id}"
        );
    }

    assert_eq!(role.runner.as_deref(), Some(template));
}

#[test]
fn runner_configuration_is_absent_without_a_role_runner() {
    let profile = validate_profile_document(
        &serde_yaml::to_string(&json!({
            "roles": {
                "reviewer": {
                    "models": ["review-model"]
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    assert!(RunnerConfig::from_role(&profile.roles["reviewer"]).is_none());
}

#[test]
fn orchestrator_state_tracks_claims_wave_and_cumulative_runner_attempts() {
    let mut state = OrchestratorState::default();
    assert_eq!(state.active_wave_index, 0);
    assert_eq!(
        state.begin_wave(2, vec!["task-a".into(), "task-b".into()]),
        OrchestratorEvent::WaveStarted {
            wave_index: 2,
            task_ids: vec!["task-a".into(), "task-b".into()],
        }
    );
    assert_eq!(state.active_wave_index, 2);

    assert_eq!(
        state.record_claim("task-a", "build", "builder-a"),
        OrchestratorEvent::TaskClaimed {
            task_id: "task-a".into(),
            stage: "build".into(),
            worker_id: "builder-a".into(),
        }
    );
    state.record_claim("task-b", "build", "builder-b");

    assert_eq!(
        state.record_runner_start("task-a", "build", "builder-a", "session-a"),
        OrchestratorEvent::RunnerStarted {
            task_id: "task-a".into(),
            stage: "build".into(),
            worker_id: "builder-a".into(),
            session_id: "session-a".into(),
            attempt: 1,
        }
    );
    assert_eq!(
        state.record_runner_start("task-a", "build", "builder-a-retry", "session-a"),
        OrchestratorEvent::RunnerStarted {
            task_id: "task-a".into(),
            stage: "build".into(),
            worker_id: "builder-a-retry".into(),
            session_id: "session-a".into(),
            attempt: 2,
        }
    );

    assert_eq!(
        state.claimed_tasks.get("task-a").map(String::as_str),
        Some("builder-a")
    );
    assert_eq!(
        state.claimed_tasks.get("task-b").map(String::as_str),
        Some("builder-b")
    );
    assert_eq!(state.attempts.get("task-a"), Some(&2));
    assert!(!state.attempts.contains_key("task-b"));
}

#[test]
fn orchestrator_events_preserve_runner_rejection_delivery_and_wave_outcomes() {
    let runner_exit = OrchestratorEvent::RunnerExited {
        task_id: "task-a".into(),
        stage: "build".into(),
        worker_id: "builder-a".into(),
        exit_code: None,
        timed_out: true,
    };
    assert!(matches!(
        runner_exit,
        OrchestratorEvent::RunnerExited {
            task_id,
            stage,
            worker_id,
            exit_code: None,
            timed_out: true,
        } if task_id == "task-a" && stage == "build" && worker_id == "builder-a"
    ));

    let rejection = OrchestratorEvent::GateRejected {
        task_id: "task-a".into(),
        stage: "review".into(),
        worker_id: "reviewer-a".into(),
        findings: json!({ "scope": ["add a missing assertion"] }),
    };
    assert!(matches!(
        rejection,
        OrchestratorEvent::GateRejected {
            task_id,
            stage,
            worker_id,
            findings,
        } if task_id == "task-a"
            && stage == "review"
            && worker_id == "reviewer-a"
            && findings["scope"][0] == "add a missing assertion"
    ));

    let delivery = OrchestratorEvent::TaskDelivered {
        task_id: "task-a".into(),
        stage: "deliver".into(),
        worker_id: "builder-a".into(),
        receipt_id: "receipt-a".into(),
    };
    assert!(matches!(
        delivery,
        OrchestratorEvent::TaskDelivered {
            task_id,
            stage,
            worker_id,
            receipt_id,
        } if task_id == "task-a" && stage == "deliver" && worker_id == "builder-a" && receipt_id == "receipt-a"
    ));

    let completed_wave = OrchestratorEvent::WaveCompleted {
        wave_index: 2,
        task_ids: vec!["task-a".into(), "task-b".into()],
    };
    assert!(matches!(
        completed_wave,
        OrchestratorEvent::WaveCompleted { wave_index: 2, task_ids }
            if task_ids == ["task-a", "task-b"]
    ));
}
