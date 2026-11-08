use super::*;

#[test]
fn command_profiles_preserve_output_rules_and_legacy_defaults() {
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

    let legacy = parse_profile(
        "commands:\n  script: echo ok\n  legacy:\n    command: echo ok\n  configured:\n    command: echo ok\n    output: {}\n  pattern_legacy:\n    by_pattern: {'*.rs': 'cargo test'}\n  pattern_configured:\n    by_pattern: {'*.py': pytest}\n    output: {require_counts: false}\n",
    )
    .unwrap();
    assert!(legacy.commands["script"].output().is_none());
    assert!(legacy.commands["legacy"].output().is_none());
    assert!(legacy.commands["pattern_legacy"].output().is_none());
    assert!(
        !legacy.commands["pattern_configured"]
            .output()
            .unwrap()
            .require_counts
    );
    let defaults = legacy.commands["configured"].output().unwrap();
    assert!(defaults.require_counts);
    assert!(defaults.counts.is_none());
    assert!(defaults.noise.is_empty());
    assert!(defaults.errors.is_empty());
    assert!(defaults.seam_ref_pattern.is_none());
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
fn configured_output_observations_do_not_determine_process_success() {
    let harness = CommandGateHarness::new();
    for (command, success, expected_passed, expected_failed) in [
        ("printf 'PASS=2 FAIL=3'", true, Some(2), Some(3)),
        ("printf 'PASS=2 FAIL=0'; exit 7", false, Some(2), Some(0)),
        ("printf 'PASS=invalid FAIL=0'", true, None, Some(0)),
    ] {
        harness.restore();
        let mut spec = output_spec(false);
        spec.counts.as_mut().unwrap().passed = r"PASS=(\d+)".into();
        harness.configure_check(command, Some(spec), profile::ProofKind::Command);
        let id = harness.id.clone();
        let root = &harness.root;
        let store = harness.store();
        let result = harness.execute_check();
        assert_eq!(result.is_ok(), success, "{command}: {result:?}");
        let run = latest_run(&store, root.root(), &id, "tasks.contract");
        let trace = store
            .claim_bound_execution_traces(run.run_id)
            .unwrap()
            .pop()
            .unwrap()
            .trace;
        let facts = store
            .execution_proof(run.run_id, &trace.command_id, trace.attempt_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            (facts.passed, facts.failed),
            (expected_passed, expected_failed)
        );
    }

    for (command, expected_error) in [
        ("printf 'PASS=2 FAIL=1\\n'", "TASK_AUTO_PROOF_INVALID"),
        (
            "printf 'successful process without a test summary\\n'",
            "TASK_AUTO_PROOF_UNAVAILABLE",
        ),
    ] {
        harness.restore();
        harness.configure_check(
            command,
            Some(output_spec(true)),
            profile::ProofKind::Command,
        );
        let result = harness.verify().unwrap();
        assert!(result["evidence"].is_null(), "{command}: {result}");
        assert!(
            result["error"]
                .as_str()
                .unwrap_or_default()
                .contains(expected_error),
            "{command}: {result}"
        );
    }

    for (command, output) in [
        ("printf checked", output_spec(false)),
        ("printf 'no summary'", output_spec(false)),
        ("printf 'PASS=invalid'", output_spec(false)),
    ] {
        harness.restore();
        harness.configure_check(command, Some(output), profile::ProofKind::Command);
        let result = submit_zero_arg(
            harness.root.root(),
            &harness.id,
            "contract",
            Action::Pass,
            None,
            Some("test-session-command-builder"),
        )
        .unwrap_or_else(|error| panic!("{command}: {error:#}"));
        assert_eq!(result["stage"], "build", "{command}: {result}");
        assert_eq!(
            harness.store().inspect(&harness.id).unwrap().stage,
            "build",
            "{command}: {result}"
        );
    }

    let embedded = profile::compiled();
    let registered = &embedded.commands["check_full_repo"];
    for (path, summary) in [
        ("src/lib.rs", "test result: ok. 3 passed; 0 failed"),
        ("tests/test_api.py", "=== 3 passed in 0.1s ==="),
        ("src/api.ts", "Tests: 3 passed, 3 total"),
    ] {
        assert!(registered.command_for_path(path).is_some());
        harness.restore();
        harness.configure_check(
            &format!("printf '%s\\n' '{summary}'"),
            registered.output().cloned(),
            profile::ProofKind::Command,
        );
        let result = harness.verify().unwrap();
        assert_eq!(
            result["evidence"]["proof"]["command_proof"]["tests_passed"], 3,
            "{path}: {result}"
        );
    }
}

#[test]
fn configured_test_names_are_reported_but_do_not_prove_a_seam() {
    let mut line_output = output_spec(false);
    line_output.tests = Some(profile::CommandTestsSpec {
        target: Some(r"^FILE (\S+)".into()),
        passed: r"^PASS (\S+)".into(),
        failed: Some(r"^FAIL (\S+)".into()),
        ignored: Some(r"^SKIP (\S+)".into()),
    });
    let pytest_output = profile::CommandOutputSpec {
        counts: Some(profile::CommandCountsSpec {
            passed: r"(?m)\b(\d+) passed\b".into(),
            failed: r"(?m)\b(\d+) failed\b".into(),
        }),
        require_counts: true,
        noise: vec![],
        errors: vec![],
        seam_ref_pattern: None,
        tests: Some(profile::CommandTestsSpec {
            target: Some(r"(?m)^(\S+?\.py)::".into()),
            passed: r"(?m)^(?:\S+?\.py::)?([A-Za-z0-9_]+)\s+PASSED\b".into(),
            failed: Some(r"(?m)^(?:\S+?\.py::)?([A-Za-z0-9_]+)\s+FAILED\b".into()),
            ignored: Some(r"(?m)^(?:\S+?\.py::)?([A-Za-z0-9_]+)\s+SKIPPED\b".into()),
        }),
    };
    let cases = [
        (
            line_output,
            "printf 'FILE tests/example.rs\\nPASS invented\\nFAIL other\\n'",
            "tests/example.rs::invented",
            None,
        ),
        (
            pytest_output,
            "printf 'tests/test_seam.py::test_custom_seam PASSED\\n1 passed in 0.05s\\n'",
            "tests/test_seam.py::test_custom_seam",
            Some(("tests/test_seam.py", "def test_custom_seam(): pass\n")),
        ),
    ];
    let harness = CommandGateHarness::new();
    for (output, command, test_ref, source_file) in cases {
        harness.restore();
        harness.configure_check(command, Some(output), profile::ProofKind::SeamTest);
        if let Some((path, source)) = source_file {
            harness.root.write(path, source);
        }

        let mut store = harness.store();
        let (task, mut evidence) = unobserved_task_evidence(&store, &harness.id);
        harness.execute_check().unwrap();
        let run = latest_run(&store, harness.root.root(), &harness.id, "tasks.contract");
        let trace = store
            .claim_bound_execution_traces(run.run_id)
            .unwrap()
            .pop()
            .unwrap()
            .trace;
        let facts = store
            .execution_proof(run.run_id, &trace.command_id, trace.attempt_id)
            .unwrap()
            .unwrap();
        assert_eq!(facts.passed_tests, BTreeSet::from([test_ref.to_owned()]));

        evidence.proof = serde_json::json!({
            "seam_test_proof": {
                "exit_code": 0,
                "seam_test_refs": [test_ref],
                "tests_passed": 1,
                "tests_failed": 0
            }
        });
        let error = submit_unobserved_pass(&mut store, &evidence).unwrap_err();
        assert!(
            error.to_string().contains("TASK_AUTO_PROOF_UNAVAILABLE"),
            "{test_ref}: {error:#}"
        );
        assert_eq!(store.inspect(&harness.id).unwrap().stage, task.stage);
    }
}

#[test]
fn typed_command_and_seam_submissions_require_claim_bound_observations() {
    let cases = [
        (
            profile::ProofKind::SeamTest,
            serde_json::json!({"seam_test_proof":{"exit_code":0,"seam_test_refs":["tests/runner_fixture.rs::seam_test"],"tests_passed":1,"tests_failed":0}}),
        ),
        (
            profile::ProofKind::Command,
            serde_json::json!({"command_proof":{"command":"check","exit_code":0,"tests_passed":1,"tests_failed":0}}),
        ),
    ];
    let harness = CommandGateHarness::new();
    for (proof_kind, proof) in cases {
        harness.restore();
        harness.configure_check("exit 9", None, proof_kind);
        let mut store = harness.store();
        let (task, mut evidence) = unobserved_task_evidence(&store, &harness.id);
        evidence.proof = proof;
        let error = submit_unobserved_pass(&mut store, &evidence).unwrap_err();
        assert!(
            error.to_string().contains("TASK_AUTO_PROOF_UNAVAILABLE"),
            "{proof_kind:?}: {error:#}"
        );
        assert_eq!(store.inspect(&harness.id).unwrap().stage, task.stage);
    }
}

#[test]
fn observed_seam_submission_rejects_unexecuted_references() {
    let harness = CommandGateHarness::seam_s0();
    harness.root.write(
        "tests/runner_fixture.rs",
        "#[test] fn seam_test() {}\n#[test] #[ignore] fn ignored() {}\n",
    );
    let compiled = std::process::Command::new("rustc")
        .current_dir(harness.root.root())
        .args([
            "--test",
            "tests/runner_fixture.rs",
            "-o",
            ".forge/runner_fixture",
        ])
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "compile ignored runner: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    harness.execute_check().unwrap();
    for references in [
        vec!["tests/runner_fixture.rs::unrelated"],
        vec!["tests/other.rs::seam_test"],
        vec!["tests/runner_fixture.rs::ignored"],
        vec![
            "tests/runner_fixture.rs::seam_test",
            "tests/runner_fixture.rs::ignored",
        ],
    ] {
        let mut store = harness.store();
        let (task, mut evidence) = unobserved_task_evidence(&store, &harness.id);
        evidence.proof = serde_json::json!({"seam_test_proof":{"exit_code":0,"seam_test_refs":references,"tests_passed":1,"tests_failed":0}});
        let error = submit_unobserved_pass(&mut store, &evidence).unwrap_err();
        assert!(
            error.to_string().contains("TASK_AUTO_PROOF_UNAVAILABLE"),
            "{error:#}"
        );
        assert_eq!(store.inspect(&harness.id).unwrap().stage, task.stage);
    }
}

#[test]
fn shipped_seam_command_reports_identities_without_authorizing_submission() {
    let root = crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
    let mut fixture_profile =
        profile::validate_profile_document(include_str!("../../../.forge/acdd/profile.yaml"))
            .unwrap();
    let integrate = fixture_profile.find_gate("seam.integrate").unwrap().clone();
    fixture_profile.gates = vec![profile::GateDef {
        proof: profile::ProofDef::Kind(profile::ProofKind::None),
        tasks: Some(profile::TaskGateBlock {
            gates: vec![
                profile::GateDef {
                    id: "seam".into(),
                    proof: profile::ProofDef::Kind(profile::ProofKind::None),
                    gates: vec![integrate],
                    ..Default::default()
                },
                profile::GateDef {
                    id: "deliver".into(),
                    proof: profile::ProofDef::Kind(profile::ProofKind::Delivery),
                    role: Some("builder".into()),
                    receipt: true,
                    auto_commit: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        ..Default::default()
    }];
    root.write("src/lib.rs", "pub fn example() {}\n");
    root.write("tests/runner_fixture.rs", "#[test] fn seam_test() {}\n");
    root.write(
        "Cargo.toml",
        "[package]\nname = \"shipped-seam-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    root.write(".gitignore", ".forge/\ntarget/\nCargo.lock\n");
    root.write("forge-mcp.yaml", "roots: [src, tests]\ndocs: [docs]\n");
    root.write(
        "docs/milestones/010-seam.md",
        "---\nid: m-shipped-seam\ntitle: Shipped seam\nstatus: active\n---\n```yaml\ntask_ref: first\ntarget: Execute the shipped seam command\nproof_policy: seam-test-first\nscope: [tests/runner_fixture.rs]\n```\n",
    );
    root.write(
        ".forge/acdd/profile.yaml",
        &serde_yaml::to_string(&fixture_profile).unwrap(),
    );
    super::support::commit_fixture_baseline(&root);
    super::support::tasks::manage(
        root.root(),
        super::support::tasks::Manage {
            action: super::support::tasks::ManageAction::Sync,
            milestone_ref: Some("docs/milestones/010-seam.md".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let id = "forge-mcp/m-shipped-seam:first".to_owned();
    let mut store = forge_tasks::engine::store_for_task(root.root(), &id).unwrap();
    store
        .claim(
            &id,
            "seam.integrate",
            "command-builder",
            root.root().to_str().unwrap(),
            Some("test-session-command-builder"),
        )
        .unwrap();
    let task = store.inspect(&id).unwrap();
    let configured = store.profile_for_task(&task).unwrap();
    let shipped =
        profile::validate_profile_document(include_str!("../../../.forge/acdd/profile.yaml"))
            .unwrap();
    let command_name = shipped
        .find_gate("seam.integrate")
        .unwrap()
        .command
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!(
        configured.commands[&command_name],
        shipped.commands[&command_name]
    );
    drop(store);
    let traces = execute_command_gate(
        root.root(),
        &id,
        "tasks.seam.integrate",
        &configured.commands[&command_name],
        &["tests/runner_fixture.rs".into()],
    )
    .unwrap();
    assert_eq!(traces.len(), 1);
    assert_eq!(traces[0].exit_code, Some(0));
    let mut store = forge_tasks::engine::store_for_task(root.root(), &id).unwrap();
    let run = latest_run(&store, root.root(), &id, "tasks.seam.integrate");
    let trace = store
        .claim_bound_execution_traces(run.run_id)
        .unwrap()
        .pop()
        .unwrap()
        .trace;
    let facts = store
        .execution_proof(run.run_id, &trace.command_id, trace.attempt_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        facts.passed_tests,
        BTreeSet::from(["tests/runner_fixture.rs::seam_test".to_owned()])
    );
    let (task, mut evidence) = unobserved_task_evidence(&store, &id);
    evidence.proof = serde_json::json!({"seam_test_proof":{"exit_code":0,"seam_test_refs":["tests/runner_fixture.rs::seam_test"],"tests_passed":1,"tests_failed":0}});
    let error = submit_unobserved_pass(&mut store, &evidence).unwrap_err();
    assert!(
        error.to_string().contains("TASK_AUTO_PROOF_UNAVAILABLE"),
        "{error:#}"
    );
    assert_eq!(store.inspect(&id).unwrap().stage, task.stage);
}

#[test]
fn seam_submission_rejects_test_output_forging_another_test_result() {
    let harness = CommandGateHarness::with_proof(
        "cargo test --test observed --message-format=json",
        None,
        profile::ProofKind::SeamTest,
    );
    write_cargo_seam_project(&harness.root);
    harness.root.write(
        "tests/runner_fixture.rs",
        "use std::io::Write;\n#[test] fn actual() { std::io::stdout().write_all(b\"\\ntest nonexistent ... ok\\n\").unwrap(); }\n",
    );
    let mut store = harness.store();
    harness.execute_check().unwrap();
    let (task, mut evidence) = unobserved_task_evidence(&store, &harness.id);
    evidence.proof = serde_json::json!({"seam_test_proof":{"exit_code":0,"seam_test_refs":["tests/runner_fixture.rs::nonexistent"],"tests_passed":1,"tests_failed":0}});
    let error = submit_unobserved_pass(&mut store, &evidence)
        .expect_err("a test's own output cannot establish another test's result");
    assert!(
        error.to_string().contains("TASK_AUTO_PROOF_UNAVAILABLE")
            || error.to_string().contains("TASK_EVIDENCE_INVALID"),
        "{error:#}"
    );
    assert_eq!(store.inspect(&harness.id).unwrap().stage, task.stage);
    drop(store);

    harness.restore();
    harness.configure_check(
        "cargo test --test observed --message-format=json",
        None,
        profile::ProofKind::SeamTest,
    );
    write_cargo_seam_project(&harness.root);
    harness.root.write(
        "tests/runner_fixture.rs",
        "#[path = \"actual.rs\"] mod shared;\n",
    );
    harness
        .root
        .write("tests/actual.rs", "#[test] fn seam_test() {}\n");
    let mut store = harness.store();
    harness.execute_check().unwrap();
    let (task, mut evidence) = unobserved_task_evidence(&store, &harness.id);
    evidence.proof = serde_json::json!({"seam_test_proof":{"exit_code":0,"seam_test_refs":["tests/actual.rs::seam_test"],"tests_passed":1,"tests_failed":0}});
    let error = submit_unobserved_pass(&mut store, &evidence)
        .expect_err("a target entrypoint does not identify the module defining a test");
    assert!(
        error.to_string().contains("TASK_AUTO_PROOF_UNAVAILABLE"),
        "{error:#}"
    );
    assert_eq!(store.inspect(&harness.id).unwrap().stage, task.stage);
}

#[test]
fn unstructured_output_cannot_prove_a_seam_test_identity() {
    let harness = CommandGateHarness::seam_s0();
    for command in [
        S0_RUNNER,
        "cargo test --test observed --message-format=json",
        S0_RUNNER_COUNTS_ONLY,
        "printf 'running 1 test\\ntest result: ok. 1 passed; 0 failed\\n'",
    ] {
        harness.restore();
        harness.configure_check(command, None, profile::ProofKind::SeamTest);
        if command.starts_with("cargo") {
            write_cargo_seam_project(&harness.root);
        }
        harness.execute_check().unwrap();
        let mut store = harness.store();
        let (task, mut evidence) = unobserved_task_evidence(&store, &harness.id);
        evidence.proof = serde_json::json!({"seam_test_proof":{"exit_code":0,"seam_test_refs":["tests/runner_fixture.rs::seam_test"],"tests_passed":1,"tests_failed":0}});
        let error = submit_unobserved_pass(&mut store, &evidence).unwrap_err();
        assert!(
            error.to_string().contains("TASK_AUTO_PROOF_UNAVAILABLE"),
            "{error:#}"
        );
        assert_eq!(store.inspect(&harness.id).unwrap().stage, task.stage);
    }

    harness.restore();
    write_member_seam_project(&harness.root);
    // Keep the same member fixture and Cargo target cache for both selectors.
    for command in [
        "cargo test --test observed --manifest-path member/Cargo.toml --message-format=json",
        "cargo test --test observed -p proof-member --message-format=json",
    ] {
        harness.configure_check(command, None, profile::ProofKind::SeamTest);
        harness.execute_check().unwrap();
        let mut store = harness.store();
        let (task, mut evidence) = unobserved_task_evidence(&store, &harness.id);
        evidence.proof = serde_json::json!({"seam_test_proof":{"exit_code":0,"seam_test_refs":["tests/runner_fixture.rs::seam_test"],"tests_passed":1,"tests_failed":0}});
        let error = submit_unobserved_pass(&mut store, &evidence).unwrap_err();
        assert!(
            error.to_string().contains("TASK_AUTO_PROOF_UNAVAILABLE"),
            "{error:#}"
        );
        evidence.proof["seam_test_proof"]["seam_test_refs"] =
            serde_json::json!(["member/tests/runner_fixture.rs::seam_test"]);
        let error = submit_unobserved_pass(&mut store, &evidence).unwrap_err();
        assert!(
            error.to_string().contains("TASK_AUTO_PROOF_UNAVAILABLE"),
            "{error:#}"
        );
        assert_eq!(store.inspect(&harness.id).unwrap().stage, task.stage);
    }
}
