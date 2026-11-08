use super::*;

#[test]
fn contract_gate_proofs_require_runtime_red_and_reject_compile_or_signature_failures() {
    let harness = CommandGateHarness::new();

    harness.configure_check("cargo test --lib", None, profile::ProofKind::Contract);
    harness
        .root
        .write(".gitignore", ".forge/\ntarget/\nCargo.lock\n");
    harness.root.write(
        "Cargo.toml",
        "[package]\nname = \"red-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    harness
        .root
        .write("src/lib.rs", "fn broken() { missing_symbol(); }\n");
    harness.configure_expected_failure(ExpectedFailure {
        exit_codes: vec![101],
        stderr_contains: Some("assertion failed".into()),
    });
    let configured = (*profile::load_for_workspace(harness.root.root()).unwrap()).clone();
    let result = harness.verify().unwrap();
    assert_eq!(result["verification"]["exit_code"], 101, "{result}");
    assert!(result["evidence"].is_null(), "{result}");
    let error = execute_command_gate(
        harness.root.root(),
        &harness.id,
        "tasks.contract",
        &configured.commands["check"],
        &[],
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("TASK_COMMAND_FAILED"),
        "{error:#}"
    );
    assert!(compile_evidence_envelope(harness.root.root(), &harness.id, "contract").is_err());

    harness.restore();
    let mut spec = output_spec(false);
    spec.seam_ref_pattern = Some(r"seam:(\S+)".into());
    harness.configure_check(
        "head -c 10000 /dev/zero | tr '\\0' x; printf 'assertion failed' >&2; exit 101 # seam:tests/acdd/runner.rs::command_gate_diagnostics_follow_the_proof_expectation_across_public_callers",
        Some(spec),
        profile::ProofKind::Contract,
    );
    harness.configure_expected_failure(ExpectedFailure {
        exit_codes: vec![101],
        stderr_contains: Some("assertion failed".into()),
    });
    let result = harness.verify().unwrap();
    assert_eq!(
        result["evidence"]["proof"]["contract_proof"]["red_exit_code"], 101,
        "{result}"
    );
    assert_eq!(
        result["evidence"]["proof"]["contract_proof"]["seam_test_ref"],
        "tests/acdd/runner.rs::command_gate_diagnostics_follow_the_proof_expectation_across_public_callers"
    );
    assert!(compile_evidence_envelope(harness.root.root(), &harness.id, "contract").is_ok());
    assert!(!harness.root.root().join(".forge/task-traces").exists());

    harness.restore();
    harness.configure_check(
        "printf 'different failure'; exit 101 # cargo test --test acdd runner::command_gate_diagnostics_follow_the_proof_expectation_across_public_callers",
        Some(output_spec(false)),
        profile::ProofKind::Contract,
    );
    let store = harness.store();
    harness.configure_expected_failure(ExpectedFailure {
        exit_codes: vec![101],
        stderr_contains: Some("assertion failed".into()),
    });
    let configured = (*profile::load_for_workspace(harness.root.root()).unwrap()).clone();
    harness
        .root
        .write("assertion failed.txt", "untrusted filename\n");
    for _ in 0..2 {
        let result = harness.verify().unwrap();
        assert!(
            result["evidence"].is_null(),
            "decorated diagnostic must not prove raw failure: {result}"
        );
    }
    assert!(submit_zero_arg(
        harness.root.root(),
        &harness.id,
        "contract",
        Action::Pass,
        None,
        Some("test-session-command-builder"),
    )
    .is_err());
    let run = latest_run(&store, harness.root.root(), &harness.id, "tasks.contract");
    let red = store
        .claim_bound_execution_traces(run.run_id)
        .unwrap()
        .pop()
        .unwrap();
    let mut green = red.clone();
    green.trace.exit_code = Some(0);
    green.trace.gate_path = "tasks.build".into();
    green.trace_order += 1;
    let red_green = verify_red_to_green(
        &red,
        &green,
        configured
            .find_gate("contract")
            .unwrap()
            .expected_failure
            .as_ref()
            .unwrap(),
    );
    assert!(red_green
        .unwrap_err()
        .to_string()
        .contains("TASK_SEAM_TRANSITION_INVALID"));
}

#[test]
fn command_gate_diagnostics_follow_the_proof_expectation_across_public_callers() {
    let mut harness = CommandGateHarness::new();
    super::support::commit_fixture_baseline(&harness.root);
    harness.recapture();
    let cases = [
        (true, 0, false, false, false, "engine"),
        (true, 0, true, true, false, "engine"),
        (true, 101, false, false, false, "engine"),
        (true, 101, false, true, false, "engine"),
        (true, 7, true, false, false, "engine"),
        (true, 7, true, true, false, "engine"),
        (true, 101, true, false, true, "engine"),
        (true, 101, true, true, true, "engine"),
        (false, 0, false, false, true, "engine"),
        (false, 0, false, true, true, "engine"),
        (false, 101, true, false, false, "engine"),
        (false, 101, true, true, false, "engine"),
        (true, 101, true, false, true, "cli"),
        (false, 101, true, true, false, "mcp"),
    ];
    for (contract, exit, signature, large, passing, caller) in cases {
        harness.restore();
        let diagnostic = format!(
            "actual-command-output-{}{}",
            "x".repeat(if large { 10_000 } else { 10 }),
            if signature {
                "assertion failed"
            } else {
                "different failure"
            }
        );
        let command = format!(
            "printf '%s' '{diagnostic}' >&2; exit {exit} # seam:tests/acdd/runner.rs::command_gate_diagnostics_follow_the_proof_expectation_across_public_callers"
        );
        let mut spec = output_spec(false);
        spec.seam_ref_pattern = Some(r"seam:(\S+)".into());
        let store = forge_tasks::engine::store_for_task(harness.root.root(), &harness.id).unwrap();
        harness.configure_check_with_timeout(
            &command,
            Some(spec),
            if contract {
                profile::ProofKind::Contract
            } else {
                profile::ProofKind::Command
            },
            5,
        );
        harness.configure_expected_failure(ExpectedFailure {
            exit_codes: vec![101],
            stderr_contains: Some("assertion failed".into()),
        });
        let result = match caller {
            "cli" => {
                let output = super::mcp::in_process_cli::run_cli_json(
                    &harness.root,
                    &["task", "verify", &harness.id],
                );
                assert!(
                    output.status.success(),
                    "CLI verify failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                serde_json::from_slice(&output.stdout).unwrap()
            }
            "mcp" => {
                let mut client =
                    super::mcp::in_process::McpTestHarness::new(&harness.root).unwrap();
                let result = client
                    .payload(
                        "task_manage",
                        serde_json::json!({"action":"verify", "task_id":harness.id}),
                    )
                    .unwrap();
                client.finish().unwrap();
                result
            }
            _ => forge_tasks::engine::verify_task(harness.root.root(), &harness.id, None).unwrap(),
        };
        harness.executed += 1;
        let context = format!(
            "{caller} contract={contract} exit={exit} signature={signature} large={large}: {result}"
        );
        assert_eq!(result["verification"]["exit_code"], exit, "{context}");
        assert_eq!(result["evidence"].is_object(), passing, "{context}");
        assert_eq!(result["error"].is_null(), passing, "{context}");
        let traces = store
            .execution_traces(&harness.id, Some("tasks.contract"))
            .unwrap();
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
                    fs::read_to_string(harness.root.root().join(path)).unwrap(),
                    format!("[stderr]\n{diagnostic}"),
                    "{context}"
                );
            } else {
                assert!(snippet.contains(&diagnostic), "{context}");
            }
        } else {
            assert!(
                !harness.root.root().join(".forge/task-traces").exists(),
                "{context}"
            );
        }
        assert_eq!(
            store.inspect(&harness.id).unwrap().stage,
            "contract",
            "{context}"
        );
    }
    assert_eq!(
        harness.executed, 14,
        "engine covers every output case; CLI and MCP each smoke once"
    );
}
#[test]
fn command_missing_registered_contract_command_fails_closed() {
    let harness = CommandGateHarness::with_command("printf checked", Some(output_spec(false)));
    let mut configured = (*profile::load_for_workspace(harness.root.root()).unwrap()).clone();
    configured.find_gate_mut("contract").unwrap().command = None;
    harness.root.write(
        ".forge/acdd/profile.yaml",
        &serde_yaml::to_string(&configured).unwrap(),
    );
    let error = harness.verify().unwrap_err().to_string();
    assert!(error.contains("TASK_COMMAND_UNCONFIGURED"), "{error}");
}

#[test]
fn command_trace_migration_preserves_inline_diagnostics_larger_than_two_kib() {
    let harness = CommandGateHarness::with_command(
        "head -c 3000 /dev/zero | tr '\\0' x; exit 2",
        Some(output_spec(false)),
    );
    let connection =
        rusqlite::Connection::open(harness.root.root().join(".forge/tasks.sqlite")).unwrap();
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
    let result = harness.verify().unwrap();
    assert_eq!(result["verification"]["diagnostic_mode"], "inline");
    let store = harness.store();
    let traces = store
        .execution_traces(&harness.id, Some("tasks.contract"))
        .unwrap();
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
fn command_subset_execution_cannot_prove_an_entire_admitted_scope_plan() {
    let harness = CommandGateHarness::with_command("printf checked", Some(output_spec(false)));
    harness
        .root
        .write("src/readme.md", "scoped documentation\n");
    let mut configured = (*profile::load_for_workspace(harness.root.root()).unwrap()).clone();
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
    harness.root.write(
        ".forge/acdd/profile.yaml",
        &serde_yaml::to_string(&configured).unwrap(),
    );
    execute_command_gate(
        harness.root.root(),
        &harness.id,
        "tasks.contract",
        &command,
        &["src/lib.rs".into()],
    )
    .unwrap();
    assert!(compile_evidence_envelope(harness.root.root(), &harness.id, "contract").is_err());
    let result = harness.verify().unwrap();
    assert!(result["evidence"].is_null(), "{result}");
    assert!(harness.root.root().join(".forge/docs-ran").exists());
}

#[test]
fn wrapper_output_cannot_authorize_seam_proof() {
    let harness =
        CommandGateHarness::with_proof("./test-runner.sh", None, profile::ProofKind::SeamTest);
    harness.root.write(
        "test-runner.sh",
        "#!/bin/sh\nprintf 'Running tests/runner_fixture.rs\\ntest seam_test ... ok\\ntest result: ok. 1 passed; 0 failed\\n'\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(harness.root.root().join("test-runner.sh"), perms).unwrap();
    }
    let mut store = harness.store();
    harness.execute_check().unwrap();
    let (task, mut evidence) = unobserved_task_evidence(&store, &harness.id);
    evidence.proof = serde_json::json!({
        "seam_test_proof": {
            "exit_code": 0,
            "seam_test_refs": ["tests/runner_fixture.rs::seam_test"],
            "tests_passed": 1,
            "tests_failed": 0
        }
    });
    let error = submit_unobserved_pass(&mut store, &evidence).unwrap_err();
    assert!(
        error.to_string().contains("TASK_AUTO_PROOF_UNAVAILABLE"),
        "{error:#}"
    );
    assert_eq!(store.inspect(&harness.id).unwrap().stage, task.stage);
}

#[test]
fn store_reopen_preserves_durable_facts_and_output_policy_invalidation() {
    let command = "printf 'Running tests/runner_fixture.rs\\ntest seam_test ... ok\\ntest result: ok. 1 passed; 0 failed\\n'";
    let harness = CommandGateHarness::with_proof(command, None, profile::ProofKind::SeamTest);
    let store = harness.store();
    harness.execute_check().unwrap();
    drop(store);

    let reopened = harness.store();
    let (task, mut evidence) = unobserved_task_evidence(&reopened, &harness.id);
    evidence.proof = serde_json::json!({
        "seam_test_proof": {
            "exit_code": 0,
            "seam_test_refs": ["tests/runner_fixture.rs::seam_test"],
            "tests_passed": 1,
            "tests_failed": 0
        }
    });

    let mut profile_mut = (*reopened.profile_for_task(&task).unwrap()).clone();
    if let CommandDef::Detailed { output, .. } = profile_mut.commands.get_mut("check").unwrap() {
        if let Some(output) = output.as_mut() {
            if let Some(tests) = output.tests.as_mut() {
                tests.passed = r"^nonexistent\s+([A-Za-z0-9_:]+)".into();
            }
        }
    }
    harness.root.write(
        ".forge/acdd/profile.yaml",
        &serde_yaml::to_string(&profile_mut).unwrap(),
    );
    drop(reopened);

    let mut invalidated_store = harness.store();
    let err = submit_unobserved_pass(&mut invalidated_store, &evidence).unwrap_err();
    assert!(
        err.to_string().contains("TASK_AUTO_PROOF_INVALID")
            || err.to_string().contains("TASK_AUTO_PROOF_UNAVAILABLE")
            || err.to_string().contains("TASK_EVIDENCE_INVALID"),
        "{err:#}"
    );
}
