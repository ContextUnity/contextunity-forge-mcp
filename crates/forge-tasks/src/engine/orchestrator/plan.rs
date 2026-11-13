use super::role::RoleObservations;
use super::workflow::{run_gate_graph, GateDecision};
use crate::{
    engine::{gate_graph, git_snapshot},
    milestones::command_gate::{observe_command, resolve_command_plan},
    tasks::{
        profile::{GateDriver, GateProfile, PersistenceAction, ProofKind},
        SubtaskSpec,
    },
};
use anyhow::{ensure, Context, Result};
use forge_core::models::DocSection;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

/// A source-owned plan step projected from production documentation sections.
#[derive(Debug, Clone, Serialize)]
pub struct PlanStep {
    pub title: String,
    pub target: String,
    pub scope: Vec<String>,
    pub depends_on: Vec<String>,
    pub status: String,
    pub evidence: Option<String>,
    pub subtasks: Vec<SubtaskSpec>,
    #[serde(skip)]
    heading: String,
}

/// Validated plan source and stable step order, without a second task registry.
#[derive(Debug, Serialize)]
pub struct PlanDocument {
    pub path: PathBuf,
    pub steps: Vec<PlanStep>,
    #[serde(skip)]
    source: String,
}

fn fields(content: &str) -> Result<BTreeMap<String, String>> {
    let mut fields = BTreeMap::new();
    for line in content.lines() {
        if let Some((key, value)) = line
            .trim()
            .strip_prefix("- **")
            .and_then(|line| line.split_once("**:"))
        {
            ensure!(
                fields
                    .insert(key.to_owned(), value.trim().to_owned())
                    .is_none(),
                "PLAN_FIELD_DUPLICATE: {key}"
            );
        }
    }
    Ok(fields)
}

fn string_list(value: &str) -> Result<Vec<String>> {
    if value.starts_with('[') {
        Ok(serde_yaml::from_str(value)?)
    } else {
        Ok(vec![value.to_owned()])
    }
}

impl PlanDocument {
    /// Project canonical numbered step sections from the existing docs extractor.
    pub fn from_sections(path: &Path, source: String, sections: &[DocSection]) -> Result<Self> {
        ensure!(
            !sections.is_empty() && sections.iter().all(|section| section.doc_type == "plan"),
            "PLAN_TYPE_INVALID: expected doc_type: plan"
        );
        let mut steps = Vec::new();
        for section in sections.iter().skip(1) {
            let (number, title) = section
                .section_title
                .split_once(". ")
                .context("PLAN_HEADING_INVALID: numbered step heading required")?;
            ensure!(
                number.parse::<usize>().is_ok() && !title.trim().is_empty(),
                "PLAN_HEADING_INVALID: {}",
                section.section_title
            );
            let fields = fields(&section.content)?;
            let target = fields
                .get("Target")
                .filter(|value| !value.is_empty())
                .context("PLAN_TARGET_MISSING")?
                .clone();
            let scope = string_list(fields.get("Scope").context("PLAN_SCOPE_MISSING")?)?;
            ensure!(
                !scope.is_empty()
                    && scope
                        .iter()
                        .all(|path| crate::fs::is_safe_project_path(path)),
                "PLAN_SCOPE_INVALID: {title}"
            );
            let depends_on = fields
                .get("Depends On")
                .map(|value| string_list(value))
                .transpose()?
                .unwrap_or_default();
            let status = fields.get("Status").context("PLAN_STATUS_MISSING")?.clone();
            ensure!(
                matches!(
                    status.as_str(),
                    "ready" | "pending" | "in_progress" | "completed" | "cancelled" | "blocked"
                ),
                "PLAN_STATUS_INVALID: {status}"
            );
            let evidence = fields.get("Evidence").cloned();
            let subtasks = fields
                .get("Subtasks")
                .map(|value| serde_yaml::from_str::<Vec<SubtaskSpec>>(value))
                .transpose()?
                .unwrap_or_default();
            steps.push(PlanStep {
                title: title.to_owned(),
                target,
                scope,
                depends_on,
                status,
                evidence,
                subtasks,
                heading: section.section_title.clone(),
            });
        }
        ensure!(!steps.is_empty(), "PLAN_STEPS_EMPTY");
        let plan = Self {
            path: path.to_owned(),
            steps,
            source,
        };
        plan.order()?;
        Ok(plan)
    }

    fn order(&self) -> Result<Vec<usize>> {
        let names = self
            .steps
            .iter()
            .map(|step| step.title.as_str())
            .collect::<BTreeSet<_>>();
        ensure!(names.len() == self.steps.len(), "PLAN_STEP_DUPLICATE");
        for step in &self.steps {
            for dependency in &step.depends_on {
                ensure!(
                    names.contains(dependency.as_str()),
                    "PLAN_DEPENDENCY_UNKNOWN: {dependency}"
                );
            }
        }
        let mut scheduled = BTreeSet::new();
        let mut order = Vec::new();
        while order.len() != self.steps.len() {
            let before = order.len();
            for (index, step) in self.steps.iter().enumerate() {
                if !scheduled.contains(step.title.as_str())
                    && step
                        .depends_on
                        .iter()
                        .all(|dependency| scheduled.contains(dependency.as_str()))
                {
                    scheduled.insert(step.title.as_str());
                    order.push(index);
                }
            }
            ensure!(order.len() > before, "PLAN_DEPENDENCY_CYCLE");
        }
        Ok(order)
    }

    fn complete_source(&self, step: &PlanStep, evidence: &Value) -> Result<String> {
        let mut heading = None;
        let mut title = String::new();
        let mut start = None;
        let mut end = self.source.len();
        for (event, range) in Parser::new(&self.source).into_offset_iter() {
            match event {
                Event::Start(Tag::Heading { .. }) => {
                    heading = Some(range.start);
                    title.clear();
                }
                Event::Text(text) | Event::Code(text) if heading.is_some() => title.push_str(&text),
                Event::End(TagEnd::Heading(_)) => {
                    if start.is_some() {
                        end = heading.context("PLAN_HEADING_INVALID")?;
                        break;
                    }
                    if title == step.heading {
                        start = heading;
                    }
                    heading = None;
                }
                _ => {}
            }
        }
        let start = start.context("PLAN_SOURCE_DRIFT: step heading is missing")?;
        let section = &self.source[start..end];
        let status = format!("- **Status**: {}", step.status);
        ensure!(
            section.lines().filter(|line| line.trim() == status).count() == 1,
            "PLAN_SOURCE_DRIFT: status is ambiguous"
        );
        let mut updated = section.replacen(&status, "- **Status**: completed", 1);
        if let Some(previous) = &step.evidence {
            updated = updated.replacen(
                &format!("- **Evidence**: {previous}"),
                &format!("- **Evidence**: {evidence}"),
                1,
            );
        } else {
            updated = format!("{}\n- **Evidence**: {evidence}\n\n", updated.trim_end());
        }
        Ok(format!(
            "{}{}{}",
            &self.source[..start],
            updated,
            &self.source[end..]
        ))
    }
}

fn plan_profile(profile: &GateProfile, explicit: bool) -> Result<GateProfile> {
    if explicit {
        ensure!(
            profile.persistence == Some(PersistenceAction::Snapshot),
            "PLAN_PROFILE_COMMIT_FORBIDDEN: explicit plans require save: snapshot"
        );
        ensure!(
            profile.gates.iter().all(|gate| gate.tasks.is_some()),
            "PLAN_PROFILE_ROOT_GATES: explicit plan profiles contain only a tasks container"
        );
    }
    let mut profile = profile.clone();
    profile.gates.retain(|gate| gate.tasks.is_some());
    ensure!(!profile.gates.is_empty(), "PLAN_PROFILE_TASKS_MISSING");
    profile.persistence = Some(PersistenceAction::Snapshot);
    if !explicit {
        fn snapshots(gates: &mut [crate::tasks::profile::GateDef]) {
            for gate in gates {
                if gate.receipt || gate.proof.kind() == Some(ProofKind::Delivery) {
                    gate.persistence.post = Some(PersistenceAction::Snapshot);
                    gate.auto_commit = false;
                }
                snapshots(&mut gate.gates);
                if let Some(tasks) = &mut gate.tasks {
                    snapshots(&mut tasks.gates);
                }
            }
        }
        snapshots(&mut profile.gates);
    }
    profile.validate()?;
    for cursor in gate_graph::collect_cursors(&profile)? {
        ensure!(
            [cursor.gate.persistence.pre, cursor.gate.persistence.post]
                .into_iter()
                .all(|save| save.is_none_or(|save| matches!(
                    save,
                    PersistenceAction::Snapshot | PersistenceAction::None
                ))),
            "PLAN_PROFILE_COMMIT_FORBIDDEN"
        );
        ensure!(
            cursor.gate.receipt
                || matches!(
                    cursor.gate.driver(),
                    Some(GateDriver::Command | GateDriver::Agent)
                ),
            "PLAN_RUNNER_REQUIRED: {}",
            cursor.canonical_stage_id
        );
    }
    Ok(profile)
}

/// Run source-owned steps in-place, persisting completed observations as Git snapshots.
pub fn run_plan_workflow(
    root: &Path,
    mut plan: PlanDocument,
    profile: &GateProfile,
    explicit: bool,
) -> Result<Value> {
    let profile = plan_profile(profile, explicit)?;
    let cursors = gate_graph::collect_cursors(&profile)?;
    let profile_digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&profile)?));
    let path = crate::tasks::confined_path(root, &plan.path.to_string_lossy())?;
    for step in &plan.steps {
        for dependency in &step.depends_on {
            ensure!(
                !plan
                    .steps
                    .iter()
                    .any(|step| &step.title == dependency && step.status == "cancelled"),
                "PLAN_DEPENDENCY_INCOMPLETE: cancelled dependency '{dependency}'"
            );
        }
        for scope in &step.scope {
            crate::tasks::confined_path(root, scope)?;
        }
        ensure!(
            step.status != "blocked",
            "PLAN_STEP_BLOCKED: {}",
            step.title
        );
    }
    let baseline = crate::git::git_stdout(
        root,
        &["rev-parse", "--verify", "HEAD"],
        "PLAN_GIT_REQUIRED",
    )?;
    let mut snapshots = Vec::new();
    for index in plan.order()? {
        let step = plan.steps[index].clone();
        if step.status == "cancelled" {
            continue;
        }
        if step.status == "completed" {
            ensure!(
                step.evidence
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty()),
                "PLAN_EVIDENCE_MISSING: {}",
                step.title
            );
            continue;
        }
        for dependency in &step.depends_on {
            ensure!(
                plan.steps
                    .iter()
                    .any(|step| &step.title == dependency && step.status == "completed"),
                "PLAN_DEPENDENCY_INCOMPLETE: {dependency}"
            );
        }
        ensure!(
            fs::read_to_string(&path)? == plan.source,
            "PLAN_SOURCE_DRIFT: source changed during execution"
        );
        let mut roles = RoleObservations::default();
        let mut accepted_candidate = None;
        let outcomes = run_gate_graph(root, &step.scope, &[], &cursors, |cursor, _| {
            ensure!(
                fs::read_to_string(&path)? == plan.source,
                "PLAN_SOURCE_DRIFT: runner changed source authority"
            );
            let before = crate::engine::candidate_fingerprint(root)?;
            if cursor.gate.persistence.immutable {
                ensure!(
                    accepted_candidate.as_ref() == Some(&before),
                    "TASK_CANDIDATE_MISMATCH: plan candidate differs from its accepted predecessor"
                );
            }
            if cursor.gate.persistence.pre == Some(PersistenceAction::Snapshot) {
                gate_snapshot(
                    root,
                    &plan.path,
                    &step.title,
                    &cursor.canonical_stage_id,
                    "pre",
                )?;
            }
            let decision = (|| -> Result<GateDecision> {
                if cursor.gate.receipt {
                    crate::tasks::gates::validate_subtask_delivery(
                        &format!("{}#{}", plan.path.display(), step.title),
                        &step.subtasks,
                    )?;
                    return Ok(GateDecision {
                        passed: true,
                        evidence: json!({"save":"snapshot"}),
                        findings: None,
                    });
                }
                if cursor.gate.driver() == Some(GateDriver::Agent) {
                    return roles.execute(
                        root,
                        &format!("{}#{}", plan.path.display(), step.title),
                        &step.target,
                        &step.scope,
                        cursor,
                        &profile,
                    );
                }
                ensure!(
                    matches!(
                        cursor.gate.proof.kind(),
                        Some(ProofKind::None | ProofKind::Command)
                    ),
                    "PLAN_PROOF_REQUIRED: {}",
                    cursor.canonical_stage_id
                );
                let name = cursor
                    .gate
                    .command
                    .as_deref()
                    .context("PLAN_COMMAND_MISSING")?;
                let definition = profile.commands.get(name).context("PLAN_COMMAND_MISSING")?;
                let commands = resolve_command_plan(root, name, definition, &step.scope, &profile)?;
                ensure!(
                    !commands.is_empty(),
                    "PLAN_COMMAND_MISSING: no scoped command"
                );
                let mut observations = Vec::new();
                let mut passed = true;
                for (_, command, timeout) in commands {
                    let observation =
                        observe_command(root, &command, timeout, definition.output())?;
                    passed &= observation["exit_code"] == 0 && observation["timed_out"] == false;
                    observations.push(observation);
                    if !passed {
                        break;
                    }
                }
                Ok(GateDecision {
                    passed,
                    evidence: json!(observations),
                    findings: None,
                })
            })()?;
            let after = crate::engine::candidate_fingerprint(root)?;
            if cursor.gate.persistence.immutable {
                ensure!(
                    before == after,
                    "TASK_CANDIDATE_MISMATCH: immutable gate changed its candidate"
                );
            }
            if decision.passed {
                accepted_candidate = Some(after);
                if !cursor.gate.receipt
                    && cursor.gate.persistence.post == Some(PersistenceAction::Snapshot)
                {
                    gate_snapshot(
                        root,
                        &plan.path,
                        &step.title,
                        &cursor.canonical_stage_id,
                        "post",
                    )?;
                }
            }
            Ok(decision)
        })?;
        ensure!(
            fs::read_to_string(&path)? == plan.source,
            "PLAN_SOURCE_DRIFT: runner changed plan source"
        );
        ensure!(
            crate::git::git_stdout(root, &["rev-parse", "HEAD"], "PLAN_HEAD_CHANGED")? == baseline,
            "PLAN_HEAD_CHANGED: plan execution must preserve HEAD"
        );
        let evidence = json!({"profile_digest":profile_digest,"outcomes":outcomes});
        let updated = plan.complete_source(&step, &evidence)?;
        crate::fs::atomic_replace(&path, updated.as_bytes())?;
        let plan_rel = plan.path.strip_prefix(root).unwrap_or(&plan.path);
        let plan_tag = format!(
            "{:x}",
            Sha256::digest(plan_rel.to_string_lossy().as_bytes())
        );
        let name = format!(
            "plan-{}-{:x}",
            &plan_tag[..16],
            Sha256::digest(format!(
                "{}\n{}\n{}",
                plan.path.display(),
                profile_digest,
                updated
            ))
        );
        let snapshot =
            git_snapshot::create_named_snapshot(root, &root.join(".forge/tasks.sqlite"), &name);
        match snapshot {
            Ok(snapshot) => snapshots.push(snapshot),
            Err(error) => {
                if fs::read_to_string(&path)? == updated {
                    crate::fs::atomic_replace(&path, plan.source.as_bytes())?;
                }
                return Err(error);
            }
        }
        plan.source = updated;
        plan.steps[index].status = "completed".into();
        plan.steps[index].evidence = Some(evidence.to_string());
    }
    Ok(json!({"status":"completed","path":plan.path,"steps":plan.steps,"snapshots":snapshots}))
}

fn gate_snapshot(root: &Path, path: &Path, step: &str, gate: &str, phase: &str) -> Result<()> {
    let plan_rel = path.strip_prefix(root).unwrap_or(path);
    let plan_tag = format!(
        "{:x}",
        Sha256::digest(plan_rel.to_string_lossy().as_bytes())
    );
    let name = format!(
        "plan-gate-{}-{:x}",
        &plan_tag[..16],
        Sha256::digest(format!(
            "{}\n{step}\n{gate}\n{phase}\n{}",
            path.display(),
            chrono::Utc::now()
                .timestamp_nanos_opt()
                .context("PLAN_CLOCK_INVALID")?
        ))
    );
    git_snapshot::create_named_snapshot(root, &root.join(".forge/tasks.sqlite"), &name)?;
    Ok(())
}
