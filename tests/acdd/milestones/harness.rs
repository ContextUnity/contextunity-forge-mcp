use super::*;
use std::{ffi::OsStr, path::Path, process::Output};

mod foreign;
mod git;
mod landings;
mod observations;
pub(super) use observations::*;
mod discovery;
mod progression;
mod store;
mod transport;
pub(crate) use foreign::*;
pub(super) use landings::*;
pub(super) use progression::*;

/// Owns milestone fixture mechanics. Stores are opened on demand so callers can
/// close every SQLite handle before physical snapshots, crash recovery, or GC.
pub(super) struct MilestoneHarness {
    root: Workspace,
    milestone_id: String,
    active_path: String,
}

pub(super) struct VerificationFacts {
    pub transition_id: i64,
    pub action: String,
    pub payload: Value,
    pub trace_id: i64,
    pub status: String,
    pub exit_code: i32,
    pub command: String,
}

pub(super) struct GateObservation {
    pub before: Value,
    pub claimed: Value,
    pub submitted: Value,
}

pub(super) struct HandoffCrashOutcome {
    pub output: Output,
    pub journals: Vec<(std::path::PathBuf, Value)>,
    pub unlink_seen: Option<bool>,
    pub exited_before_unlink: Option<bool>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum HandoffCrashPhase {
    Note,
    Enrolled,
    Unlinked,
    Committed,
}

impl HandoffCrashPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Enrolled => "enrolled",
            Self::Unlinked => "unlinked",
            Self::Committed => "committed",
        }
    }
}

impl std::fmt::Display for HandoffCrashPhase {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

pub(super) struct TombstoneFacts {
    pub terminal: i64,
    pub completed_tasks: Vec<String>,
    pub total: i64,
}

pub(super) struct LifecycleTask {
    pub id: &'static str,
    pub path: &'static str,
    pub task: &'static str,
    pub target: &'static str,
    pub subtask: &'static str,
}

impl LifecycleTask {
    pub(super) fn source(
        &self,
        status: Option<&str>,
        reason: Option<&str>,
        revision: u64,
    ) -> String {
        let closure = match reason {
            Some(reason) => format!(
                "closure:\n  reason: {}\n",
                serde_json::to_string(reason).unwrap()
            ),
            None if status == Some("cancelled") => "closure:\n  owner: platform\n".into(),
            None => String::new(),
        };
        include_str!("../fixtures/milestones/lifecycle_task.md")
            .replace("{{ID}}", self.id)
            .replace("{{TASK}}", self.task)
            .replace("{{TARGET}}", self.target)
            .replace("{{SUBTASK}}", self.subtask)
            .replace("{{REVISION}}", &revision.to_string())
            .replace(
                "{{STATUS}}",
                &status
                    .map(|status| format!("status: {status}\n"))
                    .unwrap_or_default(),
            )
            .replace("{{CLOSURE}}", &closure)
    }

    pub(super) fn task_id(&self) -> String {
        format!("forge-mcp/{}:{}", self.id, self.task)
    }
}

pub(super) enum PruningFault {
    Delay,
    RejectCancelled,
    Capture,
}

impl PruningFault {
    fn name(&self) -> &str {
        match self {
            Self::Delay => "delay_pruning",
            Self::RejectCancelled => "reject_cancelled_pruning",
            Self::Capture => "capture_handoff_journal",
        }
    }
    fn trigger(&self) -> &str {
        match self {
        Self::Delay => "CREATE TRIGGER delay_pruning BEFORE DELETE ON tasks BEGIN SELECT sum(x) FROM (WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<100000000) SELECT x FROM n); END;",
        Self::RejectCancelled => "CREATE TRIGGER reject_cancelled_pruning BEFORE DELETE ON tasks WHEN OLD.task_id LIKE '%:transferred' BEGIN SELECT RAISE(ABORT, 'fixture pruning failure'); END;",
        Self::Capture => "CREATE TRIGGER capture_handoff_journal BEFORE DELETE ON tasks BEGIN SELECT RAISE(ABORT, 'capture handoff journal'); END;",
    }
    }
}

impl std::ops::Deref for MilestoneHarness {
    type Target = Workspace;
    fn deref(&self) -> &Self::Target {
        &self.root
    }
}

impl MilestoneHarness {
    pub(super) fn review_workflow(
        path: &str,
        contract: MilestoneFixtureBuilder,
        early_review: bool,
    ) -> Self {
        let root = Self::configured_workspace();
        root.write("src/lib.rs", "pub fn candidate() { let _version = 1; }\n");
        let mut profile = fixture_profile();
        let candidate = "- {id: candidate_final, role: builder, proof: command, command: rust-check-and-test, sha_snapshot: true}\n";
        let dependency = if early_review {
            "build"
        } else {
            "candidate_final"
        };
        let review = format!("- {{id: review, role: independent_reviewer, proof: review, independent_from: {dependency}, reject_to: {dependency}, contours: guidance.contours}}\n");
        let middle = if early_review {
            format!("{review}{candidate}")
        } else {
            format!("{candidate}{review}")
        };
        profile.gates = serde_yaml::from_str(&format!(r#"
- {{id: contract, role: contract_author, proof: contract, command: test}}
- {{id: build, role: builder, proof: command, command: rust-check-and-test, sha_snapshot: true}}
{middle}- {{id: security_review, role: independent_reviewer, proof: review, independent_from: candidate_final, reject_to: candidate_final, contours: guidance.contours}}
- {{id: deliver, role: independent_reviewer, proof: delivery, receipt_review: security_review, review_sources: [review, security_review], reject_to: candidate_final, auto_commit: true}}
"#)).unwrap();
        root.write(
            ".forge/acdd/profile.yaml",
            &crate::acdd::support::standalone_profile_yaml(&profile),
        );
        // Both callers vary src/ candidates; their runner and profile stay fixed
        // through every gate, whose real red/green executions remain enabled.
        super::super::snapshot::precompile_runner(&root);
        root.write(path, &contract.build());
        root.stage_forced_paths(&[
            "src/lib.rs",
            "forge-mcp.yaml",
            ".forge/acdd/profile.yaml",
            path,
        ]);
        root.commit_staged("initialize review workflow");
        root.sync(path).unwrap();
        root
    }

    pub(super) fn reverify(
        root: &Workspace,
        selector: &str,
        base: &str,
        previous: &milestone_engine::IntegrationEvidence,
    ) -> (milestone_engine::IntegrationEvidence, Value) {
        let context = milestone_engine::handoff_context(root.root(), selector, base).unwrap();
        let observed =
            milestone_engine::execute_milestone_verification(root.root(), selector, base).unwrap();
        let mut evidence = previous.clone();
        evidence.binding = serde_json::from_value(context["binding"].clone()).unwrap();
        evidence.target_base = context["target_base"].as_str().unwrap().to_owned();
        let check = &observed["verification"];
        evidence.verification = serde_json::from_value(json!({
            "command": check["command"], "exit_code": check["exit_code"],
            "tests_passed": check["tests_passed"], "tests_failed": check["tests_failed"],
            "project_command": check["command_id"]
        }))
        .unwrap();
        (evidence, observed)
    }

    pub(super) fn checkpoint(&self) -> crate::acdd::snapshot::FixtureSnapshot {
        crate::acdd::snapshot::FixtureSnapshot::capture(self)
    }

    pub(super) fn yaml_json(value: &serde_yaml::Value) -> Value {
        serde_yaml::from_value(value.clone()).unwrap()
    }

    pub(super) fn set_ref(&self, reference: &str, sha: &str) {
        Self::git_at(self.root(), &["update-ref", reference, sha]).unwrap();
    }

    pub(super) fn has_ref(&self, reference: &str) -> bool {
        Self::git_output_at(self.root(), ["show-ref", "--verify", reference])
            .status
            .success()
    }

    pub(super) fn remove_file(&self, path: &str) {
        self.root.remove(path);
    }

    pub(super) fn make_executable(&self, path: &str) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(self.path(path), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
    }

    pub(super) fn write_journal_bytes(path: &Path, bytes: &[u8]) {
        std::fs::write(path, bytes).unwrap();
    }

    pub(super) fn git_note(&self, commit: &str) -> (Output, Value) {
        let output = Self::git_at(self.root(), &["notes", "--ref=forge", "show", commit]).unwrap();
        let value = serde_json::from_slice(&output.stdout).unwrap();
        (output, value)
    }

    pub(super) fn set_git_note(&self, commit: &str, value: &Value) {
        self.restore_git_note(commit, &serde_json::to_vec(value).unwrap());
    }

    pub(super) fn restore_git_note(&self, commit: &str, bytes: &[u8]) {
        Self::git_at(
            self.root(),
            &[
                "notes",
                "--ref=forge",
                "add",
                "-f",
                "-m",
                std::str::from_utf8(bytes).unwrap(),
                commit,
            ],
        )
        .unwrap();
    }

    pub(super) fn handoff_journals(&self) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(self.path(".forge"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("handoff-")
            })
            .collect()
    }

    pub(super) fn read_journal(path: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    pub(super) fn archived_project(&self, path: &str, repository: &str, project: &str) -> String {
        contextunity_forge_mcp::db::tasks_store::read_milestone_archive(
            self.root(),
            &self.path(path),
            repository,
            project,
        )
        .unwrap()
        .unwrap()
        .project
    }

    pub(super) fn restore_journal(path: &Path, journal: &Value) {
        Self::write_journal_bytes(path, &serde_json::to_vec(journal).unwrap());
    }

    pub(super) fn install_pruning_fault(store: &TasksStore, fault: PruningFault) {
        store.connection.execute_batch(fault.trigger()).unwrap();
    }
    pub(super) fn remove_pruning_fault(store: &TasksStore, fault: PruningFault) {
        store
            .connection
            .execute_batch(&format!("DROP TRIGGER {}", fault.name()))
            .unwrap();
    }
    pub(super) fn set_passed_gate_field(
        store: &TasksStore,
        task: &str,
        gate: &str,
        field: &str,
        value: &str,
    ) -> anyhow::Result<usize> {
        anyhow::ensure!(
            ["worker_id", "worktree", "commit"].contains(&field),
            "unsupported gate evidence field: {field}"
        );
        Ok(store.connection.execute("UPDATE task_gates SET evidence=json_set(evidence, ?1, ?2) WHERE task_id=?3 AND gate=?4 AND state='passed'", rusqlite::params![format!("$.{field}"), value, task, gate])?)
    }

    pub(super) fn rewrite_trace_identity(store: &TasksStore, from: &str, to: &str) -> usize {
        store.connection.execute("UPDATE gate_execution_traces SET milestone_id=?1 WHERE subject='milestone' AND milestone_id=?2", rusqlite::params![to, from]).unwrap()
    }

    pub(super) fn milestone_rows(
        store: &TasksStore,
        table: &str,
        identity: &str,
    ) -> anyhow::Result<i64> {
        anyhow::ensure!(
            [
                "gate_transitions",
                "gate_execution_traces",
                "gate_checkpoints"
            ]
            .contains(&table),
            "unsupported milestone table: {table}"
        );
        Ok(store.connection.query_row(
            &format!("SELECT count(*) FROM {table} WHERE milestone_id=?1"),
            [identity],
            |row| row.get(0),
        )?)
    }
    pub(super) fn milestone_telemetry_ids(store: &TasksStore) -> (i64, i64) {
        store.connection.query_row(
            "SELECT (SELECT MAX(id) FROM gate_execution_traces WHERE subject='milestone'), (SELECT MAX(id) FROM gate_transitions WHERE subject='milestone')",
            [], |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap()
    }

    pub(super) fn verification_facts(store: &TasksStore) -> VerificationFacts {
        let (transition_id, action, payload, gate_path): (i64, String, String, String) = store.connection.query_row(
            "SELECT id, action, proof_payload, gate_path FROM gate_transitions WHERE subject='milestone' ORDER BY id DESC LIMIT 1",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).unwrap();
        let payload: Value = serde_json::from_str(&payload).unwrap();
        let (trace_id, status, exit_code, command) = store.connection.query_row(
            "SELECT id, status, exit_code, command_str FROM gate_execution_traces WHERE subject='milestone' AND gate_path=?1 AND command_id=?2 AND attempt_id=?3 ORDER BY id DESC LIMIT 1",
            rusqlite::params![gate_path, payload["command_id"].as_str().unwrap(), payload["attempt_id"].as_i64().unwrap()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).unwrap();
        VerificationFacts {
            transition_id,
            action,
            payload,
            trace_id,
            status,
            exit_code,
            command,
        }
    }
    pub(super) fn workspace(seam: &str, profile: &str) -> Self {
        let root = Workspace::new();
        root.write(seam, crate::acdd::support::TASK_SEAM_FIXTURE);
        let mut profile = crate::acdd::support::runner_profile(seam, profile);
        profile
            .find_gate_mut("verify")
            .expect("runner profile declares a verification gate")
            .on_success = Some("handoff".into());
        if !profile.gates.iter().any(|g| g.id == "handoff") {
            let handoff_gate = serde_yaml::from_str(
                "id: handoff\nproof: delivery\nreceipt: true\nabsorb: true\nsave: commit\n",
            )
            .unwrap();
            profile.gates.push(handoff_gate);
        }
        profile.validate().unwrap();
        crate::acdd::support::initialize_runner_fixture_with_profile(&root, &profile);
        Self {
            root,
            milestone_id: String::new(),
            active_path: String::new(),
        }
    }

    pub(super) fn linear(id: &str) -> Self {
        let builder = MilestoneFixtureBuilder::new(id)
            .title("Close work")
            .task("finish", &["src/lib.rs"])
            .task_target("Finish work")
            .task_scope_roots(&["src/"])
            .task("retired", &["cancelled/"])
            .task_target("Transferred work")
            .task_status("cancelled");
        let mut harness = Self::configured_workspace();
        harness.milestone_id = id.to_owned();
        harness.active_path = format!("docs/milestones/010-{}.md", id.trim_start_matches("m-"));
        harness.write("src/lib.rs", "pub fn finish() {}\n");
        harness.write(&harness.active_path, &builder.build());
        commit_handoff_fixture_inputs(
            &harness,
            &["forge-mcp.yaml", "src/lib.rs", &harness.active_path],
        );
        harness.sync(&harness.active_path).unwrap();
        harness
    }

    pub(super) fn handoff(&self) -> anyhow::Result<Value> {
        anyhow::ensure!(
            !self.milestone_id.is_empty(),
            "bind a milestone before generating evidence"
        );
        let evidence = fixture_integration_evidence(
            self.root(),
            &self.milestone_id,
            "cargo test --all-targets",
            7,
            0,
        );
        self.handoff_with_evidence(&self.milestone_id, &evidence)
    }

    pub(super) fn handoff_with_evidence(
        &self,
        id: &str,
        evidence: &milestone_engine::IntegrationEvidence,
    ) -> anyhow::Result<Value> {
        milestone_engine::handoff(self.root(), id, None, evidence)
    }
}

pub(super) const HANDOFF_CRASH_WORKER: &str = "FORGE_TEST_HANDOFF_CRASH_WORKER";
pub(super) const HANDOFF_CRASH_ROOT: &str = "FORGE_TEST_HANDOFF_CRASH_ROOT";
pub(super) const HANDOFF_CRASH_MILESTONE: &str = "FORGE_TEST_HANDOFF_CRASH_MILESTONE";
pub(super) const HANDOFF_CRASH_COMMIT: &str = "FORGE_TEST_HANDOFF_CRASH_COMMIT";
pub(super) const HANDOFF_CRASH_EVIDENCE: &str = "FORGE_TEST_HANDOFF_CRASH_EVIDENCE";

pub(super) fn read_fixture_evidence(path: &str) -> milestone_engine::IntegrationEvidence {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

pub(super) fn milestone_init_options() -> milestone_engine::Init {
    milestone_engine::Init {
        num: None,
        slug: None,
        title: None,
        plan: None,
        dir: None,
        desc: None,
        depends_on: Vec::new(),
        active: false,
        stdin: String::new(),
    }
}

pub(super) fn handoff_with_journal_capture(
    root: &Workspace,
    milestone: &str,
    commit: Option<&str>,
    evidence: &str,
    phase: HandoffCrashPhase,
) -> HandoffCrashOutcome {
    use std::os::unix::fs::PermissionsExt;
    // This process is deliberately killed at a journal boundary; recovery must
    // be tested after a real SIGKILL rather than an in-process error.
    let crash = phase.as_str();
    let wrapper = root.root().join(".forge/journal-git/git");
    root.write(".forge/journal-git/git", r#"#!/bin/sh
if [ "$FORGE_FIXTURE_CRASH" = committed ] && [ -d "$PWD/.forge/captured" ] && [ ! -f "$PWD/docs/milestones/010-close.md" ]; then
  kill -KILL "$PPID"
  exit 0
fi
if [ "$1" = update-ref ] && [ "$2" = refs/notes/forge ]; then
  mkdir -p "$PWD/.forge/captured"
  for journal in "$PWD"/.forge/handoff-*.json; do
    [ ! -f "$journal" ] || cp "$journal" "$PWD/.forge/captured/"
  done
  if [ "$FORGE_FIXTURE_CRASH" = note ]; then
    /usr/bin/git "$@" || exit $?
    kill -KILL "$PPID"
    exit 0
  fi
fi
if [ "$FORGE_FIXTURE_CRASH" = enrolled ] && [ -d "$PWD/.forge/captured" ]; then
  reference=
  sha=
  for arg do
    case "$arg" in refs/forge/state/forge-mcp/forge-mcp/m-close/*) reference="$arg";;
      *) if [ -n "$reference" ] && [ -z "$sha" ]; then sha="$arg"; fi;;
    esac
  done
  case "$*" in *update-ref*)
    if [ -n "$sha" ]; then
      payload=$(/usr/bin/git show "$sha:checkpoint.json" 2>/dev/null)
      case "$payload" in *'"intent":"'*)
        /usr/bin/git "$@" || exit $?
        printf 'intent\n' >> "$PWD/.forge/intent-count"
        if [ "$(wc -l < "$PWD/.forge/intent-count")" -ge 2 ]; then kill -KILL "$PPID"; fi
        exit 0;;
      esac
    fi;;
  esac
fi
exec /usr/bin/git "$@"
"#);
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    // Re-enter an existing recovery test in a separate test process. Its worker
    // branch calls the engine directly, while the injected Git wrapper still
    // kills that process at the actual journal boundary.
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "acdd::milestones::recovery::milestone_handoff_recovers_crash_at_each_journal_phase",
            "--nocapture",
        ])
        .env(HANDOFF_CRASH_WORKER, "1")
        .env(HANDOFF_CRASH_ROOT, root.root())
        .env(HANDOFF_CRASH_MILESTONE, milestone)
        .env(HANDOFF_CRASH_EVIDENCE, evidence)
        .env_remove(HANDOFF_CRASH_COMMIT)
        .env("FORGE_FIXTURE_CRASH", crash)
        .env(
            "PATH",
            format!(
                "{}:{}",
                wrapper.parent().unwrap().display(),
                std::env::var("PATH").unwrap()
            ),
        )
        .current_dir(root.root())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(commit) = commit {
        command.env(HANDOFF_CRASH_COMMIT, commit);
    }
    let mut child = command.spawn().unwrap();
    let mut unlink_seen = None;
    let mut exited_before_unlink = None;
    if crash == "unlinked" {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while root.root().join("docs/milestones/010-close.md").exists() {
            if child.try_wait().unwrap().is_some() {
                exited_before_unlink = Some(true);
                break;
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        unlink_seen = Some(!root.root().join("docs/milestones/010-close.md").exists());
        exited_before_unlink.get_or_insert(false);
        if exited_before_unlink == Some(false) {
            child.kill().unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    let captured = match std::fs::read_dir(root.root().join(".forge/captured")) {
        Ok(entries) => entries
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("reading captured handoff journals: {error}"),
    };
    let journals = captured
        .into_iter()
        .filter_map(|captured| {
            let journal = MilestoneHarness::read_journal(&captured);
            (journal["binding"]["milestone"] == milestone).then(|| {
                (
                    root.root()
                        .join(".forge")
                        .join(captured.file_name().unwrap()),
                    journal,
                )
            })
        })
        .collect();
    HandoffCrashOutcome {
        output,
        journals,
        unlink_seen,
        exited_before_unlink,
    }
}
