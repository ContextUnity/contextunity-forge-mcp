//! Test module reserved for the AC-001 SQLite persistence contract.

use super::support::*;

fn primary_key_columns(connection: &rusqlite::Connection, table: &str) -> Vec<String> {
    let mut statement = connection
        .prepare("SELECT name FROM pragma_table_info(?1) WHERE pk > 0 ORDER BY pk")
        .expect("PRAGMA table_info should inspect primary-key columns");
    statement
        .query_map([table], |row| row.get(0))
        .expect("primary-key columns should be queryable")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("primary-key column names should decode")
}

#[test]
fn task_store_migrates_the_ac001_gate_persistence_contract() {
    let (_workspace, mut store, _milestone) = fixture();
    let schema_version: String = store
        .connection
        .query_row(
            "SELECT value FROM task_store_metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .expect("the production task store should expose its schema version");
    assert_eq!(schema_version, "7");

    let required_columns = [
        (
            "gate_transitions",
            &[
                "id",
                "milestone_id",
                "subject",
                "gate_path",
                "action",
                "proof_kind",
                "proof_payload",
                "git_tree_sha",
                "worker_id",
                "created_at",
            ][..],
        ),
        (
            "gate_execution_traces",
            &[
                "id",
                "milestone_id",
                "subject",
                "gate_path",
                "attempt_id",
                "command_id",
                "command_str",
                "status",
                "exit_code",
                "log_path",
                "output_snippet",
                "stdout_sha256",
                "duration_ms",
                "executed_at",
            ][..],
        ),
        (
            "gate_checkpoints",
            &[
                "breadcrumb",
                "milestone_id",
                "subject",
                "gate_path",
                "transition_id",
                "git_tree_sha",
                "state_payload",
                "created_at",
                "updated_at",
            ][..],
        ),
        (
            "milestone_contract_matrix",
            &[
                "milestone_id",
                "source_task",
                "target_task",
                "contract_type",
                "contract_ref",
                "revision",
                "status",
                "created_at",
            ][..],
        ),
        ("task_blackboard", &["recipient_task_id", "status"][..]),
    ];

    for (table, columns) in required_columns {
        let exists: bool = store
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .expect("the production database should expose sqlite_master");
        assert!(exists, "AC-001 requires the `{table}` table");

        for column in columns {
            let exists: bool = store
                .connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)",
                    rusqlite::params![table, column],
                    |row| row.get(0),
                )
                .expect("PRAGMA table_info should inspect the production schema");
            assert!(exists, "AC-001 requires column `{column}` in `{table}`");
        }
    }

    assert_eq!(
        primary_key_columns(&store.connection, "gate_transitions"),
        ["id"]
    );
    assert_eq!(
        primary_key_columns(&store.connection, "gate_execution_traces"),
        ["id"]
    );
    assert_eq!(
        primary_key_columns(&store.connection, "gate_checkpoints"),
        ["breadcrumb"]
    );
    assert_eq!(
        primary_key_columns(&store.connection, "milestone_contract_matrix"),
        ["milestone_id", "source_task", "target_task", "contract_ref"]
    );

    let recipient_index_exists: bool = store
        .connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name='task_blackboard_recipient_idx')",
            [],
            |row| row.get(0),
        )
        .expect("the production database should expose sqlite_master indexes");
    assert!(
        recipient_index_exists,
        "AC-001 requires recipient-addressed blackboard lookup"
    );

    let tasks = store
        .list(None, "all", None)
        .expect("the fixture tasks should be available for recipient validation");
    let sender_id = tasks[0].task_id.clone();
    let recipient_id = tasks[1].task_id.clone();
    let milestone_ref = "forge-mcp/forge-mcp/docs/010-test.md";
    let message_id = store
        .blackboard_post_scoped_with_recipient(
            contextunity_forge_mcp::db::tasks_store::BlackboardPostInput {
                milestone_ref,
                task_id: Some(&sender_id),
                subtask_ref: None,
                gate: Some("build"),
                author: "sender",
                topic: "notes",
                payload: "shared-helper RFC",
            },
            Some(&recipient_id),
        )
        .expect("the task store should persist a recipient-addressed message");
    let message = store
        .blackboard_inspect(message_id)
        .expect("the posted message should be inspectable")
        .expect("the posted message should exist");
    assert_eq!(message.task_id.as_deref(), Some(sender_id.as_str()));
    assert_eq!(
        message.recipient_task_id.as_deref(),
        Some(recipient_id.as_str())
    );
    assert_eq!(message.payload.as_deref(), Some("shared-helper RFC"));
    let message_json = serde_json::to_value(&message).unwrap();
    assert_eq!(message_json["task_id"].as_str(), Some(sender_id.as_str()));
    assert_eq!(
        message_json["recipient_task_id"].as_str(),
        Some(recipient_id.as_str())
    );
    let inbox = store
        .blackboard_page_for_recipient(milestone_ref, &recipient_id, Some("notes"), Some(10), None)
        .expect("the task store should list unread messages for their recipient");
    assert_eq!(inbox.messages.len(), 1);
    assert_eq!(inbox.messages[0].id, message_id);
    assert_eq!(
        inbox.messages[0].recipient_task_id.as_deref(),
        Some(recipient_id.as_str())
    );
    assert!(inbox.messages[0].payload.is_none());
    assert!(!store
        .blackboard_mark_read(message_id, &sender_id)
        .expect("a different task must not acknowledge this recipient's message"));
    assert!(store
        .blackboard_mark_read(message_id, &recipient_id)
        .expect("the intended recipient should be able to mark its message read"));
    let read_inbox = store
        .blackboard_page_for_recipient(milestone_ref, &recipient_id, Some("notes"), Some(10), None)
        .expect("read messages should leave the unread recipient page");
    assert!(read_inbox.messages.is_empty());
    assert!(!store
        .blackboard_mark_read(message_id, &recipient_id)
        .expect("a message already read should not be acknowledged twice"));
    assert!(store
        .blackboard_post_scoped_with_recipient(
            contextunity_forge_mcp::db::tasks_store::BlackboardPostInput {
                milestone_ref,
                task_id: None,
                subtask_ref: None,
                gate: None,
                author: "sender",
                topic: "notes",
                payload: "foreign recipient must be rejected",
            },
            Some("other-project/m-test:foreign"),
        )
        .is_err());

    let transition = contextunity_forge_mcp::db::tasks_store::GateTransition {
        milestone_id: "m-test".into(),
        subject: "task:first".into(),
        gate_path: "tasks.build".into(),
        action: "advance".into(),
        proof_kind: "command".into(),
        proof_payload: serde_json::json!({"exit_code": 0}),
        git_tree_sha: "tree-sha".into(),
        worker_id: "builder".into(),
    };
    let transition_id = store
        .record_gate_transition(&transition)
        .expect("the task store should persist gate transitions");
    let (stored_subject, stored_proof): (String, String) = store
        .connection
        .query_row(
            "SELECT subject, proof_payload FROM gate_transitions WHERE id=?1",
            [transition_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the recorded gate transition should be queryable");
    assert_eq!(stored_subject, transition.subject);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stored_proof).unwrap(),
        transition.proof_payload
    );

    let max_snippet_bytes = contextunity_forge_mcp::db::tasks_store::MAX_OUTPUT_SNIPPET_BYTES;
    let trace = contextunity_forge_mcp::db::tasks_store::GateExecutionTrace {
        milestone_id: "m-test".into(),
        subject: "task:first".into(),
        gate_path: "tasks.build".into(),
        attempt_id: 1,
        command_id: "rust-check-and-test".into(),
        command_str: "cargo test --test acdd".into(),
        status: "completed".into(),
        exit_code: Some(0),
        log_path: None,
        output_snippet: Some("λ".repeat(max_snippet_bytes / 2 + 1)),
        stdout_sha256: Some("stdout-sha256".into()),
        duration_ms: 25,
    };
    let trace_id = store
        .record_execution_trace(&trace)
        .expect("the task store should persist command execution traces");
    let (stored_output, stored_output_bytes): (String, i64) = store
        .connection
        .query_row(
            "SELECT output_snippet, length(CAST(output_snippet AS BLOB)) \
             FROM gate_execution_traces WHERE id=?1",
            [trace_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the bounded trace output should be queryable");
    assert_eq!(stored_output_bytes as usize, max_snippet_bytes);
    assert_eq!(stored_output, "λ".repeat(max_snippet_bytes / 2));
    assert!(
        store.record_execution_trace(&trace).is_err(),
        "the attempt and command uniqueness key should reject duplicate traces"
    );

    let checkpoint = contextunity_forge_mcp::db::tasks_store::GateCheckpoint {
        breadcrumb: "forge-mcp/forge-mcp/m-test/tasks.build/first/build".into(),
        milestone_id: "forge-mcp/forge-mcp/m-test".into(),
        subject: "first".into(),
        gate_path: "tasks.build".into(),
        transition_id: Some(transition_id),
        git_tree_sha: "tree-sha".into(),
        state_payload: serde_json::json!({"state": "red"}),
    };
    store
        .save_checkpoint(&checkpoint)
        .expect("the task store should save breadcrumb-keyed checkpoints");
    let mut refreshed_checkpoint = checkpoint.clone();
    refreshed_checkpoint.state_payload = serde_json::json!({"state": "green"});
    store
        .save_checkpoint(&refreshed_checkpoint)
        .expect("saving the same breadcrumb should update its checkpoint");
    let (checkpoint_count, checkpoint_transition, checkpoint_state): (i64, Option<i64>, String) =
        store
            .connection
            .query_row(
                "SELECT COUNT(*), MAX(transition_id), MAX(state_payload) \
                 FROM gate_checkpoints WHERE breadcrumb=?1",
                [&checkpoint.breadcrumb],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("the saved checkpoint should be queryable");
    assert_eq!(checkpoint_count, 1);
    assert_eq!(checkpoint_transition, Some(transition_id));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&checkpoint_state).unwrap(),
        serde_json::json!({
            "_forge_checkpoint_payload_v1": refreshed_checkpoint.state_payload.clone()
        })
    );
    assert_eq!(
        store
            .get_checkpoint(&checkpoint.breadcrumb)
            .unwrap()
            .unwrap()
            .state_payload,
        refreshed_checkpoint.state_payload
    );

    let matrix = contextunity_forge_mcp::core::tasks::MilestoneContractMatrix {
        milestone_id: "m-test".into(),
        entries: vec![
            contextunity_forge_mcp::core::tasks::MilestoneContractEntry {
                milestone_id: "m-test".into(),
                source_task: "provider".into(),
                target_task: "consumer".into(),
                contract_type: contextunity_forge_mcp::core::tasks::ContractType::SeamDependency,
                contract_ref: "GateExecutionTrace".into(),
                revision: 1,
                status: contextunity_forge_mcp::core::tasks::ContractStatus::Synthesized,
            },
        ],
    };
    store
        .replace_contract_matrix(&matrix)
        .expect("the task store should atomically replace a milestone contract matrix");
    assert_eq!(
        store.list_contract_matrix("m-test").unwrap(),
        matrix,
        "the persisted matrix should round-trip in contract-key order"
    );

    let mut duplicate_matrix = matrix.clone();
    duplicate_matrix
        .entries
        .push(duplicate_matrix.entries[0].clone());
    assert!(store.replace_contract_matrix(&duplicate_matrix).is_err());
    assert_eq!(
        store.list_contract_matrix("m-test").unwrap(),
        matrix,
        "a failed matrix replacement should preserve the previous rows"
    );

    let index_id: i64 = store
        .connection
        .query_row(
            "INSERT INTO task_blackboard \
             (milestone_ref, author, topic, payload, created_at, recipient_task_id) \
             VALUES ('forge-mcp/docs/010-test.md', 'test', 'notes', 'rfc', 1, 'provider') \
             RETURNING id",
            [],
            |row| row.get(0),
        )
        .expect("recipient-addressed blackboard messages should be insertable");
    let (recipient, status): (Option<String>, Option<String>) = store
        .connection
        .query_row(
            "SELECT recipient_task_id, status FROM task_blackboard WHERE id=?1",
            [index_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("recipient-addressed messages should have a default active status");
    assert_eq!(recipient.as_deref(), Some("provider"));
    assert_eq!(status.as_deref(), Some("active"));
}
