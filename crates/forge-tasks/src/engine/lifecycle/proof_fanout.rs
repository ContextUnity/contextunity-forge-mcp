use super::*;

pub(in crate::engine) fn milestone_identity_for_task(
    workspace: &workspaces::Workspace,
    task: &Task,
) -> Result<String> {
    let milestone_prefix = task
        .task_id
        .split_once(':')
        .map(|(milestone, _)| milestone)
        .context("TASK_ID_INVALID: task identity has no milestone key")?;
    let milestone_key = milestone_prefix
        .rsplit('/')
        .next()
        .filter(|key| !key.trim().is_empty())
        .context("TASK_ID_INVALID: task identity has no milestone key")?;
    Ok(format!(
        "{}/{}/{}",
        workspace.repository, workspace.project, milestone_key
    ))
}

pub(super) fn milestone_task_scope(tasks: &[Task]) -> Vec<String> {
    tasks
        .iter()
        .flat_map(|milestone_task| milestone_task.spec.scope.iter().cloned())
        .collect()
}

pub(super) fn applicable_pre_task_root_cursors(
    profile: &crate::tasks::profile::GateProfile,
    task: &Task,
    milestone_tasks: &[Task],
    scope: &[String],
    worktree: &Path,
) -> Result<Vec<gate_graph::GateCursor>> {
    let active_tasks = milestone_tasks
        .iter()
        .filter(|entry| entry.status != "cancelled")
        .cloned()
        .collect::<Vec<_>>();
    Ok(gate_graph::pre_task_root_cursors(profile, task)?
        .into_iter()
        .filter(|cursor| {
            gate_graph::gate_condition_matches(
                cursor.when.as_deref(),
                scope,
                &active_tasks,
                None,
                Some(worktree),
            )
        })
        .collect())
}

pub(super) fn root_proof_with_task_map(payload: &Value) -> Result<Option<&Value>> {
    fn find_task_map(value: &Value) -> Result<Option<&Value>> {
        if let Some(tasks) = value.get("tasks") {
            tasks.as_object().context(
                "MILESTONE_TASK_PROOF_INVALID: proof.tasks must be an object keyed by task_ref",
            )?;
            return Ok(Some(value));
        }
        match value {
            Value::Object(object) => {
                for nested in object.values() {
                    if let Some(proof) = find_task_map(nested)? {
                        return Ok(Some(proof));
                    }
                }
            }
            Value::Array(array) => {
                for nested in array {
                    if let Some(proof) = find_task_map(nested)? {
                        return Ok(Some(proof));
                    }
                }
            }
            _ => {}
        }
        Ok(None)
    }

    let candidates = [
        payload.pointer("/evidence/proof"),
        payload.get("proof"),
        payload.get("evidence"),
        Some(payload),
    ];
    for proof in candidates.into_iter().flatten() {
        if let Some(proof) = find_task_map(proof)? {
            return Ok(Some(proof));
        }
    }
    Ok(None)
}

pub(super) fn task_proof_slices(
    proof: &Value,
    milestone_tasks: &[Task],
    contract_matrix: &crate::tasks::MilestoneContractMatrix,
) -> Result<Vec<(String, Value)>> {
    let task_map = proof
        .get("tasks")
        .and_then(Value::as_object)
        .context("MILESTONE_TASK_PROOF_INVALID: proof.tasks must be an object")?;
    let mut tasks_by_ref = std::collections::BTreeMap::<&str, &Task>::new();
    for task in milestone_tasks {
        ensure!(
            tasks_by_ref
                .insert(task.spec.task_ref.as_str(), task)
                .is_none(),
            "MILESTONE_TASK_PROOF_INVALID: milestone contains duplicate task_ref '{}'",
            task.spec.task_ref
        );
    }
    for task_ref in task_map.keys() {
        ensure!(
            tasks_by_ref.contains_key(task_ref.as_str()),
            "MILESTONE_TASK_PROOF_INVALID: proof.tasks contains unknown task_ref '{task_ref}'"
        );
    }
    for task in milestone_tasks
        .iter()
        .filter(|task| task.status != "cancelled")
    {
        ensure!(
            task_map.contains_key(&task.spec.task_ref),
            "MILESTONE_TASK_PROOF_INVALID: proof.tasks is missing non-cancelled task_ref '{}'",
            task.spec.task_ref
        );
    }

    let slices = task_map
        .iter()
        .map(|(task_ref, slice)| {
            ensure!(
                slice.is_object(),
                "MILESTONE_TASK_PROOF_INVALID: task slice '{task_ref}' must be an object"
            );
            Ok((task_ref.clone(), slice))
        })
        .collect::<Result<std::collections::BTreeMap<_, _>>>()?;
    let active_refs = milestone_tasks
        .iter()
        .filter(|task| task.status != "cancelled")
        .map(|task| task.spec.task_ref.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    for (task_ref, task_slice) in &slices {
        let task = tasks_by_ref
            .get(task_ref.as_str())
            .copied()
            .context("MILESTONE_TASK_PROOF_INVALID: task slice owner is missing")?;
        validate_supplied_seam_edges(
            task,
            task_slice,
            &tasks_by_ref,
            &active_refs,
            contract_matrix,
        )?;
    }

    for consumer in milestone_tasks
        .iter()
        .filter(|task| task.status != "cancelled")
    {
        let consumer_slice = slices
            .get(&consumer.spec.task_ref)
            .context("MILESTONE_TASK_PROOF_INVALID: active task slice is missing")?;
        for dependency in &consumer.spec.depends_on {
            if !active_refs.contains(dependency.as_str()) {
                continue;
            }
            let provider_slice = slices
                .get(dependency)
                .context("MILESTONE_TASK_PROOF_INVALID: dependency slice is missing")?;
            ensure!(
                seam_entry_matches(
                    consumer_slice.get("incoming_seams"),
                    "source_task",
                    dependency,
                    None,
                    None,
                ) && seam_entry_matches(
                    provider_slice.get("outgoing_seams"),
                    "target_task",
                    &consumer.spec.task_ref,
                    None,
                    None,
                ),
                "MILESTONE_TASK_PROOF_INVALID: dependency seam '{} -> {}' is not represented in both task slices",
                dependency,
                consumer.spec.task_ref
            );
        }
    }

    for edge in &contract_matrix.entries {
        if !active_refs.contains(edge.source_task.as_str())
            || !active_refs.contains(edge.target_task.as_str())
        {
            continue;
        }
        let source_slice = slices
            .get(&edge.source_task)
            .context("MILESTONE_TASK_PROOF_INVALID: seam source slice is missing")?;
        let target_slice = slices
            .get(&edge.target_task)
            .context("MILESTONE_TASK_PROOF_INVALID: seam target slice is missing")?;
        let contract_type = serde_json::to_value(edge.contract_type)?;
        let contract_type = contract_type
            .as_str()
            .context("MILESTONE_CONTRACT_MATRIX_INVALID: contract_type must serialize as text")?;
        ensure!(
            seam_entry_matches(
                target_slice.get("incoming_seams"),
                "source_task",
                &edge.source_task,
                Some(contract_type),
                Some(&edge.contract_ref),
            ) && seam_entry_matches(
                source_slice.get("outgoing_seams"),
                "target_task",
                &edge.target_task,
                Some(contract_type),
                Some(&edge.contract_ref),
            ),
            "MILESTONE_TASK_PROOF_INVALID: contract seam '{} -> {} ({})' is not represented in both task slices",
            edge.source_task,
            edge.target_task,
            edge.contract_ref
        );
    }

    let shared_fields = [
        "cited_invariants",
        "cited_symbols",
        "global_negative_boundaries",
        "blackboard_decision_id",
    ];
    let mut result = Vec::new();
    for task in milestone_tasks
        .iter()
        .filter(|task| task.status != "cancelled")
    {
        let mut payload = slices
            .get(&task.spec.task_ref)
            .copied()
            .and_then(Value::as_object)
            .context("MILESTONE_TASK_PROOF_INVALID: task slice must be an object")?
            .clone();
        for field in shared_fields {
            if let Some(shared) = proof.get(field) {
                payload.insert(field.to_owned(), shared.clone());
            }
        }
        result.push((task.task_id.clone(), Value::Object(payload)));
    }
    Ok(result)
}

/// Validate a root gate's task map before the orchestrator records its pass.
/// The persisted proof must already cover every active task and admitted seam
/// so an invalid pass cannot become an unrecoverable replay checkpoint.
pub(in crate::engine) fn validate_pre_task_root_proof(
    store: &TasksStore,
    workspace: &workspaces::Workspace,
    task: &Task,
    payload: &Value,
) -> Result<()> {
    let Some(proof) = root_proof_with_task_map(payload)? else {
        return Ok(());
    };
    let milestone_tasks = store.list(Some(&task.milestone_ref), "all", None)?;
    let milestone_identity = milestone_identity_for_task(workspace, task)?;
    let contract_matrix = store.list_contract_matrix(&milestone_identity)?;
    let _ = task_proof_slices(proof, &milestone_tasks, &contract_matrix)?;
    Ok(())
}

fn validate_supplied_seam_edges(
    task: &Task,
    task_slice: &Value,
    tasks_by_ref: &std::collections::BTreeMap<&str, &Task>,
    active_refs: &std::collections::BTreeSet<&str>,
    contract_matrix: &crate::tasks::MilestoneContractMatrix,
) -> Result<()> {
    for (field, task_key, incoming) in [
        ("incoming_seams", "source_task", true),
        ("outgoing_seams", "target_task", false),
    ] {
        let Some(edges) = task_slice.get(field) else {
            continue;
        };
        let edges = edges.as_array().with_context(|| {
            format!(
                "MILESTONE_TASK_PROOF_INVALID: '{field}' for task '{}' must be an array",
                task.spec.task_ref
            )
        })?;
        ensure!(
            edges.is_empty() || active_refs.contains(task.spec.task_ref.as_str()),
            "MILESTONE_TASK_PROOF_INVALID: cancelled task_ref '{}' cannot declare seam edges",
            task.spec.task_ref
        );
        for (index, edge) in edges.iter().enumerate() {
            edge.as_object().with_context(|| {
                format!("MILESTONE_TASK_PROOF_INVALID: '{field}[{index}]' for task '{}' must be an object", task.spec.task_ref)
            })?;
            let other_ref = edge
                .get(task_key)
                .and_then(Value::as_str)
                .with_context(|| {
                    format!("MILESTONE_TASK_PROOF_INVALID: '{field}[{index}].{task_key}' for task '{}' must be a task_ref", task.spec.task_ref)
                })?;
            ensure!(
                tasks_by_ref.contains_key(other_ref) && active_refs.contains(other_ref),
                "MILESTONE_TASK_PROOF_INVALID: seam edge for task '{}' references unknown or cancelled task_ref '{other_ref}'",
                task.spec.task_ref
            );

            let (source_task, target_task) = if incoming {
                (other_ref, task.spec.task_ref.as_str())
            } else {
                (task.spec.task_ref.as_str(), other_ref)
            };
            let is_dependency = tasks_by_ref
                .get(target_task)
                .is_some_and(|target| target.spec.depends_on.iter().any(|r| r == source_task));
            let admitted_contracts = contract_matrix
                .entries
                .iter()
                .filter(|contract| {
                    contract.source_task == source_task && contract.target_task == target_task
                })
                .collect::<Vec<_>>();
            ensure!(
                is_dependency || !admitted_contracts.is_empty(),
                "MILESTONE_TASK_PROOF_INVALID: {field} edge '{source_task} -> {target_task}' is not an admitted dependency or contract seam"
            );
            if !admitted_contracts.is_empty() {
                let contract_type = edge.get("contract_type").and_then(Value::as_str);
                let contract_ref = edge.get("contract_ref").and_then(Value::as_str);
                let mut matches_admitted_contract = false;
                for contract in admitted_contracts {
                    let expected_type = serde_json::to_value(contract.contract_type)?;
                    if expected_type.as_str() == contract_type
                        && Some(contract.contract_ref.as_str()) == contract_ref
                    {
                        matches_admitted_contract = true;
                        break;
                    }
                }
                ensure!(
                    matches_admitted_contract,
                    "MILESTONE_TASK_PROOF_INVALID: {field} edge '{source_task} -> {target_task}' does not match an admitted contract seam"
                );
            }
        }
    }
    Ok(())
}

fn seam_entry_matches(
    edges: Option<&Value>,
    task_key: &str,
    task_ref: &str,
    contract_type: Option<&str>,
    contract_ref: Option<&str>,
) -> bool {
    edges.and_then(Value::as_array).is_some_and(|edges| {
        edges.iter().any(|edge| {
            edge.get(task_key).and_then(Value::as_str) == Some(task_ref)
                && contract_type.is_none_or(|expected| {
                    edge.get("contract_type").and_then(Value::as_str) == Some(expected)
                })
                && contract_ref.is_none_or(|expected| {
                    edge.get("contract_ref").and_then(Value::as_str) == Some(expected)
                })
        })
    })
}

pub(super) fn gate_proof_kind(gate: &crate::tasks::profile::GateDef) -> String {
    use crate::tasks::profile::ProofDef;

    match &gate.proof {
        ProofDef::Kind(kind) => kind.as_str().to_owned(),
        ProofDef::Scheme { .. } => "scheme".to_owned(),
        ProofDef::Named(name) | ProofDef::ResolvedNamed { name, .. } => name.clone(),
        ProofDef::Unspecified => "none".to_owned(),
    }
}

fn ready_pre_task_root_cursors(
    store: &TasksStore,
    workspace: &workspaces::Workspace,
    task: &Task,
    profile: &crate::tasks::profile::GateProfile,
    worktree: &Path,
) -> Result<(String, Vec<Task>, Vec<gate_graph::GateCursor>)> {
    let milestone_tasks = store.list(Some(&task.milestone_ref), "all", None)?;
    let scope = milestone_task_scope(&milestone_tasks);
    let cursors =
        applicable_pre_task_root_cursors(profile, task, &milestone_tasks, &scope, worktree)?;
    let milestone_identity = milestone_identity_for_task(workspace, task)?;
    if cursors.is_empty()
        || gate_graph::first_unpassed_root_cursor(&store.connection, &milestone_identity, &cursors)?
            .is_some()
    {
        return Ok((milestone_identity, milestone_tasks, Vec::new()));
    }
    Ok((milestone_identity, milestone_tasks, cursors))
}

/// Read-only lookup of passed applicable pre-task root gate paths for task context assembly.
pub(in crate::engine) fn pre_task_root_gate_paths(
    store: &TasksStore,
    workspace: &workspaces::Workspace,
    task: &Task,
    profile: &crate::tasks::profile::GateProfile,
    worktree: &Path,
) -> Result<Vec<String>> {
    let (_, _, cursors) = ready_pre_task_root_cursors(store, workspace, task, profile, worktree)?;
    Ok(cursors
        .into_iter()
        .map(|cursor| cursor.canonical_stage_id)
        .collect())
}

/// Materialize accepted pre-task proof slices once the milestone root barrier passes.
pub(in crate::engine) fn fan_out_pre_task_proofs_if_ready(
    store: &TasksStore,
    workspace: &workspaces::Workspace,
    task: &Task,
    profile: &crate::tasks::profile::GateProfile,
    worktree: &Path,
) -> Result<Vec<String>> {
    let (milestone_identity, milestone_tasks, cursors) =
        ready_pre_task_root_cursors(store, workspace, task, profile, worktree)?;
    if cursors.is_empty() {
        return Ok(Vec::new());
    }
    let contract_matrix = store.list_contract_matrix(&milestone_identity)?;
    for cursor in &cursors {
        let latest: Option<(String, String, String, String)> = store
            .connection
            .query_row(
                "SELECT action, proof_payload, git_tree_sha, worker_id FROM gate_transitions
                 WHERE milestone_id=?1 AND subject='milestone' AND gate_path=?2
                 ORDER BY id DESC LIMIT 1",
                rusqlite::params![milestone_identity, cursor.canonical_stage_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((action, raw_payload, git_tree_sha, worker_id)) = latest else {
            bail!(
                "MILESTONE_ROOT_GATE_TRANSITION_INVALID: accepted root gate '{}' has no transition",
                cursor.canonical_stage_id
            );
        };
        ensure!(
            action == "pass",
            "MILESTONE_ROOT_GATE_TRANSITION_INVALID: latest root gate '{}' is not passed",
            cursor.canonical_stage_id
        );
        let payload: Value = serde_json::from_str(&raw_payload).with_context(|| {
            format!(
                "MILESTONE_ROOT_GATE_TRANSITION_INVALID: '{}' has malformed proof payload",
                cursor.canonical_stage_id
            )
        })?;
        let Some(proof) = root_proof_with_task_map(&payload)? else {
            continue;
        };
        let task_slices = task_proof_slices(proof, &milestone_tasks, &contract_matrix)?;
        let source = crate::store::GateTransition {
            milestone_id: milestone_identity.clone(),
            subject: "milestone".to_owned(),
            gate_path: cursor.canonical_stage_id.clone(),
            action: "pass".to_owned(),
            proof_kind: gate_proof_kind(&cursor.gate),
            proof_payload: payload,
            git_tree_sha,
            worker_id,
        };
        store.record_task_proof_advances(&source, &task_slices)?;
    }
    Ok(cursors
        .into_iter()
        .map(|cursor| cursor.canonical_stage_id)
        .collect())
}
