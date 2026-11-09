use super::*;
use crate::store::{check_stage, clear_blackboard, end_claim, load, now, save, Task, TasksStore};
use crate::tasks::{
    canonical_commit_from_prefix,
    gates::{
        review_rollup_summary, validate_durable_receipt_with_profile, validate_gate_proof, Evidence,
    },
    profile::ProofKind,
    Receipt, ReceiptRollup, SubtaskSpec,
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension};
use std::path::Path;

impl TasksStore {
    /// Performs submit.
    pub fn submit(
        &mut self,
        id: &str,
        stage: &str,
        evidence: &Evidence,
        action: &str,
        findings: Option<&serde_json::Value>,
        session_fingerprint: Option<&str>,
    ) -> Result<Task> {
        let mut evidence = evidence.clone();
        self.submit_with_pre_persist(
            id,
            stage,
            &mut evidence,
            action,
            findings,
            session_fingerprint,
            |_| Ok(None),
        )
    }

    /// Performs submit after running a caller-owned operation once gate evidence is valid.
    /// The hook runs while the gate transaction is held and before its state is persisted.
    #[expect(
        clippy::too_many_arguments,
        reason = "the atomic submission hook requires evidence, action, findings, and trusted session context"
    )]
    pub fn submit_with_pre_persist(
        &mut self,
        id: &str,
        stage: &str,
        evidence: &mut Evidence,
        action: &str,
        findings: Option<&serde_json::Value>,
        session_fingerprint: Option<&str>,
        before_persist: impl FnOnce(&Task) -> Result<Option<String>>,
    ) -> Result<Task> {
        let session_fingerprint = crate::store::current_session_fingerprint(session_fingerprint);
        self.assert_id(id)?;
        if evidence.stage != stage {
            bail!("TASK_STAGE_INVALID: task '{id}'. evidence.stage '{}' != submitted stage '{stage}'. Do: set evidence.stage to '{stage}'.", evidence.stage);
        }
        if !matches!(action, "pass" | "reject") {
            bail!("invalid submission action");
        }
        let observed_task = self.inspect(id)?;
        let profile = self.profile_for_task(&observed_task)?;
        let observed_cursor =
            super::super::claim::cursor_for_stage(&profile, &observed_task.stage)?;
        let observed_proof = (action == "pass"
            && matches!(
                observed_cursor.gate.proof.kind(),
                Some(ProofKind::Contract | ProofKind::Command | ProofKind::SeamTest)
            ))
        .then(|| {
            super::super::evidence_compiler::compile_evidence_for_task(self, &observed_task, stage)
        });
        let code_map = if action == "pass"
            && observed_cursor
                .gate
                .proof
                .kind()
                .is_some_and(|kind| matches!(kind, ProofKind::Contract | ProofKind::Command))
        {
            observed_task
                .worktree
                .as_deref()
                .map(|worktree| code_map_connection(Some(&self.connection), Path::new(worktree)))
                .transpose()?
                .flatten()
        } else {
            None
        };
        let checkpoint = self.checkpoint_context()?;
        let mut tx = crate::store::recovery::transaction(&mut self.connection, checkpoint)?;
        tx.enroll(&[id.to_owned()], false)?;
        let mut task = load(&tx, id)?.context(crate::store::task_not_found_message(id))?;
        if task.profile_pin != observed_task.profile_pin
            || task.stage != observed_task.stage
            || task.claim_revision != observed_task.claim_revision
            || task.contract_revision != observed_task.contract_revision
            || task.worker_id != observed_task.worker_id
            || task.worktree != observed_task.worktree
        {
            bail!("TASK_STALE_SUBMISSION: task '{id}'. stage or profile_pin changed concurrently. Do: retry submission.");
        }
        let mut document_update = None;
        let mut auto_delivery: Option<(String, String, std::path::PathBuf)> = None;
        let mut exhausted_attempts: Option<u32> = None;
        let retry: Option<String> = tx
            .query_row(
                "SELECT result FROM task_submissions WHERE task_id=?1 AND revision=?2",
                params![id, evidence.claim_revision],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(retry) = retry {
            let accepted: Submission = serde_json::from_str(&retry)?;
            let mut expected_evidence = accepted.evidence.clone();
            if evidence.commit.is_none() {
                expected_evidence.commit = None;
            }
            if evidence.candidate_baseline_head.is_none() {
                expected_evidence.candidate_baseline_head = None;
            }
            let commit_matches = accepted.evidence.commit == evidence.commit
                || expected_evidence.commit == evidence.commit
                || observed_cursor.gate.persistence.post
                    == Some(crate::tasks::profile::PersistenceAction::Commit)
                || observed_cursor.gate.auto_commit;
            let mut expected_without_commit = expected_evidence.clone();
            expected_without_commit.commit = evidence.commit.clone();
            if (accepted.evidence == *evidence
                || expected_evidence == *evidence
                || (commit_matches && expected_without_commit == *evidence))
                && accepted.action == action
                && accepted.findings.as_ref() == findings
                && accepted.task.contract_revision == task.contract_revision
            {
                return Ok(accepted.task);
            }
            bail!("TASK_STALE_SUBMISSION: task '{id}'. claim_revision {} already submitted with different evidence. Do: resubmit the accepted evidence for claim_revision {}.", evidence.claim_revision, evidence.claim_revision);
        }
        let cursor = super::super::claim::cursor_for_stage(&profile, &task.stage)?;
        let gate_index = cursor.flat_index;
        task.gate = cursor.flat_index;
        let gate_def = cursor.gate.clone();
        check_stage(id, &task.stage, stage)?;
        if task.status != "in_progress" {
            bail!("TASK_STALE_SUBMISSION: task '{id}'. status is '{}'. Do: claim the task before submitting.", task.status);
        }
        let stale_field = if task.claim_revision != evidence.claim_revision {
            Some((
                "claim_revision",
                evidence.claim_revision.to_string(),
                task.claim_revision.to_string(),
            ))
        } else if task.contract_revision != evidence.contract_revision {
            Some((
                "contract_revision",
                evidence.contract_revision.to_string(),
                task.contract_revision.to_string(),
            ))
        } else if task.task_id != evidence.task_id {
            Some(("task_id", evidence.task_id.clone(), task.task_id.clone()))
        } else if task.worker_id.as_deref() != Some(&evidence.worker_id) {
            Some((
                "worker_id",
                evidence.worker_id.clone(),
                task.worker_id.clone().unwrap_or_default(),
            ))
        } else if task.worktree.as_deref() != Some(&evidence.worktree) {
            Some((
                "worktree",
                evidence.worktree.clone(),
                task.worktree.clone().unwrap_or_default(),
            ))
        } else {
            None
        };
        if let Some((field, evidence_value, active_value)) = stale_field {
            bail!("TASK_STALE_SUBMISSION: task '{id}'. evidence {field} {evidence_value} != active {active_value}. Do: resubmit with active {field} {active_value}.");
        }
        if evidence.proof.is_null() {
            bail!("TASK_EVIDENCE_INVALID: task '{id}' for stage '{stage}'. proof is null. Do: provide non-empty proof for stage '{stage}'.");
        }
        if evidence
            .commit
            .as_ref()
            .is_some_and(|commit| commit.trim().is_empty())
        {
            let snapshot = candidate_snapshot(&tx, id, &profile, gate_index)?
                .unwrap_or_else(|| "unavailable".into());
            bail!("TASK_EVIDENCE_INVALID: task '{id}' for stage '{stage}'. commit field is empty. Do: set commit to candidate snapshot {snapshot}.");
        }
        validate_gate_proof(
            &evidence.proof,
            &gate_def,
            &profile,
            action == "pass",
            &task.spec.proof_policy,
        )?;
        if action == "pass" && gate_def.proof.kind() == Some(ProofKind::Review) {
            validate_extended_scope_review(&evidence.proof, &task.spec.extended_scope)?;
        }
        let mut boundary_callers_snapshot = None;
        if action == "pass"
            && matches!(
                gate_def.proof.kind(),
                Some(ProofKind::Contract | ProofKind::Command)
            )
        {
            let worktree = Path::new(&evidence.worktree);
            if let Some(index) = code_map.as_ref() {
                if gate_def.proof.kind() == Some(ProofKind::Contract) {
                    let seam_test_ref = evidence
                        .proof
                        .get("contract_proof")
                        .and_then(|proof| proof.get("seam_test_ref"))
                        .and_then(serde_json::Value::as_str)
                        .context("TASK_EVIDENCE_INVALID: seam test reference required")?;
                    let gate_path = format!("tasks.{stage}");
                    let seam_already_verified = tx
                        .query_row(
                            "SELECT cep.payload
                             FROM command_execution_proofs cep
                             JOIN gate_execution_runs ger ON ger.id = cep.run_id
                             WHERE ger.subject = ?1 AND ger.gate_path = ?2 AND ger.claim_revision = ?3
                             ORDER BY ger.id DESC, cep.attempt_id DESC
                             LIMIT 1",
                            rusqlite::params![id, gate_path, task.claim_revision],
                            |row| row.get::<_, String>(0),
                        )
                        .optional()
                        .ok()
                        .flatten()
                        .and_then(|payload| {
                            serde_json::from_str::<crate::store::CommandExecutionProof>(&payload)
                                .ok()
                        })
                        .is_some_and(|proof| {
                            proof.seam_verified
                                && proof.seam_ref.as_deref() == Some(seam_test_ref.trim())
                        });
                    if seam_already_verified {
                        validate_seam_file_and_symbol(id, worktree, seam_test_ref)?;
                    } else {
                        validate_seam_reachability(
                            index,
                            id,
                            worktree,
                            &task.spec.scope,
                            seam_test_ref,
                        )?;
                    }
                }
                let captured_callers = if gate_def.proof.kind() == Some(ProofKind::Command) {
                    accepted_boundary_callers(&tx, id, &profile, gate_index)?
                } else {
                    None
                };
                validate_public_signature_scope(
                    index,
                    id,
                    worktree,
                    &task.spec.scope,
                    evidence.candidate_baseline_head.as_deref(),
                    captured_callers.as_ref(),
                )?;
                if gate_def.proof.kind() == Some(ProofKind::Contract) {
                    boundary_callers_snapshot = Some(capture_baseline_boundary_callers(
                        index,
                        worktree,
                        &task.spec.scope,
                        evidence.candidate_baseline_head.as_deref(),
                    )?);
                }
            }
        }
        if let Some(observed) = observed_proof {
            let observed = observed?;
            let observed = observed
                .get("proof")
                .context("TASK_AUTO_PROOF_UNAVAILABLE: execution proof is missing")?;
            let key = match gate_def.proof.kind() {
                Some(ProofKind::Contract) => "contract_proof",
                Some(ProofKind::SeamTest) => "seam_test_proof",
                _ => "command_proof",
            };
            let supplied = evidence
                .proof
                .get(key)
                .context("TASK_EVIDENCE_INVALID: typed proof is missing")?;
            let actual = observed
                .get(key)
                .context("TASK_AUTO_PROOF_UNAVAILABLE: observed proof is missing")?;
            if key == "seam_test_proof" {
                crate::tasks::gates::validate_observed_seam_test(supplied, actual)?;
            }
            validate_observed_proof_fields(key, supplied, actual)?;
        }
        if let Some(prior) = gate_def.independent_from.as_deref() {
            let prior_cursor = super::super::claim::cursor_for_stage(&profile, prior)?;
            let prior_evidence = accepted_evidence_for_cursor(&tx, id, &profile, &prior_cursor)?
                .context("TASK_GATES_INVALID: independent gate has no accepted proof")?;
            let builder = prior_evidence.worker_id.clone();
            if builder == evidence.worker_id {
                bail!("TASK_REVIEW_NOT_INDEPENDENT: task '{id}'. reviewer worker '{}' matches builder '{builder}'. Do: dispatch a separate independent reviewer agent session to claim and submit review (do not reuse builder '{builder}').", evidence.worker_id);
            }
            crate::store::validate_session_independence(
                &tx,
                id,
                stage,
                prior,
                &prior_evidence,
                &evidence.worker_id,
                Some(task.claim_revision),
                session_fingerprint.as_deref(),
            )?;
            let build = prior_evidence;
            if gate_def.proof.kind() != Some(ProofKind::Delivery) {
                let canonical = evidence.commit.as_deref().and_then(|supplied| {
                    build
                        .commit
                        .as_deref()
                        .and_then(|snapshot| canonical_commit_from_prefix(supplied, snapshot))
                });
                if let Some(canonical) = canonical {
                    evidence.commit = Some(canonical);
                }
                if build.commit != evidence.commit {
                    let supplied = evidence.commit.as_deref().unwrap_or("missing");
                    let snapshot = build.commit.as_deref().unwrap_or("missing");
                    bail!("TASK_CANDIDATE_MISMATCH: task '{id}'. supplied commit {supplied} != candidate snapshot {snapshot}. Do: resubmit with commit {snapshot}.");
                }
            }
        }
        if gate_def.proof.kind() == Some(ProofKind::Delivery) && action == "pass" {
            let gate_sequence = super::super::claim::gate_sequence(&profile)?;
            let (_snapshot_gate, build) = {
                let mut found = None;
                for cursor in gate_sequence[..gate_index]
                    .iter()
                    .rev()
                    .filter(|c| c.gate.sha_snapshot)
                {
                    if let Some(evidence) = accepted_evidence_for_cursor(&tx, id, &profile, cursor)?
                    {
                        found = Some((cursor, evidence));
                        break;
                    }
                }
                found.context(
                    "TASK_GATES_INVALID: delivery requires a prior accepted sha_snapshot gate",
                )?
            };
            let candidate_snapshot = build.commit.as_deref().unwrap_or("missing");
            if !gate_def.auto_commit {
                let canonical = evidence.commit.as_deref().and_then(|supplied| {
                    canonical_commit_from_prefix(supplied, candidate_snapshot)
                });
                if let Some(canonical) = canonical {
                    evidence.commit = Some(canonical);
                }
            }
            let receipt_commit = evidence
                .commit
                .as_deref()
                .context(format!("TASK_EVIDENCE_INVALID: task '{id}' for stage 'deliver'. delivery evidence missing required commit. Do: set commit to candidate snapshot {candidate_snapshot}."))?;
            let candidate_baseline = build.candidate_baseline_head.as_deref().context(
                "TASK_CANDIDATE_BASELINE_INVALID: accepted candidate has no captured baseline",
            )?;
            if evidence.candidate_baseline_head.as_deref() != Some(candidate_baseline) {
                bail!("TASK_CANDIDATE_BASELINE_MISMATCH: delivery baseline differs from the accepted candidate");
            }
            if !gate_def.auto_commit && build.commit.as_deref() != Some(receipt_commit) {
                bail!("TASK_CANDIDATE_MISMATCH: task '{id}'. supplied commit {receipt_commit} != candidate snapshot {candidate_snapshot}. Do: resubmit with commit {candidate_snapshot}.");
            }
            let mut review_proofs = Vec::with_capacity(gate_def.review_sources.len());
            for review_name in &gate_def.review_sources {
                let mut review_gate = super::super::claim::cursor_for_stage(&profile, review_name)?;
                let configured_source = review_gate.canonical_stage_id.clone();
                let mut review = accepted_evidence_for_cursor(&tx, id, &profile, &review_gate)?;
                if review.as_ref().is_some_and(|r| r.commit != build.commit) {
                    if let Some((branch_prefix, _)) = _snapshot_gate.gate.id.rsplit_once('.') {
                        let branch_review_name = format!("{branch_prefix}.{review_name}");
                        if let Ok(branch_gate) =
                            super::super::claim::cursor_for_stage(&profile, &branch_review_name)
                        {
                            if let Ok(Some(branch_review)) =
                                accepted_evidence_for_cursor(&tx, id, &profile, &branch_gate)
                            {
                                if branch_review.commit == build.commit {
                                    review_gate = branch_gate;
                                    review = Some(branch_review);
                                }
                            }
                        }
                    }
                }
                let review =
                    review.context("TASK_GATES_INVALID: review source has no accepted proof")?;
                if review.commit != build.commit {
                    bail!("TASK_CANDIDATE_MISMATCH: task '{id}'. review source '{review_name}' commit != candidate snapshot {candidate_snapshot}. Do: resubmit review evidence with commit {candidate_snapshot}.");
                }
                validate_gate_proof(
                    &review.proof,
                    &review_gate.gate,
                    &profile,
                    true,
                    &task.spec.proof_policy,
                )?;
                if review_gate.gate.proof.kind() == Some(ProofKind::Review) {
                    validate_extended_scope_review(&review.proof, &task.spec.extended_scope)?;
                }
                review_proofs.push((configured_source, review.proof.clone()));
            }
            let claim_worktree: String = tx.query_row(
                "SELECT worktree FROM task_claims WHERE task_id=?1 AND revision=?2 AND ended=0",
                params![id, task.claim_revision],
                |row| row.get(0),
            )?;
            let path =
                super::super::confined_path(Path::new(&claim_worktree), &task.milestone_ref)?;
            let should_auto_commit = match gate_def.persistence.post {
                Some(crate::tasks::profile::PersistenceAction::Commit) => true,
                Some(
                    crate::tasks::profile::PersistenceAction::Snapshot
                    | crate::tasks::profile::PersistenceAction::Logfile
                    | crate::tasks::profile::PersistenceAction::None,
                ) => false,
                None => gate_def.auto_commit,
            };
            if should_auto_commit {
                auto_delivery = Some((
                    build
                        .commit
                        .as_deref()
                        .context("TASK_SNAPSHOT_NOT_FOUND")?
                        .to_owned(),
                    candidate_baseline.to_owned(),
                    path.clone(),
                ));
            }
            let text = std::fs::read_to_string(&path)?;
            let milestone = super::super::workspaces::parse_milestone_manifest(
                &text,
                Path::new(&task.milestone_ref),
                &self.repository,
                &self.default_project,
                &self.project_owners,
                &self.project_aliases,
            )?;
            let spec = milestone
                .tasks
                .iter()
                .find(|s| milestone.task_id(s) == id)
                .context("TASK_RECEIPT_INVALID: task not found")?;
            if milestone.digest(spec)? != task.digest {
                bail!("TASK_RECEIPT_INVALID: specification mismatch");
            }
            let review_value = if review_proofs.len() == 1 {
                review_proofs[0].1.clone()
            } else {
                serde_json::json!({"review_sources": review_proofs.iter().cloned().collect::<std::collections::BTreeMap<_, _>>()})
            };
            let outcomes = tx
                .prepare(
                    "SELECT topic,payload FROM task_blackboard \
                 WHERE task_id=?1 AND topic IN ('decisions','deferred') \
                 ORDER BY created_at,id",
                )?
                .query_map([id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .map(|(topic, payload)| format!("{topic}: {payload}"))
                .collect::<Vec<_>>();
            let rollup = ReceiptRollup {
                verified_invariants: task.applicable_invariants.clone(),
                architectural_notes: outcomes,
                review_summary: review_rollup_summary(&review_proofs)?,
            };
            let document_commit = if gate_def.auto_commit {
                None
            } else {
                Some(receipt_commit.to_owned())
            };
            let generated = Receipt {
                delivery_gate: Some(cursor.canonical_stage_id.clone()),
                candidate_gate: Some(_snapshot_gate.canonical_stage_id.clone()),
                commit: document_commit.clone(),
                contract_revision: task.contract_revision,
                passed_at: chrono::Utc::now().to_rfc3339(),
                evidence: build.proof.clone(),
                review: review_value.clone(),
                decision: "pass".into(),
                rollup: Some(rollup),
            };
            let receipt = match (spec.status.as_deref(), spec.receipt.as_ref()) {
                (Some("completed"), Some(existing)) => {
                    // The document may have been replaced immediately before a process crash.
                    // Reuse only the exact accepted outcome, retaining its original timestamp.
                    validate_receipt(
                        existing,
                        &task,
                        &build,
                        &review_value,
                        document_commit.as_deref(),
                        &profile,
                    )?;
                    if existing.rollup != generated.rollup {
                        bail!("TASK_RECEIPT_INVALID: rollup differs from accepted task context");
                    }
                    existing.clone()
                }
                (Some("completed"), None) | (_, Some(_)) => {
                    bail!("TASK_RECEIPT_INVALID: incomplete terminal receipt")
                }
                _ => generated,
            };
            validate_receipt(
                &receipt,
                &task,
                &build,
                &review_value,
                document_commit.as_deref(),
                &profile,
            )?;
            let (_, task_ref) = id.rsplit_once(':').context("invalid task identity")?;
            let mut stmt = tx.prepare(
                "SELECT subtask_ref, title, status, evidence FROM task_subtasks WHERE task_id=?1 ORDER BY rowid",
            )?;
            let current_subtasks = stmt
                .query_map([id], |r| {
                    Ok(SubtaskSpec {
                        subtask_ref: r.get(0)?,
                        title: r.get(1)?,
                        status: r.get(2)?,
                        evidence: r.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let final_subtasks = if !current_subtasks.is_empty() {
                current_subtasks
            } else {
                task.spec.subtasks.clone()
            };
            crate::tasks::gates::validate_subtask_delivery(id, &final_subtasks)?;
            let written = crate::engine::milestones::render_task_receipt(
                &text,
                task_ref,
                &receipt,
                &final_subtasks,
            )?;
            task.spec.subtasks = final_subtasks;
            document_update = Some((path, text, written));
            task.receipt = Some(receipt);
            task.completed_at = Some(now());
            task.status = "completed".into();
        } else if action == "pass" {
            let next = super::super::claim::advance_gate_cursor_for_task(
                &profile,
                Some(&task.stage),
                &task,
            )?;
            task.gate = next.flat_index;
            task.stage = next.canonical_stage_id;
            task.status = "ready".into();
        } else {
            let findings = findings.context("reject requires findings")?;
            let reject_count: i64 = tx.query_row(
                "SELECT count(*) FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='rejected'",
                params![id, evidence.stage],
                |r| r.get(0),
            )?;
            let cursors = super::super::claim::collect_cursors_for_task(&profile, Some(&task))?;
            let resolution = super::super::claim::resolve_fail_target(
                &cursors,
                gate_index,
                Some(findings),
                (reject_count as u32).saturating_add(1),
                |candidate| {
                    super::super::claim::task_matches_gate_condition(
                        candidate.when.as_deref(),
                        &task,
                    )
                },
            )?;
            if resolution.exhausted {
                task.status = "exhausted".into();
                exhausted_attempts = resolution.max_attempts;
            }
            tx.execute(
                "INSERT INTO task_findings VALUES(?1,?2,?3)",
                params![id, task.claim_revision, serde_json::to_string(findings)?],
            )?;
            if exhausted_attempts.is_none() {
                let target = resolution.target.with_context(|| {
                    format!(
                        "TASK_STAGE_INVALID: task '{id}'. reject target stage '{}' has no applicable gate. Do: set reject_to to a stage that exists in the profile.",
                        resolve_rejection_target(&gate_def, Some(findings), task.stage.as_str())
                    )
                })?;
                task.gate = target.flat_index;
                task.stage = target.canonical_stage_id;
                task.status = "ready".into();
            }
        }
        let mut provisional_rollback = None;
        let pre_persist = (|| -> Result<()> {
            if let Some((path, original, written)) = &document_update {
                provisional_rollback = Some(ProvisionalReceiptRollback {
                    path: path.clone(),
                    original: original.clone(),
                    armed: true,
                });
                crate::core::fs::atomic_replace(path, written.as_bytes())?;
            }
            let created_delivery = before_persist(&task)?;
            match (auto_delivery.as_ref(), created_delivery) {
                (Some((candidate, candidate_baseline, path)), Some(landed)) => {
                    let worktree = Path::new(&evidence.worktree);
                    let relative = path.strip_prefix(worktree).context(
                        "TASK_DELIVERY_COMMIT_INVALID: receipt path is outside the worktree",
                    )?;
                    let written = document_update
                        .as_ref()
                        .map(|(_, _, written)| written.as_str())
                        .context(
                            "TASK_DELIVERY_COMMIT_INVALID: provisional receipt document is missing",
                        )?;
                    validate_delivery_commit(
                        worktree,
                        candidate,
                        candidate_baseline,
                        &task.spec.scope,
                        &landed,
                        relative,
                        written,
                    )?;
                    evidence.commit = Some(landed.clone());
                    let flattened_profile =
                        super::super::claim::flattened_profile(&profile, Some(&task))?;
                    let receipt = task
                        .receipt
                        .as_mut()
                        .context("TASK_RECEIPT_INVALID: delivery receipt is missing")?;
                    receipt.commit = Some(landed);
                    validate_durable_receipt_with_profile(
                        receipt,
                        task.contract_revision,
                        &flattened_profile,
                    )?;
                }
                (Some(_), None) => {
                    bail!("TASK_DELIVERY_COMMIT_REQUIRED: auto_commit delivery must create a hooked commit");
                }
                (None, Some(landed)) => {
                    if gate_def.persistence.post
                        == Some(crate::tasks::profile::PersistenceAction::Commit)
                    {
                        evidence.commit = Some(landed);
                    } else {
                        bail!(
                            "TASK_DELIVERY_COMMIT_INVALID: unexpected commit result for this gate"
                        );
                    }
                }
                (None, None) => {}
            }
            Ok(())
        })();
        if let Err(error) = pre_persist {
            if let Some(rollback) = provisional_rollback.as_mut() {
                rollback
                    .restore()
                    .context("failed to restore milestone document after delivery failure")?;
            }
            return Err(error);
        }
        tx.execute(
            "INSERT INTO task_gates (task_id, gate, revision, state, evidence)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(task_id, gate, revision) DO UPDATE SET
                state=excluded.state,
                evidence=excluded.evidence",
            params![
                id,
                evidence.stage,
                task.claim_revision,
                if action == "pass" {
                    "passed"
                } else {
                    "rejected"
                },
                serde_json::to_string(evidence)?
            ],
        )?;
        if let Ok((bare_milestone, task_breadcrumb)) = super::super::claim::breadcrumb(id, &cursor)
        {
            let namespace_prefix = format!("{}/{}/", self.repository, self.project);
            let milestone_id = format!("{namespace_prefix}{bare_milestone}");
            let breadcrumb = format!("{namespace_prefix}{task_breadcrumb}");
            let subject_ref = id.rsplit_once(':').map(|(_, r)| r).unwrap_or(id);
            let worktree = Path::new(&evidence.worktree);
            let git_tree_sha = crate::git::git_stdout(
                worktree,
                &["rev-parse", "--verify", "HEAD^{tree}"],
                "TASK_CHECKPOINT_STATE_INVALID",
            )
            .unwrap_or_default();
            let worker_id = evidence.worker_id.clone();
            let proof_payload = serde_json::to_string(evidence)?;
            let action_str = if action == "pass" {
                "advance"
            } else {
                "reject"
            };
            let proof_kind = gate_def.proof.kind().map(|k| k.as_str()).unwrap_or("none");
            tx.execute(
                "INSERT INTO gate_transitions (
                    milestone_id, subject, gate_path, action, proof_kind, proof_payload,
                    git_tree_sha, worker_id
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    milestone_id,
                    id,
                    cursor.gate_path,
                    action_str,
                    proof_kind,
                    proof_payload,
                    git_tree_sha,
                    worker_id,
                ],
            )?;
            let transition_id = tx.last_insert_rowid();

            let checkpoint_state = serde_json::json!({
                "task_id": id,
                "stage": cursor.stage_id.clone(),
                "gate_path": cursor.gate_path.clone(),
                "action": action,
                "transition_id": transition_id,
                "status": task.status.clone(),
                "next_stage": task.stage.clone(),
                "claim_revision": task.claim_revision,
                "contract_revision": task.contract_revision,
            });
            let mut payload_envelope = serde_json::Map::new();
            payload_envelope.insert("_forge_checkpoint_payload_v1".to_owned(), checkpoint_state);
            let state_payload_str =
                serde_json::to_string(&serde_json::Value::Object(payload_envelope))?;
            tx.execute(
                "INSERT INTO gate_checkpoints (
                    breadcrumb, milestone_id, subject, gate_path, transition_id,
                    git_tree_sha, state_payload
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT(breadcrumb) DO UPDATE SET
                    milestone_id=excluded.milestone_id,
                    subject=excluded.subject,
                    gate_path=excluded.gate_path,
                    transition_id=excluded.transition_id,
                    git_tree_sha=excluded.git_tree_sha,
                    state_payload=excluded.state_payload,
                    updated_at=CURRENT_TIMESTAMP",
                params![
                    breadcrumb,
                    milestone_id,
                    subject_ref,
                    cursor.gate_path,
                    transition_id,
                    git_tree_sha,
                    state_payload_str,
                ],
            )?;
        }
        end_claim(&tx, id)?;
        task.worker_id = None;
        task.worktree = None;
        save(&tx, &task)?;
        let submission = Submission {
            evidence: evidence.clone(),
            action: action.into(),
            findings: findings.cloned(),
            task: task.clone(),
            boundary_callers: boundary_callers_snapshot.clone(),
        };
        tx.execute(
            "INSERT INTO task_submissions VALUES(?1,?2,?3)",
            params![
                id,
                evidence.claim_revision,
                serde_json::to_string(&submission)?
            ],
        )?;
        if task.status == "completed" {
            clear_blackboard(&tx, id)?;
        }
        if let Some((path, _, written)) = &document_update {
            crate::core::fs::atomic_replace(path, written.as_bytes())?;
        }
        if let Err(error) = tx.commit() {
            if crate::store::recovery::is_pending(&error) {
                if let Some(rollback) = provisional_rollback.as_mut() {
                    rollback.disarm();
                }
                return Err(error);
            }
            if let Some(rollback) = provisional_rollback.as_mut() {
                rollback
                    .restore()
                    .context("failed to restore milestone document after SQLite commit error")?;
            } else if let Some((path, original, _)) = &document_update {
                crate::core::fs::atomic_replace(path, original.as_bytes())
                    .context("failed to restore milestone document after SQLite commit error")?;
            }
            return Err(error);
        }
        if let Some(rollback) = provisional_rollback.as_mut() {
            rollback.disarm();
        }
        if let Some(max_attempts) = exhausted_attempts {
            bail!("TASK_ATTEMPTS_EXHAUSTED: gate '{stage}' reached max_attempts ({max_attempts}) without passing");
        }
        Ok(task)
    }
}

pub(super) fn validate_observed_proof_fields(
    key: &str,
    supplied: &serde_json::Value,
    actual: &serde_json::Value,
) -> Result<()> {
    for field in match key {
        "contract_proof" => &["seam_test_ref", "red_exit_code"][..],
        "seam_test_proof" => &["exit_code", "tests_passed", "tests_failed"][..],
        _ => &["command", "exit_code", "tests_passed", "tests_failed"][..],
    } {
        if supplied.get(field) != actual.get(field) {
            bail!("TASK_EVIDENCE_INVALID: {key}.{field} differs from the claim-bound execution");
        }
    }
    Ok(())
}

fn candidate_snapshot(
    conn: &rusqlite::Connection,
    id: &str,
    profile: &crate::tasks::profile::GateProfile,
    gate_index: usize,
) -> Result<Option<String>> {
    let gate_sequence = super::super::claim::gate_sequence(profile)?;
    let Some(snapshot_gate) = gate_sequence[..gate_index]
        .iter()
        .rev()
        .find(|cursor| cursor.gate.sha_snapshot)
    else {
        return Ok(None);
    };
    Ok(
        accepted_evidence_for_cursor(conn, id, profile, snapshot_gate)?
            .and_then(|evidence| evidence.commit),
    )
}

fn validate_receipt(
    receipt: &Receipt,
    task: &Task,
    build: &Evidence,
    review: &serde_json::Value,
    expected_commit: Option<&str>,
    profile: &crate::tasks::profile::GateProfile,
) -> Result<()> {
    let flattened_profile = super::super::claim::flattened_profile(profile, Some(task))?;
    validate_durable_receipt_with_profile(receipt, task.contract_revision, &flattened_profile)?;
    if let Some(candidate_gate) = receipt.candidate_gate.as_deref() {
        let candidate = super::super::claim::cursor_for_stage(profile, candidate_gate)?;
        if candidate_gate != build.stage
            || !candidate.gate.sha_snapshot
            || candidate.flat_index >= task.gate
        {
            bail!("TASK_RECEIPT_INVALID: candidate gate does not precede delivery");
        }
    }
    if receipt.commit.as_deref() != expected_commit
        || receipt
            .delivery_gate
            .as_deref()
            .is_some_and(|gate| gate != task.stage)
        || receipt.contract_revision != task.contract_revision
        || receipt.evidence != build.proof
        || &receipt.review != review
        || receipt.decision != "pass"
        || chrono::DateTime::parse_from_rfc3339(&receipt.passed_at).is_err()
        || receipt
            .rollup
            .as_ref()
            .is_some_and(|rollup| rollup.verified_invariants != task.applicable_invariants)
    {
        bail!("TASK_RECEIPT_INVALID: accepted proof mismatch");
    }
    Ok(())
}
