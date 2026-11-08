use super::*;

#[test]
fn first_crlf_task_claim_activates_planned_milestone_and_includes_guidance() {
    let root = MilestoneHarness::configured_workspace();
    let active = "docs/milestones/010-start.md";
    let source = MilestoneFixtureBuilder::new("m-start")
        .title("Start work")
        .status("planned")
        .task("begin", &["src/"])
        .task_target("Begin work")
        .build()
        .replace('\n', "\r\n");
    root.write("src/lib.rs", "pub fn task() {}\n");
    root.write(active, &source);
    root.sync(active).unwrap();
    let claim = root
        .claim_task("forge-mcp/m-start:begin", "contract", "first")
        .unwrap();
    let guidance = &claim["workflow_guidance"];
    const PROTECTED_RULE: &str = "PROTECTED: milestone id, invariants, owners, depends_on, and acdd_profile are immutable while tasks are active. Declare task invariants inside task.invariants.";
    let guidance_text = serde_json::to_string(guidance).unwrap();
    let actual_rules = guidance["rules"].as_array().cloned().unwrap_or_default();
    let expected_rules = [
        format!(
            "ISOLATION: Develop exclusively inside dedicated worktree '{}'.",
            root.root().display()
        ),
        "IMMUTABILITY: Keep admitted scope unchanged after review pass and during deliver."
            .to_owned(),
        "INDEPENDENCE: Review stage requires a worker_id distinct from the accepted builder."
            .to_owned(),
    ];
    let missing_rules: Vec<_> = expected_rules
        .iter()
        .filter(|expected| {
            !actual_rules
                .iter()
                .any(|actual| actual.as_str() == Some(expected.as_str()))
        })
        .collect();
    assert!(
        guidance_text.contains(PROTECTED_RULE) && missing_rules.is_empty(),
        "CLI claim omitted protected guidance or operational rules; missing {missing_rules:?}; guidance: {guidance}"
    );
    let shown = root.show("m-start", true);
    assert_eq!(shown["status"], "active");
    assert!(shown["started_at"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    let started =
        chrono::DateTime::parse_from_rfc3339(shown["started_at"].as_str().unwrap()).unwrap();
    assert!(
        (chrono::Utc::now() - started.with_timezone(&chrono::Utc))
            .num_minutes()
            .abs()
            < 2
    );
}

#[test]
fn milestone_context_and_blackboard_preserve_milestone_notes_until_handoff() {
    let root = MilestoneHarness::context_retention();
    let task_id = root.task_id("implement");
    let task = TaskHarness::attach(&root, &task_id);
    let gates = task.executable_gates().unwrap();
    let mut context_posted = false;
    for gate in &gates {
        if gate.proof.kind() == Some(ProofKind::Review) && !context_posted {
            root.post_task_message(
                &task_id,
                "architect",
                "decisions",
                "Keep task context in the milestone receipt.",
            );
            root.post_task_message(
                &task_id,
                "builder",
                "notes",
                "The direct JSON proof passed SQLite verification.",
            );
            let messages = root.read_task_messages(&task_id);
            assert_eq!(messages["messages"].as_array().unwrap().len(), 2);
            assert_eq!(messages["messages"][0]["author"], "builder");
            assert_eq!(messages["messages"][1]["author"], "architect");
            context_posted = true;
        }
        let worker = task.worker_for_gate(gate);
        root.pass_gate_without_commit(&task_id, &gate.id, &worker);
    }
    assert!(
        context_posted,
        "active task profile omitted its review gate"
    );
    let delivery = root.head();
    let parsed = Milestone::parse(&root.document(root.active_path()), "forge-mcp").unwrap();
    let completed = &parsed.tasks[0];
    assert_eq!(completed.status.as_deref(), Some("completed"));
    assert_eq!(completed.agent_type.as_deref(), Some("gpt-6-sol"));
    let receipt = completed.receipt.as_ref().unwrap();
    assert!(receipt.commit.is_none());
    let mut sqlite_receipt = root.store().inspect(&task_id).unwrap().receipt.unwrap();
    assert_eq!(sqlite_receipt.commit.as_deref(), Some(delivery.as_str()));
    sqlite_receipt.commit = None;
    assert_eq!(receipt, &sqlite_receipt);
    assert!(receipt.evidence.get("command_proof").is_some());
    assert!(receipt.review.get("review_proof").is_some());
    let rollup = receipt.rollup.as_ref().unwrap();
    assert_eq!(rollup.verified_invariants, ["durable-context", "reviewed"]);
    assert_eq!(
        rollup.architectural_notes,
        ["decisions: Keep task context in the milestone receipt."]
    );
    assert_eq!(rollup.review_summary.decision, "pass");
    assert!(root.read_task_messages(&task_id)["messages"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(root.store().inspect(&task_id).unwrap().status, "completed");
}

#[test]
fn milestone_inherited_descriptors_and_gate_snapshots_preserve_task_state() {
    let root = MilestoneHarness::context_retention();
    let task_id = root.task_id("implement");
    let inherited_descriptor = contextunity_forge_mcp::db::tasks_store::recovery::lock_database(
        &root.path(".forge/tasks.sqlite"),
    )
    .unwrap()
    .try_clone()
    .unwrap();
    assert_eq!(
        root.inspect_task(&task_id)["spec"]["agent_type"],
        "gpt-6-sol"
    );
    drop(inherited_descriptor);
    drop(root.store());
    let snapshot = root.checkpoint();

    {
        let task_id = root.task_id("implement");
        let task = TaskHarness::attach(&root, &task_id);
        let gates = task.executable_gates().unwrap();
        let delivery_gate = gates
            .iter()
            .find(|gate| gate.proof.kind() == Some(ProofKind::Delivery))
            .expect("active task profile has no delivery gate");
        for gate in gates
            .iter()
            .take_while(|gate| gate.id.as_str() != delivery_gate.id.as_str())
        {
            let worker = task.worker_for_gate(gate);
            root.pass_gate_without_commit(&task_id, &gate.id, &worker);
        }
        let delivery = delivery_gate.id.clone();
        let worker = task.worker_for_gate(delivery_gate);
        let previous_name = root
            .claim_task(&task_id, "handoff/v1", "independent-reviewer")
            .unwrap_err();
        assert!(previous_name.to_string().contains("TASK_STAGE_INVALID"));
        assert_eq!(root.inspect_task(&task_id)["status"], "ready");
        root.claim_task(&task_id, &delivery, &worker).unwrap();
        let stored = root.store().inspect(&task_id).unwrap();
        let mut previous_proof = evidence(&stored);
        previous_proof.stage = "handoff/v1".into();
        let rejected = contextunity_forge_mcp::engine::tasks::submit(
            root.root(),
            TaskHarness::pass_request(&task_id, "handoff/v1", &previous_proof),
        )
        .unwrap_err();
        assert!(rejected.to_string().contains("TASK_STAGE_UNKNOWN"));
        assert_eq!(root.inspect_task(&task_id)["status"], "in_progress");
    }

    snapshot.restore(&root);

    {
        let task_id = root.task_id("implement");
        let task = TaskHarness::attach(&root, &task_id);
        let gates = task.executable_gates().unwrap();
        let mut candidate = None;
        let mut delivery = None;
        for gate in &gates {
            let stage = gate.id.as_str();
            let worker = task.worker_for_gate(gate);
            let step = root.pass_gate_without_commit(&task_id, stage, &worker);
            assert_eq!(step.before["workflow_guidance"]["active_stage"], stage);
            assert!(step.before["workflow_guidance"]["role"].is_string());
            assert!(step.before["workflow_guidance"]["steps"]
                .as_array()
                .is_some_and(|steps| !steps.is_empty()));
            assert_eq!(
                step.claimed["workflow_guidance"]["active_stage"],
                step.before["workflow_guidance"]["active_stage"]
            );
            assert_eq!(
                step.claimed["workflow_guidance"]["role"],
                step.before["workflow_guidance"]["role"]
            );
            assert!(step.claimed["workflow_guidance"]["blackboard_messages"].is_array());
            match gate.proof.kind() {
                Some(ProofKind::Command) => {
                    candidate = Some(pinned_candidate(root.root(), &task_id));
                    let candidate = candidate.as_deref().unwrap();
                    assert_eq!(step.submitted["snapshot"]["commit"], candidate);
                    assert_eq!(
                        step.submitted["snapshot"]["inspect_cmd"],
                        format!("git show {}", &candidate[..7])
                    );
                }
                Some(ProofKind::Review) => {
                    let candidate = candidate.as_deref().unwrap();
                    assert_eq!(step.submitted["snapshot"]["commit"], candidate);
                    assert_eq!(
                        step.submitted["snapshot"]["inspect_cmd"],
                        format!("git show {}", &candidate[..7])
                    );
                }
                Some(ProofKind::Delivery) => {
                    let landed = root.head();
                    assert_eq!(step.submitted["status"], "completed");
                    assert_eq!(step.submitted["snapshot"]["commit"], landed);
                    assert_eq!(step.submitted["receipt"]["commit"], landed);
                    delivery = Some(landed);
                }
                _ => {}
            }
        }
        let details = root.store().inspect_details(&task_id).unwrap();
        let candidate = candidate.as_deref().unwrap();
        assert_eq!(details["latest_snapshot"]["commit"], candidate);
        assert_eq!(
            details["latest_snapshot"]["inspect_cmd"],
            format!("git show {}", &candidate[..7])
        );
        assert_ne!(Some(candidate), delivery.as_deref());
    }

    snapshot.restore(&root);
}
