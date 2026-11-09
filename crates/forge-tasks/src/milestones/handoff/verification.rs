use super::*;
use rusqlite::OptionalExtension;

/// Validate the externally reviewed proof for the current handoff candidate.
pub(super) fn require_closed_verification(
    store: &TasksStore,
    identity: &str,
    evidence: &IntegrationEvidence,
    binding: &HandoffBinding,
    candidate_commit: &str,
) -> Result<()> {
    evidence.validate(binding)?;

    let latest_transition: Option<(String, String, String, String)> = store
        .connection
        .query_row(
            "SELECT action, git_tree_sha, proof_payload, gate_path FROM gate_transitions
             WHERE milestone_id=?1 AND subject='milestone' AND proof_kind='command'
             ORDER BY id DESC LIMIT 1",
            [identity],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;

    let (action, git_tree_sha, proof_payload, gate_path) = latest_transition.context(format!(
        "HANDOFF_VERIFICATION_REQUIRED: no milestone verification transition recorded for candidate {candidate_commit}"
    ))?;

    if action != "pass" {
        bail!(
            "HANDOFF_VERIFICATION_REQUIRED: latest verification is not 'pass' for candidate {candidate_commit} (action='{action}')"
        );
    }

    let payload: Value = serde_json::from_str(&proof_payload).context(
        "HANDOFF_VERIFICATION_REQUIRED: malformed verification proof payload in transition record",
    )?;

    let matches_digest = payload["implementation_digest"]
        .as_str()
        .is_some_and(|d| d == binding.implementation_digest)
        && payload["target_base_digest"]
            .as_str()
            .is_some_and(|d| d == binding.target_base_digest);

    if !matches_digest {
        bail!(
            "HANDOFF_VERIFICATION_REQUIRED: verification record for commit '{git_tree_sha}' does not match candidate '{candidate_commit}' implementation and target-base digests"
        );
    }

    let command_id = payload["command_id"]
        .as_str()
        .context("HANDOFF_VERIFICATION_REQUIRED: command ID missing")?;
    let attempt_id = payload["attempt_id"]
        .as_i64()
        .context("HANDOFF_VERIFICATION_REQUIRED: attempt missing; rerun verification")?;
    let command = store
        .profile
        .commands
        .get(command_id)
        .context("HANDOFF_VERIFICATION_REQUIRED: command no longer registered")?;
    if payload["command_digest"].as_str() != Some(integration::digest(command)?.as_str()) {
        bail!("HANDOFF_VERIFICATION_REQUIRED: registered command changed; rerun verification");
    }
    if payload["profile_digest"].as_str()
        != Some(integration::digest(store.profile.as_ref())?.as_str())
    {
        bail!("HANDOFF_VERIFICATION_REQUIRED: verification profile changed; rerun verification");
    }
    let latest_trace: Option<(String, Option<i32>, String)> = store
        .connection
        .query_row(
            "SELECT status, exit_code, command_str FROM gate_execution_traces
             WHERE milestone_id=?1 AND subject='milestone' AND gate_path=?2
               AND command_id=?3 AND attempt_id=?4
               AND id=(SELECT MAX(id) FROM gate_execution_traces
                       WHERE milestone_id=?1 AND subject='milestone')",
            rusqlite::params![identity, gate_path, command_id, attempt_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;

    let (status, exit_code, command_str) = latest_trace
        .context("HANDOFF_VERIFICATION_REQUIRED: no milestone verification trace recorded")?;
    if status != "completed" || exit_code != Some(0) {
        bail!(
            "HANDOFF_VERIFICATION_REQUIRED: latest verification execution failed (status='{status}', exit_code={exit_code:?})"
        );
    }
    if payload["command"].as_str() != Some(command_str.as_str())
        || evidence.verification.command != command_str
    {
        bail!("HANDOFF_VERIFICATION_REQUIRED: submitted command does not match recorded execution");
    }

    if payload["exit_code"].as_i64().is_some_and(|code| code != 0) {
        bail!(
            "HANDOFF_VERIFICATION_REQUIRED: recorded verification proof contains a failed exit code for candidate {candidate_commit}"
        );
    }

    if payload["tests_failed"]
        .as_u64()
        .is_some_and(|failed| failed != 0)
        || evidence.verification.tests_failed != 0
    {
        bail!("HANDOFF_VERIFICATION_REQUIRED: recorded verification proof contains test failures for candidate {candidate_commit}");
    }
    let require_counts = command.output().is_none_or(|spec| spec.require_counts);
    if require_counts
        && (payload["tests_passed"].as_u64().is_none()
            || payload["tests_failed"].as_u64().is_none())
    {
        bail!("HANDOFF_VERIFICATION_REQUIRED: recorded verification proof is missing required test counts");
    }
    if require_counts || command.output().is_some_and(|spec| spec.counts.is_some()) {
        if payload["tests_passed"].as_u64() != Some(evidence.verification.tests_passed) {
            bail!("HANDOFF_VERIFICATION_REQUIRED: submitted tests_passed does not match recorded verification");
        }
        if payload["tests_failed"].as_u64() != Some(evidence.verification.tests_failed) {
            bail!("HANDOFF_VERIFICATION_REQUIRED: submitted tests_failed does not match recorded verification");
        }
    }

    Ok(())
}

fn milestone_gate_paths(root: &Path, prepared: &PreparedHandoff) -> Result<Vec<String>> {
    let changed = crate::core::git::git_command(
        root,
        &[
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            &prepared.target,
            &prepared.commit,
        ],
    )?;
    if !changed.status.success() {
        bail!("MILESTONE_GATE_CHANGES_UNAVAILABLE: cannot inspect candidate changes");
    }
    let manifest = canonical_git_path(
        &prepared
            .item
            .path
            .strip_prefix(root)
            .unwrap_or(&prepared.item.path)
            .to_string_lossy(),
    );
    Ok(String::from_utf8(changed.stdout)?
        .split('\0')
        .filter(|path| !path.is_empty() && *path != manifest)
        .map(str::to_owned)
        .collect())
}

fn cursor_applicable(
    root: &Path,
    prepared: &PreparedHandoff,
    paths: &[String],
    cursor: &crate::engine::gate_graph::GateCursor,
) -> bool {
    crate::engine::gate_graph::gate_condition_matches(
        cursor.when.as_deref(),
        paths,
        &prepared.tasks,
        None,
        Some(root),
    )
}

/// Run one applicable command gate from the registered milestone gate graph.
pub fn execute_milestone_verification(
    root: &Path,
    selector: &str,
    target_base: &str,
) -> Result<Value> {
    execute_verification_at(root, selector, target_base, None)
}

/// Execute the exact applicable root command selected by the wave runner.
pub fn execute_wave_verification(
    root: &Path,
    selector: &str,
    target_base: &str,
    gate_path: &str,
) -> Result<Value> {
    if gate_path.trim().is_empty() {
        bail!("MILESTONE_VERIFICATION_GATE_REQUIRED: supply the current root cursor path");
    }
    execute_verification_at(root, selector, target_base, Some(gate_path))
}

fn execute_verification_at(
    root: &Path,
    selector: &str,
    target_base: &str,
    gate_path: Option<&str>,
) -> Result<Value> {
    let prepared = prepare_handoff_context(root, selector, target_base)?;
    let root_gates = crate::engine::gate_graph::collect_root_cursors(&prepared.profile)?;
    let changed_paths = milestone_gate_paths(root, &prepared)?;
    let (gate, command_id) = root_gates
        .iter()
        .find_map(|cursor| {
            ((gate_path.is_none_or(|path| cursor.canonical_stage_id == path))
                && (cursor.gate.command.is_some()
                || cursor.gate.proof.kind()
                    == Some(crate::core::tasks::profile::ProofKind::Command))
                && cursor_applicable(root, &prepared, &changed_paths, cursor))
            .then(|| {
                cursor
                    .gate
                    .command
                    .as_deref()
                    .map(|command_id| (cursor, command_id))
            })
            .flatten()
        })
        .context(
            "MILESTONE_VERIFICATION_PROFILE_INVALID: profile has no applicable root verification command gate",
        )?;
    let command = prepared.profile.commands.get(command_id).with_context(|| {
        format!("MILESTONE_VERIFICATION_PROFILE_INVALID: '{command_id}' is not registered")
    })?;
    let identity = format!(
        "{}/{}/{}",
        prepared.item.repository, prepared.item.project, prepared.item.id
    );
    let attempt_id = prepared.store.connection.query_row(
        "SELECT COALESCE(MAX(attempt_id), 0) + 1
         FROM gate_execution_traces
         WHERE milestone_id=?1 AND subject='milestone' AND gate_path=?2 AND command_id=?3",
        rusqlite::params![identity, gate.canonical_stage_id, command_id],
        |row| row.get::<_, i64>(0),
    )?;
    let (verification, _) = integration::run_milestone_check(
        root,
        &prepared.store,
        &identity,
        attempt_id,
        &gate.canonical_stage_id,
        command_id,
        command,
    )?;
    if let Err(e) = (|| -> Result<()> {
        if integration::resolve_commit(root, "HEAD")? != prepared.commit {
            bail!(
                "MILESTONE_VERIFICATION_CANDIDATE_CHANGED: {command_id} changed HEAD while running"
            );
        }
        integration::require_clean_tree(
            root,
            &prepared.commit,
            &archive_outputs(root, &prepared.item)?,
        )?;
        Ok(())
    })() {
        let _ = prepared
            .store
            .record_gate_transition(&crate::store::GateTransition {
                milestone_id: identity.clone(),
                subject: "milestone".to_owned(),
                gate_path: gate.canonical_stage_id.clone(),
                action: "fail".to_owned(),
                proof_kind: "command".to_owned(),
                proof_payload: json!({
                    "command_id": command_id,
                    "command": verification.command,
                    "exit_code": 1,
                    "error": e.to_string(),
                    "implementation_digest": prepared.binding.implementation_digest,
                    "target_base_digest": prepared.binding.target_base_digest,
                }),
                git_tree_sha: integration::resolve_commit(root, "HEAD").unwrap_or_default(),
                worker_id: "milestone_verification".to_owned(),
            });
        return Err(e);
    }
    let action = if verification.passed() {
        "pass"
    } else {
        "fail"
    };
    prepared
        .store
        .record_gate_transition(&crate::store::GateTransition {
            milestone_id: identity.clone(),
            subject: "milestone".to_owned(),
            gate_path: gate.canonical_stage_id.clone(),
            action: action.to_owned(),
            proof_kind: "command".to_owned(),
            proof_payload: json!({
                "command_id": command_id,
                "attempt_id": attempt_id,
                "command_digest": integration::digest(command)?,
                "profile_digest": integration::digest(&prepared.profile)?,
                "command": verification.command,
                "exit_code": verification.exit_code,
                "tests_passed": verification.tests_passed,
                "tests_failed": verification.tests_failed,
                "implementation_digest": prepared.binding.implementation_digest,
                "target_base_digest": prepared.binding.target_base_digest,
            }),
            git_tree_sha: prepared.commit.clone(),
            worker_id: "milestone_verification".to_owned(),
        })?;
    Ok(json!({
        "status": if verification.passed() { "passed" } else { "failed" },
        "binding": prepared.binding,
        "target_base": prepared.target,
        "candidate_head": prepared.commit,
        "gate": gate.canonical_stage_id,
        "verification": verification,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_failed_verification_cannot_reuse_earlier_successful_trace() {
        let mut store = TasksStore::open_in_memory("r", "p").unwrap();
        let command = crate::tasks::profile::CommandDef::Script("true".into());
        std::sync::Arc::make_mut(&mut store.profile)
            .commands
            .insert("check".into(), command.clone());
        let binding: HandoffBinding = serde_json::from_value(json!({"version":1,"repository":"r","project":"p","milestone":"m","authority_digest":"a","implementation_digest":"i","target_base_digest":"b","builders":[]})).unwrap();
        let evidence: IntegrationEvidence = serde_json::from_value(json!({"binding":binding,"target_base":"main","review":{"worker_id":"reviewer","decision":"pass","summary":"reviewed","scope_findings":[],"contract_findings":[]},"verification":{"command":"true","exit_code":0,"tests_passed":1,"tests_failed":0}})).unwrap();
        let mut trace = crate::store::GateExecutionTrace {
            milestone_id: "r/p/m".into(),
            subject: "milestone".into(),
            gate_path: "verify".into(),
            attempt_id: 1,
            command_id: "check".into(),
            command_str: "true".into(),
            status: "completed".into(),
            exit_code: Some(0),
            log_path: None,
            output_snippet: None,
            stdout_sha256: None,
            duration_ms: 0,
        };
        store.record_execution_trace(&trace).unwrap();
        store.record_gate_transition(&crate::store::GateTransition { milestone_id:"r/p/m".into(), subject:"milestone".into(), gate_path:"verify".into(), action:"pass".into(), proof_kind:"command".into(), proof_payload:json!({"command_id":"check","attempt_id":1,"command_digest":integration::digest(&command).unwrap(),"profile_digest":integration::digest(store.profile.as_ref()).unwrap(),"command":"true","exit_code":0,"tests_passed":1,"tests_failed":0,"implementation_digest":"i","target_base_digest":"b"}), git_tree_sha:"commit".into(), worker_id:"verify".into() }).unwrap();
        assert!(
            require_closed_verification(&store, "r/p/m", &evidence, &binding, "commit").is_ok()
        );
        trace.attempt_id = 2;
        trace.status = "failed".into();
        trace.exit_code = Some(1);
        store.record_execution_trace(&trace).unwrap();
        let error = require_closed_verification(&store, "r/p/m", &evidence, &binding, "commit")
            .unwrap_err();
        assert!(
            error.to_string().contains("HANDOFF_VERIFICATION_REQUIRED"),
            "{error:#}"
        );
    }

    #[test]
    fn verification_requires_matching_command_profile_and_trace() {
        let path = std::env::temp_dir().join(format!(
            "forge-verification-{}-{}.sqlite",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let mut store = TasksStore::open(&path).unwrap();
        let command = crate::tasks::profile::CommandDef::Script("true".into());
        std::sync::Arc::make_mut(&mut store.profile)
            .commands
            .insert("check".into(), command.clone());
        let binding: HandoffBinding = serde_json::from_value(json!({"version":1,"repository":"r","project":"p","milestone":"m","authority_digest":"a","implementation_digest":"i","target_base_digest":"b","builders":[]})).unwrap();
        let mut evidence: IntegrationEvidence = serde_json::from_value(json!({"binding":binding,"target_base":"main","review":{"worker_id":"reviewer","decision":"pass","summary":"reviewed","scope_findings":[],"contract_findings":[]},"verification":{"command":"true","exit_code":0,"tests_passed":1,"tests_failed":0}})).unwrap();
        store
            .record_execution_trace(&crate::store::GateExecutionTrace {
                milestone_id: "r/p/m".into(),
                subject: "milestone".into(),
                gate_path: "verify".into(),
                attempt_id: 1,
                command_id: "check".into(),
                command_str: "true".into(),
                status: "completed".into(),
                exit_code: Some(0),
                log_path: None,
                output_snippet: None,
                stdout_sha256: None,
                duration_ms: 0,
            })
            .unwrap();
        store.record_gate_transition(&crate::store::GateTransition { milestone_id:"r/p/m".into(), subject:"milestone".into(), gate_path:"verify".into(), action:"pass".into(), proof_kind:"command".into(), proof_payload:json!({"command_id":"check","attempt_id":1,"command_digest":integration::digest(&command).unwrap(),"profile_digest":integration::digest(store.profile.as_ref()).unwrap(),"command":"true","exit_code":0,"tests_passed":1,"tests_failed":0,"implementation_digest":"i","target_base_digest":"b"}), git_tree_sha:"commit".into(), worker_id:"verify".into() }).unwrap();
        assert!(
            require_closed_verification(&store, "r/p/m", &evidence, &binding, "commit").is_ok()
        );
        evidence.verification.command = "unverified".into();
        assert!(
            require_closed_verification(&store, "r/p/m", &evidence, &binding, "commit").is_err()
        );
        evidence.verification.command = "true".into();
        std::sync::Arc::make_mut(&mut store.profile)
            .commands
            .insert(
                "check".into(),
                crate::tasks::profile::CommandDef::Script("false".into()),
            );
        assert!(
            require_closed_verification(&store, "r/p/m", &evidence, &binding, "commit").is_err()
        );
        drop(store);
        let _ = fs::remove_file(path);
    }
}
