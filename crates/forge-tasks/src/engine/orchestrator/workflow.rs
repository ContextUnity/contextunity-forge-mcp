use super::role::RoleObservations;
use crate::{
    engine::gate_graph,
    milestones::command_gate::{observe_command, resolve_command_plan},
    tasks::profile::{
        validate_profile_document, GateDef, GateDriver, GateProfile, PersistenceAction, ProofKind,
    },
};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// Discover standalone workflow documents without executing gates or opening a store.
pub fn list_workflows(root: &Path) -> Result<Vec<PathBuf>> {
    let directory = root.join("docs/workflows");
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let directory = directory.canonicalize()?;
    ensure!(
        directory.starts_with(root.canonicalize()?),
        "WORKFLOW_PATH_INVALID: workflow directory escapes root"
    );
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("yaml" | "yml")
        ) {
            let canonical = path.canonicalize()?;
            ensure!(
                canonical.starts_with(root.canonicalize()?),
                "WORKFLOW_PATH_INVALID: workflow escapes root"
            );
            if canonical.is_file() {
                paths.push(path.strip_prefix(root)?.to_owned());
            }
        }
    }
    paths.sort();
    Ok(paths)
}

/// Select one discovered workflow; ambiguous names and arbitrary paths fail closed.
pub fn resolve_workflow(root: &Path, selector: &str) -> Result<PathBuf> {
    let paths = list_workflows(root)?;
    let matches = paths
        .into_iter()
        .filter(|path| {
            path.to_str() == Some(selector)
                || path.file_stem().and_then(|name| name.to_str()) == Some(selector)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [path] => Ok(path.clone()),
        [] => bail!("WORKFLOW_NOT_FOUND: {selector}"),
        _ => bail!("WORKFLOW_AMBIGUOUS: {selector}"),
    }
}

/// Read and validate the source through the canonical profile parser.
pub fn read_workflow(root: &Path, path: &Path) -> Result<GateProfile> {
    let canonical = root.join(path).canonicalize()?;
    ensure!(
        canonical.starts_with(root.canonicalize()?),
        "WORKFLOW_PATH_INVALID: workflow escapes root"
    );
    let profile = validate_profile_document(&fs::read_to_string(canonical)?)?;
    ensure!(
        profile.persistence == Some(PersistenceAction::None),
        "WORKFLOW_SAVE_INVALID: standalone workflows require save: none"
    );
    fn validate_taskless(gates: &[GateDef]) -> Result<()> {
        for gate in gates {
            ensure!(
                gate.tasks.is_none(),
                "WORKFLOW_TASKS_INVALID: standalone workflows cannot contain tasks"
            );
            ensure!(
                [gate.persistence.pre, gate.persistence.post]
                    .into_iter()
                    .all(|save| save.is_none_or(|save| save == PersistenceAction::None)),
                "WORKFLOW_SAVE_INVALID: standalone workflow gates require save: none"
            );
            validate_taskless(&gate.gates)?;
        }
        Ok(())
    }
    validate_taskless(&profile.gates)?;
    for template in profile.gate_templates.values() {
        validate_taskless(&template.gates)?;
    }
    Ok(profile)
}

/// Execute root gates with ephemeral observations and the existing graph router.
pub fn run_standalone_workflow(root: &Path, path: &Path) -> Result<Value> {
    let profile = read_workflow(root, path)?;
    let cursors = gate_graph::collect_root_cursors(&profile)?;
    ensure!(
        !cursors.is_empty(),
        "WORKFLOW_GATES_EMPTY: no executable root gates"
    );
    // Validate the entire execution capability before the first side effect.
    // Agent gates need accepted structured decisions and cannot be inferred from
    // a process exit code.
    for cursor in &cursors {
        match cursor.gate.driver() {
            Some(GateDriver::Agent) => {
                ensure!(cursor.gate.role.as_ref().and_then(|role| profile.roles.get(role)).and_then(crate::tasks::profile::RunnerConfig::from_role).is_some(), "WORKFLOW_RUNNER_REQUIRED: gate '{}' needs a configured runner", cursor.canonical_stage_id);
                ensure!(!cursor.gate.receipt, "WORKFLOW_PROOF_REQUIRED: taskless workflow cannot create a task receipt");
            }
            Some(GateDriver::Command) => ensure!(matches!(cursor.gate.proof.kind(), Some(ProofKind::None | ProofKind::Command)) && cursor.gate.expected_failure.is_none() && cursor.gate.independent_from.is_none() && cursor.gate.policy.is_none(), "WORKFLOW_PROOF_REQUIRED: gate '{}' needs typed proof handling beyond command execution", cursor.canonical_stage_id),
            _ => anyhow::bail!("WORKFLOW_RUNNER_REQUIRED: gate '{}' has no executable driver", cursor.canonical_stage_id),
        }
        ensure!(
            [cursor.gate.persistence.pre, cursor.gate.persistence.post]
                .into_iter()
                .all(|save| save.is_none_or(|save| save == PersistenceAction::None)),
            "WORKFLOW_SAVE_INVALID: gate cannot persist standalone workflow state"
        );
    }
    let paths = vec![path.to_string_lossy().into_owned()];
    let mut roles = RoleObservations::default();
    let outcomes = run_gate_graph(root, &paths, &cursors, |cursor, _| {
        if cursor.gate.driver() == Some(GateDriver::Agent) {
            return roles.execute(
                root,
                &path.to_string_lossy(),
                &format!("Execute standalone workflow {}", path.display()),
                &paths,
                cursor,
                &profile,
            );
        }
        let name = cursor
            .gate
            .command
            .as_deref()
            .context("WORKFLOW_COMMAND_MISSING")?;
        let definition = profile
            .commands
            .get(name)
            .context("WORKFLOW_COMMAND_MISSING")?;
        let plan = resolve_command_plan(root, name, definition, &paths, &profile)?;
        ensure!(
            !plan.is_empty(),
            "WORKFLOW_COMMAND_MISSING: no command matched gate '{name}'"
        );
        let mut passed = true;
        let mut observations = Vec::new();
        for (command_id, command, timeout) in plan {
            let observation = observe_command(root, &command, timeout, definition.output())?;
            passed &= observation["exit_code"] == 0 && observation["timed_out"] == false;
            observations.push(json!({"command_id": command_id, "observation": observation}));
            if !passed {
                break;
            }
        }
        Ok(GateDecision {
            passed,
            evidence: json!(observations),
            findings: None,
        })
    })?;
    Ok(json!({"status":"completed", "path":path, "outcomes":outcomes}))
}

pub(super) struct GateDecision {
    pub passed: bool,
    pub evidence: Value,
    pub findings: Option<Value>,
}

/// Route subject-neutral observations through the canonical gate cursor graph.
pub(super) fn run_gate_graph(
    root: &Path,
    paths: &[String],
    cursors: &[gate_graph::GateCursor],
    mut execute: impl FnMut(&gate_graph::GateCursor, u32) -> Result<GateDecision>,
) -> Result<Vec<Value>> {
    let applicable = |cursor: &gate_graph::GateCursor| {
        gate_graph::gate_condition_matches(cursor.when.as_deref(), paths, &[], None, Some(root))
    };
    let mut next = cursors.iter().position(applicable);
    let mut attempts = BTreeMap::<usize, u32>::new();
    let mut outcomes = Vec::new();
    while let Some(index) = next {
        let cursor = &cursors[index];
        let used = attempts.entry(index).or_default();
        *used += 1;
        ensure!(
            *used <= cursor.gate.max_attempts.unwrap_or(1),
            "WORKFLOW_ATTEMPTS_EXHAUSTED: {}",
            cursor.canonical_stage_id
        );
        let decision = execute(cursor, *used)?;
        outcomes.push(json!({"gate":cursor.canonical_stage_id,"attempt":used,"passed":decision.passed,"evidence":decision.evidence}));
        next = if decision.passed {
            gate_graph::resolve_pass_target(cursors, index, applicable)?
        } else {
            let route = gate_graph::resolve_fail_target(
                cursors,
                index,
                decision.findings.as_ref(),
                *used,
                applicable,
            )?;
            ensure!(
                !route.exhausted,
                "WORKFLOW_ATTEMPTS_EXHAUSTED: {}",
                cursor.canonical_stage_id
            );
            let target = route
                .target
                .context("WORKFLOW_COMMAND_FAILED: no failure route")?;
            Some(
                cursors
                    .iter()
                    .position(|cursor| cursor.canonical_stage_id == target.canonical_stage_id)
                    .context("WORKFLOW_ROUTE_INVALID")?,
            )
        };
    }
    Ok(outcomes)
}
