//! Thin command-gate harness: one S0 runner, in-process engine, CLI/MCP smokes.
//!
//! `precompile_runner` writes `.forge/runner_fixture` once. Gate execution uses
//! that binary. Red/green stay `FORGE_FIXTURE_RED` / `runner_fixture_green`.
//! `#[test]` owns mutations and assertions.

use super::support::fixture;
use contextunity_forge_mcp::core::tasks::profile::{self, CommandDef, ExpectedFailure};
use contextunity_forge_mcp::db::tasks_store::TasksStore;
use contextunity_forge_mcp::engine::milestones::execute_command_gate;

/// Execute the process-wide precompiled S0 runner (green: no `FORGE_FIXTURE_RED`).
pub(super) const S0_RUNNER: &str = "test -x .forge/runner_fixture && .forge/runner_fixture";

/// S0 binary is present but not executed; only unstructured counts are printed.
pub(super) const S0_RUNNER_COUNTS_ONLY: &str =
    "test -x .forge/runner_fixture && printf 'test result: ok. 1 passed; 0 failed\\n'";

pub(super) struct CommandGateHarness {
    pub root: crate::common::Workspace,
    pub id: String,
    baseline: super::snapshot::FixtureSnapshot,
    pub executed: usize,
}

impl CommandGateHarness {
    pub fn new() -> Self {
        Self::with_command("true", Some(output_spec(false)))
    }

    pub fn with_command(command: &str, output: Option<profile::CommandOutputSpec>) -> Self {
        Self::with_proof(command, output, profile::ProofKind::Command)
    }

    pub fn with_proof(
        command: &str,
        output: Option<profile::CommandOutputSpec>,
        proof: profile::ProofKind,
    ) -> Self {
        let (root, mut store, milestone) = fixture(super::support::DEFAULT_SEAM);
        root.write(".gitignore", ".forge/\n");
        let id = milestone.task_id(&milestone.tasks[0]);
        let mut configured = (*profile::compiled()).clone();
        let output = output.or_else(|| {
            configured
                .commands
                .get("rust-check-and-test")
                .and_then(|command| command.output().cloned())
        });
        configured.commands.insert(
            "check".into(),
            CommandDef::Detailed {
                command: command.into(),
                timeout_seconds: Some(5),
                description: None,
                scope_args_by_pattern: Default::default(),
                output,
            },
        );
        let gate = configured.find_gate_mut("contract").unwrap();
        gate.command = Some("check".into());
        gate.proof = profile::ProofDef::Kind(proof);
        gate.expected_failure = None;
        root.write(
            ".forge/acdd/profile.yaml",
            &serde_yaml::to_string(&configured).unwrap(),
        );
        store
            .claim(
                &id,
                "contract",
                "command-builder",
                root.root().to_str().unwrap(),
                Some("test-session-command-builder"),
            )
            .unwrap();
        drop(store);
        let baseline = super::snapshot::FixtureSnapshot::capture(&root);
        Self {
            root,
            id,
            baseline,
            executed: 0,
        }
    }

    /// Claimed SeamTest gate whose check command is the precompiled S0 binary.
    pub fn seam_s0() -> Self {
        let harness = Self::with_proof(S0_RUNNER, None, profile::ProofKind::SeamTest);
        debug_assert!(
            harness.root.root().join(".forge/runner_fixture").is_file(),
            "S0 runner must already be precompiled"
        );
        harness
    }

    pub fn restore(&self) {
        self.baseline.restore(&self.root);
    }

    pub fn recapture(&mut self) {
        self.baseline = super::snapshot::FixtureSnapshot::capture(&self.root);
    }

    pub fn configure_check(
        &self,
        command: &str,
        output: Option<profile::CommandOutputSpec>,
        proof: profile::ProofKind,
    ) {
        self.configure_check_with_timeout(command, output, proof, 30);
    }

    pub fn configure_check_with_timeout(
        &self,
        command: &str,
        output: Option<profile::CommandOutputSpec>,
        proof: profile::ProofKind,
        timeout_seconds: u64,
    ) {
        let mut configured = (*profile::load_for_workspace(self.root.root()).unwrap()).clone();
        let output = output.or_else(|| {
            configured
                .commands
                .get("check")
                .and_then(|command| command.output().cloned())
                .or_else(|| {
                    configured
                        .commands
                        .get("rust-check-and-test")
                        .and_then(|command| command.output().cloned())
                })
        });
        configured.commands.insert(
            "check".into(),
            CommandDef::Detailed {
                command: command.into(),
                timeout_seconds: Some(timeout_seconds),
                description: None,
                scope_args_by_pattern: Default::default(),
                output,
            },
        );
        let gate = configured.find_gate_mut("contract").unwrap();
        gate.command = Some("check".into());
        gate.proof = profile::ProofDef::Kind(proof);
        gate.expected_failure = None;
        self.root.write(
            ".forge/acdd/profile.yaml",
            &serde_yaml::to_string(&configured).unwrap(),
        );
    }

    pub fn configure_expected_failure(&self, expected_failure: ExpectedFailure) {
        let mut configured = (*profile::load_for_workspace(self.root.root()).unwrap()).clone();
        configured
            .find_gate_mut("contract")
            .unwrap()
            .expected_failure = Some(expected_failure);
        self.root.write(
            ".forge/acdd/profile.yaml",
            &serde_yaml::to_string(&configured).unwrap(),
        );
    }

    pub fn store(&self) -> TasksStore {
        forge_tasks::engine::store_for_task(self.root.root(), &self.id).unwrap()
    }

    pub fn execute_check(
        &self,
    ) -> anyhow::Result<Vec<contextunity_forge_mcp::db::tasks_store::GateExecutionTrace>> {
        let store = self.store();
        let task = store.inspect(&self.id)?;
        let configured = store.profile_for_task(&task)?;
        let command_name = configured
            .find_gate("contract")
            .and_then(|gate| gate.command.as_deref())
            .ok_or_else(|| anyhow::anyhow!("no command configured for tasks.contract"))?;
        let command = configured.commands[command_name].clone();
        drop(store);
        execute_command_gate(
            self.root.root(),
            &self.id,
            "tasks.contract",
            &command,
            &["src/".into()],
        )
    }

    pub fn verify(&self) -> anyhow::Result<serde_json::Value> {
        forge_tasks::engine::verify_task(self.root.root(), &self.id, None)
    }
}

pub(super) fn output_spec(require_counts: bool) -> profile::CommandOutputSpec {
    profile::CommandOutputSpec {
        require_counts,
        counts: Some(profile::CommandCountsSpec {
            passed: r"PASS=(\d+)".into(),
            failed: r"FAIL=(\d+)".into(),
        }),
        ..Default::default()
    }
}
