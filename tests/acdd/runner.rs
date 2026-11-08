//! Contract seams for the AC-001 transition engine and command runner.

pub(super) use super::command_gate_harness::output_spec;
use super::command_gate_harness::{CommandGateHarness, S0_RUNNER, S0_RUNNER_COUNTS_ONLY};
use super::support::fixture;
use super::{mcp, support};
use contextunity_forge_mcp::core::tasks::profile::{
    self, CommandDef, ExpectedFailure, PatternCommands, RejectRouting,
};
use contextunity_forge_mcp::db::tasks_store::{
    ClaimBoundExecutionRun, GateExecutionTrace, TasksStore,
};
use contextunity_forge_mcp::engine::milestones::execute_command_gate;
use contextunity_forge_mcp::engine::tasks::compile_evidence_envelope;
use contextunity_forge_mcp::engine::tasks::evaluate_reject_routing;
use contextunity_forge_mcp::engine::tasks::rebase_speculative_dependency;
use contextunity_forge_mcp::engine::tasks::submit_zero_arg;
use contextunity_forge_mcp::engine::tasks::telemetry_passport_note;
use contextunity_forge_mcp::engine::tasks::verify_red_to_green;
use contextunity_forge_mcp::engine::tasks::Action;
use forge_tasks::tasks::profile::parse_profile;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

pub(super) fn command_fixture(
    command: &str,
    output: Option<profile::CommandOutputSpec>,
) -> (crate::common::Workspace, TasksStore, String) {
    let harness = CommandGateHarness::with_command(command, output);
    let store = harness.store();
    (harness.root, store, harness.id)
}

fn submit_unobserved_pass(
    store: &mut TasksStore,
    evidence: &super::support::Evidence,
) -> anyhow::Result<()> {
    let session = format!("test-session-{}", evidence.worker_id);
    store
        .submit(
            &evidence.task_id,
            &evidence.stage,
            evidence,
            "pass",
            None,
            Some(&session),
        )
        .map(|_| ())
}

fn unobserved_task_evidence(
    store: &TasksStore,
    task_id: &str,
) -> (
    contextunity_forge_mcp::db::tasks_store::Task,
    super::support::Evidence,
) {
    let task = store.inspect(task_id).unwrap();
    let evidence = super::support::unobserved_evidence(&task);
    (task, evidence)
}
fn write_cargo_seam_project(root: &crate::common::Workspace) {
    root.write("Cargo.toml", "[package]\nname = \"seam-proof-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[[test]]\nname = \"observed\"\npath = \"tests/runner_fixture.rs\"\n");
    root.write(".gitignore", ".forge/\ntarget/\nCargo.lock\n");
}

fn write_member_seam_project(root: &crate::common::Workspace) {
    root.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
    );
    root.write("member/Cargo.toml", "[package]\nname = \"proof-member\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[[test]]\nname = \"observed\"\npath = \"tests/runner_fixture.rs\"\n");
    root.write(
        "member/tests/runner_fixture.rs",
        "#[test] fn seam_test() {}\n",
    );
    root.write(".gitignore", ".forge/\ntarget/\nCargo.lock\n");
}
fn latest_run(
    store: &TasksStore,
    root: &Path,
    task_id: &str,
    gate_path: &str,
) -> ClaimBoundExecutionRun {
    let task = store.inspect(task_id).unwrap();
    let worker = task.worker_id.as_deref().unwrap();
    let worktree = Path::new(task.worktree.as_deref().unwrap())
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(Path::new(&worktree), root.canonicalize().unwrap());
    store
        .latest_claim_bound_execution_run(
            task_id,
            gate_path,
            task.claim_revision,
            task.contract_revision,
            worker,
            &worktree,
        )
        .unwrap()
        .unwrap()
}

#[path = "runner/command_gate_proofs.rs"]
mod command_gate_proofs;
#[path = "runner/command_verification.rs"]
mod command_verification;
#[path = "runner/output_contracts.rs"]
mod output_contracts;
#[path = "runner/transition_runner.rs"]
mod transition_runner;
