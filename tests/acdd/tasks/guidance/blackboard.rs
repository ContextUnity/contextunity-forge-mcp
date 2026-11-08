use super::*;

#[test]
fn default_claim_bundle_carries_task_milestone_and_cross_gate_messages() {
    let (root, store, _) = fixture(DEFAULT_SEAM);
    let task_id = "forge-mcp/m-test:first";

    // Post message to task
    store
        .blackboard_post(task_id, "agent_1", "notes", "Initial hypothesis for task")
        .unwrap();

    // Post message from another task in same milestone
    let task_2_id = "forge-mcp/m-test:second";
    store
        .blackboard_post(task_2_id, "agent_2", "decisions", "Milestone level finding")
        .unwrap();

    let milestone_ref = store
        .milestone_scope_ref(&store.inspect(task_id).unwrap().milestone_ref)
        .unwrap();
    store
        .blackboard_post_scoped(
            &milestone_ref,
            None,
            None,
            "architect",
            "draft",
            "RFC: share the task claim breadcrumb with the receiving worker",
        )
        .unwrap();
    store
        .blackboard_post_scoped_with_recipient(
            contextunity_forge_mcp::db::tasks_store::BlackboardPostInput {
                milestone_ref: &milestone_ref,
                task_id: Some(task_2_id),
                subtask_ref: None,
                gate: None,
                author: "architect",
                topic: "draft",
                payload: "RFC addressed to the first task",
            },
            Some(task_id),
        )
        .unwrap();
    store
        .blackboard_post_scoped_with_recipient(
            contextunity_forge_mcp::db::tasks_store::BlackboardPostInput {
                milestone_ref: &milestone_ref,
                task_id: Some(task_2_id),
                subtask_ref: None,
                gate: None,
                author: "architect",
                topic: "draft",
                payload: "RFC addressed to the second task",
            },
            Some(task_2_id),
        )
        .unwrap();

    drop(store);
    let mut persistent_ids = Vec::new();
    let mut review_notes_id = None;
    for topic in ["decisions", "findings", "blockers", "notes"] {
        let posted = tasks::blackboard(
            root.root(),
            serde_json::from_value(json!({
                "action": "post",
                "task_id": task_id,
                "gate": "review",
                "topic": topic,
                "payload": format!("Cross-gate {topic}")
            }))
            .unwrap(),
            "cross-gate-test",
        )
        .unwrap();
        let id = posted["id"].as_u64().unwrap();
        if topic == "notes" {
            review_notes_id = Some(id);
        } else {
            persistent_ids.push(id);
        }
    }
    let review_notes_id = review_notes_id.expect("review notes post has an identifier");

    // The persisted task stage supplies the claim gate when stage is omitted.
    let claim_res = TaskHarness::claim_at(root.root(), task_id, "", "builder").unwrap();

    for board in [
        &claim_res["blackboard"],
        &claim_res["context_bundle"]["blackboard"],
    ] {
        let messages = board
            .as_array()
            .expect("claim and context bundle carry blackboard messages");
        let review_messages: Vec<_> = messages
            .iter()
            .filter(|message| message["gate"] == "review")
            .collect();
        assert_eq!(review_messages.len(), persistent_ids.len());
        for id in &persistent_ids {
            assert!(review_messages
                .iter()
                .any(|message| { message["id"].as_u64() == Some(*id) }));
        }
        assert!(!review_messages
            .iter()
            .any(|message| message["id"].as_u64() == Some(review_notes_id)));
    }

    // Response should be the full bundle by default
    assert_eq!(claim_res["task_id"], task_id);
    assert_eq!(claim_res["context_bundle"]["contract"]["task_id"], task_id);
    assert!(
        claim_res["context_bundle"].get("proof_schema").is_none(),
        "built-in proof gates must not expose a scheme proof schema"
    );
    let bb = claim_res["blackboard"]
        .as_array()
        .expect("blackboard array in bundle");
    assert_eq!(
        bb.len(),
        4 + persistent_ids.len(),
        "the claim sees task, milestone, and cross-gate messages"
    );
    assert_eq!(
        claim_res["breadcrumb"],
        "forge-mcp/forge-mcp/m-test/tasks/first/contract"
    );
    assert_eq!(
        claim_res["workflow_guidance"]["breadcrumb"],
        claim_res["breadcrumb"]
    );
    assert_eq!(
        claim_res["workflow_guidance"]["gate_cursor"]["gate_path"],
        "tasks"
    );
    assert_eq!(
        claim_res["workflow_guidance"]["gate_cursor"]["stage_id"],
        "contract"
    );
    let guidance_messages = claim_res["workflow_guidance"]["blackboard_messages"]
        .as_array()
        .unwrap();
    assert_eq!(guidance_messages.len(), 2);
    assert!(guidance_messages.iter().any(|message| {
        message["payload"]
            .as_str()
            .is_some_and(|payload| payload.contains("RFC: share the task claim breadcrumb"))
    }));
    assert!(guidance_messages.iter().any(|message| {
        message["recipient_task_id"] == task_id
            && message["payload"] == "RFC addressed to the first task"
    }));
    assert!(!guidance_messages
        .iter()
        .any(|message| { message["payload"] == "RFC addressed to the second task" }));

    let store = tasks::store_for_task(root.root(), task_id).unwrap();
    let checkpoint = store
        .get_checkpoint(claim_res["breadcrumb"].as_str().unwrap())
        .unwrap()
        .expect("claim checkpoint persisted");
    assert_eq!(checkpoint.breadcrumb, claim_res["breadcrumb"]);
    assert_eq!(checkpoint.state_payload["task_id"], task_id);
    assert_eq!(checkpoint.state_payload["stage"], "contract");
}
#[test]
fn blackboard_schema_exposes_gate_targeted_storage() {
    let h = TaskHarness::memory();
    let schema = TaskHarness::blackboard_schema(&h.store).unwrap();
    assert_eq!(schema.version, "7");
    assert!(schema.gate_column, "current schema stores gate targeting");
    assert!(
        schema.gate_indexes > 0,
        "gate-targeted lookups need an index"
    );
}
#[test]
fn task_blackboard_gate_targeting_and_schema_hint() {
    let (root, store, milestone) = fixture(DEFAULT_SEAM);
    let task_id = milestone.task_id(&milestone.tasks[0]);

    drop(store);

    let call = |request| {
        tasks::blackboard(
            root.root(),
            serde_json::from_value(request).unwrap(),
            "blackboard-test",
        )
    };
    let contract_post = call(json!({
        "action": "post",
        "task_id": task_id,
        "gate": "contract",
        "topic": "decisions",
        "payload": "Choose the public gate routing shape"
    }))
    .unwrap();
    let contract_id = contract_post["id"].as_u64().unwrap();
    let build_post = call(json!({
        "action": "post",
        "task_id": task_id,
        "gate": "build",
        "topic": "deferred",
        "payload": "Retain the build follow-up"
    }))
    .unwrap();
    let build_id = build_post["id"].as_u64().unwrap();

    let contract_page = call(json!({
        "action": "read",
        "task_id": task_id,
        "gate": "contract",
        "limit": 50
    }))
    .unwrap();
    let contract_messages = contract_page["messages"].as_array().unwrap();
    assert_eq!(contract_messages.len(), 1);
    assert_eq!(contract_messages[0]["id"], contract_id);
    assert_eq!(contract_messages[0]["gate"], "contract");

    let invalid_topic = call(json!({
        "action": "post",
        "task_id": task_id,
        "gate": "contract",
        "topic": "architectural_notes",
        "payload": "This legacy topic must be rejected at runtime"
    }))
    .unwrap_err();
    let schema_hint = format!("{invalid_topic:#}");
    for expected in [
        "draft",
        "notes",
        "findings",
        "blockers",
        "decisions",
        "deferred",
        "contract",
        "build",
        "review",
        "deliver",
        "example",
    ] {
        assert!(
            schema_hint.contains(expected),
            "actionable blackboard schema hint omitted {expected:?}: {schema_hint}"
        );
    }
    let invalid_gate = call(json!({
        "action": "read",
        "task_id": task_id,
        "gate": "unknown-gate"
    }))
    .unwrap_err();
    let gate_hint = format!("{invalid_gate:#}");
    for expected in [
        "unknown-gate",
        "contract",
        "build",
        "review",
        "deliver",
        "example",
    ] {
        assert!(
            gate_hint.contains(expected),
            "actionable blackboard gate hint omitted {expected:?}: {gate_hint}"
        );
    }

    let claim =
        TaskHarness::claim_at(root.root(), &task_id, "contract", "blackboard-claim-test").unwrap();
    let claim_messages = claim["workflow_guidance"]["blackboard_messages"]
        .as_array()
        .expect("claim guidance carries the active gate's blackboard messages");
    assert_eq!(claim_messages.len(), 1);
    assert_eq!(claim_messages[0]["id"], contract_id);
    assert_eq!(claim_messages[0]["gate"], "contract");
    let blackboard_info = claim["blackboard_info"]
        .as_object()
        .expect("claim returns a compact blackboard_info summary");
    assert!(
        serde_json::to_vec(blackboard_info).unwrap().len() <= 1024,
        "blackboard_info should remain compact"
    );
    assert_ne!(contract_id, build_id);
}
