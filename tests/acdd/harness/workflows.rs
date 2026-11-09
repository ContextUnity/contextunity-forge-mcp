use crate::acdd::support::Workspace;
use contextunity_forge_mcp::forge_tasks::tasks::profile::{self, GateProfile};

/// Shared root-gate profile construction for ADR 0021 workflow fixtures.
/// Profile validation does not prove standalone workflow dispatch or execution.
pub(crate) struct WorkflowHarness {
    pub profile: GateProfile,
    source: &'static str,
    path: &'static str,
}

impl WorkflowHarness {
    pub fn simple() -> Self {
        Self::gate_profile(
            "docs/workflows/simple.yaml",
            include_str!("../fixtures/workflows/simple.yaml"),
        )
    }

    pub fn rejection_routing() -> Self {
        Self::gate_profile(
            "docs/workflows/rejection_routing.yaml",
            include_str!("../fixtures/workflows/rejection_routing.yaml"),
        )
    }

    pub fn ordered() -> Self {
        Self::gate_profile(
            "docs/workflows/ordered.yaml",
            include_str!("../fixtures/workflows/ordered.yaml"),
        )
    }

    pub fn retry() -> Self {
        Self::gate_profile(
            "docs/workflows/retry.yaml",
            include_str!("../fixtures/workflows/retry.yaml"),
        )
    }

    pub fn failing() -> Self {
        Self::gate_profile(
            "docs/workflows/failing.yaml",
            include_str!("../fixtures/workflows/failing.yaml"),
        )
    }

    pub fn timeout() -> Self {
        Self::gate_profile(
            "docs/workflows/timeout.yaml",
            include_str!("../fixtures/workflows/timeout.yaml"),
        )
    }

    pub fn agents() -> Self {
        Self::gate_profile(
            "docs/workflows/agents.yaml",
            include_str!("../fixtures/workflows/agents.yaml"),
        )
    }

    pub fn materialize(&self) -> Workspace {
        let root = Workspace::new();
        self.write_to(&root);
        root
    }

    pub fn write_to(&self, root: &Workspace) {
        root.write(self.path, self.source);
        root.write(
            "workflow-runner.py",
            include_str!("../fixtures/workflows/runner.py"),
        );
    }

    fn gate_profile(path: &'static str, source: &'static str) -> Self {
        Self {
            profile: profile::validate_profile_document(source)
                .expect("canonical workflow gate profile must validate"),
            source,
            path,
        }
    }
}

#[test]
fn simple_workflow_resolves_registered_commands_in_a_taskless_gate_profile() {
    let harness = WorkflowHarness::simple();
    let profile = &harness.profile;
    assert_eq!(profile.persistence, Some(profile::PersistenceAction::None));
    assert_eq!(
        profile
            .gates
            .iter()
            .map(|g| g.id.as_str())
            .collect::<Vec<_>>(),
        ["lint", "test"]
    );
    for gate in &profile.gates {
        assert!(gate.tasks.is_none());
        assert_eq!(gate.driver(), Some(profile::GateDriver::Command));
        assert!(profile
            .commands
            .contains_key(gate.command.as_ref().unwrap()));
    }
}

#[test]
fn rejection_workflow_resolves_roles_and_failure_target_in_its_gate_profile() {
    let harness = WorkflowHarness::rejection_routing();
    let profile = &harness.profile;
    assert_eq!(profile.persistence, Some(profile::PersistenceAction::None));
    assert_eq!(
        profile
            .gates
            .iter()
            .map(|g| g.id.as_str())
            .collect::<Vec<_>>(),
        ["build", "review"]
    );
    for gate in &profile.gates {
        assert!(gate.tasks.is_none());
        assert_eq!(gate.driver(), Some(profile::GateDriver::Agent));
        assert!(profile.roles.contains_key(gate.role.as_ref().unwrap()));
    }
    let review = profile.by_stage("review").unwrap();
    assert_eq!(review.on_fail.as_deref(), Some("build"));
    assert_eq!(review.independent_from.as_deref(), Some("build"));
}
