use super::*;

#[test]
fn cargo_seam_observation_rejects_mixed_executable_test_targets() {
    let harness = CommandGateHarness::seam_s0();
    write_member_seam_project(&harness.root);
    // Replace only the selected target source and reuse Cargo's build cache.
    for (selection, source) in [
        ("--lib", "member/src/lib.rs"),
        ("--bins", "member/src/main.rs"),
        ("--examples", "member/examples/demo.rs"),
        ("--all-targets", "member/src/lib.rs"),
    ] {
        let command =
            format!("cargo test --test observed -p proof-member {selection} --message-format=json");
        harness.configure_check(&command, None, profile::ProofKind::SeamTest);
        for target in [
            "member/src/lib.rs",
            "member/src/main.rs",
            "member/examples/demo.rs",
        ] {
            let target = harness.root.root().join(target);
            if target.exists() {
                fs::remove_file(target).unwrap();
            }
        }
        harness.root.write(
            source,
            &format!(
                "#[test] fn other_target_only() {{}}\n{}",
                if source.ends_with("lib.rs") {
                    ""
                } else {
                    "fn main() {}\n"
                }
            ),
        );
        harness.execute_check().unwrap();
        let mut store = harness.store();
        let (task, mut evidence) = unobserved_task_evidence(&store, &harness.id);
        evidence.proof = serde_json::json!({"seam_test_proof":{"exit_code":0,"seam_test_refs":["member/tests/runner_fixture.rs::other_target_only"],"tests_passed":2,"tests_failed":0}});
        let error = submit_unobserved_pass(&mut store, &evidence).unwrap_err();
        assert!(
            error.to_string().contains("TASK_AUTO_PROOF_UNAVAILABLE")
                || error.to_string().contains("TASK_EVIDENCE_INVALID"),
            "{error:#}"
        );
        assert_eq!(store.inspect(&harness.id).unwrap().stage, task.stage);
    }
}

#[test]
fn seam_observations_are_bound_to_claim_candidate_and_command_plan() {
    let mut harness = CommandGateHarness::seam_s0();
    harness.execute_check().unwrap();
    harness.recapture();
    for mutation in ["claim", "candidate", "plan"] {
        harness.restore();
        let mut store = harness.store();
        let mut configured = (*store
            .profile_for_task(&store.inspect(&harness.id).unwrap())
            .unwrap())
        .clone();
        match mutation {
            "claim" => {
                store
                    .release_failed_claim(
                        &harness.id,
                        store.inspect(&harness.id).unwrap().claim_revision,
                    )
                    .unwrap();
                store
                    .claim(
                        &harness.id,
                        "contract",
                        "new-author",
                        harness.root.root().to_str().unwrap(),
                        Some("test-session-new-author"),
                    )
                    .unwrap();
            }
            "candidate" => harness
                .root
                .write("src/lib.rs", "pub fn changed_candidate() {}\n"),
            _ => {
                let CommandDef::Detailed { command, .. } =
                    configured.commands.get_mut("check").unwrap()
                else {
                    unreachable!()
                };
                command.push_str(" && true");
                harness.root.write(
                    ".forge/acdd/profile.yaml",
                    &serde_yaml::to_string(&configured).unwrap(),
                );
            }
        }
        drop(store);
        let mut store = harness.store();
        let (_task, mut evidence) = unobserved_task_evidence(&store, &harness.id);
        evidence.proof = serde_json::json!({"seam_test_proof":{"exit_code":0,"seam_test_refs":["tests/runner_fixture.rs::seam_test"],"tests_passed":1,"tests_failed":0}});
        let error = submit_unobserved_pass(&mut store, &evidence).unwrap_err();
        assert!(
            error.to_string().contains("TASK_AUTO_PROOF_UNAVAILABLE"),
            "{mutation}: {error:#}"
        );
    }
}

#[test]
fn typed_contract_submission_requires_claim_bound_observation() {
    let root = crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
    let milestone = super::support::Milestone::parse(super::support::SPEC, "forge-mcp").unwrap();
    let mut store = TasksStore::open_in_memory("forge-mcp", "forge-mcp").unwrap();
    store.profile = std::sync::Arc::new(super::support::fixture_profile());
    store
        .sync(&milestone, "docs/010-test.md", root.root())
        .unwrap();
    let id = milestone.task_id(&milestone.tasks[0]);
    let task = store
        .claim(
            &id,
            "contract",
            "author",
            root.root().to_str().unwrap(),
            Some("test-session-author"),
        )
        .unwrap();
    let evidence = super::support::Evidence {
        task_id: task.task_id.clone(),
        stage: task.stage.clone(),
        claim_revision: task.claim_revision,
        contract_revision: task.contract_revision,
        worker_id: task.worker_id.clone().unwrap(),
        worktree: task.worktree.clone().unwrap(),
        commit: None,
        candidate_baseline_head: None,
        proof: serde_json::json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":101}}),
    };
    let error = submit_unobserved_pass(&mut store, &evidence).unwrap_err();
    assert!(
        error.to_string().contains("TASK_AUTO_PROOF_UNAVAILABLE"),
        "{error:#}"
    );
    assert_eq!(store.inspect(&id).unwrap().stage, task.stage);
    assert!(!root.root().join(".forge/tasks.sqlite").exists());
}

#[test]
fn command_verification_reuses_durable_facts_and_invalidates_changed_candidates() {
    let harness = CommandGateHarness::with_command(
        "printf run >> .forge/counter; printf 'PASS=3 FAIL=0\\nPASS=4 FAIL=0\\n'",
        Some(output_spec(true)),
    );
    let store = harness.store();
    let first = harness.verify().unwrap();
    assert_eq!(
        first["evidence"]["proof"]["command_proof"]["tests_passed"], 7,
        "{first}"
    );
    assert_eq!(first["verification"]["diagnostic_mode"], "pass");
    assert_eq!(store.inspect(&harness.id).unwrap().stage, "contract");
    drop(store);
    let cached = harness.verify().unwrap();
    assert_eq!(cached["evidence"], first["evidence"]);
    assert_eq!(
        fs::read_to_string(harness.root.root().join(".forge/counter")).unwrap(),
        "run"
    );
    harness
        .root
        .write("src/lib.rs", "pub fn changed_candidate() {}\n");
    harness.verify().unwrap();
    assert_eq!(
        fs::read_to_string(harness.root.root().join(".forge/counter")).unwrap(),
        "runrun"
    );
    assert!(!harness.root.root().join(".forge/task-traces").exists());

    for mutation in ["add", "remove", "change"] {
        harness.restore();
        harness.configure_check(
            "printf checked",
            Some(output_spec(false)),
            profile::ProofKind::Command,
        );
        let root = &harness.root;
        let id = harness.id.clone();
        root.write("src/readme.md", "scoped documentation\\n");
        let mut configured = (*profile::load_for_workspace(root.root()).unwrap()).clone();
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
            forge_tasks::engine::verify_task(root.root(), &id, None).unwrap()["evidence"]
                .is_object()
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
            compile_evidence_envelope(root.root(), &id, "contract").is_err(),
            "{mutation} must invalidate cached plan"
        );
        let result = forge_tasks::engine::verify_task(root.root(), &id, None).unwrap();
        if mutation == "remove" {
            assert!(result["evidence"].is_object(), "{result}");
            assert_eq!(
                fs::read_to_string(root.root().join(".forge/runs")).unwrap(),
                "rustdocsrust"
            );
        } else {
            assert!(result["evidence"].is_null(), "{result}");
            assert!(root
                .root()
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
fn command_diagnostics_filter_noise_and_confine_large_failures() {
    let mut spec = output_spec(false);
    spec.noise = vec!["^progress".into()];
    spec.errors = vec!["^ERROR".into()];
    let harness = CommandGateHarness::with_command(
        "i=0; while [ $i -lt 500 ]; do printf 'progress compiling\\n'; i=$((i+1)); done; printf 'ERROR useful failure\\n' >&2; exit 2",
        Some(spec),
    );
    let result = harness.verify().unwrap();
    assert_eq!(
        result["verification"]["diagnostic_mode"], "inline",
        "{result}"
    );
    let output = result["verification"]["output"].as_str().unwrap();
    assert!(output.contains("ERROR useful failure"));
    assert!(!output.contains("progress compiling"));
    assert!(result["verification"]["log_path"].is_null());
    assert!(!harness.root.root().join(".forge/task-traces").exists());

    harness.restore();
    harness.configure_check(
        "printf 'HEAD\\n'; head -c 10000 /dev/zero | tr '\\0' x; printf '\\nTAIL\\n'; exit 2",
        Some(output_spec(false)),
        profile::ProofKind::Command,
    );
    let result = harness.verify().unwrap();
    assert_eq!(
        result["verification"]["diagnostic_mode"], "truncated",
        "{result}"
    );
    let output = result["verification"]["output"].as_str().unwrap();
    assert!(output.contains("HEAD") && output.contains("TAIL") && output.contains("file:///"));
    let log = result["verification"]["log_path"].as_str().unwrap();
    assert!(log.starts_with(".forge/task-traces/"));
    assert!(fs::read(harness.root.root().join(log)).unwrap().len() > 10000);
}

#[test]
fn command_scope_guard_observes_paths_before_and_after_execution() {
    let harness = CommandGateHarness::with_command(
        "printf after > after.txt; printf checked",
        Some(output_spec(false)),
    );
    harness
        .root
        .write("before.txt", "existing out-of-scope change\n");
    let result = harness.verify().unwrap();
    let output = result["verification"]["output"].as_str().unwrap();
    assert!(output.starts_with("SCOPE_VIOLATION_WARNING"), "{result}");
    let paths = result["verification"]["scope_violations"]
        .as_array()
        .unwrap();
    assert!(paths.iter().any(|path| path["path"] == "before.txt"));
    assert!(paths.iter().any(|path| path["path"] == "after.txt"));
    assert_eq!(result["verification"]["exit_code"], 0);

    for kind in ["script", "detailed", "pattern"] {
        for placeholder in [false, true] {
            harness.restore();
            let command = format!(
                "printf 'test result: ok. 1 passed; 0 failed\\n'; printf '%s\\n'{}",
                if placeholder { " {scope_target}" } else { "" }
            );
            harness.configure_check(&command, None, profile::ProofKind::Command);
            let mut configured =
                (*profile::load_for_workspace(harness.root.root()).unwrap()).clone();
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
            harness.root.write("src/other.rs", "pub fn other() {}\\n");
            let store = harness.store();
            let id = harness.id.clone();
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
                harness.root.write(
                    ".forge/acdd/profile.yaml",
                    &serde_yaml::to_string(&configured).unwrap(),
                );
                let result =
                    forge_tasks::engine::verify_task(harness.root.root(), &id, None).unwrap();
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
                let cached =
                    forge_tasks::engine::verify_task(harness.root.root(), &id, None).unwrap();
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
fn command_timeout_overrides_are_bounded_and_profile_changes_invalidate_proofs() {
    let harness = CommandGateHarness::with_command("sleep 30", Some(output_spec(false)));
    for timeout in [0, 1801] {
        assert!(
            forge_tasks::engine::verify_task(harness.root.root(), &harness.id, Some(timeout))
                .is_err()
        );
    }
    let started = Instant::now();
    let result =
        forge_tasks::engine::verify_task(harness.root.root(), &harness.id, Some(1)).unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        result["verification"]["diagnostic_mode"], "timeout",
        "{result}"
    );
    assert!(result["verification"]["output"]
        .as_str()
        .unwrap()
        .contains("timeout_seconds: 2"));

    harness.restore();
    harness.configure_check(
        "printf run >> .forge/counter; printf checked",
        Some(output_spec(false)),
        profile::ProofKind::Command,
    );
    harness.verify().unwrap();
    let mut configured = (*profile::load_for_workspace(harness.root.root()).unwrap()).clone();
    configured.limits.max_command_timeout_seconds = 2;
    if let CommandDef::Detailed { output, .. } = configured.commands.get_mut("check").unwrap() {
        output.as_mut().unwrap().noise.push("^new-noise".into());
    }
    harness.root.write(
        ".forge/acdd/profile.yaml",
        &serde_yaml::to_string(&configured).unwrap(),
    );
    assert!(forge_tasks::engine::verify_task(harness.root.root(), &harness.id, Some(3)).is_err());
    forge_tasks::engine::verify_task(harness.root.root(), &harness.id, Some(2)).unwrap();
    assert_eq!(
        fs::read_to_string(harness.root.root().join(".forge/counter")).unwrap(),
        "runrun"
    );
}
