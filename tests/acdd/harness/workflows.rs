use contextunity_forge_mcp::forge_tasks::tasks::profile::{self, GateProfile};

/// Shared root-gate profile construction for ADR 0021 workflow fixtures.
/// Profile validation does not prove standalone workflow dispatch or execution.
pub(crate) struct WorkflowHarness {
    pub profile: GateProfile,
}

impl WorkflowHarness {
    pub fn simple() -> Self {
        Self::gate_profile(include_str!("../fixtures/workflows/simple.yaml"))
    }

    pub fn rejection_routing() -> Self {
        Self::gate_profile(include_str!("../fixtures/workflows/rejection_routing.yaml"))
    }

    fn gate_profile(source: &str) -> Self {
        Self {
            profile: profile::validate_profile_document(source)
                .expect("canonical workflow gate profile must validate"),
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
