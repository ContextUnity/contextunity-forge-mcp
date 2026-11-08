use super::{
    resolve_session_fingerprint, validate_session_independence, UNKNOWN_SESSION_FINGERPRINT,
};
use crate::tasks::gates::Evidence;
use rusqlite::Connection;
use std::collections::BTreeMap;

#[test]
fn explicit_session_fingerprint_wins_and_blank_values_fall_back() {
    assert_eq!(
        resolve_session_fingerprint(Some("explicit-session"), |_| {
            panic!("explicit session read the environment")
        }),
        Some("explicit-session".into())
    );

    let environment = BTreeMap::from([
        ("FORGE_AGENT_SESSION_ID", " \t"),
        ("CODEX_THREAD_ID", "codex-session"),
        ("AGY_CONVERSATION_ID", "antigravity-session"),
    ]);
    assert_eq!(
        resolve_session_fingerprint(Some(" \t"), |key| {
            environment.get(key).map(|value| (*value).to_owned())
        }),
        Some("codex-session".into())
    );
}

#[test]
fn canonical_forge_session_precedes_host_fallbacks_and_blank_values_are_ignored() {
    let environment = BTreeMap::from([
        ("FORGE_AGENT_SESSION_ID", "forge-session"),
        ("CODEX_THREAD_ID", "codex-session"),
    ]);
    assert_eq!(
        resolve_session_fingerprint(None, |key| {
            environment.get(key).map(|value| (*value).to_owned())
        }),
        Some("forge-session".into())
    );

    let blank = BTreeMap::from([("FORGE_AGENT_SESSION_ID", " "), ("CODEX_THREAD_ID", "")]);
    assert_eq!(
        resolve_session_fingerprint(None, |key| {
            blank.get(key).map(|value| (*value).to_owned())
        }),
        None
    );
}

#[test]
fn independent_gate_fails_closed_when_session_identity_is_missing() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE task_claims (
                task_id TEXT NOT NULL,
                revision INTEGER NOT NULL,
                session_fingerprint TEXT
            );
            INSERT INTO task_claims VALUES ('repo/project/m:first', 1, 'builder-session');",
        )
        .unwrap();
    let evidence = prior_evidence();

    let error = validate_session_independence(
        &connection,
        "repo/project/m:first",
        "review",
        "build",
        &evidence,
        "reviewer",
        None,
        None,
    )
    .unwrap_err();

    assert!(error.to_string().contains("TASK_REVIEW_SESSION_UNKNOWN"));
}

#[test]
fn independent_gate_fails_closed_when_prior_claim_has_no_session_fingerprint() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE task_claims (
                task_id TEXT NOT NULL,
                revision INTEGER NOT NULL,
                session_fingerprint TEXT
            );
            INSERT INTO task_claims VALUES ('repo/project/m:first', 1, NULL);",
        )
        .unwrap();

    let error = validate_session_independence(
        &connection,
        "repo/project/m:first",
        "review",
        "build",
        &prior_evidence(),
        "reviewer",
        None,
        Some("reviewer-session"),
    )
    .unwrap_err();

    assert!(error.to_string().contains("TASK_REVIEW_SESSION_UNKNOWN"));
}

#[test]
fn independent_gate_fails_closed_for_an_mcp_server_without_host_identity() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE task_claims (
                task_id TEXT NOT NULL,
                revision INTEGER NOT NULL,
                session_fingerprint TEXT
            );
            INSERT INTO task_claims VALUES ('repo/project/m:first', 1, 'builder-session');",
        )
        .unwrap();

    let error = validate_session_independence(
        &connection,
        "repo/project/m:first",
        "review",
        "build",
        &prior_evidence(),
        "reviewer",
        None,
        Some(UNKNOWN_SESSION_FINGERPRINT),
    )
    .unwrap_err();

    assert!(error.to_string().contains("TASK_REVIEW_SESSION_UNKNOWN"));
}

#[test]
fn independent_gate_rejects_a_claim_with_the_builder_session_fingerprint() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE task_claims (
                task_id TEXT NOT NULL,
                revision INTEGER NOT NULL,
                session_fingerprint TEXT
            );
            INSERT INTO task_claims VALUES ('repo/project/m:first', 1, 'test-session-colliding');
            INSERT INTO task_claims VALUES ('repo/project/m:first', 2, 'test-session-colliding');",
        )
        .unwrap();
    let evidence = prior_evidence();

    let error = validate_session_independence(
        &connection,
        "repo/project/m:first",
        "review",
        "build",
        &evidence,
        "reviewer",
        Some(2),
        Some("test-session-colliding"),
    )
    .unwrap_err();

    assert!(error.to_string().contains("TASK_REVIEW_NOT_INDEPENDENT"));
}

fn prior_evidence() -> Evidence {
    Evidence {
        task_id: "repo/project/m:first".into(),
        stage: "build".into(),
        claim_revision: 1,
        contract_revision: 1,
        worker_id: "builder".into(),
        worktree: "/tmp/worktree".into(),
        commit: None,
        candidate_baseline_head: None,
        proof: serde_json::json!({}),
    }
}
