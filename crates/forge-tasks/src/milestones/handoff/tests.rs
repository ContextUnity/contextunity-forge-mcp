use super::*;
mod harness;
use harness::{NativeHandoffHarness, ACTIVE, ARCHIVE};

#[test]
fn handoff_accepts_completed_reviewed_shared_scope() {
    // Direct calls retain the native public handoff seam in the static index.
    let fixture = NativeHandoffHarness::new().unwrap();
    let root = fixture.root();
    let base = fixture.base.as_str();
    let mut landings = Vec::new();
    for (index, spec) in fixture.milestone.tasks.iter().enumerate() {
        let id = fixture.milestone.task_id(spec);
        fixture.write_shared_value(index + 1).unwrap();
        fixture.complete_task(&id).unwrap();
        landings.push((id, fixture.head().unwrap()));
    }
    let before = fixture.file_bytes(ACTIVE).unwrap();
    let context = handoff_context(root, "m-native-shared", base).unwrap();
    let evidence = NativeHandoffHarness::evidence(
        &context,
        base,
        NativeHandoffHarness::unobserved_verification("cargo test --test acdd", "check_full_repo"),
    )
    .unwrap();
    let missing_trace = handoff(root, "m-native-shared", None, &evidence).unwrap_err();
    assert!(missing_trace
        .to_string()
        .contains("HANDOFF_VERIFICATION_REQUIRED"));
    let observed = execute_milestone_verification(root, "m-native-shared", base).unwrap();
    let check = &observed["verification"];
    let evidence = NativeHandoffHarness::evidence(
        &context,
        base,
        NativeHandoffHarness::observed_verification(check).unwrap(),
    )
    .unwrap();
    let failed_check = fixture.fail_verification(&observed).unwrap();
    assert_eq!(failed_check.exit_code, Some(1));
    assert!(!failed_check.passed);
    let stale_success = handoff(root, "m-native-shared", None, &evidence).unwrap_err();
    assert!(stale_success
        .to_string()
        .contains("HANDOFF_VERIFICATION_REQUIRED"));

    let observed = execute_milestone_verification(root, "m-native-shared", base).unwrap();
    let check = &observed["verification"];
    assert_eq!(check["exit_code"], 0);

    // Changing code after verification creates a new candidate commit that must be re-verified.
    fixture.write_shared_value(42).unwrap();
    fixture
        .commit_shared_change("unverified edit after passing verification")
        .unwrap();
    let new_context = handoff_context(root, "m-native-shared", base).unwrap();
    let unverified_evidence = NativeHandoffHarness::evidence(
        &new_context,
        base,
        NativeHandoffHarness::unobserved_verification(
            check["command"].as_str().unwrap(),
            check["command_id"].as_str().unwrap(),
        ),
    )
    .unwrap();
    let candidate_mismatch_err =
        handoff(root, "m-native-shared", None, &unverified_evidence).unwrap_err();
    assert!(candidate_mismatch_err
        .to_string()
        .contains("HANDOFF_VERIFICATION_REQUIRED"));

    // Once verified on the actual current candidate, handoff succeeds.
    let observed_current = execute_milestone_verification(root, "m-native-shared", base).unwrap();
    assert_eq!(observed_current["status"], "passed", "{observed_current}");
    let check_current = &observed_current["verification"];
    assert_eq!(check_current["exit_code"], 0);
    let evidence = NativeHandoffHarness::evidence(
        &new_context,
        base,
        NativeHandoffHarness::observed_verification(check_current).unwrap(),
    )
    .unwrap();
    let result = handoff(root, "m-native-shared", None, &evidence);
    if result.is_err() {
        assert_eq!(fixture.file_bytes(ACTIVE).unwrap(), before);
        assert!(!fixture.exists(ARCHIVE));
    }
    result.expect("a later completed reviewed task may change an earlier task's shared path");
    let reopened = fixture.store().unwrap();
    let archived_doc = fixture.document(ARCHIVE).unwrap();
    let parsed = crate::core::tasks::Milestone::parse(&archived_doc, "forge-mcp").unwrap();
    assert!(reopened.list(None, "all", None).unwrap().is_empty());
    for table in [
        "gate_checkpoints",
        "gate_execution_traces",
        "gate_transitions",
    ] {
        let count = fixture.telemetry_rows(table).unwrap();
        assert_eq!(count, 0, "{table} should be pruned at handoff");
    }
    for (index, (_, landed)) in landings.iter().enumerate() {
        assert_eq!(
            parsed.tasks[index]
                .receipt
                .as_ref()
                .unwrap()
                .commit
                .as_deref(),
            Some(landed.as_str())
        );
    }
    assert!(!fixture.exists(ACTIVE));
}
