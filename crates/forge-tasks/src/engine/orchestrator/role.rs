use super::{
    spawner::{
        monitor_runner_process, prepare_runner_session, spawn_role_runner, RunnerSessionPolicy,
        RunnerStream,
    },
    types::RunnerInvocationContext,
    workflow::GateDecision,
};
use crate::{
    engine::gate_graph::GateCursor,
    tasks::profile::{GateProfile, RunnerConfig},
};
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleDecision {
    gate: String,
    session_id: String,
    decision: String,
    proof: Value,
    #[serde(default)]
    findings: Option<Value>,
    #[serde(default)]
    integration_review: Option<crate::milestones::IntegrationReview>,
    #[serde(default)]
    binding: Option<crate::milestones::HandoffBinding>,
}

#[derive(Default)]
pub(super) struct RoleObservations {
    sessions: BTreeMap<String, String>,
    executed_gates: BTreeSet<String>,
    findings: Option<Value>,
    subject_context: Value,
    forbidden_sessions: Vec<String>,
}

impl RoleObservations {
    pub(super) fn note_executed(&mut self, stage: &str) {
        self.executed_gates.insert(stage.to_owned());
    }

    pub(super) fn restore(&mut self, stage: &str, decision: &GateDecision) {
        self.note_executed(stage);
        if let Some(session) = decision.evidence["session_id"].as_str() {
            self.sessions.insert(stage.to_owned(), session.to_owned());
        }
        self.findings = if decision.passed {
            None
        } else {
            decision.findings.clone()
        };
    }
    pub(super) fn set_subject_context(&mut self, context: Value, forbidden_sessions: Vec<String>) {
        self.subject_context = context;
        self.forbidden_sessions = forbidden_sessions;
    }
    /// Run a taskless role, accepting only its validated structured decision.
    pub(super) fn execute(
        &mut self,
        root: &Path,
        subject: &str,
        goal: &str,
        scope: &[String],
        cursor: &GateCursor,
        profile: &GateProfile,
    ) -> Result<GateDecision> {
        let role_id = cursor
            .gate
            .role
            .as_deref()
            .context("WORKFLOW_RUNNER_REQUIRED: gate has no role")?;
        let role = profile
            .roles
            .get(role_id)
            .context("WORKFLOW_RUNNER_REQUIRED: role is missing")?;
        let config = RunnerConfig::from_role(role)
            .context("WORKFLOW_RUNNER_REQUIRED: role has no configured runner")?;
        let session = if role.effective_reuse_on_reject() {
            self.sessions.get(&cursor.canonical_stage_id).cloned()
        } else {
            None
        }
        .map(Ok)
        .unwrap_or_else(|| prepare_runner_session("", RunnerSessionPolicy::Fresh))?;
        ensure!(
            !self.forbidden_sessions.contains(&session),
            "TASK_REVIEW_NOT_INDEPENDENT: role session matches an accepted milestone builder"
        );
        if let Some(prior_ref) = &cursor.gate.independent_from {
            let prior_session = self.sessions.get(prior_ref).or_else(|| {
                self.sessions.iter().find_map(|(stage, session)| {
                    stage
                        .rsplit('.')
                        .next()
                        .filter(|leaf| *leaf == prior_ref)
                        .map(|_| session)
                })
            });
            let prior_executed = self.executed_gates.contains(prior_ref)
                || self.executed_gates.iter().any(|stage| {
                    stage
                        .rsplit('.')
                        .next()
                        .is_some_and(|leaf| leaf == prior_ref)
                });
            if let Some(prior) = prior_session {
                ensure!(
                    &session != prior,
                    "TASK_REVIEW_NOT_INDEPENDENT: role gate reused its predecessor session"
                );
            } else if prior_executed {
                anyhow::bail!(
                    "WORKFLOW_INDEPENDENCE_UNKNOWN: prior gate has no observed role session"
                );
            }
        }
        let model = role.models.first();
        let context = json!({
            "subject": subject, "goal":goal,"gate":cursor.canonical_stage_id,
            "session_id":session,"allowed_write_scope":scope,"profile":profile,
            "gate_definition":cursor.gate,"findings":self.findings,
            "subject_context": self.subject_context,
            "expected_response":{"gate":cursor.canonical_stage_id,"session_id":session,"decision":"pass or reject","proof":"typed proof required by gate_definition.proof","findings":"structured rejection findings, if rejected","integration_review":"For an independent milestone integration review, return {worker_id: session_id, decision: pass or reject, summary: substantive review coverage, scope_findings: [string], contract_findings: [string]}. Omit for other roles.","binding":"With integration_review, return the exact subject_context.binding inspected by this role; otherwise omit."},
            "instructions":"Execute the assigned role in the admitted scope. Return exactly one JSON response on stdout matching expected_response. Send progress to stderr. A process exit code does not submit a decision. Use the supplied independent session identity; do not invoke task claim or task submit for this taskless subject."
        });
        let mut invocation = RunnerInvocationContext {
            task_id: subject.into(),
            stage: cursor.canonical_stage_id.clone(),
            role: role_id.into(),
            model: model.map(|model| model.model.clone()).unwrap_or_default(),
            reasoning: model
                .and_then(|model| model.reasoning.clone())
                .unwrap_or_default(),
            session_id: session.clone(),
            prompt: String::new(),
            worktree: root.canonicalize()?,
        };
        let rendered = invocation.render_prompt_template(
            config.prompt_template.as_deref().unwrap_or(
                "Execute {role} for {task_id} at {stage}. Goal: {goal}. {role_guidance}",
            ),
            goal,
            role.guidance.as_deref().unwrap_or_default(),
        );
        invocation.prompt = format!("{rendered}\n\nGate context:\n{context}");
        let process = spawn_role_runner(&config, &invocation, RunnerSessionPolicy::Resume)?;
        let mut stdout = Vec::new();
        let mut overflow = false;
        let outcome = monitor_runner_process(
            process,
            Duration::from_secs(profile.limits.max_command_timeout_seconds),
            &mut |stream, bytes| {
                if stream == RunnerStream::Stdout {
                    let available = (1024_usize * 1024).saturating_sub(stdout.len());
                    overflow |= bytes.len() > available;
                    stdout.extend_from_slice(&bytes[..available.min(bytes.len())]);
                    Ok(())
                } else {
                    std::io::stderr().write_all(bytes)
                }
            },
        )?;
        ensure!(!overflow, "WORKFLOW_DECISION_TOO_LARGE");
        ensure!(
            outcome.exit_code == Some(0) && !outcome.timed_out,
            "WORKFLOW_RUNNER_FAILED: role did not complete successfully"
        );
        let decision: RoleDecision = serde_json::from_slice(&stdout).context(
            "WORKFLOW_DECISION_REQUIRED: runner must return one structured gate decision",
        )?;
        ensure!(
            decision.session_id == session && decision.gate == cursor.canonical_stage_id,
            "WORKFLOW_DECISION_IDENTITY_MISMATCH"
        );
        ensure!(
            matches!(decision.decision.as_str(), "pass" | "reject"),
            "WORKFLOW_DECISION_INVALID"
        );
        let passed = decision.decision == "pass";
        if !passed {
            ensure!(
                decision.findings.as_ref().is_some_and(|value| value
                    .as_object()
                    .is_some_and(|object| !object.is_empty())),
                "WORKFLOW_FINDINGS_REQUIRED: rejection requires structured findings"
            );
        }
        if let Some(review) = &decision.integration_review {
            ensure!(review.worker_id == session && review.decision == decision.decision, "HANDOFF_REVIEW_IDENTITY_MISMATCH: integration reviewer identity must be the executing role session");
            ensure!(
                decision
                    .binding
                    .as_ref()
                    .is_some_and(|binding| serde_json::to_value(binding).ok().as_ref()
                        == self.subject_context.get("binding")),
                "HANDOFF_EVIDENCE_STALE: review must return the exact supplied content binding"
            );
        }
        // Command proofs must agree with observations made by Forge itself.
        if let Some(command_id) = cursor.gate.command.as_deref() {
            let command = profile
                .commands
                .get(command_id)
                .context("WORKFLOW_COMMAND_MISSING")?;
            let plan =
                crate::milestones::resolve_command_plan(root, command_id, command, scope, profile)?;
            ensure!(!plan.is_empty(), "WORKFLOW_COMMAND_MISSING");
            let mut observations = Vec::new();
            for (_, command_str, timeout) in plan {
                observations.push(crate::milestones::command_gate::observe_command(
                    root,
                    &command_str,
                    timeout,
                    command.output(),
                )?);
            }
            let observed_pass = observations
                .iter()
                .all(|value| value["exit_code"] == 0 && value["timed_out"] == false);
            ensure!(
                passed == observed_pass,
                "WORKFLOW_PROOF_UNOBSERVED: decision disagrees with executed commands"
            );
            if cursor.gate.proof.kind() == Some(crate::tasks::profile::ProofKind::Command) {
                let payload = &decision.proof["command_proof"];
                for (key, observed_key) in [
                    ("tests_passed", "tests_passed"),
                    ("tests_failed", "tests_failed"),
                ] {
                    let observed = observations
                        .iter()
                        .map(|value| value[observed_key].as_u64())
                        .collect::<Option<Vec<_>>>()
                        .map(|values| values.into_iter().sum::<u64>());
                    ensure!(
                        payload[key].as_u64() == observed,
                        "WORKFLOW_PROOF_UNOBSERVED: {key} disagrees with executed commands"
                    );
                }
            }
        } else {
            ensure!(!matches!(cursor.gate.proof.kind(), Some(crate::tasks::profile::ProofKind::Command|crate::tasks::profile::ProofKind::Contract|crate::tasks::profile::ProofKind::SeamTest)), "WORKFLOW_PROOF_UNOBSERVED: executable proof requires a registered observed command");
        }
        crate::tasks::gates::validate_gate_proof(
            &decision.proof,
            &cursor.gate,
            profile,
            passed,
            "seam-test-first",
        )?;
        self.sessions
            .insert(cursor.canonical_stage_id.clone(), session.clone());
        self.findings = if passed {
            None
        } else {
            decision.findings.clone()
        };
        Ok(GateDecision {
            passed,
            evidence: json!({"session_id":session,"proof":decision.proof,"integration_review":decision.integration_review,"binding":decision.binding}),
            findings: decision.findings,
        })
    }
}
