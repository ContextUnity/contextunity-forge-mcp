use super::super::support::*;
use contextunity_forge_mcp::core::tasks::profile::{GateDef, GateProfile};
use contextunity_forge_mcp::db::tasks_store::Task;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Owns the real task store; filesystem scenarios retain the canonical Git fixture.
pub(crate) struct TaskHarness {
    pub store: TasksStore,
    pub id: String,
    root: PathBuf,
    _workspace: Option<Workspace>,
    in_memory: bool,
}

mod setup;
mod workspaces;

impl TaskHarness {
    pub(super) fn git_output(root: &Path, args: &[&str]) -> std::process::Output {
        crate::acdd::snapshot::git_command(root, args)
            .output()
            .expect("Git command should launch")
    }

    pub fn persisted_gate_proof(&self, stage: &str) -> Value {
        self.persisted_gate_evidence(stage)["proof"].clone()
    }

    pub fn persisted_gate_evidence(&self, stage: &str) -> Value {
        let details = self.store.inspect_details(&self.id).unwrap();
        let gate = details["gates"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|gate| gate["stage"] == stage && gate["state"] == "passed")
            .unwrap();
        serde_json::from_str::<Value>(gate["evidence"].as_str().unwrap()).unwrap()
    }

    pub fn persisted_gate_evidence_for_state(
        &self,
        stage: &str,
        state: &str,
    ) -> anyhow::Result<Value> {
        let text: String = self.store.connection.query_row(
            "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate=?2 AND state=?3 ORDER BY revision DESC LIMIT 1",
            rusqlite::params![self.id, stage, state],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn persisted_submission_proof(&self, revision: u64) -> Value {
        self.persisted_submission_evidence(revision)["proof"].clone()
    }

    pub fn persisted_submission_evidence(&self, revision: u64) -> Value {
        let text: String = self
            .store
            .connection
            .query_row(
                "SELECT result FROM task_submissions WHERE task_id=?1 AND revision=?2",
                rusqlite::params![self.id, revision],
                |row| row.get(0),
            )
            .unwrap();
        serde_json::from_str::<Value>(&text).unwrap()["evidence"].clone()
    }

    pub fn evidence_for_claim(claim: &Value, proof: Value) -> Evidence {
        Evidence {
            task_id: claim["task_id"].as_str().unwrap().into(),
            stage: claim["stage"].as_str().unwrap().into(),
            claim_revision: claim["claim_revision"].as_u64().unwrap(),
            contract_revision: claim["contract_revision"].as_u64().unwrap(),
            worker_id: claim["worker_id"].as_str().unwrap().into(),
            worktree: claim["worktree"].as_str().unwrap().into(),
            commit: None,
            candidate_baseline_head: None,
            proof,
        }
    }

    pub fn attach(workspace: &Workspace, id: &str) -> Self {
        Self {
            store: tasks::store_for_task(workspace.root(), id).unwrap(),
            id: id.into(),
            root: workspace.root().to_owned(),
            _workspace: None,
            in_memory: false,
        }
    }

    pub fn claim_gate(&self, stage: &str, worker: &str) -> Task {
        Self::claim_at(self.root(), &self.id, stage, worker).unwrap();
        self.store.inspect(&self.id).unwrap()
    }

    pub fn claim_gate_in_session(
        &mut self,
        stage: &str,
        worker: &str,
        session: Option<&str>,
    ) -> anyhow::Result<Task> {
        if self.in_memory {
            return self.store.claim(
                &self.id,
                stage,
                worker,
                &self.root.to_string_lossy(),
                session,
            );
        }
        tasks::claim(
            self.root(),
            tasks::Claim {
                task_id: self.id.clone(),
                stage: stage.into(),
                worker_id: worker.into(),
                worktree: self.root().to_string_lossy().into_owned(),
                session_fingerprint: session.map(str::to_owned),
                ..Default::default()
            },
        )?;
        self.store.inspect(&self.id)
    }

    pub fn run_cli_for_root(root: &Path, args: &[&str]) -> std::process::Output {
        let mut command_args = vec![OsString::from("--root"), root.as_os_str().to_os_string()];
        command_args.extend(args.iter().map(OsString::from));
        crate::acdd::mcp::in_process_cli::run_cli_args(&command_args)
    }

    pub fn run_cli_task(&self, args: &[&str]) -> std::process::Output {
        let mut task_args = vec!["--format", "json", "task"];
        task_args.extend_from_slice(args);
        Self::run_cli_for_root(self.root(), &task_args)
    }

    pub fn submit_evidence_in_session(
        &mut self,
        proof: &Evidence,
        session: Option<&str>,
    ) -> anyhow::Result<Task> {
        if self.in_memory {
            return self
                .store
                .submit(&self.id, &proof.stage, proof, "pass", None, session);
        }
        tasks::submit(
            self.root(),
            Self::pass_request_in_session(&self.id, &proof.stage, proof, session),
        )?;
        self.store.inspect(&self.id)
    }

    pub fn submit_rejection_in_session(
        &mut self,
        proof: &Evidence,
        session: Option<&str>,
        findings: Option<&Value>,
    ) -> anyhow::Result<Task> {
        if self.in_memory {
            return self
                .store
                .submit(&self.id, &proof.stage, proof, "reject", findings, session);
        }
        let mut request = Self::pass_request_in_session(&self.id, &proof.stage, proof, session);
        request.action = tasks::Action::Reject;
        request.findings = findings.cloned();
        tasks::submit(self.root(), request)?;
        self.store.inspect(&self.id)
    }

    pub fn submit_pass(&mut self) -> anyhow::Result<Value> {
        let task = self.store.inspect(&self.id)?;
        let proof = self.evidence_for_gate(&task)?;
        let session = Self::session_for(&proof.worker_id);
        self.submit_evidence_in_session(&proof, Some(&session)).map(
            |task| json!({"task_id": task.task_id, "status": task.status, "stage": task.stage}),
        )
    }

    /// Executes every preceding production gate, including observed command proofs and Git snapshots.
    /// Return the profile resolved for this task, including a task-level profile pin.
    pub fn profile(&self) -> anyhow::Result<Arc<GateProfile>> {
        let task = self.store.inspect(&self.id)?;
        self.store.profile_for_task(&task)
    }

    /// Return the profile's executable leaves in their declared order.
    pub fn executable_gates(&self) -> anyhow::Result<Vec<GateDef>> {
        Ok(self
            .profile()?
            .executable_gate_paths()
            .into_iter()
            .map(|(path, gate)| {
                let mut gate = gate.clone();
                gate.id = path;
                gate
            })
            .collect())
    }

    /// Choose a deterministic worker identity from the production-resolved gate definition.
    pub fn worker_for_gate(&self, gate: &GateDef) -> String {
        self.worker_for_stage(&gate.id)
            .expect("profile gate passed to worker_for_gate must resolve through its task profile")
    }

    /// Resolve a worker by persisted stage when the caller has a qualified cursor path.
    pub fn worker_for_stage(&self, stage: &str) -> anyhow::Result<String> {
        let worktree = self.root().to_string_lossy().into_owned();
        let gate = self
            .store
            .resolve_gate_for_task(&self.id, &worktree, Some(stage))?;
        let role = gate.role.as_deref().unwrap_or("task-worker");
        let Some(prior) = gate.independent_from.as_deref() else {
            return Ok(role.to_owned());
        };
        let prior_worker = self
            .persisted_gate_evidence_for_state(prior, "passed")
            .ok()
            .and_then(|value| {
                value
                    .get("worker_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
        if prior_worker.as_deref() == Some(role) {
            Ok(format!("{role}-independent-{}", gate.id))
        } else {
            Ok(role.to_owned())
        }
    }

    /// Claim the task's actual next gate using the production engine and an isolated session.
    pub fn claim_next(&mut self) -> anyhow::Result<Task> {
        self.claim_next_with_session(None)
    }

    /// Claim the profile's next gate with a supplied session for collision tests.
    pub fn claim_next_with_session(&mut self, session: Option<&str>) -> anyhow::Result<Task> {
        let current = self.store.inspect(&self.id)?;
        if current.status == "completed" {
            anyhow::bail!(
                "TASK_ALREADY_COMPLETED: task '{}' has no next gate",
                self.id
            );
        }
        let worktree = self.root().to_string_lossy().into_owned();
        let gate = self
            .store
            .resolve_gate_for_task(&self.id, &worktree, None)?;
        let worker = self.worker_for_gate(&gate);
        let isolated_session = Self::session_for(&worker);
        self.claim_gate_in_session("", &worker, Some(session.unwrap_or(&isolated_session)))
    }

    /// Execute and pass each actual gate until the requested stage is reached.
    /// `completed` is accepted as a completion target for existing lifecycle callers.
    pub fn advance_to(&mut self, target: &str) -> anyhow::Result<Task> {
        use std::collections::HashSet;

        let completion_target = target == "completed";
        let canonical_target = if completion_target {
            None
        } else {
            let profile = self.profile()?;
            let index = profile.index_of(target).ok_or_else(|| {
                anyhow::anyhow!(
                    "TASK_STAGE_UNKNOWN: target stage '{target}' is absent from task profile"
                )
            })?;
            Some(
                profile
                    .executable_gate_paths()
                    .get(index)
                    .map(|(path, _)| path.clone())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                        "TASK_STAGE_UNKNOWN: target stage '{target}' is absent from task profile"
                    )
                    })?,
            )
        };
        let mut visited = HashSet::new();
        loop {
            let current = self.store.inspect(&self.id)?;
            if completion_target && current.status == "completed" {
                return Ok(current);
            }
            if canonical_target.as_deref() == Some(current.stage.as_str()) {
                return Ok(current);
            }
            if current.status == "completed" {
                anyhow::bail!(
                    "TASK_TARGET_UNREACHABLE: task '{}' completed before stage '{target}'",
                    self.id
                );
            }
            if !visited.insert(current.stage.clone()) {
                anyhow::bail!(
                    "TASK_TRANSITION_CYCLE: task '{}' revisited stage '{}' before reaching '{target}'",
                    self.id,
                    current.stage
                );
            }
            let prior = (current.stage.clone(), current.status.clone());
            self.claim_next()?;
            self.submit_pass()?;
            let advanced = self.store.inspect(&self.id)?;
            if (advanced.stage.as_str(), advanced.status.as_str())
                == (prior.0.as_str(), prior.1.as_str())
            {
                anyhow::bail!(
                    "TASK_GATE_STALLED: task '{}' did not advance from stage '{}'",
                    self.id,
                    prior.0
                );
            }
        }
    }

    /// Execute every gate through the production profile transitions to delivery.
    pub fn advance_to_completion(&mut self) -> anyhow::Result<Task> {
        self.advance_to("completed")
    }

    pub fn evidence_for_gate(&self, claimed: &Task) -> anyhow::Result<Evidence> {
        use contextunity_forge_mcp::core::tasks::profile::ProofKind;

        let profile = self.profile()?;
        let (gate_path, gate) = profile
            .executable_gate_paths()
            .into_iter()
            .find(|(path, _)| {
                path == &claimed.stage || path.rsplit('.').next() == Some(claimed.stage.as_str())
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "TASK_STAGE_UNKNOWN: claimed stage '{}' is absent from task profile",
                    claimed.stage
                )
            })?;
        match gate.proof.kind() {
            Some(ProofKind::Contract | ProofKind::Command | ProofKind::Delivery) => {
                anyhow::ensure!(
                    !self.in_memory,
                    "TASK_HARNESS_PROOF_UNAVAILABLE: gate '{}' requires production-observed evidence, unavailable from an in-memory store",
                    claimed.stage
                );
                let root = std::path::Path::new(claimed.worktree.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("TASK_HARNESS_CLAIM_MISSING: worktree is absent")
                })?);
                let observed = match tasks::verify_task(root, &claimed.task_id, None) {
                    Ok(result) if gate.proof.kind() == Some(ProofKind::Delivery) => result,
                    Ok(result) if !result["evidence"].is_null() => result["evidence"].clone(),
                    Ok(result) if gate.proof.kind() == Some(ProofKind::Contract) => {
                        tasks::compile_evidence_envelope(root, &claimed.task_id, &claimed.stage)
                            .map_err(|compile_error| anyhow::anyhow!(
                                "TASK_HARNESS_PROOF_UNAVAILABLE: contract gate '{}' did not yield a production-compiled red proof; verify returned {}: {compile_error}",
                                gate_path,
                                result["error"]
                            ))?
                    }
                    Ok(result) => anyhow::bail!(
                        "TASK_HARNESS_PROOF_UNAVAILABLE: production verification for gate '{}' returned no evidence: {}",
                        gate_path,
                        result["error"]
                    ),
                    Err(verify_error) if gate.proof.kind() == Some(ProofKind::Contract) => {
                        tasks::compile_evidence_envelope(root, &claimed.task_id, &claimed.stage)
                            .map_err(|compile_error| anyhow::anyhow!(
                                "TASK_HARNESS_PROOF_UNAVAILABLE: contract gate '{}' did not yield a production-compiled red proof; verify failed: {verify_error}; compile failed: {compile_error}",
                                gate_path
                            ))?
                    }
                    Err(error) => anyhow::bail!(
                        "TASK_HARNESS_PROOF_UNAVAILABLE: production verification for gate '{}' failed: {error}",
                        gate_path
                    ),
                };
                let proof: Evidence = serde_json::from_value(observed)?;
                anyhow::ensure!(
                    proof.claim_revision == claimed.claim_revision
                        && proof.contract_revision == claimed.contract_revision,
                    "TASK_HARNESS_PROOF_STALE: observed proof for gate '{}' is not bound to the current claim",
                    claimed.stage
                );
                let key = gate.proof.kind().expect("proof kind checked").as_str();
                anyhow::ensure!(
                    proof.proof.get(format!("{key}_proof")).is_some(),
                    "TASK_HARNESS_PROOF_UNAVAILABLE: observed evidence for gate '{}' did not produce {key}_proof",
                    claimed.stage
                );
                Ok(proof)
            }
            Some(ProofKind::SeamTest) => anyhow::bail!(
                "TASK_HARNESS_PROOF_UNAVAILABLE: production runner does not expose structured seam-test observations for gate '{}'",
                gate_path
            ),
            Some(ProofKind::Review) => self.evidence_from_task(claimed, passing_review_proof()),
            Some(ProofKind::None) => self.evidence_from_task(claimed, json!({"none_proof":{}})),
            None => anyhow::bail!(
                "TASK_HARNESS_PROOF_UNAVAILABLE: gate '{}' has no proof definition",
                claimed.stage
            ),
        }
    }

    fn evidence_from_task(&self, claimed: &Task, proof: Value) -> anyhow::Result<Evidence> {
        Ok(Evidence {
            task_id: claimed.task_id.clone(),
            stage: claimed.stage.clone(),
            claim_revision: claimed.claim_revision,
            contract_revision: claimed.contract_revision,
            worker_id: claimed.worker_id.clone().ok_or_else(|| {
                anyhow::anyhow!("TASK_HARNESS_CLAIM_MISSING: worker id is absent")
            })?,
            worktree: claimed
                .worktree
                .clone()
                .ok_or_else(|| anyhow::anyhow!("TASK_HARNESS_CLAIM_MISSING: worktree is absent"))?,
            commit: None,
            candidate_baseline_head: None,
            proof,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn claim(&mut self, stage: &str) -> Task {
        self.store
            .claim(
                &self.id,
                stage,
                "diagnostic-builder",
                &self.root.to_string_lossy(),
                Some(&Self::session_for("diagnostic-builder")),
            )
            .unwrap()
    }

    pub fn set_persisted_status(&self, status: &str) {
        self.set_persisted_task_status(&self.id, status);
    }

    pub fn set_persisted_task_status(&self, id: &str, status: &str) {
        self.store
            .connection
            .execute(
                "UPDATE tasks SET descriptor=json_set(descriptor,'$.status',?2) WHERE task_id=?1",
                rusqlite::params![id, status],
            )
            .unwrap();
    }

    pub fn recovery_workspace() -> Workspace {
        Self::workspace_with_file("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n")
    }
}
