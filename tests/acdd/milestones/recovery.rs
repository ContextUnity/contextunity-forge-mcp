use super::*;
use std::os::unix::process::ExitStatusExt;

const BUSY: &str = "forge-mcp/m-wait:busy";
const NEXT: &str = "forge-mcp/m-wait:next";
const CLOSED: &str = "forge-mcp/m-close:__milestone__";

#[test]
fn milestone_handoff_recovers_crash_at_each_journal_phase() {
    if let Some(marker) = std::env::var_os(HANDOFF_CRASH_WORKER) {
        assert_eq!(marker, "1", "unexpected crash worker marker");
        let root = std::path::PathBuf::from(std::env::var_os(HANDOFF_CRASH_ROOT).unwrap());
        let milestone = std::env::var(HANDOFF_CRASH_MILESTONE).unwrap();
        let commit = std::env::var(HANDOFF_CRASH_COMMIT).ok();
        let evidence = read_fixture_evidence(&std::env::var(HANDOFF_CRASH_EVIDENCE).unwrap());
        contextunity_forge_mcp::mcp::tasks::ensure_extractors();
        let result = milestone_engine::handoff(&root, &milestone, commit.as_deref(), &evidence)
            .unwrap_or_else(|error| panic!("crash worker failed before SIGKILL: {error:#}"));
        panic!("crash worker completed before SIGKILL: {result:?}");
    }
    let root =
        MilestoneHarness::from_catalog(include_str!("../fixtures/milestones/crash_recovery.json"));
    commit_handoff_fixture_inputs(&root, &["forge-mcp.yaml", "src", "docs"]);
    for path in [
        "docs/milestones/010-close.md",
        "docs/milestones/011-wait.md",
    ] {
        root.sync_document(path, &root.document(path));
    }
    let evidence_file =
        fixture_evidence_file(root.root(), "m-close", "cargo test --test acdd", 1, 0);
    root.claim_task(BUSY, "contract", "busy-worker").unwrap();
    let busy = {
        let store = root.store();
        MilestoneHarness::task_snapshot(&store, BUSY)
    };
    let seed_snapshot = root.checkpoint();

    for (phase, interrupted_tasks) in [
        (HandoffCrashPhase::Enrolled, 4),
        (HandoffCrashPhase::Unlinked, 4),
        (HandoffCrashPhase::Committed, 2),
    ] {
        seed_snapshot.restore(&root);
        let store = root.store();
        if phase == HandoffCrashPhase::Unlinked {
            MilestoneHarness::install_pruning_fault(&store, PruningFault::Delay);
        }
        let outcome = handoff_with_journal_capture(&root, "m-close", None, &evidence_file, phase);
        let status = &outcome.output.status;
        let rows = crate::common::sqlite::query_value::<i64>(
            &store.connection,
            "SELECT count(*) FROM tasks",
            [],
        );
        assert_eq!(
            (
                status.signal(),
                status.success(),
                outcome.journals.len(),
                rows
            ),
            (Some(9), false, 1, interrupted_tasks),
            "{phase}: {}",
            String::from_utf8_lossy(&outcome.output.stderr)
        );
        let (journal_path, _) = outcome.journals.into_iter().next().unwrap();
        assert!(journal_path.exists(), "{phase}");
        if phase == HandoffCrashPhase::Unlinked {
            assert_eq!(
                (outcome.unlink_seen, outcome.exited_before_unlink),
                (Some(true), Some(false)),
                "{phase}"
            );
            MilestoneHarness::remove_pruning_fault(&store, PruningFault::Delay);
        }
        let evidence = read_fixture_evidence(&evidence_file);
        root.handoff_with_evidence("m-close", &evidence)
            .unwrap_or_else(|error| panic!("{phase}: {error:#}"));
        assert!(!journal_path.exists(), "{phase}: journal leaked");
        let remaining = store.list(None, "all", None).unwrap();
        let busy_after = MilestoneHarness::task_snapshot(&store, BUSY);
        let satisfied = MilestoneHarness::dependency_satisfied(&store, NEXT, CLOSED);
        let recovered = (remaining.len(), busy_after, satisfied);
        assert_eq!(recovered, (2, busy.clone(), 1), "{phase}");
        let retried = root.handoff_with_evidence("m-close", &evidence);
        assert!(retried.is_ok(), "{phase}: repeat retry");
        drop(store);
    }
}
