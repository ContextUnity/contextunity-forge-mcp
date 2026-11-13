use crate::tasks::profile::{GateDef, GateProfile};
use anyhow::{bail, Context, Result};
use rusqlite::OptionalExtension;
use std::path::Path;

#[derive(Debug, Clone)]
struct ContainerSuccess {
    end_index: usize,
    target: String,
}

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
    /// Effective conditional filter for this gate or its enclosing container.
    pub when: Option<String>,
    container_success: Vec<ContainerSuccess>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GateCollection {
    Task,
    Root,
}

struct ExpansionContext<'a> {
    profile: &'a GateProfile,
    scope: GateCollection,
    task: Option<&'a crate::store::Task>,
}

pub(crate) fn collect_cursors(profile: &GateProfile) -> Result<Vec<GateCursor>> {
    collect_task_cursors(profile, None)
}

pub(crate) fn collect_task_cursors(
    profile: &GateProfile,
    task: Option<&crate::store::Task>,
) -> Result<Vec<GateCursor>> {
    collect_cursors_for_scope(profile, GateCollection::Task, task)
}

pub(crate) fn executable_task_gate_defs(
    profile: &GateProfile,
    task: &crate::store::Task,
) -> Result<Vec<GateDef>> {
    let task_position = task_container_position(profile, Some(task))?;
    Ok(collect_task_cursors(profile, Some(task))?
        .into_iter()
        .filter(|cursor| {
            task_position.is_none_or(|position| cursor.root_index == position)
                && task_matches_gate_condition(cursor.when.as_deref(), task)
        })
        .map(|cursor| {
            let mut gate = cursor.gate;
            gate.id = cursor.canonical_stage_id;
            gate
        })
        .collect())
}

pub(crate) fn collect_cursors_for_task(
    profile: &GateProfile,
    task: Option<&crate::store::Task>,
) -> Result<Vec<GateCursor>> {
    collect_task_cursors(profile, task)
}

/// Flatten all executable root gates, excluding every task lifecycle subtree.
pub(crate) fn collect_root_cursors(profile: &GateProfile) -> Result<Vec<GateCursor>> {
    collect_cursors_for_scope(profile, GateCollection::Root, None)
}

/// Return the root index of the task container applicable to a task.
pub(crate) fn task_container_position(
    profile: &GateProfile,
    task: Option<&crate::store::Task>,
) -> Result<Option<usize>> {
    let matches = profile
        .gates
        .iter()
        .enumerate()
        .filter_map(|(index, gate)| {
            let tasks = gate.tasks.as_ref()?;
            let condition = crate::tasks::profile::combine_gate_conditions(
                gate.when.as_deref(),
                tasks.when.as_deref(),
            );
            task.is_none_or(|task| task_matches_gate_condition(condition.as_deref(), task))
                .then_some(index)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Ok(None),
        [index] => Ok(Some(*index)),
        _ => bail!("TASK_GATE_CONTAINER_AMBIGUOUS: more than one task lifecycle container applies"),
    }
}

/// Return applicable root gates positioned before the task container.
pub(crate) fn pre_task_root_cursors(
    profile: &GateProfile,
    task: &crate::store::Task,
) -> Result<Vec<GateCursor>> {
    let Some(task_position) = task_container_position(profile, Some(task))? else {
        return Ok(Vec::new());
    };
    Ok(collect_root_cursors(profile)?
        .into_iter()
        .filter(|cursor| cursor.root_index < task_position)
        .collect())
}

/// Check that each supplied root cursor has its latest accepted milestone transition.
pub(crate) fn ensure_root_gate_transitions_passed(
    connection: &rusqlite::Connection,
    milestone_identity: &str,
    cursors: &[GateCursor],
) -> Result<()> {
    if let Some(cursor) = first_unpassed_root_cursor(connection, milestone_identity, cursors)? {
        bail!("MILESTONE_PRE_TASK_GATE_PENDING: milestone '{milestone_identity}' has not passed root gate '{}' (gate_path '{}', root_index {}).", cursor.canonical_stage_id, cursor.gate_path, cursor.root_index);
    }
    Ok(())
}

/// Return the first cursor without a latest milestone-scoped pass transition.
pub(crate) fn first_unpassed_root_cursor(
    connection: &rusqlite::Connection,
    milestone_identity: &str,
    cursors: &[GateCursor],
) -> Result<Option<GateCursor>> {
    for cursor in cursors {
        let latest: Option<String> = connection
            .query_row(
                "SELECT action FROM gate_transitions
                 WHERE milestone_id=?1 AND subject='milestone' AND gate_path=?2
                 ORDER BY id DESC LIMIT 1",
                rusqlite::params![milestone_identity, cursor.canonical_stage_id],
                |row| row.get(0),
            )
            .optional()?;
        if latest.as_deref() != Some("pass") {
            return Ok(Some(cursor.clone()));
        }
    }
    Ok(None)
}

fn collect_cursors_for_scope(
    profile: &GateProfile,
    scope: GateCollection,
    task: Option<&crate::store::Task>,
) -> Result<Vec<GateCursor>> {
    fn collect_sequence(
        context: &ExpansionContext<'_>,
        gates: &[GateDef],
        root_index: usize,
        parents: &[String],
        inherited_when: Option<&str>,
        output: &mut Vec<GateCursor>,
    ) -> Result<()> {
        for gate in gates {
            let mut path = parents.to_vec();
            if !gate.id.trim().is_empty() {
                path.push(gate.id.clone());
            }
            let effective_when = crate::tasks::profile::combine_gate_conditions(
                inherited_when,
                gate.when.as_deref(),
            );
            let task_when = (context.scope == GateCollection::Task)
                .then(|| gate.tasks.as_ref().and_then(|tasks| tasks.when.as_deref()))
                .flatten();
            let effective_when = crate::tasks::profile::combine_gate_conditions(
                effective_when.as_deref(),
                task_when,
            );
            let mut children = gate.gates.clone();
            if context.scope == GateCollection::Task {
                if let Some(tasks) = &gate.tasks {
                    children.extend(tasks.gates.iter().cloned());
                }
            }
            if let Some(template_name) = &gate.use_template {
                if let Some(template) = context.profile.gate_templates.get(template_name) {
                    children.extend(template.gates.iter().cloned());
                } else {
                    bail!("GATE_INVALID_CONFIGURATION: undefined gate template '{template_name}'");
                }
            }
            if children.is_empty() {
                if context.scope == GateCollection::Root && gate.tasks.is_some() {
                    continue;
                }
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
                    canonical_stage_id
                        .rsplit_once('.')
                        .map(|(p, _)| p)
                        .unwrap_or("")
                        .to_owned()
                } else {
                    path[..path.len().saturating_sub(1)].join(".")
                };
                let namespace = match context.scope {
                    GateCollection::Task => "tasks",
                    GateCollection::Root => "gates",
                };
                let gate_path = if parent_path.is_empty() || parent_path == namespace {
                    namespace.to_owned()
                } else {
                    format!("{namespace}.{parent_path}")
                };
                output.push(GateCursor {
                    gate_path,
                    stage_id: canonical_stage_id.clone(),
                    canonical_stage_id,
                    root_index,
                    flat_index: output.len(),
                    gate: gate.clone(),
                    when: effective_when,
                    container_success: Vec::new(),
                });
            } else {
                let start = output.len();
                collect_sequence(
                    context,
                    &children,
                    root_index,
                    &path,
                    effective_when.as_deref(),
                    output,
                )?;
                if output.len() > start {
                    for child in &mut output[start..] {
                        if child.gate.on_fail.is_none() && child.gate.reject_to.is_none() {
                            child.gate.on_fail =
                                gate.on_fail.clone().or_else(|| gate.reject_to.clone());
                        }
                        if child.gate.reject_routing.is_none() {
                            child.gate.reject_routing = gate.reject_routing.clone();
                        }
                        if child.gate.max_attempts.is_none() {
                            child.gate.max_attempts = gate.max_attempts;
                        }
                    }
                    if let Some(target) = gate.on_success.as_ref() {
                        let end_index = output.len();
                        for child in &mut output[start..end_index] {
                            child.container_success.push(ContainerSuccess {
                                end_index,
                                target: target.clone(),
                            });
                        }
                        if let Some(terminal) = output[start..].iter_mut().rev().find(|cursor| {
                            context.task.is_none_or(|task| {
                                task_matches_gate_condition(cursor.when.as_deref(), task)
                            })
                        }) {
                            if terminal.gate.on_success.is_none() {
                                terminal.gate.on_success = Some(target.clone());
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    let context = ExpansionContext {
        profile,
        scope,
        task,
    };
    let has_task_container =
        scope == GateCollection::Task && profile.gates.iter().any(|gate| gate.tasks.is_some());
    let mut cursors = Vec::new();
    for (root_index, gate) in profile.gates.iter().enumerate() {
        if has_task_container && gate.tasks.is_none() {
            continue;
        }
        collect_sequence(
            &context,
            std::slice::from_ref(gate),
            root_index,
            &[],
            None,
            &mut cursors,
        )?;
    }
    if scope == GateCollection::Task && cursors.is_empty() {
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

pub(super) fn has_predecessor_snapshot(
    profile: &GateProfile,
    task: &crate::store::Task,
    stage: &str,
) -> Result<bool> {
    let cursors = collect_cursors_for_task(profile, Some(task))?;
    let selected = cursor_for_stage(profile, stage)?;
    let is_track = profile.gates[selected.root_index].tasks.is_some();
    Ok(cursors
        .iter()
        .take_while(|cursor| cursor.canonical_stage_id != selected.canonical_stage_id)
        .any(|cursor| {
            (!is_track || cursor.root_index == selected.root_index)
                && task_matches_gate_condition(cursor.when.as_deref(), task)
                && cursor.gate.sha_snapshot
        }))
}

/// Build the flat profile view expected by receipt validation and lookup APIs.
pub(crate) fn flattened_profile(
    profile: &GateProfile,
    task: Option<&crate::store::Task>,
) -> Result<GateProfile> {
    let cursors = collect_cursors_for_task(profile, task)?;
    let mut flattened = profile.clone();
    flattened.gates = cursors
        .iter()
        .map(|cursor| {
            let mut gate = cursor.gate.clone();
            gate.id = cursor.canonical_stage_id.clone();
            gate.gates.clear();
            gate.tasks = None;
            gate.when = cursor.when.clone();
            for target in [&mut gate.on_success, &mut gate.on_fail, &mut gate.reject_to, &mut gate.independent_from, &mut gate.receipt_review] {
                if let Some(stage) = target.clone() {
                    *target = Some(
                        cursor_for_reference(profile, &cursor.canonical_stage_id, &stage)
                            .with_context(|| {
                                format!("TASK_GATE_CURSOR_INVALID: referenced stage '{stage}' is not executable")
                            })?
                            .0
                            .canonical_stage_id,
                    );
                }
            }
            if let Some(routing) = gate.reject_routing.as_mut() {
                for target in [&mut routing.scope, &mut routing.symbols] {
                    if let Some(reference) = target.clone() {
                        *target = Some(cursor_for_reference(profile, &cursor.canonical_stage_id, &reference)?.0.canonical_stage_id);
                    }
                }
            }
            gate.review_sources = gate
                .review_sources
                .iter()
                .map(|stage| {
                    cursor_for_reference(profile, &cursor.canonical_stage_id, stage)
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
        profile: &GateProfile,
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
            if let Some(template_name) = &gate.use_template {
                if let Some(template) = profile.gate_templates.get(template_name) {
                    children.extend(template.gates.iter().cloned());
                }
            }
            if children.is_empty() {
                continue;
            }
            output.push((gate.id.clone(), path.join("."), root_index));
            collect_containers(profile, &children, root_index, &path, output);
        }
    }
    let has_task_container = profile.gates.iter().any(|g| g.tasks.is_some());
    for (root_index, gate) in profile.gates.iter().enumerate() {
        if has_task_container && gate.tasks.is_none() {
            continue;
        }
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
        collect_containers(profile, &children, root_index, &parents, &mut containers);
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
    let (mut cursor, _) = cursor_for_path(profile, stage)?;
    for target in [
        &mut cursor.gate.on_success,
        &mut cursor.gate.on_fail,
        &mut cursor.gate.reject_to,
        &mut cursor.gate.independent_from,
        &mut cursor.gate.receipt_review,
    ] {
        if let Some(reference) = target.clone() {
            *target = Some(
                cursor_for_reference(profile, &cursor.canonical_stage_id, &reference)?
                    .0
                    .canonical_stage_id,
            );
        }
    }
    if let Some(routing) = cursor.gate.reject_routing.as_mut() {
        for target in [&mut routing.scope, &mut routing.symbols] {
            if let Some(reference) = target.clone() {
                *target = Some(
                    cursor_for_reference(profile, &cursor.canonical_stage_id, &reference)?
                        .0
                        .canonical_stage_id,
                );
            }
        }
    }
    cursor.gate.review_sources = cursor
        .gate
        .review_sources
        .iter()
        .map(|reference| {
            cursor_for_reference(profile, &cursor.canonical_stage_id, reference)
                .map(|(cursor, _)| cursor.canonical_stage_id)
        })
        .collect::<Result<_>>()?;
    Ok(cursor)
}

fn cursor_for_reference(
    profile: &GateProfile,
    owner: &str,
    target: &str,
) -> Result<(GateCursor, usize)> {
    let cursors = collect_task_cursors(profile, None)?;
    let index = resolve_cursor_index(&cursors, owner, target)?;
    Ok((cursors[index].clone(), index))
}

fn resolve_cursor_index(cursors: &[GateCursor], owner: &str, target: &str) -> Result<usize> {
    fn candidate_index(cursors: &[GateCursor], candidate: &str) -> Result<Option<usize>> {
        let exact = cursors
            .iter()
            .enumerate()
            .filter(|(_, cursor)| cursor.canonical_stage_id == candidate)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        match exact.as_slice() {
            [index] => return Ok(Some(*index)),
            [] => {}
            _ => bail!("TASK_GATE_CURSOR_AMBIGUOUS: gate reference '{candidate}' is not unique"),
        }

        let prefix = format!("{candidate}.");
        let descendants = cursors
            .iter()
            .enumerate()
            .filter(|(_, cursor)| cursor.canonical_stage_id.starts_with(&prefix))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        Ok(descendants.first().copied())
    }

    if target.contains('.') {
        if let Some(index) = candidate_index(cursors, target)? {
            return Ok(index);
        }
    }
    let mut namespace = owner.rsplit_once('.').map(|(parent, _)| parent);
    while let Some(parent) = namespace {
        if let Some(index) = candidate_index(cursors, &format!("{parent}.{target}"))? {
            return Ok(index);
        }
        namespace = parent.rsplit_once('.').map(|(parent, _)| parent);
    }
    if let Some(index) = candidate_index(cursors, target)? {
        return Ok(index);
    }

    let matches = cursors
        .iter()
        .enumerate()
        .filter(|(_, cursor)| {
            cursor.gate.id == target || cursor.canonical_stage_id.rsplit('.').next() == Some(target)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [index] => Ok(*index),
        [] => bail!("TASK_GATE_CURSOR_UNKNOWN: gate reference '{target}' is not executable"),
        _ => bail!("TASK_GATE_CURSOR_AMBIGUOUS: gate reference '{target}' is not unique"),
    }
}

pub(crate) use forge_core::is_doc_path;

pub(crate) fn task_matches_gate_condition(
    condition: Option<&str>,
    task: &crate::store::Task,
) -> bool {
    task_matches_gate_condition_at(condition, task, task.worktree.as_deref().map(Path::new))
}

pub(crate) fn task_matches_gate_condition_at(
    condition: Option<&str>,
    task: &crate::store::Task,
    worktree: Option<&Path>,
) -> bool {
    gate_condition_matches(
        condition,
        &task.spec.scope,
        std::slice::from_ref(task),
        Some(task),
        worktree,
    )
}

/// Evaluate the shared, closed `when:` vocabulary against task or milestone scope.
pub(crate) fn gate_condition_matches(
    condition: Option<&str>,
    paths: &[String],
    tasks: &[crate::store::Task],
    task: Option<&crate::store::Task>,
    worktree: Option<&Path>,
) -> bool {
    let Some(cond) = condition else {
        return true;
    };
    cond.split([',', '+', '&'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .all(|part| match part {
            "seam" => task.into_iter().chain(tasks.iter()).any(|task| {
                task.spec.concurrency.as_ref().is_some_and(|concurrency| {
                    matches!(
                        concurrency.mode,
                        crate::tasks::ConcurrencyMode::Speculative
                            | crate::tasks::ConcurrencyMode::Parallel
                    )
                })
            }),
            "code" => paths.iter().any(|path| !is_doc_path(path)),
            "doc" | "docs" => !paths.is_empty() && paths.iter().all(|path| is_doc_path(path)),
            "db_migration" | "db" => paths.iter().any(|path| {
                path.ends_with(".sql")
                    || path.contains("/migrations/")
                    || path.contains("\\migrations\\")
            }),
            "public_api" => worktree.is_some_and(|worktree| {
                paths
                    .iter()
                    .chain(tasks.iter().flat_map(|task| task.spec.scope.iter()))
                    .chain(task.into_iter().flat_map(|task| task.spec.scope.iter()))
                    .any(|path| has_public_api_in_scope(worktree, path))
            }),
            _ => false,
        })
}

fn has_public_api_in_scope(worktree: &Path, path: &str) -> bool {
    let Ok(target) = super::confined_path(worktree, path) else {
        return false;
    };
    if target.is_file() {
        return std::fs::read_to_string(&target)
            .ok()
            .and_then(|source| super::gates::public_callable_nodes(path, &source).ok())
            .is_some_and(|nodes| !nodes.is_empty());
    }
    if !target.is_dir() {
        return false;
    }

    fn scan_directory(directory: &Path, relative: &str) -> bool {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return false;
        };
        entries.flatten().any(|entry| {
            let Ok(file_type) = entry.file_type() else {
                return false;
            };
            if file_type.is_symlink() {
                return false;
            }
            let path = entry.path();
            let child = format!("{relative}/{}", entry.file_name().to_string_lossy());
            if file_type.is_file() {
                return std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|source| super::gates::public_callable_nodes(&child, &source).ok())
                    .is_some_and(|nodes| !nodes.is_empty());
            }
            file_type.is_dir()
                && !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| forge_core::config::DEFAULT_IGNORED_NAMES.contains(&name))
                && scan_directory(&path, &child)
        })
    }

    scan_directory(&target, path.trim_end_matches('/'))
}

#[derive(Debug, Clone)]
pub(crate) struct FailTarget {
    pub target: Option<GateCursor>,
    pub max_attempts: Option<u32>,
    pub exhausted: bool,
}

fn first_applicable_cursor(
    cursors: &[GateCursor],
    start: usize,
    mut applicable: impl FnMut(&GateCursor) -> bool,
) -> Option<usize> {
    cursors
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, cursor)| applicable(cursor).then_some(index))
}

/// Resolve a pass edge or the next applicable cursor in the same flattened graph.
pub(crate) fn resolve_pass_target(
    cursors: &[GateCursor],
    current_index: usize,
    mut applicable: impl FnMut(&GateCursor) -> bool,
) -> Result<Option<usize>> {
    let current = cursors
        .get(current_index)
        .context("TASK_GATE_CURSOR_INVALID: current gate is outside the flattened graph")?;
    if let Some(target) = current.gate.on_success.as_deref() {
        if target == "pass" {
            return Ok(None);
        }
        let target_index = resolve_cursor_index(cursors, &current.canonical_stage_id, target)?;
        return Ok(first_applicable_cursor(cursors, target_index, applicable));
    }

    for exit in &current.container_success {
        let has_applicable_child = cursors
            .iter()
            .take(exit.end_index)
            .skip(current_index + 1)
            .any(&mut applicable);
        if !has_applicable_child {
            let target_index =
                resolve_cursor_index(cursors, &current.canonical_stage_id, &exit.target)?;
            return Ok(first_applicable_cursor(cursors, target_index, applicable));
        }
    }
    Ok(first_applicable_cursor(
        cursors,
        current_index + 1,
        applicable,
    ))
}

fn has_findings(value: &serde_json::Value, key: &str) -> bool {
    match value.get(key) {
        Some(serde_json::Value::Array(values)) => !values.is_empty(),
        Some(serde_json::Value::Object(values)) => !values.is_empty(),
        Some(serde_json::Value::String(value)) => !value.trim().is_empty(),
        Some(serde_json::Value::Bool(value)) => *value,
        _ => false,
    }
}

fn failure_class(findings: Option<&serde_json::Value>) -> Option<&'static str> {
    let findings = findings?;
    let kind = findings
        .get("kind")
        .or_else(|| findings.get("category"))
        .and_then(serde_json::Value::as_str);
    if has_findings(findings, "scope_findings") || kind == Some("scope") {
        Some("scope")
    } else if has_findings(findings, "contract_findings")
        || has_findings(findings, "symbol_findings")
        || matches!(kind, Some("symbols" | "contract"))
    {
        Some("symbols")
    } else {
        None
    }
}

/// Resolve the declared `loop`, `on_fail`, or `reject_to` edge for one gate.
pub(crate) fn resolve_failure_reference<'a>(
    gate: &'a GateDef,
    findings: Option<&serde_json::Value>,
    current_stage: &'a str,
) -> &'a str {
    let loop_target = match failure_class(findings) {
        Some("scope") => gate
            .reject_routing
            .as_ref()
            .and_then(|routing| routing.scope.as_deref()),
        Some("symbols") => gate
            .reject_routing
            .as_ref()
            .and_then(|routing| routing.symbols.as_deref()),
        _ => None,
    };
    loop_target
        .or(gate.on_fail.as_deref())
        .or(gate.reject_to.as_deref())
        .unwrap_or(current_stage)
}

/// Resolve a failure route, skipping inapplicable gates and enforcing this gate's retry bound.
pub(crate) fn resolve_fail_target(
    cursors: &[GateCursor],
    current_index: usize,
    findings: Option<&serde_json::Value>,
    attempts_used: u32,
    mut applicable: impl FnMut(&GateCursor) -> bool,
) -> Result<FailTarget> {
    let current = cursors
        .get(current_index)
        .context("TASK_GATE_CURSOR_INVALID: failed gate is outside the flattened graph")?;
    let max_attempts = current.gate.max_attempts;
    if max_attempts.is_some_and(|limit| attempts_used >= limit) {
        return Ok(FailTarget {
            target: None,
            max_attempts,
            exhausted: true,
        });
    }
    let reference = resolve_failure_reference(&current.gate, findings, &current.canonical_stage_id);
    let target_index = resolve_cursor_index(cursors, &current.canonical_stage_id, reference)?;
    let target = first_applicable_cursor(cursors, target_index, &mut applicable)
        .and_then(|index| cursors.get(index).cloned());
    Ok(FailTarget {
        target,
        max_attempts,
        exhausted: false,
    })
}

pub(crate) fn gate_applicable(task: &crate::store::Task, gate: &GateDef) -> bool {
    task_matches_gate_condition(gate.when.as_deref(), task)
}

/// Resolve the next executable task gate for a specific task, skipping conditional gates that do not apply.
pub(crate) fn advance_gate_cursor_for_task(
    profile: &GateProfile,
    stage_id: Option<&str>,
    task: &crate::store::Task,
) -> Result<GateCursor> {
    let cursors = collect_task_cursors(profile, Some(task))?;
    let next_index = match stage_id {
        None => first_applicable_cursor(&cursors, 0, |cursor| {
            task_matches_gate_condition(cursor.when.as_deref(), task)
        }),
        Some(stage) => {
            let (_, current_idx) = cursor_for_path(profile, stage)?;
            resolve_pass_target(&cursors, current_idx, |cursor| {
                task_matches_gate_condition(cursor.when.as_deref(), task)
            })?
        }
    };
    next_index
        .and_then(|index| cursors.get(index).cloned())
        .context("TASK_GATE_CURSOR_COMPLETE: no active task gate remains")
}

/// Fall through to the next sequential applicable gate when a gate is skipped due to unmet preconditions.
pub(crate) fn skip_gate_cursor_for_task(
    profile: &GateProfile,
    stage_id: &str,
    task: &crate::store::Task,
) -> Result<GateCursor> {
    let cursors = collect_task_cursors(profile, Some(task))?;
    let (_, current_idx) = cursor_for_path(profile, stage_id)?;
    first_applicable_cursor(&cursors, current_idx + 1, |cursor| {
        task_matches_gate_condition(cursor.when.as_deref(), task)
    })
    .and_then(|index| cursors.get(index).cloned())
    .context("TASK_GATE_CURSOR_COMPLETE: no active task gate remains")
}

/// Select the persisted active gate and validate any explicitly requested stage for a task.
pub(crate) fn claim_nested_gate_for_task(
    profile: &GateProfile,
    active_stage: &str,
    requested_stage: &str,
    task: &crate::store::Task,
) -> Result<GateCursor> {
    let cursor = if active_stage.trim().is_empty() {
        advance_gate_cursor_for_task(profile, None, task)?
    } else {
        let current = cursor_for_stage(profile, active_stage)?;
        if !task_matches_gate_condition(current.when.as_deref(), task) {
            skip_gate_cursor_for_task(profile, active_stage, task)?
        } else {
            current
        }
    };
    if !requested_stage.trim().is_empty() {
        let requested = cursor_for_stage(profile, requested_stage).or_else(|error| {
            if !error.to_string().contains("TASK_STAGE_AMBIGUOUS") {
                return Err(error);
            }
            let cursors = collect_cursors_for_task(profile, Some(task))?;
            let mut matching = cursors.into_iter().filter(|candidate| {
                candidate.canonical_stage_id.rsplit('.').next() == Some(requested_stage)
                    && task_matches_gate_condition(candidate.when.as_deref(), task)
            });
            match (matching.next(), matching.next()) {
                (Some(cursor), None) => Ok(cursor),
                _ => Err(error),
            }
        }).map_err(|error| {
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
        if !task_matches_gate_condition(requested.when.as_deref(), task) {
            bail!(
                "TASK_STAGE_INVALID: requested gate '{}' is skipped for task '{}' (when condition '{}' not met). Active gate is '{}'.",
                requested.canonical_stage_id,
                task.task_id,
                requested.when.as_deref().unwrap_or(""),
                cursor.canonical_stage_id
            );
        }
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
