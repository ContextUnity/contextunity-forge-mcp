use super::*;
use std::path::Path;

mod blackboard;
mod boundaries;
mod snapshot_git;

pub(crate) use blackboard::{file_backed_blackboard_fixture, sync_blackboard_tasks};
pub(crate) use boundaries::*;
pub(crate) use snapshot_git::{
    git_bytes as snapshot_git_bytes, git_command as snapshot_git_command, NamedSnapshotHarness,
};

pub(crate) fn restart_mcp_session(
    current: crate::common::mcp_harness::McpTestHarness,
    workspace: &Workspace,
    session: &str,
) -> crate::common::mcp_harness::HarnessResult<crate::common::mcp_harness::McpTestHarness> {
    current.finish()?;
    crate::common::mcp_harness::McpTestHarness::with_session(workspace, session)
}

pub(crate) fn unconfigured_test_root() -> std::path::PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/acdd/fixtures/empty-root");
    for name in ["forge-mcp.yaml", ".git", "src", "tests"] {
        assert!(matches!(
            std::fs::symlink_metadata(root.join(name)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        ));
    }
    root
}

pub(crate) const DEFAULT_SEAM: &str = "tests/acdd/tasks.rs";

pub(crate) const TASK_SEAM_FIXTURE: &str = "#[test]\nfn task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite() {}\n#[test]\nfn seam_test() {}\n";
pub(crate) const RUNNER_FIXTURE: &str =
    "#[test]\nfn seam_test() { assert!(std::env::var_os(\"FORGE_FIXTURE_RED\").is_none()); }\n";
pub(crate) const FIXTURE_EXCLUDE: &str =
    ".forge/\n.forge/runner_fixture*\n/tasks.sqlite*\n/target/\n/Cargo.lock\n";

pub(crate) const FIXTURE_COMMANDS: &str = r#"commands:
  test:
    command: ": --seam-ref tests/acdd/tasks.rs {scope_target} && test -x .forge/runner_fixture && if test -f .forge/runner_fixture_green; then .forge/runner_fixture; else FORGE_FIXTURE_RED=1 .forge/runner_fixture --nocapture; fi"
    output:
      require_counts: false
      seam_ref_pattern: '--seam-ref (\S+)'
  rust-check-and-test:
    command: ": {scope_target} && test -x .forge/runner_fixture && .forge/runner_fixture"
    output:
      require_counts: true
      counts: {passed: '(\d+) passed', failed: '(\d+) failed'}
  check_full_repo:
    command: ": {scope_target} && test -x .forge/runner_fixture && .forge/runner_fixture"
    output:
      require_counts: true
      counts: {passed: '(\d+) passed', failed: '(\d+) failed'}
"#;

pub(crate) fn fixture_profile() -> contextunity_forge_mcp::core::tasks::profile::GateProfile {
    runner_profile(DEFAULT_SEAM, "")
}

pub(crate) fn runner_profile(
    seam: &str,
    extra_profile: &str,
) -> contextunity_forge_mcp::core::tasks::profile::GateProfile {
    let source = format!(
        "{}\n{extra_profile}",
        FIXTURE_COMMANDS.replace(DEFAULT_SEAM, seam)
    );
    let mut profile =
        contextunity_forge_mcp::core::tasks::profile::validate_profile_document(&source).unwrap();
    let handoff = profile.find_gate_mut("handoff").unwrap();
    handoff.auto_commit = false;
    handoff.persistence.post =
        Some(contextunity_forge_mcp::core::tasks::profile::PersistenceAction::Snapshot);
    profile
}

pub(crate) fn fixture_integration_evidence(
    root: &Path,
    selector: &str,
    command: &str,
    passed: u64,
    failed: u64,
) -> contextunity_forge_mcp::engine::milestones::IntegrationEvidence {
    use contextunity_forge_mcp::engine::{milestones, tasks::TaskSettings};
    let exclude = root.join(
        contextunity_forge_mcp::core::git::git_stdout(
            root,
            &["rev-parse", "--git-path", "info/exclude"],
            "fixture Git directory",
        )
        .unwrap(),
    );
    let mut ignored = std::fs::read_to_string(&exclude).unwrap_or_default();
    if !ignored.lines().any(|line| line == ".forge/") {
        ignored.push_str("\n.forge/\n");
    }
    std::fs::write(exclude, ignored).unwrap();
    let settings: TaskSettings = serde_yaml::from_str(
        &std::fs::read_to_string(root.join("forge-mcp.yaml")).unwrap_or_default(),
    )
    .unwrap();
    let view = milestones::show(root, selector, false).unwrap();
    let repository = view["frontmatter"]["repository"]
        .as_str()
        .unwrap_or(&settings.task_repository);
    let listed = tasks::list(
        root,
        tasks::List {
            repository: Some("all".into()),
            milestone_ref: Some(selector.into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .ok();
    let canonical_project = listed
        .as_ref()
        .and_then(|listed| listed["tasks"][0]["task_id"].as_str())
        .and_then(|id| id.split_once('/').map(|(project, _)| project));
    let declared_project = view["frontmatter"]["project"]
        .as_str()
        .or(settings.task_project.as_deref())
        .unwrap_or(repository);
    let project = canonical_project.unwrap_or_else(|| {
        settings
            .aliases
            .get(declared_project)
            .map(String::as_str)
            .unwrap_or(declared_project)
    });
    let store = TasksStore::open_project(&tasks::database_path(root).unwrap(), repository, project)
        .unwrap();
    let prefix = format!("{project}/{}:", view["id"].as_str().unwrap());
    let mut candidates = store.list(None, "all", None).unwrap();
    candidates.retain(|task| task.task_id.starts_with(&prefix));
    candidates.sort_by_key(|task| (task.wave_index, task.task_id.clone()));
    let target = candidates
        .first()
        .and_then(|task| {
            let details = store.inspect_details(&task.task_id).unwrap();
            details["gates"]
                .as_array()
                .unwrap()
                .iter()
                .find_map(|gate| {
                    if gate["stage"] != "build" {
                        return None;
                    }
                    let evidence: Value = serde_json::from_str(gate["evidence"].as_str()?).ok()?;
                    evidence["candidate_baseline_head"]
                        .as_str()
                        .map(str::to_owned)
                })
        })
        .unwrap_or_else(|| git_head(root));
    let context = milestones::handoff_context(root, selector, &target)
        .expect("handoff-context for ready milestone integration evidence");
    let verification = if failed == 0 {
        match milestones::execute_milestone_verification(root, selector, &target) {
            Ok(observed) => {
                let check = &observed["verification"];
                json!({
                    "command":check["command"],
                    "exit_code":check["exit_code"],
                    "tests_passed":check["tests_passed"],
                    "tests_failed":check["tests_failed"],
                    "project_command":check["command_id"]
                })
            }
            Err(verr) => {
                let msg = format!("{verr:#}");
                let is_verification_failure = msg.contains("MILESTONE_VERIFICATION_FAILED")
                    || msg.contains("exit")
                    || msg.contains("failed")
                    || msg.contains("command finished with exit code");
                if !is_verification_failure {
                    panic!("unexpected milestone verification failure: {verr:#}");
                }
                json!({"command":command,"exit_code":1,"tests_passed":passed,"tests_failed":failed.max(1),"project_command":"check_full_repo"})
            }
        }
    } else {
        json!({"command":command,"exit_code":1,"tests_passed":passed,"tests_failed":failed,"project_command":"check_full_repo"})
    };
    let binding = context["binding"].clone();
    serde_json::from_value(json!({
        "binding":binding,"target_base":target,
        "review":{"worker_id":"integration-reviewer","decision":"pass","summary":"Fixture independently reviews the combined delta against admitted tasks","scope_findings":[],"contract_findings":[]},
        "verification":verification
    })).unwrap()
}

pub(crate) fn unready_fixture_integration_evidence(
    root: &Path,
    selector: &str,
    review_worker: &str,
) -> contextunity_forge_mcp::engine::milestones::IntegrationEvidence {
    use contextunity_forge_mcp::engine::{milestones, tasks::TaskSettings};
    let settings: TaskSettings = serde_yaml::from_str(
        &std::fs::read_to_string(root.join("forge-mcp.yaml")).unwrap_or_default(),
    )
    .unwrap();
    let view = milestones::show(root, selector, false).ok();
    let repository = view
        .as_ref()
        .and_then(|v| v["frontmatter"]["repository"].as_str())
        .unwrap_or(&settings.task_repository);
    let project = view
        .as_ref()
        .and_then(|v| v["frontmatter"]["project"].as_str())
        .or(settings.task_project.as_deref())
        .unwrap_or(repository);
    let milestone_id = view
        .as_ref()
        .and_then(|v| v["id"].as_str())
        .unwrap_or(selector);
    serde_json::from_value(json!({
        "binding": {
            "version": 1,
            "repository": repository,
            "project": project,
            "milestone": milestone_id,
            "authority_digest": "unavailable",
            "implementation_digest": "unavailable",
            "target_base_digest": "unavailable",
            "builders": ["builder"],
        },
        "target_base": "main",
        "review": {
            "worker_id": review_worker,
            "decision": "pass",
            "summary": "Unready fixture integration review",
            "scope_findings": [],
            "contract_findings": [],
        },
        "verification": {
            "command": "check_full_repo",
            "exit_code": 1,
            "tests_passed": 0,
            "tests_failed": 1,
            "project_command": "check_full_repo",
        }
    }))
    .unwrap()
}

pub(crate) fn handoff_with_fixture_evidence(
    root: &Path,
    selector: &str,
    commit: Option<&str>,
    command: &str,
    passed: u64,
    failed: u64,
) -> anyhow::Result<Value> {
    contextunity_forge_mcp::engine::milestones::handoff(
        root,
        selector,
        commit,
        &fixture_integration_evidence(root, selector, command, passed, failed),
    )
}

pub(crate) fn fixture_evidence_file(
    root: &Path,
    selector: &str,
    command: &str,
    passed: u64,
    failed: u64,
) -> String {
    let path = root.join(".forge/integration-evidence.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        serde_json::to_vec(&fixture_integration_evidence(
            root, selector, command, passed, failed,
        ))
        .unwrap(),
    )
    .unwrap();
    path.to_string_lossy().into_owned()
}

/// Creates an ACDD workspace with an explicit, locally configured runner profile.
pub(crate) fn acdd_workspace(seam: &str, extra_profile: &str) -> Workspace {
    let workspace = Workspace::new();
    workspace.write("tests/acdd/tasks.rs", TASK_SEAM_FIXTURE);
    initialize_runner_fixture_with_profile(&workspace, &runner_profile(seam, extra_profile));
    workspace
}

pub(crate) fn initialize_runner_fixture_with_profile(
    workspace: &Workspace,
    profile: &contextunity_forge_mcp::core::tasks::profile::GateProfile,
) {
    workspace.write("tests/runner_fixture.rs", RUNNER_FIXTURE);
    init_git(workspace.root());
    workspace.write(
        ".forge/acdd/profile.yaml",
        &serde_yaml::to_string(profile).unwrap(),
    );
    crate::acdd::snapshot::precompile_runner(workspace);
}

pub(crate) fn initialize_runner_fixture(root: &Workspace) {
    initialize_runner_fixture_with_profile(root, &fixture_profile());
}

pub(crate) fn review_proof(decision: &str) -> serde_json::Value {
    let contours: serde_json::Map<String, serde_json::Value> =
        contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS
            .iter()
            .map(|name| {
                (
                    (*name).into(),
                    json!({"applicable":true,"evidence":"public-seam verification"}),
                )
            })
            .collect();
    json!({"review_proof":{"decision":decision,"contours":contours}})
}

pub(crate) fn passing_review_proof() -> serde_json::Value {
    review_proof("pass")
}

pub(crate) fn evidence(task: &contextunity_forge_mcp::db::tasks_store::Task) -> Evidence {
    observe_fixture_evidence(task);
    if let Some(root) = task.worktree.as_deref() {
        if let Ok(observed) =
            tasks::compile_evidence_envelope(Path::new(root), &task.task_id, &task.stage)
        {
            let observed: Evidence = serde_json::from_value(observed).unwrap();
            if observed.claim_revision == task.claim_revision
                && observed.contract_revision == task.contract_revision
                && (observed.proof.get("contract_proof").is_some()
                    || observed.proof.get("command_proof").is_some())
            {
                return observed;
            }
        }
    }
    unobserved_evidence(task)
}

pub(crate) fn observe_fixture_evidence(task: &contextunity_forge_mcp::db::tasks_store::Task) {
    if task.status != "in_progress" {
        return;
    }
    let root = Path::new(task.worktree.as_deref().unwrap());
    let store = tasks::store_for_task(root, &task.task_id).unwrap();
    let current = store.inspect(&task.task_id).unwrap();
    if current.claim_revision != task.claim_revision || current.status != "in_progress" {
        return;
    }
    let profile = store.profile_for_task(task).unwrap();
    if tasks::compile_evidence_envelope(root, &task.task_id, &task.stage).is_ok() {
        return;
    }
    if let Some(gate) = profile.by_stage(&task.stage) {
        if matches!(
            gate.proof.kind(),
            Some(
                contextunity_forge_mcp::core::tasks::profile::ProofKind::Contract
                    | contextunity_forge_mcp::core::tasks::profile::ProofKind::Command
            )
        ) {
            if let Some(command) = gate
                .command
                .as_deref()
                .and_then(|name| profile.commands.get(name))
            {
                std::fs::create_dir_all(root.join(".forge")).unwrap();
                let runner = root.join(".forge/runner_fixture");
                if !runner.is_file() {
                    if let Ok(git_ref) = std::fs::read_to_string(root.join(".git")) {
                        if let Some(git_dir) = git_ref.strip_prefix("gitdir: ") {
                            let git_path = Path::new(git_dir.trim());
                            if let Some(main_git) = git_path.parent().and_then(Path::parent) {
                                if let Some(main_root) = main_git.parent() {
                                    let main_runner = main_root.join(".forge/runner_fixture");
                                    if main_runner.is_file() {
                                        let _ = std::fs::copy(&main_runner, &runner);
                                        if let Ok(meta) = std::fs::metadata(&main_runner) {
                                            let _ = std::fs::set_permissions(
                                                &runner,
                                                meta.permissions(),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let paths = task
                    .spec
                    .scope
                    .iter()
                    .chain(&task.spec.extended_scope)
                    .cloned()
                    .collect::<Vec<_>>();
                let gate_path = format!("tasks.{}", task.stage);
                let allows_zero = profile
                    .contract_zero_exit_policies()
                    .iter()
                    .any(|policy| policy == &task.spec.proof_policy);
                let is_contract = gate.proof.kind()
                    == Some(contextunity_forge_mcp::core::tasks::profile::ProofKind::Contract);
                let query_latest_exit_code = || -> Option<i32> {
                    let store = tasks::store_for_task(root, &task.task_id).ok()?;
                    store
                        .connection
                        .query_row(
                            "SELECT exit_code FROM gate_execution_traces WHERE subject = ?1 AND (gate_path = ?2 OR gate_path = ?3) ORDER BY id DESC LIMIT 1",
                            [&task.task_id, &gate_path, &task.stage],
                            |row| row.get::<_, Option<i32>>(0),
                        )
                        .ok()
                        .flatten()
                };

                match contextunity_forge_mcp::engine::milestones::execute_command_gate(
                    root,
                    &task.task_id,
                    &gate_path,
                    command,
                    &paths,
                ) {
                    Ok(_) => {
                        if is_contract {
                            let observed_exit_code = query_latest_exit_code();
                            if !allows_zero {
                                assert_eq!(
                                    observed_exit_code,
                                    Some(101),
                                    "contract gate execution trace must record exit_code 101"
                                );
                            } else {
                                assert!(
                                    observed_exit_code == Some(0)
                                        || observed_exit_code == Some(101),
                                    "zero-exit contract policy must record exit_code 0 or 101"
                                );
                            }
                        }
                    }
                    Err(error) => {
                        let err_text = error.to_string();
                        assert!(err_text.contains("TASK_COMMAND_FAILED"), "{error:#}");
                        let observed_exit_code = query_latest_exit_code();
                        if is_contract {
                            if !allows_zero {
                                assert_eq!(
                                    observed_exit_code,
                                    Some(101),
                                    "contract stage runtime-red failure exit_code must be 101"
                                );
                            } else {
                                assert!(
                                    observed_exit_code == Some(0)
                                        || observed_exit_code == Some(101),
                                    "zero-exit contract policy must record exit_code 0 or 101"
                                );
                            }
                        } else {
                            if gate.expected_failure.is_none() {
                                panic!(
                                    "unexpected command gate failure in stage '{}': {error:#}",
                                    task.stage
                                );
                            }
                            if let Some(ef) = &gate.expected_failure {
                                assert!(
                                    observed_exit_code
                                        .is_some_and(|code| ef.exit_codes.contains(&code)),
                                    "intentional non-contract failure exit_code {observed_exit_code:?} must be in expected exit_codes {:?}",
                                    ef.exit_codes
                                );
                            } else {
                                assert_eq!(
                                    observed_exit_code,
                                    Some(1),
                                    "intentional non-contract failure exit_code must match expected"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

pub(crate) fn unobserved_evidence(
    task: &contextunity_forge_mcp::db::tasks_store::Task,
) -> Evidence {
    let stage = task.stage.as_str();
    Evidence {
        task_id: task.task_id.clone(),
        stage: task.stage.clone(),
        claim_revision: task.claim_revision,
        contract_revision: task.contract_revision,
        worker_id: task.worker_id.clone().unwrap(),
        worktree: task.worktree.clone().unwrap(),
        // Snapshot and baseline hashes are engine-owned acceptance data, never test fixture input.
        commit: None,
        candidate_baseline_head: None,
        proof: match stage {
            "contract" => {
                json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":101}})
            }
            "build" => {
                json!({"command_proof":{"command":"rust-check-and-test","exit_code":0,"tests_passed":1,"tests_failed":0}})
            }
            "review" => passing_review_proof(),
            "deliver" => json!({"delivery_proof":{}}),
            _ => json!({"none_proof":{}}),
        },
    }
}

pub(crate) fn fixture(seam: &str) -> (Workspace, TasksStore, Milestone) {
    let root = Workspace::new();
    root.write("tests/acdd/tasks.rs", TASK_SEAM_FIXTURE);
    root.write("src/lib.rs", "pub fn example() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-test.md", SPEC);
    initialize_runner_fixture_with_profile(&root, &runner_profile(seam, ""));
    let milestone = Milestone::parse(SPEC, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.path(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-test.md", root.root())
        .unwrap();
    let store =
        tasks::store_for_task(root.root(), &milestone.task_id(&milestone.tasks[0])).unwrap();
    (root, store, milestone)
}
