//! Contract seams for the AC-001 transition engine and command runner.

use super::support::{fixture, git_head};
use contextunity_forge_mcp::core::tasks::profile::{
    self, CommandDef, ExpectedFailure, PatternCommands, RejectRouting,
};
use contextunity_forge_mcp::db::tasks_store::{
    ClaimBoundExecutionRun, GateExecutionTrace, TasksStore,
};
use contextunity_forge_mcp::engine::milestones::execute_command_gate;
use contextunity_forge_mcp::engine::tasks::compile_evidence_envelope;
use contextunity_forge_mcp::engine::tasks::evaluate_reject_routing;
use contextunity_forge_mcp::engine::tasks::rebase_speculative_dependency;
use contextunity_forge_mcp::engine::tasks::submit_zero_arg;
use contextunity_forge_mcp::engine::tasks::telemetry_passport_note;
use contextunity_forge_mcp::engine::tasks::verify_red_to_green;
use contextunity_forge_mcp::engine::tasks::Action;
use forge_tasks::tasks::profile::parse_profile;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};

#[test]
fn command_profile_preserves_declarative_output_specification() {
    let profile = parse_profile(
        "commands:\n  check:\n    command: pytest\n    output:\n      require_counts: false\n      counts:\n        passed: '(\\d+) passed'\n        failed: '(\\d+) failed'\n      noise: ['^progress']\n      errors: ['^FAIL']\n      seam_ref_pattern: 'test_(\\w+)'\n",
    )
    .unwrap();
    let command = &profile.commands["check"];
    assert_eq!(command.command(), "pytest");
    let output = serde_json::to_value(command).unwrap()["output"].clone();
    assert_eq!(output["require_counts"], false);
    assert_eq!(output["counts"]["passed"], r"(\d+) passed");
    assert_eq!(output["counts"]["failed"], r"(\d+) failed");
    assert_eq!(output["noise"][0], "^progress");
    assert_eq!(output["errors"][0], "^FAIL");
    assert_eq!(output["seam_ref_pattern"], r"test_(\w+)");
}

#[test]
fn command_profile_accepts_legacy_formats_and_defaults_output_rules() {
    let profile = parse_profile(
        "commands:\n  script: echo ok\n  legacy:\n    command: echo ok\n  configured:\n    command: echo ok\n    output: {}\n  pattern_legacy:\n    by_pattern: {'*.rs': 'cargo test'}\n  pattern_configured:\n    by_pattern: {'*.py': pytest}\n    output: {require_counts: false}\n",
    )
    .unwrap();
    assert!(profile.commands["script"].output().is_none());
    assert!(profile.commands["legacy"].output().is_none());
    assert!(profile.commands["pattern_legacy"].output().is_none());
    assert!(
        !profile.commands["pattern_configured"]
            .output()
            .unwrap()
            .require_counts
    );
    let output = profile.commands["configured"].output().unwrap();
    assert!(output.require_counts);
    assert!(output.counts.is_none());
    assert!(output.noise.is_empty());
    assert!(output.errors.is_empty());
    assert!(output.seam_ref_pattern.is_none());
}

#[test]
fn profile_validation_rejects_invalid_output_patterns() {
    for field in [
        "counts:\n        passed: '['\n        failed: '(\\d+)'",
        "noise: ['[']",
        "errors: ['[']",
        "seam_ref_pattern: '['",
    ] {
        let yaml =
            format!("commands:\n  check:\n    command: pytest\n    output:\n      {field}\n");
        let error = profile::validate_profile_document(&yaml)
            .unwrap_err()
            .to_string();
        assert!(error.contains("commands.check.output"), "{error}");
    }
}

#[test]
fn successful_gate_execution_does_not_write_a_trace_log() {
    let (root, mut store, milestone) = fixture();
    root.write(".gitignore", ".forge/\n");
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let mut configured = (*profile::compiled()).clone();
    let command = CommandDef::Script("printf 'test result: ok. 1 passed; 0 failed\\n'".into());
    configured
        .commands
        .insert("trace-check".into(), command.clone());
    let gate = configured
        .gates
        .iter_mut()
        .find(|gate| gate.id == "contract")
        .unwrap();
    gate.command = Some("trace-check".into());
    gate.proof = profile::ProofDef::Kind(profile::ProofKind::Command);
    gate.expected_failure = None;
    fs::create_dir_all(root.0.join(".forge/acdd")).unwrap();
    let profile_yaml = serde_yaml::to_string(&configured).unwrap();
    let parsed_profile = parse_profile(&profile_yaml).unwrap();
    let command = &parsed_profile.commands["trace-check"];
    fs::write(root.0.join(".forge/acdd/profile.yaml"), profile_yaml).unwrap();
    store
        .claim(
            &task_id,
            "contract",
            "trace-author",
            root.0.to_str().unwrap(),
        )
        .unwrap();
    let traces = forge_tasks::milestones::execute_command_gate(
        &root.0,
        &task_id,
        "tasks.contract",
        command,
        &[],
    )
    .unwrap();
    assert_eq!(traces.len(), 1);
    assert_eq!(
        traces[0].log_path, None,
        "successful execution must not create a trace log"
    );
}

pub(super) fn command_fixture(
    command: &str,
    output: Option<profile::CommandOutputSpec>,
) -> (super::support::ScopedWorkspace, TasksStore, String) {
    let (root, mut store, milestone) = fixture();
    root.write(".gitignore", ".forge/\n");
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let mut configured = (*profile::compiled()).clone();
    configured.commands.insert(
        "check".into(),
        CommandDef::Detailed {
            command: command.into(),
            timeout_seconds: Some(5),
            description: None,
            scope_args_by_pattern: Default::default(),
            output,
        },
    );
    let gate = configured
        .gates
        .iter_mut()
        .find(|gate| gate.id == "contract")
        .unwrap();
    gate.command = Some("check".into());
    gate.proof = profile::ProofDef::Kind(profile::ProofKind::Command);
    gate.expected_failure = None;
    root.write(
        ".forge/acdd/profile.yaml",
        &serde_yaml::to_string(&configured).unwrap(),
    );
    store
        .claim(
            &task_id,
            "contract",
            "command-builder",
            root.0.to_str().unwrap(),
        )
        .unwrap();
    (root, store, task_id)
}

pub(super) fn output_spec(require_counts: bool) -> profile::CommandOutputSpec {
    profile::CommandOutputSpec {
        require_counts,
        counts: Some(profile::CommandCountsSpec {
            passed: r"PASS=(\d+)".into(),
            failed: r"FAIL=(\d+)".into(),
        }),
        ..Default::default()
    }
}

#[test]
fn command_verification_reuses_durable_facts_and_invalidates_changed_candidates() {
    let (root, store, id) = command_fixture(
        "printf run >> .forge/counter; printf 'PASS=3 FAIL=0\\nPASS=4 FAIL=0\\n'",
        Some(output_spec(true)),
    );
    let first = forge_tasks::engine::verify_task(&root.0, &id, None).unwrap();
    assert_eq!(
        first["evidence"]["proof"]["command_proof"]["tests_passed"], 7,
        "{first}"
    );
    assert_eq!(first["verification"]["diagnostic_mode"], "pass");
    assert_eq!(store.inspect(&id).unwrap().stage, "contract");
    drop(store);
    let cached = forge_tasks::engine::verify_task(&root.0, &id, None).unwrap();
    assert_eq!(cached["evidence"], first["evidence"]);
    assert_eq!(
        fs::read_to_string(root.0.join(".forge/counter")).unwrap(),
        "run"
    );
    root.write("src/lib.rs", "pub fn changed_candidate() {}\n");
    forge_tasks::engine::verify_task(&root.0, &id, None).unwrap();
    assert_eq!(
        fs::read_to_string(root.0.join(".forge/counter")).unwrap(),
        "runrun"
    );
    assert!(!root.0.join(".forge/task-traces").exists());
}

#[test]
fn zero_argument_submit_executes_countless_linter_and_advances_gate() {
    let (root, store, id) = command_fixture("printf checked", Some(output_spec(false)));
    let result = submit_zero_arg(&root.0, &id, "contract", Action::Pass, None).unwrap();
    assert_eq!(result["stage"], "build");
    assert_eq!(store.inspect(&id).unwrap().stage, "build");
}

#[test]
fn command_count_requirements_reject_missing_and_malformed_output() {
    for (command, passed) in [
        ("printf 'no summary'", r"PASS=(\d+)"),
        ("printf 'PASS=invalid FAIL=0'", r"PASS=(\S+)"),
    ] {
        let mut spec = output_spec(true);
        spec.counts.as_mut().unwrap().passed = passed.into();
        let (root, _, id) = command_fixture(command, Some(spec));
        assert!(submit_zero_arg(&root.0, &id, "contract", Action::Pass, None).is_err());
    }
}

#[test]
fn command_diagnostics_remove_noise_and_keep_short_failures_inline() {
    let mut spec = output_spec(false);
    spec.noise = vec!["^progress".into()];
    spec.errors = vec!["^ERROR".into()];
    let (root, _, id) = command_fixture("i=0; while [ $i -lt 500 ]; do printf 'progress compiling\\n'; i=$((i+1)); done; printf 'ERROR useful failure\\n' >&2; exit 2", Some(spec));
    let result = forge_tasks::engine::verify_task(&root.0, &id, None).unwrap();
    assert_eq!(
        result["verification"]["diagnostic_mode"], "inline",
        "{result}"
    );
    let output = result["verification"]["output"].as_str().unwrap();
    assert!(output.contains("ERROR useful failure"));
    assert!(!output.contains("progress compiling"));
    assert!(result["verification"]["log_path"].is_null());
    assert!(!root.0.join(".forge/task-traces").exists());
}

#[test]
fn command_large_failure_retains_confined_raw_log_and_head_tail_excerpt() {
    let (root, _, id) = command_fixture(
        "printf 'HEAD\\n'; head -c 10000 /dev/zero | tr '\\0' x; printf '\\nTAIL\\n'; exit 2",
        Some(output_spec(false)),
    );
    let result = forge_tasks::engine::verify_task(&root.0, &id, None).unwrap();
    assert_eq!(
        result["verification"]["diagnostic_mode"], "truncated",
        "{result}"
    );
    let output = result["verification"]["output"].as_str().unwrap();
    assert!(output.contains("HEAD") && output.contains("TAIL") && output.contains("file:///"));
    let log = result["verification"]["log_path"].as_str().unwrap();
    assert!(log.starts_with(".forge/task-traces/"));
    assert!(fs::read(root.0.join(log)).unwrap().len() > 10000);
}

#[test]
fn command_scope_guard_observes_paths_before_and_after_execution() {
    let (root, _, id) = command_fixture(
        "printf after > after.txt; printf checked",
        Some(output_spec(false)),
    );
    root.write("before.txt", "existing out-of-scope change\n");
    let result = forge_tasks::engine::verify_task(&root.0, &id, None).unwrap();
    let output = result["verification"]["output"].as_str().unwrap();
    assert!(output.starts_with("SCOPE_VIOLATION_WARNING"), "{result}");
    let paths = result["verification"]["scope_violations"]
        .as_array()
        .unwrap();
    assert!(paths.iter().any(|path| path["path"] == "before.txt"));
    assert!(paths.iter().any(|path| path["path"] == "after.txt"));
    assert_eq!(result["verification"]["exit_code"], 0);
}

#[test]
fn command_timeout_override_is_bounded_and_returns_rerun_hint() {
    let (root, _, id) = command_fixture("sleep 30", Some(output_spec(false)));
    for timeout in [0, 1801] {
        assert!(forge_tasks::engine::verify_task(&root.0, &id, Some(timeout)).is_err());
    }
    let started = Instant::now();
    let result = forge_tasks::engine::verify_task(&root.0, &id, Some(1)).unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        result["verification"]["diagnostic_mode"], "timeout",
        "{result}"
    );
    assert!(result["verification"]["output"]
        .as_str()
        .unwrap()
        .contains("timeout_seconds: 2"));
}

#[test]
fn command_profile_output_changes_invalidate_cached_proof_and_timeout_limit() {
    let (root, _, id) = command_fixture(
        "printf run >> .forge/counter; printf checked",
        Some(output_spec(false)),
    );
    forge_tasks::engine::verify_task(&root.0, &id, None).unwrap();
    let mut configured = (*profile::load_for_workspace(&root.0).unwrap()).clone();
    configured.limits.max_command_timeout_seconds = 2;
    if let CommandDef::Detailed { output, .. } = configured.commands.get_mut("check").unwrap() {
        output.as_mut().unwrap().noise.push("^new-noise".into());
    }
    root.write(
        ".forge/acdd/profile.yaml",
        &serde_yaml::to_string(&configured).unwrap(),
    );
    assert!(forge_tasks::engine::verify_task(&root.0, &id, Some(3)).is_err());
    forge_tasks::engine::verify_task(&root.0, &id, Some(2)).unwrap();
    assert_eq!(
        fs::read_to_string(root.0.join(".forge/counter")).unwrap(),
        "runrun"
    );
}

#[test]
fn command_scope_arguments_expand_once_for_directory_scope() {
    for kind in ["script", "detailed", "pattern"] {
        for placeholder in [false, true] {
            let command = format!(
                "printf 'test result: ok. 1 passed; 0 failed\\n'; printf '%s\\n'{}",
                if placeholder { " {scope_target}" } else { "" }
            );
            let (root, store, id) = command_fixture(&command, None);
            let mut configured = (*profile::load_for_workspace(&root.0).unwrap()).clone();
            let definition = match kind {
                "script" => CommandDef::Script(command.clone()),
                "pattern" => CommandDef::Pattern(PatternCommands {
                    by_pattern: BTreeMap::from([("src/*.rs".into(), command.clone())]),
                    description: None,
                    timeout_seconds: None,
                    output: None,
                }),
                _ => configured.commands["check"].clone(),
            };
            configured.commands.insert("check".into(), definition);
            root.write("src/other.rs", "pub fn other() {}\n");
            for (attempt, args) in [Some("scoped-token"), Some("changed-token"), None]
                .into_iter()
                .enumerate()
            {
                configured.scope_args_by_pattern.clear();
                if let Some(args) = args {
                    configured
                        .scope_args_by_pattern
                        .insert("src/*.rs".into(), args.into());
                }
                root.write(
                    ".forge/acdd/profile.yaml",
                    &serde_yaml::to_string(&configured).unwrap(),
                );
                let result = forge_tasks::engine::verify_task(&root.0, &id, None).unwrap();
                let context = format!("{kind} placeholder={placeholder} args={args:?}: {result}");
                assert_eq!(
                    result["verification"]["diagnostic_mode"], "pass",
                    "{context}"
                );
                assert!(result["evidence"].is_object(), "{context}");
                let traces = store.execution_traces(&id, Some("tasks.contract")).unwrap();
                assert_eq!(traces.len(), attempt + 1, "{context}");
                let expected = match args {
                    Some(args) if placeholder => command.replace("{scope_target}", args),
                    Some(args) => format!("{command} {args}"),
                    None => command.clone(),
                };
                assert_eq!(traces.last().unwrap().command_str, expected, "{context}");
                let cached = forge_tasks::engine::verify_task(&root.0, &id, None).unwrap();
                assert_eq!(cached["evidence"], result["evidence"], "{context}");
                assert_eq!(
                    store
                        .execution_traces(&id, Some("tasks.contract"))
                        .unwrap()
                        .len(),
                    attempt + 1,
                    "{context}"
                );
            }
        }
    }
}

#[test]
fn command_contract_proof_recovers_expected_red_and_configured_seam_without_logs() {
    let mut spec = output_spec(false);
    spec.seam_ref_pattern = Some(r"seam:(\S+)".into());
    let (root, store, id) = command_fixture("head -c 10000 /dev/zero | tr '\\0' x; printf 'assertion failed'; exit 101 # seam:tests/acdd/runner.rs::successful_gate_execution_does_not_write_a_trace_log", Some(spec));
    let mut configured = (*profile::load_for_workspace(&root.0).unwrap()).clone();
    let gate = configured
        .gates
        .iter_mut()
        .find(|gate| gate.id == "contract")
        .unwrap();
    gate.proof = profile::ProofDef::Kind(profile::ProofKind::Contract);
    gate.expected_failure = Some(ExpectedFailure {
        exit_codes: vec![101],
        stderr_contains: Some("assertion failed".into()),
    });
    root.write(
        ".forge/acdd/profile.yaml",
        &serde_yaml::to_string(&configured).unwrap(),
    );
    let result = forge_tasks::engine::verify_task(&root.0, &id, None).unwrap();
    assert_eq!(
        result["evidence"]["proof"]["contract_proof"]["red_exit_code"], 101,
        "{result}"
    );
    assert_eq!(
        result["evidence"]["proof"]["contract_proof"]["seam_test_ref"],
        "tests/acdd/runner.rs::successful_gate_execution_does_not_write_a_trace_log"
    );
    drop(store);
    assert!(compile_evidence_envelope(&root.0, &id, "contract").is_ok());
    assert!(!root.0.join(".forge/task-traces").exists());
}

#[test]
fn command_gate_diagnostics_follow_the_proof_expectation_across_public_callers() {
    use crate::common::mcp_client::StdioClient;

    for (contract, exit, signature, large, passing) in [
        (true, 0, false, false, false),
        (true, 0, true, true, false),
        (true, 101, false, false, false),
        (true, 101, false, true, false),
        (true, 7, true, false, false),
        (true, 7, true, true, false),
        (true, 101, true, false, true),
        (true, 101, true, true, true),
        (false, 0, false, false, true),
        (false, 0, false, true, true),
        (false, 101, true, false, false),
        (false, 101, true, true, false),
    ] {
        for caller in ["engine", "cli", "mcp"] {
            let diagnostic = format!(
                "actual-command-output-{}{}",
                "x".repeat(if large { 10_000 } else { 10 }),
                if signature {
                    "assertion failed"
                } else {
                    "different failure"
                }
            );
            let mut spec = output_spec(false);
            spec.seam_ref_pattern = Some(r"seam:(\S+)".into());
            let command = format!("printf '%s' '{diagnostic}'; exit {exit} # seam:tests/acdd/runner.rs::command_gate_diagnostics_follow_the_proof_expectation_across_public_callers");
            let (root, store, id) = command_fixture(&command, Some(spec));
            let mut configured = (*profile::load_for_workspace(&root.0).unwrap()).clone();
            let gate = configured
                .gates
                .iter_mut()
                .find(|gate| gate.id == "contract")
                .unwrap();
            gate.proof = profile::ProofDef::Kind(if contract {
                profile::ProofKind::Contract
            } else {
                profile::ProofKind::Command
            });
            gate.expected_failure = Some(ExpectedFailure {
                exit_codes: vec![101],
                stderr_contains: Some("assertion failed".into()),
            });
            root.write(
                ".forge/acdd/profile.yaml",
                &serde_yaml::to_string(&configured).unwrap(),
            );
            let result = match caller {
                "cli" => {
                    let output = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
                        .args([
                            "--root",
                            root.0.to_str().unwrap(),
                            "--format",
                            "json",
                            "task",
                            "verify",
                            &id,
                        ])
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
                }
                "mcp" => {
                    let mut command = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"));
                    command.args(["--root", root.0.to_str().unwrap(), "serve"]);
                    let mut client = StdioClient::with_command(&mut command);
                    client.payload(
                        "task_manage",
                        serde_json::json!({"action":"verify", "task_id":id}),
                    )
                }
                _ => forge_tasks::engine::verify_task(&root.0, &id, None).unwrap(),
            };
            let context = format!("{caller} contract={contract} exit={exit} signature={signature} large={large}: {result}");
            assert_eq!(result["verification"]["exit_code"], exit, "{context}");
            assert_eq!(result["evidence"].is_object(), passing, "{context}");
            assert_eq!(result["error"].is_null(), passing, "{context}");
            let traces = store.execution_traces(&id, Some("tasks.contract")).unwrap();
            assert_eq!(traces.len(), 1, "{context}");
            let trace = &traces[0];
            let snippet = trace.output_snippet.as_deref().unwrap();
            assert_eq!(snippet.contains("PASS check:"), passing, "{context}");
            assert_eq!(trace.log_path.is_some(), !passing && large, "{context}");
            assert_eq!(
                result["verification"]["diagnostic_mode"],
                if passing {
                    "pass"
                } else if large {
                    "truncated"
                } else {
                    "inline"
                },
                "{context}"
            );
            if !passing {
                assert!(snippet.contains("actual-command-output-"), "{context}");
                if let Some(path) = &trace.log_path {
                    assert_eq!(
                        fs::read_to_string(root.0.join(path)).unwrap(),
                        diagnostic,
                        "{context}"
                    );
                } else {
                    assert!(snippet.contains(&diagnostic), "{context}");
                }
            } else {
                assert!(!root.0.join(".forge/task-traces").exists(), "{context}");
            }
            assert_eq!(store.inspect(&id).unwrap().stage, "contract", "{context}");
        }
    }
}

#[test]
fn command_missing_registered_contract_command_fails_closed() {
    let (root, _, id) = command_fixture("printf checked", Some(output_spec(false)));
    let mut configured = (*profile::load_for_workspace(&root.0).unwrap()).clone();
    configured
        .gates
        .iter_mut()
        .find(|gate| gate.id == "contract")
        .unwrap()
        .command = None;
    root.write(
        ".forge/acdd/profile.yaml",
        &serde_yaml::to_string(&configured).unwrap(),
    );
    let error = forge_tasks::engine::verify_task(&root.0, &id, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("TASK_COMMAND_UNCONFIGURED"), "{error}");
}

#[test]
fn command_trace_migration_preserves_inline_diagnostics_larger_than_two_kib() {
    let (root, store, id) = command_fixture(
        "head -c 3000 /dev/zero | tr '\\0' x; exit 2",
        Some(output_spec(false)),
    );
    drop(store);
    let connection = rusqlite::Connection::open(root.0.join(".forge/tasks.sqlite")).unwrap();
    let schema: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name='gate_execution_traces'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    connection
        .execute_batch("DROP TABLE gate_execution_traces")
        .unwrap();
    connection
        .execute_batch(&schema.replace("<= 65536", "<= 2048"))
        .unwrap();
    drop(connection);
    let result = forge_tasks::engine::verify_task(&root.0, &id, None).unwrap();
    assert_eq!(result["verification"]["diagnostic_mode"], "inline");
    let store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    let traces = store.execution_traces(&id, Some("tasks.contract")).unwrap();
    assert!(
        traces
            .last()
            .unwrap()
            .output_snippet
            .as_ref()
            .unwrap()
            .len()
            > 2048
    );
    assert!(traces.last().unwrap().log_path.is_none());
}

#[test]
fn command_embedded_language_output_profiles_prove_real_command_summaries() {
    let embedded = profile::compiled();
    let registered = &embedded.commands["check_full_repo"];
    for (path, summary) in [
        ("src/lib.rs", "test result: ok. 3 passed; 0 failed"),
        ("tests/test_api.py", "=== 3 passed in 0.1s ==="),
        ("src/api.ts", "Tests: 3 passed, 3 total"),
    ] {
        assert!(registered.command_for_path(path).is_some());
        let (root, _, id) = command_fixture(
            &format!("printf '%s\\n' '{}'", summary),
            registered.output().cloned(),
        );
        let result = forge_tasks::engine::verify_task(&root.0, &id, None).unwrap();
        assert_eq!(
            result["evidence"]["proof"]["command_proof"]["tests_passed"], 3,
            "{path}: {result}"
        );
    }
}

#[test]
fn command_raw_failure_facts_cannot_be_forged_by_scope_warning_filenames() {
    let (root, store, id) = command_fixture("printf 'different failure'; exit 101 # cargo test --test acdd runner::successful_gate_execution_does_not_write_a_trace_log", Some(output_spec(false)));
    let mut configured = (*profile::load_for_workspace(&root.0).unwrap()).clone();
    let gate = configured
        .gates
        .iter_mut()
        .find(|gate| gate.id == "contract")
        .unwrap();
    gate.proof = profile::ProofDef::Kind(profile::ProofKind::Contract);
    gate.expected_failure = Some(ExpectedFailure {
        exit_codes: vec![101],
        stderr_contains: Some("assertion failed".into()),
    });
    root.write(
        ".forge/acdd/profile.yaml",
        &serde_yaml::to_string(&configured).unwrap(),
    );
    root.write("assertion failed.txt", "untrusted filename\n");
    for _ in 0..2 {
        let result = forge_tasks::engine::verify_task(&root.0, &id, None).unwrap();
        assert!(
            result["evidence"].is_null(),
            "decorated diagnostic must not prove raw failure: {result}"
        );
    }
    assert!(submit_zero_arg(&root.0, &id, "contract", Action::Pass, None).is_err());
    let run = latest_run(&store, &root.0, &id, "tasks.contract");
    let red = store
        .claim_bound_execution_traces(run.run_id)
        .unwrap()
        .pop()
        .unwrap();
    let mut green = red.clone();
    green.trace.exit_code = Some(0);
    green.trace.gate_path = "tasks.build".into();
    green.trace_order += 1;
    assert!(verify_red_to_green(
        &red,
        &green,
        configured.gates[0].expected_failure.as_ref().unwrap()
    )
    .is_err());
}

#[test]
fn command_cached_proof_requires_the_complete_current_pattern_plan() {
    for mutation in ["add", "remove", "change"] {
        let (root, _, id) = command_fixture("printf checked", Some(output_spec(false)));
        root.write("src/readme.md", "scoped documentation\n");
        let mut configured = (*profile::load_for_workspace(&root.0).unwrap()).clone();
        let mut patterns = PatternCommands {
            by_pattern: BTreeMap::from([("src/*.rs".into(), "printf rust >> .forge/runs".into())]),
            description: None,
            timeout_seconds: Some(5),
            output: Some(output_spec(false)),
        };
        if mutation == "remove" {
            patterns
                .by_pattern
                .insert("src/*.md".into(), "printf docs >> .forge/runs".into());
        }
        configured
            .commands
            .insert("check".into(), CommandDef::Pattern(patterns.clone()));
        root.write(
            ".forge/acdd/profile.yaml",
            &serde_yaml::to_string(&configured).unwrap(),
        );
        assert!(
            forge_tasks::engine::verify_task(&root.0, &id, None).unwrap()["evidence"].is_object()
        );
        match mutation {
            "add" => {
                patterns.by_pattern.insert(
                    "src/*.md".into(),
                    "printf docs > .forge/docs-ran; exit 2".into(),
                );
            }
            "remove" => {
                patterns.by_pattern.remove("src/*.md");
            }
            "change" => {
                patterns.by_pattern.insert(
                    "src/*.rs".into(),
                    "printf changed > .forge/changed-ran; exit 2".into(),
                );
            }
            _ => unreachable!(),
        }
        configured
            .commands
            .insert("check".into(), CommandDef::Pattern(patterns));
        root.write(
            ".forge/acdd/profile.yaml",
            &serde_yaml::to_string(&configured).unwrap(),
        );
        assert!(
            compile_evidence_envelope(&root.0, &id, "contract").is_err(),
            "{mutation} must invalidate cached plan"
        );
        let result = forge_tasks::engine::verify_task(&root.0, &id, None).unwrap();
        if mutation == "remove" {
            assert!(result["evidence"].is_object(), "{result}");
            assert_eq!(
                fs::read_to_string(root.0.join(".forge/runs")).unwrap(),
                "rustdocsrust"
            );
        } else {
            assert!(result["evidence"].is_null(), "{result}");
            assert!(root
                .0
                .join(if mutation == "add" {
                    ".forge/docs-ran"
                } else {
                    ".forge/changed-ran"
                })
                .exists());
        }
    }
}

#[test]
fn command_subset_execution_cannot_prove_an_entire_admitted_scope_plan() {
    let (root, _, id) = command_fixture("printf checked", Some(output_spec(false)));
    root.write("src/readme.md", "scoped documentation\n");
    let mut configured = (*profile::load_for_workspace(&root.0).unwrap()).clone();
    let command = CommandDef::Pattern(PatternCommands {
        by_pattern: BTreeMap::from([
            ("src/*.rs".into(), "printf rust".into()),
            (
                "src/*.md".into(),
                "printf docs > .forge/docs-ran; exit 2".into(),
            ),
        ]),
        description: None,
        timeout_seconds: Some(5),
        output: Some(output_spec(false)),
    });
    configured.commands.insert("check".into(), command.clone());
    root.write(
        ".forge/acdd/profile.yaml",
        &serde_yaml::to_string(&configured).unwrap(),
    );
    execute_command_gate(
        &root.0,
        &id,
        "tasks.contract",
        &command,
        &["src/lib.rs".into()],
    )
    .unwrap();
    assert!(compile_evidence_envelope(&root.0, &id, "contract").is_err());
    let result = forge_tasks::engine::verify_task(&root.0, &id, None).unwrap();
    assert!(result["evidence"].is_null(), "{result}");
    assert!(root.0.join(".forge/docs-ran").exists());
}

#[test]
fn transition_runner_compiles_proofs_and_covers_all_task3_subtasks() {
    let (root, mut store, _) = fixture();
    let task_id = "forge-mcp/m-test:first";
    let outside_trace_root = root.0.with_extension("task3-outside-traces");
    let outside_trace_root_arg = shell_quote(&outside_trace_root.to_string_lossy());
    root.write(".gitignore", ".forge/\n");
    root.write("src/readme.md", "Scoped documentation command input.\n");
    fs::create_dir_all(root.0.join(".forge/task3-bin")).unwrap();
    let fake_cargo = root.0.join(".forge/task3-bin/cargo");
    fs::write(
        &fake_cargo,
        "#!/bin/sh\ncase \"$TASK3_MODE\" in\n  contract-red) printf 'assertion failed\\n' >&2; exit 101 ;;\n  large-fail) head -c 10000 /dev/zero | tr '\\0' x; exit 1 ;;\n  build-fail) printf 'test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\\n'; exit 1 ;;\n  forge-symlink) mv .forge .forge-preserved && ln -s \"$TASK3_OUTSIDE\" .forge && head -c 10000 /dev/zero | tr '\\0' x; exit 1 ;;\n  *) printf 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\\n'; printf 'label=%s\\n' \"$TASK3_LABEL\" ;;\nesac\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&fake_cargo).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&fake_cargo, permissions).unwrap();
    }

    let fake_path = shell_quote(&root.0.join(".forge/task3-bin").to_string_lossy());
    let seam_filter =
        "acdd::runner::transition_runner_compiles_proofs_and_covers_all_task3_subtasks";
    let cargo_seam = format!("cargo test --test acdd {seam_filter} -- --exact");
    let contract_command =
        format!("PATH={fake_path}:$PATH TASK3_MODE=contract-red TASK3_LABEL=contract {cargo_seam}");
    let rust_command = format!(
        "printf 'rust\\n' >> .forge/runner-order.log && TASK3_OUTSIDE={outside_trace_root_arg} PATH={fake_path}:$PATH TASK3_MODE=$(cat .forge/runner-mode) TASK3_LABEL=rust {cargo_seam} && printf 'pid=%s\\n' \"$$\""
    );
    let docs_command = format!(
        "printf 'docs\\n' >> .forge/runner-order.log && PATH={fake_path}:$PATH TASK3_MODE=$(cat .forge/runner-mode) TASK3_LABEL=docs {cargo_seam} && printf '%s' '{}' && printf 'pid=%s\\n' \"$$\"",
        "x".repeat(4096)
    );
    let mut profile_override = (*profile::compiled()).clone();
    profile_override.commands.insert(
        "task3-contract".into(),
        CommandDef::Detailed {
            command: contract_command,
            timeout_seconds: Some(5),
            description: Some("Run the persisted contract seam fixture".into()),
            scope_args_by_pattern: Default::default(),
            output: None,
        },
    );
    profile_override.commands.insert(
        "task3-check".into(),
        CommandDef::Pattern(PatternCommands {
            by_pattern: BTreeMap::from([
                ("src/*.rs".into(), rust_command),
                ("src/*.md".into(), docs_command),
                ("src/*.txt".into(), "sleep 30; printf done".into()),
            ]),
            description: Some("Dispatch commands for each matching path".into()),
            timeout_seconds: Some(1),
            output: None,
        }),
    );
    let contract_gate = profile_override
        .gates
        .iter_mut()
        .find(|gate| gate.id == "contract")
        .unwrap();
    contract_gate.command = Some("task3-contract".into());
    contract_gate.expected_failure = Some(ExpectedFailure {
        exit_codes: vec![101],
        stderr_contains: Some("assertion failed".into()),
    });
    profile_override
        .gates
        .iter_mut()
        .find(|gate| gate.id == "build")
        .unwrap()
        .command = Some("task3-check".into());
    fs::create_dir_all(root.0.join(".forge/acdd")).unwrap();
    let profile_yaml = serde_yaml::to_string(&profile_override).unwrap();
    fs::write(root.0.join(".forge/acdd/profile.yaml"), &profile_yaml).unwrap();
    fs::write(root.0.join(".forge/runner-mode"), "green\n").unwrap();
    let profile = profile::load_for_workspace(&root.0).unwrap();
    let contract_commands = profile.commands["task3-contract"].clone();
    let build_commands = profile.commands["task3-check"].clone();

    store
        .claim(
            task_id,
            "contract",
            "contract-author",
            root.0.to_str().unwrap(),
        )
        .unwrap();
    let ownership_db = root.0.join(".forge/tasks.sqlite");
    let mut foreign_store =
        TasksStore::open_project(&ownership_db, "forge-mcp", "another-project").unwrap();
    let contract_execution =
        execute_command_gate(&root.0, task_id, "tasks.contract", &contract_commands, &[]);
    let contract_run = latest_run(&store, &root.0, task_id, "tasks.contract");
    let red_entries = store
        .claim_bound_execution_traces(contract_run.run_id)
        .unwrap();
    let red = red_entries.last().cloned().unwrap();
    let envelope = compile_evidence_envelope(&root.0, task_id, "contract");
    let contract_proof_ok = envelope.as_ref().is_ok_and(|proof| {
        proof["task_id"] == task_id
            && proof["stage"] == "contract"
            && proof["proof"]["contract_proof"]["red_exit_code"] == 101
            && proof["proof"]["contract_proof"]["seam_test_ref"]
                == "tests/acdd/runner.rs::transition_runner_compiles_proofs_and_covers_all_task3_subtasks"
            && proof["candidate_baseline_head"].as_str().is_some()
    });
    let submitted = submit_zero_arg(&root.0, task_id, "contract", Action::Pass, None);
    let zero_arg_submit_ok = submitted.as_ref().is_ok_and(|response| {
        response["stage"] == "build"
            && store
                .inspect(task_id)
                .is_ok_and(|task| task.stage == "build")
    });
    store
        .claim(task_id, "build", "builder", root.0.to_str().unwrap())
        .unwrap();

    let timeout_started = Instant::now();
    let timeout_result = execute_command_gate(
        &root.0,
        task_id,
        "tasks.build",
        &build_commands,
        &["src/slow.txt".to_owned()],
    );
    let timeout_bounded =
        timeout_result.is_err() && timeout_started.elapsed() < Duration::from_secs(5);

    let source_paths = vec!["src/lib.rs".to_owned(), "src/readme.md".to_owned()];
    let execution = execute_command_gate(
        &root.0,
        task_id,
        "tasks.build",
        &build_commands,
        &source_paths,
    );
    let command_order =
        fs::read_to_string(root.0.join(".forge/runner-order.log")).unwrap_or_default();
    let command_execution_ok = execution.as_ref().is_ok_and(|traces| {
        traces.len() == 2
            && traces
                .iter()
                .all(|trace| trace.status == "completed" && trace.exit_code == Some(0))
            && command_order == "rust\ndocs\n"
    });
    let complete_log_preserved = execution
        .as_ref()
        .is_ok_and(|traces| traces.iter().all(|trace| trace.log_path.is_none()));
    let green_run = latest_run(&store, &root.0, task_id, "tasks.build");
    let green_entries = store
        .claim_bound_execution_traces(green_run.run_id)
        .unwrap();
    let green = green_entries.last().cloned().unwrap();
    let red_green = verify_red_to_green(
        &red,
        &green,
        &ExpectedFailure {
            exit_codes: vec![101],
            stderr_contains: Some("assertion failed".into()),
        },
    );
    let red_green_ok = red_green.as_ref().is_ok_and(|proof| {
        proof["verified"] == true
            && proof["red_exit_code"] == 101
            && proof["green_exit_code"] == 0
            && proof["seam_test_ref"]
                == "tests/acdd/runner.rs::transition_runner_compiles_proofs_and_covers_all_task3_subtasks"
            && proof["red_execution_order"].as_i64().unwrap()
                < proof["green_execution_order"].as_i64().unwrap()
    });
    let mut mismatched_seam = green.clone();
    mismatched_seam.trace.command_str = mismatched_seam
        .trace
        .command_str
        .replace(seam_filter, "acdd::runner::different_test");
    let mismatched_seam_rejected = verify_red_to_green(
        &red,
        &mismatched_seam,
        &ExpectedFailure {
            exit_codes: vec![101],
            stderr_contains: Some("assertion failed".into()),
        },
    )
    .is_err();
    let mut reversed = green.clone();
    reversed.trace_order = red.trace_order;
    let reversed_rejected = verify_red_to_green(
        &red,
        &reversed,
        &ExpectedFailure {
            exit_codes: vec![101],
            stderr_contains: Some("assertion failed".into()),
        },
    )
    .is_err();

    let first_command_evidence = compile_evidence_envelope(&root.0, task_id, "build");
    let command_proof_ok = first_command_evidence.as_ref().is_ok_and(|proof| {
        proof["proof"]["command_proof"]["command"] == "task3-check"
            && proof["proof"]["command_proof"]["exit_code"] == 0
            && proof["proof"]["command_proof"]["tests_passed"] == 2
            && proof["proof"]["command_proof"]["tests_failed"] == 0
    });
    let mut altered_profile = (*profile).clone();
    if let Some(CommandDef::Pattern(patterns)) = altered_profile.commands.get_mut("task3-check") {
        patterns
            .by_pattern
            .insert("src/*.rs".into(), "printf 'unregistered command\\n'".into());
    }
    fs::write(
        root.0.join(".forge/acdd/profile.yaml"),
        serde_yaml::to_string(&altered_profile).unwrap(),
    )
    .unwrap();
    let command_identity_rejected = compile_evidence_envelope(&root.0, task_id, "build").is_err();
    fs::write(root.0.join(".forge/acdd/profile.yaml"), &profile_yaml).unwrap();
    fs::write(
        root.0.join("src/lib.rs"),
        "pub fn candidate() { /* drift */ }\n",
    )
    .unwrap();
    let candidate_drift_rejected = compile_evidence_envelope(&root.0, task_id, "build").is_err();
    fs::write(root.0.join("src/lib.rs"), "pub fn example() {}\n").unwrap();

    fs::write(root.0.join(".forge/runner-mode"), "build-fail\n").unwrap();
    let newer_failure = execute_command_gate(
        &root.0,
        task_id,
        "tasks.build",
        &build_commands,
        &["src/lib.rs".to_owned()],
    );
    let stale_success_rejected =
        newer_failure.is_err() && compile_evidence_envelope(&root.0, task_id, "build").is_err();
    fs::write(root.0.join(".forge/runner-mode"), "green\n").unwrap();
    let recovery_run = execute_command_gate(
        &root.0,
        task_id,
        "tasks.build",
        &build_commands,
        &source_paths,
    );
    let recovery_proof_ok =
        recovery_run.is_ok() && compile_evidence_envelope(&root.0, task_id, "build").is_ok();

    let concurrent_root_a = root.0.clone();
    let concurrent_root_b = root.0.clone();
    let concurrent_commands_a = build_commands.clone();
    let concurrent_commands_b = build_commands.clone();
    let first = thread::spawn(move || {
        execute_command_gate(
            &concurrent_root_a,
            task_id,
            "tasks.build",
            &concurrent_commands_a,
            &["src/lib.rs".to_owned()],
        )
    });
    let second = thread::spawn(move || {
        execute_command_gate(
            &concurrent_root_b,
            task_id,
            "tasks.build",
            &concurrent_commands_b,
            &["src/lib.rs".to_owned()],
        )
    });
    let concurrent_a = first.join().unwrap();
    let concurrent_b = second.join().unwrap();
    let concurrent_traces = concurrent_a
        .as_ref()
        .ok()
        .and_then(|traces| traces.first())
        .into_iter()
        .chain(concurrent_b.as_ref().ok().and_then(|traces| traces.first()))
        .cloned()
        .collect::<Vec<_>>();
    let concurrent_attempts = concurrent_traces
        .iter()
        .map(|trace| trace.attempt_id)
        .collect::<BTreeSet<_>>();
    let concurrent_log_paths = concurrent_traces
        .iter()
        .filter_map(|trace| trace.log_path.clone())
        .collect::<BTreeSet<_>>();
    let concurrency_safe = concurrent_a.is_ok()
        && concurrent_b.is_ok()
        && concurrent_traces.len() == 2
        && concurrent_attempts.len() == 2
        && concurrent_log_paths.is_empty();

    #[cfg(unix)]
    let (final_symlink_rejected, parent_symlink_rejected, forged_parent_symlink_rejected) = {
        use std::os::unix::fs::symlink;
        fs::write(root.0.join(".forge/runner-mode"), "large-fail\n").unwrap();

        let path_command_id = "task3-check::src/lib.rs";
        let previous = store
            .execution_traces(task_id, Some("tasks.build"))
            .unwrap();
        let next_attempt = previous
            .iter()
            .filter(|trace| trace.command_id == path_command_id)
            .map(|trace| trace.attempt_id)
            .max()
            .unwrap_or(0)
            + 1;
        let final_log = root
            .0
            .join(".forge/task-traces")
            .join(digest(task_id))
            .join(digest("tasks.build"))
            .join(format!("{next_attempt}-{}.log", digest(path_command_id)));
        fs::create_dir_all(final_log.parent().unwrap()).unwrap();
        let protected_target = root.0.join(".forge/symlink-target.txt");
        fs::write(&protected_target, "keep this target\n").unwrap();
        symlink(&protected_target, &final_log).unwrap();
        let final_result = execute_command_gate(
            &root.0,
            task_id,
            "tasks.build",
            &build_commands,
            &["src/lib.rs".to_owned()],
        );
        let final_rejected = final_result.is_err()
            && fs::read_to_string(&protected_target).unwrap() == "keep this target\n";
        fs::remove_file(&final_log).unwrap();

        let trace_root = root.0.join(".forge/task-traces");
        fs::remove_dir_all(&trace_root).unwrap();
        let parent_target = root.0.join(".forge/parent-target");
        fs::create_dir_all(&parent_target).unwrap();
        fs::write(parent_target.join("sentinel.txt"), "untouched\n").unwrap();
        symlink(&parent_target, &trace_root).unwrap();
        let parent_result = execute_command_gate(
            &root.0,
            task_id,
            "tasks.build",
            &build_commands,
            &["src/lib.rs".to_owned()],
        );
        let parent_rejected = parent_result.is_err()
            && fs::read_dir(&parent_target).unwrap().count() == 1
            && fs::read_to_string(parent_target.join("sentinel.txt")).unwrap() == "untouched\n";
        fs::remove_file(&trace_root).unwrap();

        fs::create_dir_all(&outside_trace_root).unwrap();
        let outside_task_traces = outside_trace_root.join("task-traces");
        assert!(!outside_task_traces.exists());
        let forge_path = root.0.join(".forge");
        let forge_backup = root.0.join(".forge-preserved");
        fs::write(root.0.join(".forge/runner-mode"), "forge-symlink\n").unwrap();
        let forge_result = execute_command_gate(
            &root.0,
            task_id,
            "tasks.build",
            &build_commands,
            &["src/lib.rs".to_owned()],
        );
        let forged_parent = fs::symlink_metadata(&forge_path)
            .is_ok_and(|metadata| metadata.file_type().is_symlink());
        let outside_untouched = forge_result.is_err() && !outside_task_traces.exists();
        if forged_parent {
            fs::remove_file(&forge_path).unwrap();
        }
        if forge_backup.exists() {
            fs::rename(&forge_backup, &forge_path).unwrap();
        }
        fs::write(forge_path.join("runner-mode"), "green\n").unwrap();
        fs::remove_dir_all(&outside_trace_root).unwrap();
        (
            final_rejected,
            parent_rejected,
            forged_parent && outside_untouched,
        )
    };
    #[cfg(not(unix))]
    let (final_symlink_rejected, parent_symlink_rejected, forged_parent_symlink_rejected) =
        (true, true, true);

    let final_run = execute_command_gate(
        &root.0,
        task_id,
        "tasks.build",
        &build_commands,
        &source_paths,
    );
    let final_proof_ok =
        final_run.is_ok() && compile_evidence_envelope(&root.0, task_id, "build").is_ok();
    let latest_build_traces = store
        .execution_traces(task_id, Some("tasks.build"))
        .unwrap();
    let telemetry_note =
        telemetry_passport_note(task_id, &latest_build_traces, &"diff ".repeat(512));
    let telemetry_note_ok = telemetry_note.as_ref().is_ok_and(|note| note.len() <= 2048);

    let start_ownership_run = |store: &mut TasksStore| {
        store.start_execution_run(
            task_id,
            "tasks.build",
            &root.0,
            "task3-project-ownership-candidate",
        )
    };
    let make_ownership_trace = |run: &ClaimBoundExecutionRun, command_id: &str, attempt_id: i64| {
        let mut trace: GateExecutionTrace = red.trace.clone();
        trace.milestone_id = run.milestone_id.clone();
        trace.subject = run.subject.clone();
        trace.gate_path = run.gate_path.clone();
        trace.attempt_id = attempt_id;
        trace.command_id = command_id.to_owned();
        trace.command_str = "printf ownership".to_owned();
        trace.status = "completed".to_owned();
        trace.exit_code = Some(0);
        trace.log_path = None;
        trace.output_snippet = Some("ownership trace".to_owned());
        trace.stdout_sha256 = None;
        trace.duration_ms = 1;
        trace
    };

    let reserve_run = start_ownership_run(&mut store).unwrap();
    let foreign_reserve =
        foreign_store.reserve_execution_attempt(reserve_run.run_id, "ownership-reserve");
    let reserve_rejected = foreign_reserve
        .as_ref()
        .err()
        .is_some_and(|error| error.to_string() == "TASK_PROJECT_MISMATCH");
    if reserve_rejected {
        let attempt = store
            .reserve_execution_attempt(reserve_run.run_id, "ownership-reserve")
            .unwrap();
        let trace = make_ownership_trace(&reserve_run, "ownership-reserve", attempt);
        store
            .record_claim_bound_execution_trace(reserve_run.run_id, &trace)
            .unwrap();
        store.complete_execution_run(reserve_run.run_id).unwrap();
    }

    let record_run = start_ownership_run(&mut store).unwrap();
    let record_attempt = store
        .reserve_execution_attempt(record_run.run_id, "ownership-record")
        .unwrap();
    let record_trace = make_ownership_trace(&record_run, "ownership-record", record_attempt);
    let foreign_record =
        foreign_store.record_claim_bound_execution_trace(record_run.run_id, &record_trace);
    let record_rejected = foreign_record
        .as_ref()
        .err()
        .is_some_and(|error| error.to_string() == "TASK_PROJECT_MISMATCH");
    let owner_recorded = store
        .record_claim_bound_execution_trace(record_run.run_id, &record_trace)
        .is_ok();
    if owner_recorded {
        store.complete_execution_run(record_run.run_id).unwrap();
    }

    let complete_run = start_ownership_run(&mut store).unwrap();
    let complete_attempt = store
        .reserve_execution_attempt(complete_run.run_id, "ownership-complete")
        .unwrap();
    let complete_trace =
        make_ownership_trace(&complete_run, "ownership-complete", complete_attempt);
    store
        .record_claim_bound_execution_trace(complete_run.run_id, &complete_trace)
        .unwrap();
    let foreign_complete = foreign_store.complete_execution_run(complete_run.run_id);
    let complete_rejected = foreign_complete
        .as_ref()
        .err()
        .is_some_and(|error| error.to_string() == "TASK_PROJECT_MISMATCH");
    let owner_completed = if complete_rejected {
        store.complete_execution_run(complete_run.run_id).is_ok()
    } else {
        false
    };

    let read_run = start_ownership_run(&mut store).unwrap();
    let read_attempt = store
        .reserve_execution_attempt(read_run.run_id, "ownership-read")
        .unwrap();
    let read_trace = make_ownership_trace(&read_run, "ownership-read", read_attempt);
    store
        .record_claim_bound_execution_trace(read_run.run_id, &read_trace)
        .unwrap();
    let foreign_read = foreign_store.claim_bound_execution_traces(read_run.run_id);
    let read_rejected = foreign_read
        .as_ref()
        .err()
        .is_some_and(|error| error.to_string() == "TASK_PROJECT_MISMATCH");
    let owner_read_ok = store
        .claim_bound_execution_traces(read_run.run_id)
        .is_ok_and(|traces| traces.len() == 1);
    store.complete_execution_run(read_run.run_id).unwrap();
    let project_ownership_isolated = reserve_rejected
        && record_rejected
        && owner_recorded
        && complete_rejected
        && owner_completed
        && read_rejected
        && owner_read_ok;

    let routing = RejectRouting {
        scope: Some("build".into()),
        symbols: Some("contract".into()),
    };
    let scope_route = evaluate_reject_routing(&routing, "scope");
    let symbol_route = evaluate_reject_routing(&routing, "symbols");
    let rejection_routing_ok = scope_route
        .as_deref()
        .is_ok_and(|destination| destination == "build")
        && symbol_route
            .as_deref()
            .is_ok_and(|destination| destination == "contract");

    // These fixture artifacts are verified above. Clear them before the real Git
    // rebase proof so the production rebase helper receives a clean worktree.
    drop(store);
    if root.0.join(".forge").exists() {
        fs::remove_dir_all(root.0.join(".forge")).unwrap();
    }

    let run_git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(&root.0)
            .output()
            .unwrap()
    };
    assert!(run_git(&["add", "-A"]).status.success());
    assert!(run_git(&["commit", "-m", "capture fixture baseline"])
        .status
        .success());
    let base_head = git_head(&root.0);
    assert!(
        run_git(&["switch", "-c", "upstream-snapshot", base_head.as_str()])
            .status
            .success()
    );
    fs::write(root.0.join("upstream-note.txt"), "upstream snapshot\n").unwrap();
    assert!(run_git(&["add", "upstream-note.txt"]).status.success());
    assert!(run_git(&["commit", "-m", "advance upstream snapshot"])
        .status
        .success());
    assert!(
        run_git(&["switch", "-c", "speculative-task", base_head.as_str()])
            .status
            .success()
    );
    fs::write(root.0.join("task-note.txt"), "speculative task\n").unwrap();
    assert!(run_git(&["add", "task-note.txt"]).status.success());
    assert!(run_git(&["commit", "-m", "speculative task change"])
        .status
        .success());
    assert!(run_git(&["switch", "upstream-snapshot"]).status.success());
    let rebased = rebase_speculative_dependency(&root.0, "speculative-task", "upstream-snapshot");
    let upstream_is_ancestor = run_git(&[
        "merge-base",
        "--is-ancestor",
        "upstream-snapshot",
        "speculative-task",
    ])
    .status
    .success();

    assert!(
        contract_execution.is_ok(),
        "contract fixture produces an expected runtime red"
    );
    assert!(
        contract_proof_ok,
        "claim-bound compiler accepts the current seam red: {envelope:?}"
    );
    assert!(
        zero_arg_submit_ok,
        "zero-argument proof advances the contract gate"
    );
    assert!(
        timeout_bounded,
        "timeout terminates the process group and bounds pipe readers"
    );
    assert!(
        command_execution_ok,
        "by_pattern commands execute sequentially: {execution:?}; order={command_order:?}"
    );
    assert!(red_green_ok, "same seam red precedes green: {red_green:?}");
    assert!(
        mismatched_seam_rejected,
        "red-to-green rejects different seam tests"
    );
    assert!(
        reversed_rejected,
        "red-to-green rejects reversed execution chronology"
    );
    assert!(
        command_proof_ok,
        "command proof uses the registered command and complete current run: {first_command_evidence:?}"
    );
    assert!(
        command_identity_rejected,
        "command proof rejects a trace when the registered command changes"
    );
    assert!(
        candidate_drift_rejected,
        "candidate drift invalidates prior command evidence"
    );
    assert!(
        stale_success_rejected,
        "newer failure invalidates an older success"
    );
    assert!(
        recovery_proof_ok,
        "a later successful run can prove the current candidate"
    );
    assert!(
        concurrency_safe,
        "concurrent executions reserve unique attempts and immutable logs"
    );
    assert!(
        final_symlink_rejected,
        "final log symlink cannot replace its target"
    );
    assert!(
        parent_symlink_rejected,
        "parent log symlink is rejected before writing through it"
    );
    assert!(
        forged_parent_symlink_rejected,
        "a .forge symlink is rejected before creating task-traces outside the worktree"
    );
    assert!(
        final_proof_ok,
        "the latest safe command run remains eligible for proof"
    );
    assert!(
        rejection_routing_ok,
        "scope and symbol findings route to configured gates"
    );
    assert!(
        project_ownership_isolated,
        "run-id operations reject cross-project access to shared SQLite rows"
    );
    assert!(
        telemetry_note_ok,
        "telemetry passport note is bounded to 2 KiB"
    );
    assert!(
        complete_log_preserved,
        "full command output remains available in its log file"
    );
    assert!(
        rebased.is_ok() && upstream_is_ancestor,
        "speculative branch rebases to upstream snapshot; rebase={rebased:?}; ancestor={upstream_is_ancestor}"
    );
}

fn latest_run(
    store: &TasksStore,
    root: &Path,
    task_id: &str,
    gate_path: &str,
) -> ClaimBoundExecutionRun {
    let task = store.inspect(task_id).unwrap();
    let worker = task.worker_id.as_deref().unwrap();
    let worktree = Path::new(task.worktree.as_deref().unwrap())
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(Path::new(&worktree), root.canonicalize().unwrap());
    store
        .latest_claim_bound_execution_run(
            task_id,
            gate_path,
            task.claim_revision,
            task.contract_revision,
            worker,
            &worktree,
        )
        .unwrap()
        .unwrap()
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
