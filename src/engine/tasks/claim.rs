use crate::core::tasks::profile::{GateDef, GateDriver, GateProfile};
use anyhow::{bail, Context, Result};

/// Active leaf gate selected from the task lifecycle container.
#[derive(Debug, Clone)]
pub(crate) struct GateCursor {
    /// Locator of the parent container, rooted at the task lifecycle.
    pub gate_path: String,
    /// Stage value persisted on the task and used for claim/submission equality.
    pub stage_id: String,
    /// Canonical fully qualified gate path for nested profile gates.
    pub canonical_stage_id: String,
    /// Top-level gate index used by existing task metadata and receipt logic.
    pub root_index: usize,
    /// Position among executable leaf gates, including nested containers.
    pub flat_index: usize,
    /// Leaf gate definition used to resolve role, proof, and guidance.
    pub gate: GateDef,
}

fn collect_cursors(profile: &GateProfile) -> Result<Vec<GateCursor>> {
    fn collect_sequence(
        gates: &[GateDef],
        root_index: usize,
        parents: &[String],
        output: &mut Vec<GateCursor>,
    ) -> Result<()> {
        for gate in gates {
            let mut path = parents.to_vec();
            if !gate.id.trim().is_empty() {
                path.push(gate.id.clone());
            }
            let mut children = gate.gates.clone();
            if let Some(tasks) = &gate.tasks {
                children.extend(tasks.gates.iter().cloned());
            }
            if children.is_empty() {
                if gate.id.trim().is_empty() {
                    bail!("TASK_GATE_CURSOR_INVALID: executable gate has no id");
                }
                let canonical_stage_id = if gate.id.contains('.') {
                    gate.id.clone()
                } else if !path.is_empty() {
                    path.join(".")
                } else {
                    gate.id.clone()
                };
                let parent_path = if canonical_stage_id.contains('.') {
                    canonical_stage_id.rsplit_once('.').map(|(p, _)| p).unwrap_or("").to_owned()
                } else {
                    path[..path.len().saturating_sub(1)].join(".")
                };
                let gate_path = if parent_path.is_empty() || parent_path == "tasks" {
                    "tasks".to_owned()
                } else {
                    format!("tasks.{parent_path}")
                };
                output.push(GateCursor {
                    gate_path,
                    stage_id: canonical_stage_id.clone(),
                    canonical_stage_id,
                    root_index,
                    flat_index: output.len(),
                    gate: gate.clone(),
                });
            } else {
                collect_sequence(&children, root_index, &path, output)?;
            }
        }
        Ok(())
    }

    let mut cursors = Vec::new();
    for (root_index, gate) in profile.gates.iter().enumerate() {
        let children = gate
            .gates
            .iter()
            .chain(gate.tasks.iter().flat_map(|tasks| tasks.gates.iter()));
        if children.clone().next().is_some() {
            let mut path = Vec::new();
            if !gate.id.trim().is_empty() {
                path.push(gate.id.clone());
            }
            let mut nested = gate.gates.clone();
            if let Some(tasks) = &gate.tasks {
                nested.extend(tasks.gates.iter().cloned());
            }
            collect_sequence(&nested, root_index, &path, &mut cursors)?;
        } else {
            collect_sequence(std::slice::from_ref(gate), root_index, &[], &mut cursors)?;
        }
    }
    if cursors.is_empty() {
        bail!("TASK_GATE_CURSOR_COMPLETE: no executable task gate remains");
    }
    let mut seen = std::collections::BTreeSet::new();
    if let Some(duplicate) = cursors
        .iter()
        .map(|cursor| cursor.canonical_stage_id.as_str())
        .find(|stage| !seen.insert(*stage))
    {
        bail!("TASK_GATE_CURSOR_AMBIGUOUS: nested gate path '{duplicate}' is not unique");
    }
    Ok(cursors)
}

/// Return executable leaf gates in their persisted transition order.
pub(super) fn gate_sequence(profile: &GateProfile) -> Result<Vec<GateCursor>> {
    collect_cursors(profile)
}

/// Build the flat profile view expected by receipt validation and lookup APIs.
pub(crate) fn flattened_profile(profile: &GateProfile) -> Result<GateProfile> {
    let cursors = collect_cursors(profile)?;
    let mut flattened = profile.clone();
    flattened.gates = cursors
        .iter()
        .map(|cursor| {
            let mut gate = cursor.gate.clone();
            gate.id = cursor.canonical_stage_id.clone();
            gate.gates.clear();
            gate.tasks = None;
            for target in [&mut gate.reject_to, &mut gate.independent_from, &mut gate.receipt_review] {
                if let Some(stage) = target.clone() {
                    *target = Some(
                        cursor_for_path(profile, &stage)
                            .with_context(|| {
                                format!("TASK_GATE_CURSOR_INVALID: referenced stage '{stage}' is not executable")
                            })?
                            .0
                            .canonical_stage_id,
                    );
                }
            }
            gate.review_sources = gate
                .review_sources
                .iter()
                .map(|stage| {
                    cursor_for_path(profile, stage)
                        .with_context(|| {
                            format!("TASK_GATE_CURSOR_INVALID: referenced review stage '{stage}' is not executable")
                        })
                        .map(|(cursor, _)| cursor.canonical_stage_id)
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(gate)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(flattened)
}

fn cursor_for_path(profile: &GateProfile, stage: &str) -> Result<(GateCursor, usize)> {
    let cursors = collect_cursors(profile)?;
    if let Some((index, cursor)) = cursors
        .iter()
        .enumerate()
        .find(|(_, cursor)| cursor.canonical_stage_id == stage)
    {
        return Ok((cursor.clone(), index));
    }

    let mut containers = Vec::new();
    fn collect_containers(
        gates: &[GateDef],
        root_index: usize,
        parents: &[String],
        output: &mut Vec<(String, String, usize)>,
    ) {
        for gate in gates {
            let mut path = parents.to_vec();
            if !gate.id.trim().is_empty() {
                path.push(gate.id.clone());
            }
            let mut children = gate.gates.clone();
            if let Some(tasks) = &gate.tasks {
                children.extend(tasks.gates.iter().cloned());
            }
            if children.is_empty() {
                continue;
            }
            output.push((gate.id.clone(), path.join("."), root_index));
            collect_containers(&children, root_index, &path, output);
        }
    }
    for (root_index, gate) in profile.gates.iter().enumerate() {
        let mut children = gate.gates.clone();
        if let Some(tasks) = &gate.tasks {
            children.extend(tasks.gates.iter().cloned());
        }
        if children.is_empty() {
            continue;
        }
        let parents = if gate.id.trim().is_empty() {
            Vec::new()
        } else {
            vec![gate.id.clone()]
        };
        collect_containers(&children, root_index, &parents, &mut containers);
        containers.push((gate.id.clone(), gate.id.clone(), root_index));
    }

    let matching_containers = containers
        .iter()
        .filter(|(id, path, _)| path == stage || id == stage)
        .collect::<Vec<_>>();
    let matching_paths = matching_containers
        .iter()
        .map(|(_, path, _)| path.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if matching_paths.len() == 1 {
        let (_, path, root_index) = matching_containers[0];
        if let Some((index, cursor)) = cursors.iter().enumerate().find(|(_, cursor)| {
            cursor.root_index == *root_index
                && cursor.canonical_stage_id.starts_with(&format!("{path}."))
        }) {
            let mut selected = cursor.clone();
            selected.stage_id = cursor.canonical_stage_id.clone();
            return Ok((selected, index));
        }
    } else if matching_paths.len() > 1 {
        bail!("TASK_STAGE_AMBIGUOUS: '{stage}' identifies more than one nested container; use its qualified path");
    }

    let matches = cursors
        .iter()
        .enumerate()
        .filter(|(_, cursor)| {
            cursor.canonical_stage_id == stage
                || cursor.gate.id == stage
                || cursor.canonical_stage_id.rsplit('.').next() == Some(stage)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [(index, cursor)] => Ok(((*cursor).clone(), *index)),
        [] => bail!("TASK_STAGE_UNKNOWN: '{stage}' is not in the active profile"),
        _ => bail!("TASK_STAGE_AMBIGUOUS: '{stage}' identifies more than one nested gate; use its qualified path"),
    }
}

/// Resolve one persisted stage to its executable leaf gate.
pub(crate) fn cursor_for_stage(profile: &GateProfile, stage: &str) -> Result<GateCursor> {
    cursor_for_path(profile, stage).map(|(cursor, _)| cursor)
}

/// Resolve the next executable task gate, or the first one when `stage_id` is omitted.
pub(super) fn advance_gate_cursor(
    profile: &GateProfile,
    stage_id: Option<&str>,
) -> Result<GateCursor> {
    let cursors = collect_cursors(profile)?;
    let next_index = match stage_id {
        None => 0,
        Some(stage) => cursor_for_path(profile, stage)?.1 + 1,
    };
    cursors
        .get(next_index)
        .cloned()
        .context("TASK_GATE_CURSOR_COMPLETE: no active task gate remains")
}

/// Select the persisted active gate and validate any explicitly requested stage.
pub(super) fn claim_nested_gate(
    profile: &GateProfile,
    active_stage: &str,
    requested_stage: &str,
) -> Result<GateCursor> {
    let cursor = if active_stage.trim().is_empty() {
        advance_gate_cursor(profile, None)?
    } else {
        cursor_for_stage(profile, active_stage)?
    };
    if !requested_stage.trim().is_empty() {
        let requested = cursor_for_stage(profile, requested_stage).map_err(|error| {
            if error.to_string().contains("TASK_STAGE_UNKNOWN") {
                anyhow::anyhow!(
                    "TASK_STAGE_INVALID: active gate is '{}'; requested '{}'. Omit stage to claim from persisted task state.",
                    cursor.canonical_stage_id,
                    requested_stage
                )
            } else {
                error
            }
        })?;
        if requested.canonical_stage_id != cursor.canonical_stage_id {
            bail!(
                "TASK_STAGE_INVALID: active gate is '{}'; requested '{}'. Omit stage to claim from persisted task state.",
                cursor.canonical_stage_id,
                requested_stage
            );
        }
    }
    Ok(cursor)
}

/// Resolve the registered agent role for the selected gate, if it is agent-driven.
pub(super) fn resolve_subagent_role(cursor: &GateCursor) -> Option<String> {
    match cursor.gate.driver() {
        Some(GateDriver::Agent) => cursor.gate.role.clone(),
        Some(GateDriver::Command | GateDriver::Container) | None => None,
    }
}

/// Build the stable breadcrumb key for a task gate claim.
pub(crate) fn breadcrumb(task_id: &str, cursor: &GateCursor) -> Result<(String, String)> {
    let (milestone, task_ref) = task_id
        .split_once(':')
        .context("TASK_BREADCRUMB_INVALID: task id must contain a milestone and task reference")?;
    let milestone_id = milestone
        .rsplit('/')
        .next()
        .filter(|value| !value.trim().is_empty())
        .context("TASK_BREADCRUMB_INVALID: milestone id is missing")?;
    if task_ref.trim().is_empty() {
        bail!("TASK_BREADCRUMB_INVALID: task reference is missing");
    }
    let leaf_stage = cursor
        .canonical_stage_id
        .rsplit_once('.')
        .map(|(_, leaf)| leaf)
        .unwrap_or(&cursor.stage_id);
    let breadcrumb = format!(
        "{milestone_id}/{}/{task_ref}/{leaf_stage}",
        cursor.gate_path
    );
    Ok((milestone_id.to_owned(), breadcrumb))
}
