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

#[test]
fn authoring_workflow_profiles_validate_cleanly_and_enforce_standalone_authoring_invariants() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let expected_commands = [
        (
            "docs/workflows/author-milestone.workflow.yaml",
            "skills/contextunity-forge/references/acdd-profile-examples/author-milestone.workflow.yaml",
            "contextunity-forge-mcp milestone init {manifest_path} --in-place",
        ),
        (
            "docs/workflows/author-plan.workflow.yaml",
            "skills/contextunity-forge/references/acdd-profile-examples/author-plan.workflow.yaml",
            "contextunity-forge-mcp plan show {plan_name}",
        ),
        (
            "docs/workflows/author-workflow.workflow.yaml",
            "skills/contextunity-forge/references/acdd-profile-examples/author-workflow.workflow.yaml",
            "contextunity-forge-mcp workflow show {workflow_name}",
        ),
    ];

    for (path, skill_path, expected_cmd) in expected_commands {
        let full_path = repo_root.join(path);
        let content = std::fs::read_to_string(&full_path)
            .unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
        let skill_content = std::fs::read_to_string(repo_root.join(skill_path))
            .unwrap_or_else(|e| panic!("failed to read {skill_path}: {e}"));
        assert_eq!(
            content, skill_content,
            "skill example {skill_path} drifted from {path}"
        );
        let profile = profile::validate_profile_document(&content)
            .unwrap_or_else(|e| panic!("profile in {path} failed validation: {e}"));

        // Invariant 1: Standalone workflows require inherit: none and save: none
        assert_eq!(
            profile.persistence,
            Some(profile::PersistenceAction::None),
            "{path} must have save: none"
        );

        // Invariant 2: Pure root gate graph without tasks: container or receipts
        assert!(
            profile.gates.iter().all(|g| g.tasks.is_none()),
            "{path} must not contain tasks"
        );
        assert!(
            profile.gates.iter().all(|g| !g.receipt),
            "{path} must not declare task receipts"
        );

        // Invariant 3: Three-stage sequence (synthesize -> review -> preflight)
        let gate_ids: Vec<&str> = profile.gates.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(gate_ids, ["synthesize", "review", "preflight"], "{path}");

        // Invariant 4: Roles architect and independent_reviewer configured with tools, models, and executable runners
        let synthesize = profile.find_gate("synthesize").unwrap();
        assert_eq!(synthesize.role.as_deref(), Some("architect"));
        assert!(
            !synthesize.tools.is_empty(),
            "{path} synthesize needs tools"
        );
        assert!(
            matches!(synthesize.proof, profile::ProofDef::Scheme { .. }),
            "{path} synthesize needs inline proof.scheme"
        );

        let review = profile.find_gate("review").unwrap();
        assert_eq!(review.role.as_deref(), Some("independent_reviewer"));
        assert_eq!(review.independent_from.as_deref(), Some("synthesize"));
        assert_eq!(review.on_fail.as_deref(), Some("synthesize"));
        assert_eq!(review.on_success.as_deref(), Some("preflight"));
        assert!(
            matches!(review.proof, profile::ProofDef::Scheme { .. }),
            "{path} review needs inline proof.scheme"
        );

        for gate in [synthesize, review] {
            let role_id = gate.role.as_deref().unwrap();
            let role = profile.roles.get(role_id).expect("gate role must exist");
            assert_eq!(
                role.reuse_on_reject,
                Some(true),
                "{path} role {role_id} must set reuse_on_reject"
            );
            assert!(
                profile::RunnerConfig::from_role(role).is_some(),
                "{path} role {role_id} must define an executable runner"
            );
        }

        let workflow_stem = std::path::Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".workflow.yaml"))
            .unwrap();
        let resolved =
            contextunity_forge_mcp::forge_tasks::engine::orchestrator::workflow::resolve_workflow(
                repo_root,
                workflow_stem,
            )
            .unwrap_or_else(|e| panic!("failed to resolve workflow {workflow_stem}: {e}"));
        assert_eq!(resolved, std::path::Path::new(path));

        let preflight = profile.find_gate("preflight").unwrap();
        let command_name = preflight.command.as_ref().expect("preflight command name");
        let command_def = profile
            .commands
            .get(command_name)
            .expect("preflight command definition");
        assert_eq!(
            command_def.command_for_path(""),
            Some(expected_cmd),
            "{path}"
        );
    }
}
