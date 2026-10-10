use super::support::*;

const CANONICAL_FORGE_GITIGNORE: &str = concat!(
    "target/\n",
    "!/.forge/\n",
    "!/.forge/acdd/\n",
    "!/.forge/acdd/**\n",
    "!/.forge/frameworks/\n",
    "!/.forge/frameworks/**\n",
    ".forge/*.sqlite*\n",
    ".forge/*.lock\n",
    ".forge/*.log\n",
    ".forge/*.jsonl\n",
    ".forge/tasks/\n",
    ".forge/checkpoints.json\n",
);

#[test]
fn task_guidance_follows_agent_metadata_workspace_config_and_active_stage() {
    let specification = |agent_type: &str| {
        format!(
        "---\nid: m-guided\ntitle: Guided tasks\ndoc_type: contract\n---\n# Guided tasks\n```yaml\ntask_ref: guided\ntarget: Deliver guidance\nagent_type: {agent_type}\nproof_policy: seam-test-first\nscope: [src/]\n```\n"
    )
    };
    let root = ScopedWorkspace::new("forge_guidance_root");
    let linked = ScopedWorkspace::new("forge_guidance_linked");
    let missing = ScopedWorkspace::new("forge_guidance_missing");
    let missing_configured = ScopedWorkspace::new("forge_guidance_missing_configured");
    for (workspace, agent_type) in [
        (&root, "gpt-6-sol"),
        (&linked, "reviewer"),
        (&missing, "flash"),
        (&missing_configured, "worker"),
    ] {
        workspace.write("src/lib.rs", "pub fn guided() {}\n");
        workspace.write("docs/010-guided.md", &specification(agent_type));
    }
    root.write(
        "docs/team-guidance.md",
        "# Root task guidance\nLoad the contextunity-forge skill.\n",
    );
    linked.write(
        "docs/linked-guidance.md",
        "# Linked task guidance\nLoad the contextunity-forge skill.\n",
    );
    root.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    linked.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    root.write(
        "forge-mcp.yaml",
        &format!(
            "tasks_db: .forge/tasks.sqlite\nagents_guidance: docs/team-guidance.md\nlinked_workspaces:\n  - name: linked\n    path: {}\n    tasks:\n      enabled: true\n      agents_guidance: docs/linked-guidance.md\n",
            linked.0.display()
        ),
    );
    root.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    linked.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    linked.write(
        "forge-mcp.yaml",
        "task_repository: linked\ntask_project: linked\n",
    );
    missing.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n");
    missing_configured.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nagents_guidance: docs/unavailable.md\n",
    );

    for (server, workspace, selector, repository, agent_type, expected_path, absent) in [
        (
            &root.0,
            &root.0,
            None,
            "forge-mcp",
            "gpt-6-sol",
            root.0.join("docs/team-guidance.md"),
            false,
        ),
        (
            &root.0,
            &linked.0,
            Some("linked"),
            "linked",
            "reviewer",
            linked.0.join("docs/linked-guidance.md"),
            false,
        ),
        (
            &missing.0,
            &missing.0,
            None,
            "forge-mcp",
            "flash",
            missing.0.join("AGENTS.md"),
            true,
        ),
        (
            &missing_configured.0,
            &missing_configured.0,
            None,
            "forge-mcp",
            "worker",
            missing_configured.0.join("docs/unavailable.md"),
            true,
        ),
    ] {
        let id = format!("{repository}/{repository}/m-guided:guided");
        tasks::manage(
            server,
            serde_json::from_value(json!({
                "action": "sync",
                "workspace": selector,
                "milestone_ref": "docs/010-guided.md"
            }))
            .unwrap(),
        )
        .unwrap();
        let inspect = || {
            tasks::manage(
                server,
                serde_json::from_value(json!({"action": "inspect", "task_id": id})).unwrap(),
            )
            .unwrap()
        };
        let before = inspect();
        assert_eq!(before["spec"]["agent_type"], agent_type);
        assert_eq!(before["agents_guidance"], expected_path.to_str().unwrap());
        let guidance = &before["workflow_guidance"];
        assert_eq!(guidance["active_stage"], "contract");
        assert_eq!(guidance["agent_type"], agent_type);
        assert_eq!(guidance["subagent_role"], "contract_author");
        assert!(guidance["steps"]
            .as_array()
            .is_some_and(|steps| !steps.is_empty()));
        if absent {
            assert_eq!(guidance["warning"]["code"], "TASK_GUIDANCE_MISSING");
        } else {
            assert!(guidance["warning"].is_null());
        }
        let claimed = tasks::claim(
            server,
            tasks::Claim {
                task_id: id.clone(),
                stage: "contract".into(),
                worker_id: "guidance-contract-author".into(),
                worktree: workspace.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(claimed["workflow_guidance"], *guidance);
        if repository == "forge-mcp" && !absent {
            let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
            let task = store.inspect(&id).unwrap();
            store
                .submit(&id, "contract", &evidence(&task), "pass", None)
                .unwrap();
            let build = inspect();
            assert_eq!(build["workflow_guidance"]["active_stage"], "build");
            assert_eq!(build["workflow_guidance"]["subagent_role"], "builder");
            assert_ne!(build["workflow_guidance"]["steps"], guidance["steps"]);
            assert!(build["workflow_guidance"]["steps"]
                .as_array()
                .is_some_and(|steps| steps.len() >= 2));
            let claimed_build = tasks::claim(
                server,
                tasks::Claim {
                    task_id: id.clone(),
                    stage: "build".into(),
                    worker_id: "guidance-builder".into(),
                    worktree: workspace.to_string_lossy().into_owned(),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                claimed_build["workflow_guidance"]["subagent_role"],
                "builder"
            );
            let task = store.inspect(&id).unwrap();
            store
                .submit(&id, "build", &evidence(&task), "pass", None)
                .unwrap();
            let review = inspect();
            assert_eq!(review["workflow_guidance"]["active_stage"], "review");
            assert_eq!(
                review["workflow_guidance"]["subagent_role"],
                "independent_reviewer"
            );
            assert_eq!(
                review["workflow_guidance"]["independent_from_worker_id"],
                "guidance-builder"
            );
            assert!(review["workflow_guidance"]["independence_rule"]
                .as_str()
                .is_some_and(|rule| rule.contains("different")));
        }
    }
}

#[test]
fn task_guidance_requires_named_and_installed_forge_skill() {
    let specification = "---\nid: m-skill\ntitle: Skill guidance\ndoc_type: contract\n---\n# Skill guidance\n```yaml\ntask_ref: skill\ntarget: Deliver skill guidance\nagent_type: worker\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let named = ScopedWorkspace::new("forge_skill_named");
    let named_mcp_skill = ScopedWorkspace::new("forge_skill_named_mcp_skill");
    let unnamed = ScopedWorkspace::new("forge_skill_unnamed");
    let global_only = ScopedWorkspace::new("forge_skill_global_only");
    let ignored_profile = ScopedWorkspace::new("forge_skill_ignored_profile");
    let prefixed_name = ScopedWorkspace::new("forge_skill_prefixed_name");
    let extended_mcp_name = ScopedWorkspace::new("forge_skill_extended_mcp_name");
    let mcp_mention_without_skill_context =
        ScopedWorkspace::new("forge_mcp_mention_without_skill_context");
    let ignored_profile_yaml_exception =
        ScopedWorkspace::new("forge_skill_ignored_profile_yaml_exception");
    let wildcard_yaml_ignore = ScopedWorkspace::new("forge_skill_wildcard_yaml_ignore");
    let wildcard_dot_ignore = ScopedWorkspace::new("forge_skill_wildcard_dot_ignore");
    let runtime_path_negation = ScopedWorkspace::new("forge_skill_runtime_path_negation");
    let markdown_mcp_skill_link = ScopedWorkspace::new("forge_skill_markdown_mcp_link");
    let nested_framework_ignore = ScopedWorkspace::new("forge_skill_nested_framework_ignore");
    let json_configuration_ignore = ScopedWorkspace::new("forge_skill_json_configuration_ignore");
    let runtime_scan_negation = ScopedWorkspace::new("forge_skill_runtime_scan_negation");
    let effective_protection_suffix =
        ScopedWorkspace::new("forge_skill_effective_protection_suffix");
    let unmatched_wildcard_configuration_ignore =
        ScopedWorkspace::new("forge_skill_unmatched_wildcard_configuration_ignore");
    let markdown_heading_reference = ScopedWorkspace::new("forge_skill_markdown_heading_reference");
    let incomplete_gitignore = ScopedWorkspace::new("forge_skill_incomplete_gitignore");
    let missing_gitignore = ScopedWorkspace::new("forge_skill_missing_gitignore");
    for workspace in [
        &named,
        &named_mcp_skill,
        &unnamed,
        &global_only,
        &ignored_profile,
        &prefixed_name,
        &extended_mcp_name,
        &mcp_mention_without_skill_context,
        &ignored_profile_yaml_exception,
        &wildcard_yaml_ignore,
        &wildcard_dot_ignore,
        &runtime_path_negation,
        &markdown_mcp_skill_link,
        &nested_framework_ignore,
        &json_configuration_ignore,
        &runtime_scan_negation,
        &effective_protection_suffix,
        &unmatched_wildcard_configuration_ignore,
        &markdown_heading_reference,
        &incomplete_gitignore,
        &missing_gitignore,
    ] {
        workspace.write("src/lib.rs", "pub fn skill() {}\n");
        workspace.write("docs/010-skill.md", specification);
        workspace.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n");
    }
    for workspace in [
        &named,
        &named_mcp_skill,
        &unnamed,
        &prefixed_name,
        &extended_mcp_name,
        &mcp_mention_without_skill_context,
        &incomplete_gitignore,
        &missing_gitignore,
        &ignored_profile_yaml_exception,
        &wildcard_yaml_ignore,
        &wildcard_dot_ignore,
        &runtime_path_negation,
        &markdown_mcp_skill_link,
        &nested_framework_ignore,
        &json_configuration_ignore,
        &runtime_scan_negation,
        &effective_protection_suffix,
        &unmatched_wildcard_configuration_ignore,
        &markdown_heading_reference,
    ] {
        workspace.write(
            ".agents/skills/contextunity-forge/SKILL.md",
            "# ContextUnity Forge\n",
        );
    }
    for workspace in [
        &named,
        &named_mcp_skill,
        &unnamed,
        &global_only,
        &prefixed_name,
        &extended_mcp_name,
        &mcp_mention_without_skill_context,
        &markdown_mcp_skill_link,
        &markdown_heading_reference,
        &effective_protection_suffix,
        &unmatched_wildcard_configuration_ignore,
    ] {
        workspace.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    }
    named.write("AGENTS.md", "# Rules\nLoad the contextunity-forge skill.\n");
    named_mcp_skill.write(
        "AGENTS.md",
        "# Rules\nLoad contextunity-forge-mcp skill for ACDD.\n",
    );
    unnamed.write(
        "AGENTS.md",
        "# Rules\nUse contextunity-forge-mcp for queries.\n",
    );
    prefixed_name.write("AGENTS.md", "# Rules\nLoad xcontextunity-forge skill.\n");
    extended_mcp_name.write(
        "AGENTS.md",
        "# Rules\nLoad contextunity-forge-mcp-tools skill for ACDD.\n",
    );
    mcp_mention_without_skill_context.write(
        "AGENTS.md",
        "# Rules\nUse contextunity-forge-mcp for queries.\nThe worker skill is installed.\n",
    );
    markdown_mcp_skill_link.write(
        "AGENTS.md",
        "# Rules\nLoad [contextunity-forge-mcp](https://example.com/SKILL.md) skill for ACDD.\n",
    );
    markdown_heading_reference.write("AGENTS.md", "# Intro\n\ncontextunity-forge skill.\n");
    for workspace in [
        &wildcard_yaml_ignore,
        &wildcard_dot_ignore,
        &runtime_path_negation,
        &nested_framework_ignore,
        &json_configuration_ignore,
        &runtime_scan_negation,
        &effective_protection_suffix,
        &unmatched_wildcard_configuration_ignore,
    ] {
        workspace.write("AGENTS.md", "# Rules\nLoad the contextunity-forge skill.\n");
    }
    global_only.write("AGENTS.md", "# Rules\nLoad the contextunity-forge skill.\n");
    ignored_profile.write("AGENTS.md", "# Rules\nLoad the contextunity-forge skill.\n");
    ignored_profile.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    ignored_profile.write(".gitignore", "target/\n.forge/\n");
    ignored_profile_yaml_exception
        .write("AGENTS.md", "# Rules\nLoad the contextunity-forge skill.\n");
    wildcard_yaml_ignore.write(
        ".gitignore",
        &format!("{CANONICAL_FORGE_GITIGNORE}*.yaml\n"),
    );
    wildcard_dot_ignore.write(".gitignore", &format!("{CANONICAL_FORGE_GITIGNORE}.*\n"));
    runtime_path_negation.write(
        ".gitignore",
        &format!("{CANONICAL_FORGE_GITIGNORE}!.forge/tasks.sqlite\n"),
    );
    nested_framework_ignore.write(
        ".gitignore",
        &format!("{CANONICAL_FORGE_GITIGNORE}.forge/frameworks/nested/\n"),
    );
    json_configuration_ignore.write(
        ".gitignore",
        &format!("{CANONICAL_FORGE_GITIGNORE}*.json\n"),
    );
    runtime_scan_negation.write(
        ".gitignore",
        &format!("{CANONICAL_FORGE_GITIGNORE}!.forge/scan.sqlite\n"),
    );
    effective_protection_suffix.write(
        ".gitignore",
        &format!(
            ".*\n*.json\n.forge/frameworks/nested/\n!.forge/scan.sqlite\n{CANONICAL_FORGE_GITIGNORE}"
        ),
    );
    // This active wildcard misses the validator's representative paths; the
    // final-suffix invariant must reject it without enumerating file types.
    unmatched_wildcard_configuration_ignore
        .write(".gitignore", &format!("{CANONICAL_FORGE_GITIGNORE}*.md\n"));
    ignored_profile_yaml_exception.write(".gitignore", "target/\n.forge/\n!.forge/*.yaml\n");
    markdown_heading_reference.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    incomplete_gitignore.write("AGENTS.md", "# Rules\nLoad the contextunity-forge skill.\n");
    incomplete_gitignore.write(".gitignore", "target/\n");
    missing_gitignore.write("AGENTS.md", "# Rules\nLoad the contextunity-forge skill.\n");
    let global_installed = std::env::var_os("HOME").is_some_and(|home| {
        std::path::Path::new(&home)
            .join(".agents/skills/contextunity-forge/SKILL.md")
            .is_file()
    });
    for (workspace, warning_fragment) in [
        (&named, None),
        (&named_mcp_skill, None),
        (&markdown_mcp_skill_link, None),
        (&markdown_heading_reference, None),
        (&effective_protection_suffix, None),
        (
            &unmatched_wildcard_configuration_ignore,
            Some("Edit `.gitignore`"),
        ),
        (&unnamed, Some("do not name the contextunity-forge skill")),
        (
            &prefixed_name,
            Some("do not name the contextunity-forge skill"),
        ),
        (
            &extended_mcp_name,
            Some("do not name the contextunity-forge skill"),
        ),
        (
            &mcp_mention_without_skill_context,
            Some("do not name the contextunity-forge skill"),
        ),
        (&global_only, (!global_installed).then_some("not installed")),
        (&ignored_profile, Some("Edit `.gitignore`")),
        (&ignored_profile_yaml_exception, Some("Edit `.gitignore`")),
        (&wildcard_yaml_ignore, Some("Edit `.gitignore`")),
        (&wildcard_dot_ignore, Some("Edit `.gitignore`")),
        (&runtime_path_negation, Some("Edit `.gitignore`")),
        (&nested_framework_ignore, Some("Edit `.gitignore`")),
        (&json_configuration_ignore, Some("Edit `.gitignore`")),
        (&runtime_scan_negation, Some("Edit `.gitignore`")),
        (&incomplete_gitignore, Some("Edit `.gitignore`")),
        (&missing_gitignore, Some("Edit `.gitignore`")),
    ] {
        tasks::manage(
            &workspace.0,
            serde_json::from_value(json!({
                "action": "sync",
                "milestone_ref": "docs/010-skill.md"
            }))
            .unwrap(),
        )
        .unwrap();
        let inspected = tasks::manage(
            &workspace.0,
            serde_json::from_value(json!({
                "action": "inspect",
                "task_id": "forge-mcp/forge-mcp/m-skill:skill"
            }))
            .unwrap(),
        )
        .unwrap();
        let warning = &inspected["workflow_guidance"]["warning"];
        match warning_fragment {
            None => assert!(warning.is_null()),
            Some(fragment) => {
                assert_eq!(warning["code"], "TASK_GUIDANCE_MISSING");
                let message = warning["message"].as_str().unwrap();
                assert!(message.contains(fragment), "{message}");
                if fragment == "Edit `.gitignore`" {
                    assert!(warning["path"]
                        .as_str()
                        .is_some_and(|path| path.ends_with(".gitignore")));
                    for rule in [
                        "!/.forge/",
                        "!/.forge/acdd/",
                        "!/.forge/acdd/**",
                        "!/.forge/frameworks/",
                        "!/.forge/frameworks/**",
                        ".forge/*.sqlite*",
                        ".forge/*.lock",
                        ".forge/*.log",
                        ".forge/*.jsonl",
                        ".forge/tasks/",
                        ".forge/checkpoints.json",
                    ] {
                        assert!(message.contains(rule), "missing `{rule}` in: {message}");
                    }
                }
            }
        }
    }
}

#[test]
fn task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite() {
    let schema = serde_json::to_value(schemars::schema_for!(tasks::Submit)).unwrap();
    assert_eq!(schema["properties"]["evidence"]["type"], "object");
    let (root, mut store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let worktree = root.0.to_str().unwrap();
    let contract = store
        .claim(&task_id, "contract", "contract-author", worktree)
        .unwrap();
    let mut proof = evidence(&contract);
    proof.commit = None;
    let raw_evidence = serde_json::to_value(&proof).unwrap();
    for invalid in [
        json!({"task_id":task_id,"stage":"contract","action":"pass"}),
        json!({"task_id":task_id,"stage":"contract","action":"pass","evidence_ref":"proof.yaml"}),
        json!({"task_id":task_id,"stage":"contract","action":"pass","evidence":raw_evidence,"evidence_ref":"proof.yaml"}),
    ] {
        assert!(serde_json::from_value::<tasks::Submit>(invalid).is_err());
        assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
    }
    let nonobject: tasks::Submit = serde_json::from_value(json!({
        "task_id": task_id,
        "stage": "contract",
        "action": "pass",
        "evidence": 42
    }))
    .unwrap();
    assert!(tasks::submit(&root.0, nonobject).is_err());
    assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
    let request: tasks::Submit = serde_json::from_value(json!({
        "task_id": task_id,
        "stage": "contract",
        "action": "pass",
        "evidence": raw_evidence
    }))
    .unwrap();

    let submitted = tasks::submit(&root.0, request).unwrap();
    assert_eq!(submitted["status"], "ready");
    let stored: String = store
        .connection
        .query_row(
            "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate='contract' AND state='passed'",
            [&task_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stored).unwrap(),
        raw_evidence
    );
    let submission: String = store
        .connection
        .query_row(
            "SELECT result FROM task_submissions WHERE task_id=?1 AND revision=?2",
            rusqlite::params![task_id, proof.claim_revision],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&submission).unwrap()["evidence"],
        raw_evidence
    );
}

#[test]
fn task_cli_submit_accepts_json_object_and_persists_gate_evidence() {
    let (root, mut store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let claimed = store
        .claim(&task_id, "contract", "cli-worker", root.0.to_str().unwrap())
        .unwrap();
    let proof = evidence(&claimed);
    let raw = serde_json::to_string(&proof).unwrap();
    for invalid in ["proof.yaml", "42"] {
        let rejected = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--root",
                root.0.to_str().unwrap(),
                "task",
                "submit",
                &task_id,
                "--stage",
                "contract",
                "--action",
                "pass",
                "--evidence",
                invalid,
            ])
            .output()
            .unwrap();
        assert!(!rejected.status.success());
        assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
    }
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            root.0.to_str().unwrap(),
            "task",
            "submit",
            &task_id,
            "--stage",
            "contract",
            "--action",
            "pass",
            "--evidence",
            &raw,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "ready");
    let stored: String = store
        .connection
        .query_row(
            "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate='contract' AND state='passed'",
            [&task_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stored).unwrap(),
        serde_json::to_value(proof).unwrap()
    );
}

#[test]
fn typed_task_proofs_pass_through_engine_and_real_store() {
    let (root, mut store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let worktree = root.0.to_str().unwrap();
    let contours: serde_json::Map<String, serde_json::Value> =
        contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS
            .iter()
            .map(|name| {
                (
                    (*name).into(),
                    json!({"applicable": true, "evidence": "verified at the public seam"}),
                )
            })
            .collect();
    let cases = [
        (
            "contract",
            "contract-reviewer",
            json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs::task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite","red_exit_code":101}}),
            vec![
                json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs::task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite","red_exit_code":0}}),
                json!({"seam_test_ref":"tests/acdd/tasks.rs::task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite","red_exit_code":101}),
                json!({"contract_proof":{"seam_test":"tests/acdd/tasks.rs","red_exit_code":101}}),
            ],
        ),
        (
            "build",
            "builder",
            json!({"test_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":2,"tests_failed":0,"log":"running 2 tests\ntest result: ok. 2 passed ✓"}}),
            vec![
                json!({"test_proof":{"command":"cargo test --test acdd","exit_code":1,"tests_passed":1,"tests_failed":1}}),
                json!({"test_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":2,"tests_failed":0,"log":"x".repeat(65_537)}}),
                json!({"command":"cargo test --test acdd","result":"passed","artifacts":[]}),
                json!({"test_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":2,"tests_failed":0,"output":"passed"}}),
            ],
        ),
        (
            "review",
            "independent-reviewer",
            json!({"review_proof":{"decision":"pass","contours":contours.clone()}}),
            vec![
                json!({"review_proof":{"decision":"pass","contours":{}}}),
                json!({"decision":"pass","contours":contours}),
            ],
        ),
    ];
    for (stage, worker, proof, invalid_cases) in cases {
        let claimed = store.claim(&task_id, stage, worker, worktree).unwrap();
        let mut evidence = evidence(&claimed);
        for invalid in invalid_cases {
            evidence.proof = invalid;
            let request: tasks::Submit = serde_json::from_value(json!({
                "task_id": task_id,
                "stage": stage,
                "action": "pass",
                "evidence": evidence,
            }))
            .unwrap();
            assert!(tasks::submit(&root.0, request).is_err());
            assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
        }
        evidence.proof = proof.clone();
        let request: tasks::Submit = serde_json::from_value(json!({
            "task_id": task_id,
            "stage": stage,
            "action": "pass",
            "evidence": evidence,
        }))
        .unwrap();
        let result = tasks::submit(&root.0, request).unwrap();
        assert_eq!(result["status"], "ready");
        let stored: String = store
            .connection
            .query_row(
                "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='passed'",
                rusqlite::params![task_id, GATES[claimed.gate]],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&stored).unwrap()["proof"],
            proof
        );
        let submission: String = store
            .connection
            .query_row(
                "SELECT result FROM task_submissions WHERE task_id=?1 AND revision=?2",
                rusqlite::params![task_id, claimed.claim_revision],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&submission).unwrap()["evidence"]["proof"],
            proof
        );
    }
}

#[test]
fn rejected_task_gates_require_typed_proof_and_persist_failing_evidence() {
    for gate in [0_usize, 1, 2] {
        let (root, mut store, milestone) = fixture();
        let task_id = milestone.task_id(&milestone.tasks[0]);
        let worktree = root.0.to_str().unwrap();
        for stage in GATES.iter().take(gate) {
            let claimed = store.claim(&task_id, stage, "builder", worktree).unwrap();
            store
                .submit(&task_id, stage, &evidence(&claimed), "pass", None)
                .unwrap();
        }
        let worker = if gate == 2 { "reviewer" } else { "builder" };
        let claimed = store
            .claim(&task_id, GATES[gate], worker, worktree)
            .unwrap();
        let (bare, typed) = match gate {
            0 => (
                json!({"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":0}),
                json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":0}}),
            ),
            1 => (
                json!({"command":"cargo test","result":"failed","artifacts":[]}),
                json!({"test_proof":{"command":"cargo test","exit_code":101,"tests_passed":0,"tests_failed":1,"log":"one test failed"}}),
            ),
            _ => (
                json!({"decision":"reject","contours":{}}),
                review_proof("reject"),
            ),
        };
        let findings = json!({"decision":"remediate"});
        let mut proof = evidence(&claimed);
        proof.proof = bare;
        assert!(store
            .submit(&task_id, GATES[gate], &proof, "reject", Some(&findings))
            .is_err());
        assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
        if gate == 2 {
            for invalid in [
                json!({"review_proof":{"decision":"reject","contours":{}}}),
                passing_review_proof(),
            ] {
                proof.proof = invalid;
                assert!(store
                    .submit(&task_id, GATES[gate], &proof, "reject", Some(&findings))
                    .is_err());
                assert_eq!(store.inspect(&task_id).unwrap().status, "in_progress");
            }
        }
        proof.proof = typed.clone();
        let submitted = store
            .submit(&task_id, GATES[gate], &proof, "reject", Some(&findings))
            .unwrap();
        assert_eq!(submitted.status, "ready");
        let stored: String = store
            .connection
            .query_row(
                "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='rejected'",
                rusqlite::params![task_id, GATES[gate]],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&stored).unwrap()["proof"],
            typed
        );
        let submission: String = store
            .connection
            .query_row(
                "SELECT result FROM task_submissions WHERE task_id=?1 AND revision=?2",
                rusqlite::params![task_id, claimed.claim_revision],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&submission).unwrap()["evidence"]["proof"],
            typed
        );
    }
}

#[test]
fn linked_task_workspaces_share_storage_and_confine_local_scope() {
    let (root, _, _) = fixture();
    let linked = ScopedWorkspace::new("forge_linked_tasks");
    let missing = ScopedWorkspace::new("forge_missing_milestones");
    let empty = ScopedWorkspace::new("forge_empty_milestones");
    empty.write("docs/milestones/README.md", "# No task manifests\n");
    linked.write("src/lib.rs", "pub fn linked() {}\n");
    linked.write("docs/AGENTS.md", "# Linked repository rules\n");
    linked.write(
        "forge-mcp.yaml",
        "task_repository: traverse-library\ntask_project: tooling\n",
    );
    linked.write(
        "contracts/010-linked.md",
        &SPEC
            .replace("id: m-test", "id: m-linked")
            .replace("isolated", "linked-rules"),
    );
    root.write("forge-mcp.yaml", &format!("roots: [src]\nlinked_workspaces:\n  - name: traverse\n    path: {}\n    tasks:\n      enabled: true\n      milestones_dir: contracts\n      agents_guidance: docs/AGENTS.md\n  - name: missing\n    path: {}\n    tasks: {{enabled: true}}\n  - name: empty\n    path: {}\n    tasks: {{enabled: true}}\n  - name: disabled\n    path: {}\n    tasks: {{enabled: false}}\n  - name: index-only\n    path: {}\n",linked.0.display(),missing.0.display(),empty.0.display(),linked.0.display(),linked.0.display()));
    let sync = |workspace: &str| {
        tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"sync","workspace":workspace})).unwrap(),
        )
        .unwrap()
    };
    assert_eq!(sync("missing")["tasks"], json!([]));
    assert_eq!(sync("empty")["tasks"], json!([]));
    let imported = sync("traverse");
    assert_eq!(imported["tasks"].as_array().unwrap().len(), 2);
    let id = imported["tasks"][0]["task_id"].as_str().unwrap();
    assert!(id.starts_with("traverse-library/tooling/"));
    let list =
        |arguments| tasks::list(&root.0, serde_json::from_value(arguments).unwrap()).unwrap();
    let linked_by_milestone = list(json!({"milestone_ref":"m-linked","status":"all"}));
    let linked_by_milestone_tasks = linked_by_milestone["tasks"].as_array().unwrap();
    assert_eq!(linked_by_milestone_tasks.len(), 2);
    assert!(linked_by_milestone_tasks.iter().all(|task| {
        task["task_id"]
            .as_str()
            .unwrap()
            .starts_with("traverse-library/tooling/")
    }));
    assert_eq!(list(json!({}))["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(
        list(json!({"repository":"all"}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        list(json!({"repository":"traverse"}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        list(json!({"repository":"all","status":"all"}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        4
    );

    linked.write(
        "contracts/020-linked.md",
        &SPEC.replace("m-test", "m-linked-020"),
    );
    root.write(
        "docs/milestones/020-primary-alpha.md",
        &SPEC.replace("m-test", "m-primary-alpha"),
    );
    root.write(
        "docs/milestones/020-primary-beta.md",
        &SPEC.replace("m-test", "m-primary-beta"),
    );
    let ambiguous_primary = tasks::list(
        &root.0,
        serde_json::from_value(json!({"milestone_ref":"020","status":"all"})).unwrap(),
    )
    .unwrap_err();
    assert!(
        ambiguous_primary
            .to_string()
            .contains("multiple active documents"),
        "a primary ambiguity must not fall through to a unique linked match: {ambiguous_primary}"
    );
    let inspect = tasks::manage(
        &root.0,
        serde_json::from_value(json!({"action":"inspect","task_id":id})).unwrap(),
    )
    .unwrap();
    assert_eq!(inspect["workspace_root"], linked.0.to_str().unwrap());
    assert_eq!(
        inspect["agents_guidance"],
        linked.0.join("docs/AGENTS.md").to_str().unwrap()
    );
    assert_eq!(inspect["applicable_invariants"], json!(["linked-rules"]));
    let extend = |path: &str| {
        tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"extend_scope","task_id":id,"paths":[path]}))
                .unwrap(),
        )
    };
    assert!(extend("src/new.rs").is_ok());
    std::os::unix::fs::symlink(root.0.join("src"), linked.0.join("src/foreign")).unwrap();
    assert!(extend("src/foreign/main.rs").is_err());
    assert!(extend("../outside.rs").is_err());
    let claim = |worktree: &std::path::Path| {
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: id.into(),
                stage: "contract".into(),
                worker_id: "linked-builder".into(),
                worktree: worktree.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
    };
    assert!(claim(&root.0).is_err());
    let claimed = claim(&linked.0).unwrap();
    assert_eq!(claimed["workspace_root"], inspect["workspace_root"]);
    assert_eq!(claimed["agents_guidance"], inspect["agents_guidance"]);
    let connection = rusqlite::Connection::open(root.0.join(".forge/tasks.sqlite")).unwrap();
    let namespaces: i64 = connection
        .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(namespaces, 4);
}
#[test]
fn linked_scope_extension_validates_owner_and_claimed_worktree() {
    let (root, _, _) = fixture();
    let owner = ScopedWorkspace::new("forge_scope_owner");
    let worktree = ScopedWorkspace::new("forge_scope_worktree");
    for workspace in [&owner, &worktree] {
        workspace.write("src/lib.rs", "pub fn local() {}\n");
        workspace.write("docs/010-test.md", SPEC);
    }
    root.write(
        "forge-mcp.yaml",
        &format!(
            "linked_workspaces:\n  - name: traverse\n    path: {}\n    tasks: {{enabled: true}}\n",
            owner.0.display()
        ),
    );
    let synced = tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action":"sync","workspace":"traverse","milestone_ref":"docs/010-test.md"}),
        )
        .unwrap(),
    )
    .unwrap();
    let id = synced["tasks"][0]["task_id"].as_str().unwrap();
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: id.into(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            worktree: worktree.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    let extend = |path: &str| {
        tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"extend_scope","task_id":id,"paths":[path]}))
                .unwrap(),
        )
    };
    std::os::unix::fs::symlink(root.0.join("src"), owner.0.join("src/foreign")).unwrap();
    assert!(extend("src/foreign/new.rs").is_err());
    std::fs::remove_file(owner.0.join("src/foreign")).unwrap();
    std::os::unix::fs::symlink(root.0.join("src"), worktree.0.join("src/foreign")).unwrap();
    assert!(extend("src/foreign/new.rs").is_err());
    assert!(extend("src/local.rs").is_ok());
}
#[test]
fn nested_repository_tasks_preserve_distinct_write_perimeters() {
    let main = ScopedWorkspace::new("forge_nested_repositories");
    main.write("src/main.rs", "pub fn main_owner() {}\n");
    main.write(
        "docs/010-main.md",
        &SPEC.replace("scope: [src/]", "scope: [src/main.rs]"),
    );
    main.write("src/library/src/lib.rs", "pub fn library_owner() {}\n");
    main.write("src/library/docs/milestones/010-lib.md", SPEC);
    main.write("forge-mcp.yaml", "linked_workspaces:\n  - name: library\n    path: src/library\n    tasks: {enabled: true}\n");
    let imported = tasks::manage(
        &main.0,
        serde_json::from_value(json!({"action":"sync","milestone_ref":"docs/010-main.md"}))
            .unwrap(),
    )
    .unwrap();
    let primary_id = imported["tasks"][0]["task_id"].as_str().unwrap();
    let imported_linked = tasks::manage(
        &main.0,
        serde_json::from_value(json!({"action":"sync","workspace":"library"})).unwrap(),
    )
    .unwrap();
    let library_id = imported_linked["tasks"][0]["task_id"].as_str().unwrap();
    let extend = |path: &str| {
        tasks::manage(
            &main.0,
            serde_json::from_value(
                json!({"action":"extend_scope","task_id":primary_id,"paths":[path]}),
            )
            .unwrap(),
        )
    };
    assert!(extend("src/library/src/foreign.rs").is_err());
    assert!(extend("src/main_helpers.rs").is_ok());
    let claimed = tasks::claim(
        &main.0,
        tasks::Claim {
            task_id: library_id.into(),
            stage: "contract".into(),
            worker_id: "library-builder".into(),
            worktree: main.0.join("src/library").to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        claimed["workspace_root"],
        main.0.join("src/library").to_str().unwrap()
    );
    main.write(
        "docs/010-wide.md",
        &SPEC.replace("id: m-test", "id: m-wide"),
    );
    assert!(tasks::manage(
        &main.0,
        serde_json::from_value(json!({"action":"sync","milestone_ref":"docs/010-wide.md"}))
            .unwrap()
    )
    .is_err());
}
#[test]
fn task_specification_and_store_survive_code_index_rebuild() {
    let (root, mut store, milestone) = fixture();
    let first = &milestone.tasks[0];
    let digest = milestone.digest(first).unwrap();
    let mut changed = milestone.clone();
    changed.tasks[1].target = "Sibling amendment".into();
    assert_eq!(digest, changed.digest(first).unwrap());
    let id = milestone.task_id(first);
    let claim = store
        .claim(&id, "contract", "builder", root.0.to_str().unwrap())
        .unwrap();
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    drop(store);
    let store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    assert_eq!(
        store.inspect(&id).unwrap().claim_revision,
        claim.claim_revision
    );
    assert_eq!(store.inspect(&id).unwrap().status, "in_progress");
    let wal: String = store
        .connection
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(wal, "wal");
    let timeout: i64 = store
        .connection
        .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
        .unwrap();
    assert_eq!(timeout, 5000);
}
#[test]
fn task_configuration_defaults_and_crlf_preserve_operational_identity() {
    let (root, _, milestone) = fixture();
    root.write("forge-mcp.yaml", "roots: [src]\n");
    assert_eq!(
        tasks::database_path(&root.0).unwrap(),
        root.0.join(".forge/tasks.sqlite")
    );
    let crlf = Milestone::parse(&SPEC.replace('\n', "\r\n"), "forge-mcp").unwrap();
    assert_eq!(
        crlf.task_id(&crlf.tasks[0]),
        milestone.task_id(&milestone.tasks[0])
    );
    assert_eq!(
        crlf.digest(&crlf.tasks[0]).unwrap(),
        milestone.digest(&milestone.tasks[0]).unwrap()
    );
    let id = milestone.task_id(&milestone.tasks[0]);
    let missing = root.0.join("not-created");
    let error = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: id.clone(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            worktree: missing.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().starts_with("WORKTREE_NOT_FOUND"));
    let task = tasks::store(&root.0).unwrap().inspect(&id).unwrap();
    assert_eq!(task.status, "ready");
    assert!(task.worker_id.is_none());
}
#[test]
fn linked_worktree_resolves_tasks_db_to_primary_worktree_root() {
    let primary = ScopedWorkspace::new("forge_primary_git");
    let wt = ScopedWorkspace::new("forge_wt_git");
    let run = |dir: &std::path::Path, args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap()
    };
    run(&primary.0, &["init", "-b", "main"]);
    run(&primary.0, &["config", "user.name", "Test User"]);
    run(&primary.0, &["config", "user.email", "test@example.com"]);
    primary.write("README.md", "# test\n");
    primary.write("forge-mcp.yaml", "roots: [src]\n");
    run(&primary.0, &["add", "."]);
    run(&primary.0, &["commit", "-m", "initial"]);
    let wt_out = run(
        &primary.0,
        &["worktree", "add", wt.0.to_str().unwrap(), "-b", "test-wt"],
    );
    if wt_out.status.success() {
        let expected = primary
            .0
            .canonicalize()
            .unwrap()
            .join(".forge/tasks.sqlite");
        let resolved = tasks::database_path(&wt.0).unwrap();
        assert_eq!(resolved, expected);
        // Fallback when forge-mcp.yaml is absent in worktree:
        let _ = std::fs::remove_file(wt.0.join("forge-mcp.yaml"));
        let resolved_fallback = tasks::database_path(&wt.0).unwrap();
        assert_eq!(resolved_fallback, expected);
        let _ = run(
            &primary.0,
            &["worktree", "remove", "--force", wt.0.to_str().unwrap()],
        );
    }
}
#[test]
fn tasks_coordinate_claims_dependencies_reset_and_scope() {
    let (root, mut store, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let listed = tasks::list(
        &root.0,
        tasks::List {
            repository: None,
            milestone_ref: None,
            milestone_status: None,
            status: None,
            stage: None,
            detail: None,
        },
    )
    .unwrap();
    assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(listed["tasks"][0]["task_id"], id);
    let path = root.0.join(".forge/tasks.sqlite");
    let worktree = root.0.to_str().unwrap().to_owned();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|n| {
            let path = path.clone();
            let worktree = worktree.clone();
            let id = id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut store = TasksStore::open(&path).unwrap();
                barrier.wait();
                store.claim(&id, "contract", &format!("worker-{n}"), &worktree)
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(results
        .iter()
        .filter_map(|r| r.as_ref().err())
        .all(|e| e.to_string() == "TASK_ALREADY_CLAIMED"));
    let before = store.inspect(&id).unwrap();
    let reset = store.reset(&id).unwrap();
    assert!(reset.claim_revision > before.claim_revision);
    assert_eq!(reset.status, "ready");
    let extended = store
        .extend_scope(
            &id,
            &["src/next.rs".into(), "tests/fixture.rs".into()],
            &root.0,
        )
        .unwrap();
    assert!(extended.spec.scope.contains(&"src/next.rs".into()));
    assert!(extended.spec.scope.contains(&"tests/fixture.rs".into()));
    for path in ["../escape", "docs/outside.rs"] {
        assert!(store.extend_scope(&id, &[path.into()], &root.0).is_err());
    }
    let external = ScopedWorkspace::new("forge_scope_external");
    std::os::unix::fs::symlink(&external.0, root.0.join("src/escape")).unwrap();
    assert!(store
        .extend_scope(&id, &["src/escape/file.rs".into()], &root.0)
        .is_err());
    std::os::unix::fs::symlink(root.0.join("docs"), root.0.join("src/module_escape")).unwrap();
    assert!(store
        .extend_scope(&id, &["src/module_escape/file.rs".into()], &root.0)
        .is_err());
}
#[test]
fn gates_write_receipts_and_preserve_retention_outcomes() {
    let (root, mut store, milestone) = fixture();
    let claimed_worktree = ScopedWorkspace::new("forge_claim_receipt");
    claimed_worktree.write("src/lib.rs", "pub fn example() {}\n");
    let id = milestone.task_id(&milestone.tasks[0]);
    for (gate, stage) in GATES.iter().enumerate() {
        let worker = match gate {
            2 => "reviewer",
            3 => "delivery-reviewer",
            _ => "builder",
        };
        let worktree = if gate == 3 { &claimed_worktree } else { &root };
        let task = store
            .claim(&id, stage, worker, worktree.0.to_str().unwrap())
            .unwrap();
        let evidence = Evidence {
            task_id: id.clone(),
            stage: stage.to_string(),
            claim_revision: task.claim_revision,
            contract_revision: task.contract_revision,
            worker_id: worker.into(),
            worktree: worktree.0.to_str().unwrap().into(),
            commit: Some("0123456789abcdef0123456789abcdef01234567".into()),
            proof: match gate {
                0 => {
                    json!({"contract_proof":{"seam_test_ref":"tests/acdd/tasks.rs","red_exit_code":101}})
                }
                1 => {
                    json!({"test_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":1,"tests_failed":0}})
                }
                2 => passing_review_proof(),
                _ => json!({"stage":stage}),
            },
        };
        if gate == 3 {
            assert!(store.submit(&id, stage, &evidence, "pass", None).is_err());
            assert_eq!(store.inspect(&id).unwrap().status, "in_progress");
            claimed_worktree.write("docs/010-test.md", SPEC);
        }
        let result = store.submit(&id, stage, &evidence, "pass", None).unwrap();
        let retry = store.submit(&id, stage, &evidence, "pass", None).unwrap();
        assert_eq!(retry.claim_revision, result.claim_revision);
        assert_eq!(result.status, if gate == 3 { "completed" } else { "ready" });
    }
    let completed = store.inspect(&id).unwrap();
    assert!(completed.receipt.as_ref().unwrap().rollup.is_some());
    let at = completed.completed_at.unwrap();
    assert!(store
        .delete(Some(&id), None, false, at + RETENTION_SECONDS)
        .is_err());
    assert_eq!(store.cleanup(at + RETENTION_SECONDS + 1).unwrap(), vec![id]);
    assert_eq!(store.list(None, "ready", None).unwrap().len(), 1);
    let rows: i64 = store
        .connection
        .query_row("SELECT COUNT(*) FROM task_claims", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}
#[test]
fn specifications_validate_unique_identities_and_governing_digests() {
    let parsed = Milestone::parse(SPEC, "forge-mcp").unwrap();
    let first = &parsed.tasks[0];
    let digest = parsed.digest(first).unwrap();
    let mut completed = first.clone();
    completed.status = Some("completed".into());
    completed.receipt = Some(Receipt {
        commit: Some("candidate".into()),
        contract_revision: 1,
        passed_at: "timestamp".into(),
        evidence: json!({}),
        review: json!({}),
        decision: "pass".into(),
        rollup: None,
    });
    assert_eq!(parsed.digest(&completed).unwrap(), digest);
    let mut constrained = parsed.clone();
    constrained.invariants.push("WAL coordination".into());
    assert_ne!(constrained.digest(first).unwrap(), digest);
    for malformed in [
        SPEC.replace("task_ref: second", "task_ref: first"),
        SPEC.replace("depends_on: [first]", "depends_on: [missing]"),
        SPEC.replace("scope: [src/]", "scope: [../outside]"),
        SPEC.replace("id: m-test", "id: []"),
    ] {
        assert!(Milestone::parse(&malformed, "forge-mcp").is_err());
    }
}
#[test]
fn contract_readmission_and_reset_fence_old_evidence() {
    let (root, mut store, mut milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let first = store
        .claim(&id, "contract", "builder", root.0.to_str().unwrap())
        .unwrap();
    let stale = evidence(&first);
    let reset = store.reset(&id).unwrap();
    assert!(store.submit(&id, "contract", &stale, "pass", None).is_err());
    let second = store
        .claim(&id, "contract", "builder", root.0.to_str().unwrap())
        .unwrap();
    assert!(second.claim_revision > reset.claim_revision);
    milestone.tasks[0].target = "Amended target".into();
    assert!(store.sync(&milestone, "docs/010-test.md", &root.0).is_err());
    assert_eq!(
        store.inspect(&id).unwrap().worker_id.as_deref(),
        Some("builder")
    );
    milestone.tasks[0].contract_revision = 2;
    let synced = store.sync(&milestone, "docs/010-test.md", &root.0).unwrap();
    assert_eq!(synced[0].contract_revision, 2);
    assert_eq!(synced[0].gate, 0);
    assert!(synced[0].claim_revision > second.claim_revision);
    assert!(store
        .submit(&id, "contract", &evidence(&second), "pass", None)
        .is_err());
}
#[test]
fn review_rejection_retains_findings_and_requires_a_new_build() {
    let (root, mut store, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    for stage in ["contract", "build"] {
        let claimed = store
            .claim(&id, stage, "builder", root.0.to_str().unwrap())
            .unwrap();
        store
            .submit(&id, stage, &evidence(&claimed), "pass", None)
            .unwrap();
    }
    let claimed = store
        .claim(&id, "review", "builder", root.0.to_str().unwrap())
        .unwrap();
    let mut review = evidence(&claimed);
    review.proof = passing_review_proof();
    assert!(store.submit(&id, "review", &review, "pass", None).is_err());
    store.reset(&id).unwrap();
    let claimed = store
        .claim(&id, "review", "reviewer", root.0.to_str().unwrap())
        .unwrap();
    let findings = json!({"paths":["src/lib.rs"],"decision":"remediate"});
    let mut review = evidence(&claimed);
    review.proof = review_proof("reject");
    let rejected = store
        .submit(&id, "review", &review, "reject", Some(&findings))
        .unwrap();
    assert_eq!(rejected.gate, 1);
    assert_eq!(rejected.status, "ready");
    assert_eq!(
        store.inspect_details(&id).unwrap()["findings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let rebuilt = store
        .claim(&id, "build", "builder", root.0.to_str().unwrap())
        .unwrap();
    store
        .submit(&id, "build", &evidence(&rebuilt), "pass", None)
        .unwrap();
    assert_eq!(store.inspect(&id).unwrap().gate, 2);
}
#[test]
fn force_deletion_preserves_project_boundaries_and_revision_fencing() {
    let (root, mut store, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let first = store
        .claim(&id, "contract", "worker", root.0.to_str().unwrap())
        .unwrap();
    assert!(store
        .delete(None, Some("docs/010-test.md"), false, 0)
        .is_err());
    assert_eq!(store.list(None, "all", None).unwrap().len(), 2);
    let deleted = store.delete(Some(&id), None, true, 0).unwrap();
    assert_eq!(deleted.revoked_claims, 1);
    assert_eq!(deleted.count, 1);
    assert!(store.list(None, "ready", None).unwrap().is_empty());
    let recreated = store
        .create(&milestone, "docs/010-test.md", &root.0, "first")
        .unwrap();
    assert!(recreated[0].claim_revision > first.claim_revision);
    let other = contextunity_forge_mcp::db::tasks_store::TasksStore::open_project(
        &root.0.join(".forge/tasks.sqlite"),
        "other",
        "project",
    )
    .unwrap();
    assert!(other.list(None, "all", None).unwrap().is_empty());
    assert!(other.inspect(&id).is_err());
}
#[test]
fn dedicated_store_rejects_index_and_unknown_schema_without_ddl() {
    let root = ScopedWorkspace::new("forge_task_schema");
    root.write("src/lib.rs", "pub fn sample() {}\n");
    root.write("forge-mcp.yaml", "roots: [src]\ndocs: []\n");
    let index = root.0.join("index.sqlite");
    writer::build(&root.0, &index, None).unwrap();
    assert!(TasksStore::open(&index).is_err());
    let conn = rusqlite::Connection::open(&index).unwrap();
    let task_tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name='tasks'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(task_tables, 0);
    let future = root.0.join("future.sqlite");
    let conn = rusqlite::Connection::open(&future).unwrap();
    conn.execute_batch("CREATE TABLE task_store_metadata(key TEXT PRIMARY KEY,value TEXT); INSERT INTO task_store_metadata VALUES('schema_version','99');").unwrap();
    assert!(TasksStore::open(&future).is_err());
    let tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type='table'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tables, 1);
}
#[test]
fn infer_project_from_path_supports_arbitrary_monorepo_structures_universally() {
    use contextunity_forge_mcp::core::tasks::infer_project_from_path;
    use std::path::Path;

    // Standard docs/milestones and docs/plans under arbitrary subprojects
    assert_eq!(
        infer_project_from_path(Path::new("extensions/commerce/docs/milestones/010.md")),
        Some("extensions.commerce".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("packages/cli/docs/milestones/010.md")),
        Some("packages.cli".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("services/brain/docs/plans/010.md")),
        Some("services.brain".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("apps/web/docs/milestones/010.md")),
        Some("apps.web".into())
    );

    // Arbitrary custom folders - Forge does not dictate folder naming conventions
    assert_eq!(
        infer_project_from_path(Path::new("custom_dir/my-plugin/milestones/010.md")),
        Some("custom_dir.my-plugin".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new(
            "team_alpha/backend/analytics/docs/milestones/020.md"
        )),
        Some("team_alpha.backend.analytics".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("user_service/docs/milestones/010.md")),
        Some("user_service".into())
    );
    assert_ne!(
        infer_project_from_path(Path::new("foo.bar/docs/milestones/010.md")),
        infer_project_from_path(Path::new("foo/bar/docs/milestones/010.md"))
    );
    assert_ne!(
        infer_project_from_path(Path::new("a~d/docs/milestones/010.md")),
        infer_project_from_path(Path::new("a.b/docs/milestones/010.md"))
    );
    assert_eq!(
        infer_project_from_path(Path::new("packages/my app/docs/milestones/010.md")),
        Some("packages.my~u32~app".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("packages/über/docs/milestones/010.md")),
        Some("packages.~u252~ber".into())
    );
    assert_eq!(
        infer_project_from_path(Path::new("microservices/billing/docs/plans/030.md")),
        Some("microservices.billing".into())
    );
    assert_ne!(
        infer_project_from_path(Path::new("packages/api/docs/milestones/010.md")),
        infer_project_from_path(Path::new("services/api/docs/milestones/010.md"))
    );
    assert_eq!(
        infer_project_from_path(Path::new("standalone_tool/milestones/010.md")),
        Some("standalone_tool".into())
    );

    // Root repository milestones and plans have no subproject
    assert_eq!(
        infer_project_from_path(Path::new("docs/milestones/010.md")),
        None
    );
    assert_eq!(
        infer_project_from_path(Path::new("milestones/010.md")),
        None
    );
    assert_eq!(
        infer_project_from_path(Path::new("docs/plans/010.md")),
        None
    );
    assert_eq!(infer_project_from_path(Path::new("plans/010.md")), None);

    let root = ScopedWorkspace::new("forge_project_hierarchy");
    root.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\nmilestones:\n  - docs/milestones\n  - packages/api/docs/milestones\n  - services/api/docs/milestones\n  - plugins/api/docs/milestones\n");
    let task_block = "# Tasks\n```yaml\ntask_ref: shared\ntarget: Resolve one project\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    for (dir, project) in [
        ("packages/api", ""),
        ("services/api", ""),
        ("plugins/api", "project: explicit-api\n"),
    ] {
        root.write(
            &format!("{dir}/docs/milestones/010-shared.md"),
            &format!(
                "---\nid: m-shared\ntitle: Shared\ndoc_type: contract\n{project}---\n{task_block}"
            ),
        );
    }
    for (filename, project) in [
        ("010-default.md", ""),
        ("020-alpha.md", "project: alpha\n"),
        ("030-beta.md", "project: beta\n"),
    ] {
        root.write(
            &format!("docs/milestones/{filename}"),
            &format!(
                "---\nid: m-shared\ntitle: Shared\ndoc_type: contract\n{project}---\n{task_block}"
            ),
        );
    }
    let mut ids = std::collections::BTreeSet::new();
    for reference in [
        "packages/api/docs/milestones/010-shared.md",
        "services/api/docs/milestones/010-shared.md",
        "plugins/api/docs/milestones/010-shared.md",
        "docs/milestones/010-default.md",
        "docs/milestones/020-alpha.md",
        "docs/milestones/030-beta.md",
    ] {
        let response = tasks::manage(
            &root.0,
            tasks::Manage {
                action: tasks::ManageAction::Sync,
                milestone_ref: Some(reference.to_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        ids.insert(response["tasks"][0]["task_id"].as_str().unwrap().to_owned());
    }
    assert_eq!(ids.len(), 6);
    assert!(ids.iter().any(|id| id.contains("/explicit-api/")));
    assert!(ids.iter().any(|id| id.contains("/forge-mcp/")));
    assert!(ids.iter().any(|id| id.contains("/alpha/")));
    assert!(ids.iter().any(|id| id.contains("/beta/")));
}

#[test]
fn completed_task_reopen_and_mcp_manage_action_lifecycle() {
    let spec = "---\nid: m-reopen\ntitle: Reopen Test\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: reopenable\ntarget: Deliver reopenable feature\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    let root = ScopedWorkspace::new("forge_reopen");
    root.write("src/lib.rs", "pub fn reopenable() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-reopen.md", spec);

    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-reopen.md", &root.0)
        .unwrap();
    let task_id = "forge-mcp/forge-mcp/m-reopen:reopenable";

    // Advance task through all 4 gates to completed
    let stages = ["contract", "build", "review", "deliver"];
    for (index, stage) in stages.iter().enumerate() {
        let worker = if index >= 2 { "reviewer" } else { "builder" };
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: task_id.into(),
                stage: (*stage).into(),
                worker_id: worker.into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();

        let stored = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
        let mut ev = evidence(&stored);
        if *stage == "deliver" {
            ev.proof = json!({
                "review_proof": {
                    "decision": "pass",
                    "contours": {
                        "audit": {"applicable": true, "evidence": "Reopen verification passed"}
                    }
                }
            });
        }
        tasks::submit(
            &root.0,
            serde_json::from_value(json!({
                "task_id": task_id,
                "stage": stage,
                "action": "pass",
                "evidence": serde_json::to_value(&ev).unwrap(),
            }))
            .unwrap(),
        )
        .unwrap();
    }

    // Verify task is completed in SQLite
    let completed_task = store.inspect(task_id).unwrap();
    assert_eq!(completed_task.status, "completed");
    assert!(completed_task.completed_at.is_some());
    assert!(completed_task.receipt.is_some());

    let milestone_path = root.0.join("docs/010-reopen.md");
    let completed_text = std::fs::read_to_string(&milestone_path).unwrap();

    // Subtask modifications fail with informative TASK_TERMINAL message
    let add_err = store
        .subtask_add(task_id, "sub-audit", "Post-completion audit")
        .unwrap_err();
    assert!(add_err.to_string().contains("TASK_TERMINAL"));
    assert!(add_err.to_string().contains("reopen or reset"));

    // Reopen requires its durable milestone receipt to be available before SQLite changes.
    let held_path = root.0.join("docs/010-reopen-held.md");
    std::fs::rename(&milestone_path, &held_path).unwrap();
    assert!(tasks::reopen(&root.0, task_id).is_err());
    assert_eq!(store.inspect(task_id).unwrap().status, "completed");
    std::fs::rename(&held_path, &milestone_path).unwrap();
    assert_eq!(
        std::fs::read_to_string(&milestone_path).unwrap(),
        completed_text
    );

    // MCP task_manage with action: "reopen"
    let reopen_res = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Reopen,
            task_id: Some(task_id.into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(reopen_res["status"], "ready");
    assert_eq!(reopen_res["gate"], 0);
    assert!(reopen_res["receipt"].is_null());
    assert!(reopen_res["completed_at"].is_null());
    let reopened_text = std::fs::read_to_string(&milestone_path).unwrap();
    assert!(!reopened_text.contains("status: completed"));
    assert!(!reopened_text.contains("receipt:"));

    // Reopened task can accept subtasks
    let sub = store
        .subtask_add(task_id, "sub-audit", "Post-completion audit")
        .unwrap();
    assert_eq!(sub.subtask_ref, "sub-audit");
    assert_eq!(sub.status, "pending");

    // Reopened task can be claimed again at contract
    let re_claimed = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "contract".into(),
            worker_id: "auditor".into(),
            worktree: root.0.to_str().unwrap().into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(re_claimed["status"], "in_progress");

    // MCP task_manage with action: "reset" resets in-progress claim
    let reset_res = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Reset,
            task_id: Some(task_id.into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(reset_res["status"], "ready");
}

#[test]
fn task_manage_sync_and_list_resolve_milestone_reference_by_id_prefix_stem_and_path() {
    let root = ScopedWorkspace::new("forge_milestone_ref_resolution");
    root.write("AGENTS.md", "# Rules\n");
    root.write(
        "forge-mcp.yaml",
        "task_repository: test\ntask_project: test\nroots: []\n",
    );
    let milestone_content = "---\nid: m-tool-performance-and-storage-compaction\ntitle: Performance\ndoc_type: contract\ninvariants: []\n---\n```yaml\ntask_ref: compaction\ntarget: Compact storage\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write(
        "docs/milestones/030-tool-performance-and-storage-compaction.md",
        milestone_content,
    );

    // 1. Sync by milestone ID ("m-tool-performance-and-storage-compaction")
    let sync_by_id = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("m-tool-performance-and-storage-compaction".into()),
            workspace: Some("test".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_by_id["tasks"].as_array().unwrap().len(), 1);

    // List by milestone ID
    let list_by_id = tasks::list(
        &root.0,
        tasks::List {
            repository: Some("test".into()),
            milestone_ref: Some("m-tool-performance-and-storage-compaction".into()),
            milestone_status: None,
            status: Some(tasks::Status::All),
            stage: None,
            detail: None,
        },
    )
    .unwrap();
    assert_eq!(list_by_id["tasks"].as_array().unwrap().len(), 1);

    // 2. Sync by numeric prefix ("030" and "30")
    let sync_by_prefix = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("030".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_by_prefix["tasks"].as_array().unwrap().len(), 1);

    let list_by_prefix = tasks::list(
        &root.0,
        tasks::List {
            repository: None,
            milestone_ref: Some("30".into()),
            milestone_status: None,
            status: Some(tasks::Status::All),
            stage: None,
            detail: None,
        },
    )
    .unwrap();
    assert_eq!(list_by_prefix["tasks"].as_array().unwrap().len(), 1);

    // 3. Sync by filename stem ("030-tool-performance-and-storage-compaction")
    let sync_by_stem = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("030-tool-performance-and-storage-compaction".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_by_stem["tasks"].as_array().unwrap().len(), 1);

    // 4. Sync by full relative file path
    let sync_by_path = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some(
                "docs/milestones/030-tool-performance-and-storage-compaction.md".into(),
            ),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_by_path["tasks"].as_array().unwrap().len(), 1);

    // 5. Sync with a workspace selector imports all workspace manifests
    let sync_all = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            workspace: Some("test".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(sync_all["tasks"].as_array().unwrap().len(), 1);

    // 6. Non-existent milestone reference produces descriptive error instead of raw os error 2
    let err = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("m-non-existent".into()),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("not found in workspace"),
        "error must be descriptive: {err}"
    );
}

#[test]
fn task_list_filters_milestone_status_and_controls_subtask_details() {
    let root = ScopedWorkspace::new("forge_task_list_archive_and_evidence");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let active_spec = "---\nid: m-active\ntitle: Active Milestone\ndoc_type: contract\nstatus: active\n---\n# Tasks\n```yaml\ntask_ref: active-task\ntarget: Active task target\nproof_policy: seam-test-first\nscope: [src/]\nsubtasks:\n  - subtask_ref: sub-1\n    title: Active subtask\n    status: in_progress\n    evidence: Secret heavy evidence blob\n```\n";
    root.write("docs/milestones/010-active.md", active_spec);

    let archived_spec = "---\nid: m-archived\ntitle: Archived Milestone\ndoc_type: contract\nstatus: completed\n---\n# Tasks\n```yaml\ntask_ref: archived-task\ntarget: Archived task target\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write("docs/milestones/archive/009-archived.md", archived_spec);

    let planned_spec = "---\nid: m-planned\ntitle: Planned Milestone\ndoc_type: contract\n---\n# Tasks\n```yaml\ntask_ref: planned-task\ntarget: Planned task target\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write("docs/milestones/008-planned.md", planned_spec);

    let cancelled_active_spec = "---\nid: m-cancelled\ntitle: Cancelled Milestone\ndoc_type: contract\nstatus: active\n---\n# Tasks\n```yaml\ntask_ref: cancelled-task\ntarget: Cancelled task target\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write(
        "docs/milestones/archive/007-cancelled.md",
        cancelled_active_spec,
    );
    let cancelled_spec = "---\nid: m-cancelled\ntitle: Cancelled Milestone\ndoc_type: contract\nstatus: cancelled\nclosure:\n  reason: Replaced by a later milestone\n---\n# Tasks\n```yaml\ntask_ref: cancelled-task\ntarget: Cancelled task target\nproof_policy: direct-proof\nscope: [src/]\n```\n";

    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    for (spec, path) in [
        (active_spec, "docs/milestones/010-active.md"),
        (archived_spec, "docs/milestones/archive/009-archived.md"),
        (planned_spec, "docs/milestones/008-planned.md"),
        (
            cancelled_active_spec,
            "docs/milestones/archive/007-cancelled.md",
        ),
    ] {
        store
            .sync(&Milestone::parse(spec, "forge-mcp").unwrap(), path, &root.0)
            .unwrap();
    }
    // Simulate the pre-sync window: the manifest is cancelled, but its stored task has not been pruned.
    root.write("docs/milestones/archive/007-cancelled.md", cancelled_spec);

    // Active milestones are the default, regardless of task status selection.
    let active_only = tasks::list(
        &root.0,
        tasks::List {
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap();
    let tasks_arr = active_only["tasks"].as_array().unwrap();
    assert_eq!(tasks_arr.len(), 1);
    assert_eq!(
        tasks_arr[0]["task_id"],
        "forge-mcp/forge-mcp/m-active:active-task"
    );

    // Compact listings retain the subtask reference and status, omitting details.
    let subtasks = tasks_arr[0]["subtasks"].as_array().unwrap();
    assert_eq!(subtasks.len(), 1);
    assert_eq!(subtasks[0]["subtask_ref"], "sub-1");
    assert!(subtasks[0].get("title").is_none());
    assert_eq!(subtasks[0]["status"], "in_progress");
    assert!(
        subtasks[0].get("evidence").is_none(),
        "subtask evidence must be stripped in task_list"
    );

    let full_detail = tasks::list(
        &root.0,
        tasks::List {
            status: Some(tasks::Status::All),
            detail: Some(tasks::TaskListDetail::Full),
            ..Default::default()
        },
    )
    .unwrap();
    let full_subtask = &full_detail["tasks"][0]["subtasks"][0];
    assert_eq!(full_subtask["title"], "Active subtask");
    assert_eq!(full_subtask["evidence"], "Secret heavy evidence blob");

    // The only explicit milestone filters are active, planned, completed, and all.
    for (filter, expected_id) in [
        (
            tasks::MilestoneStatusFilter::Active,
            "forge-mcp/forge-mcp/m-active:active-task",
        ),
        (
            tasks::MilestoneStatusFilter::Planned,
            "forge-mcp/forge-mcp/m-planned:planned-task",
        ),
        (
            tasks::MilestoneStatusFilter::Completed,
            "forge-mcp/forge-mcp/m-archived:archived-task",
        ),
    ] {
        let filtered = tasks::list(
            &root.0,
            tasks::List {
                milestone_status: Some(filter),
                status: Some(tasks::Status::All),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(filtered["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(filtered["tasks"][0]["task_id"], expected_id);
    }

    let all_statuses = tasks::list(
        &root.0,
        tasks::List {
            milestone_status: Some(tasks::MilestoneStatusFilter::All),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap();
    let all_ids: Vec<_> = all_statuses["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["task_id"].as_str().unwrap())
        .collect();
    assert_eq!(all_ids.len(), 4);
    assert!(all_ids.contains(&"forge-mcp/forge-mcp/m-cancelled:cancelled-task"));

    // A targeted milestone reference exposes its tasks by default, including a pre-sync cancellation.
    let direct_archived = tasks::list(
        &root.0,
        tasks::List {
            milestone_ref: Some("009".into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap();
    let direct_arr = direct_archived["tasks"].as_array().unwrap();
    assert_eq!(direct_arr.len(), 1);
    assert_eq!(
        direct_arr[0]["task_id"],
        "forge-mcp/forge-mcp/m-archived:archived-task"
    );
    let direct_cancelled = tasks::list(
        &root.0,
        tasks::List {
            milestone_ref: Some("007".into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(direct_cancelled["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(
        direct_cancelled["tasks"][0]["task_id"],
        "forge-mcp/forge-mcp/m-cancelled:cancelled-task"
    );
}

#[test]
fn scoped_git_snapshot_captures_untracked_and_modified_files_and_cleans_up_on_handoff() {
    let root = ScopedWorkspace::new("forge_snapshot_test");
    root.write("src/lib.rs", "pub fn example() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let spec = "---\nid: m-test\ntitle: Tasks\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: first\ntarget: Deliver first\nproof_policy: seam-test-first\nscope: [src/]\n```\n```yaml\ntask_ref: second\ntarget: Deliver second\nproof_policy: seam-test-first\nscope: [src/]\ndepends_on: [first]\n```\n";
    root.write("docs/milestones/010-test.md", spec);
    tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action": "sync", "milestone_ref": "docs/milestones/010-test.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    // Initialize git repository
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&root.0)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&root.0)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Tester"])
        .current_dir(&root.0)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(&root.0)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(&root.0)
        .status()
        .unwrap();

    let tasks_in_store = tasks::store(&root.0)
        .unwrap()
        .list(None, "ready", None)
        .unwrap();
    let task_id = &tasks_in_store[0].task_id;
    root.write("src/new_feature.rs", "pub fn new_feature() -> i32 { 42 }\n");

    // 1. Claim contract
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            worktree: root.0.to_str().unwrap().into(),
            ..Default::default()
        },
    )
    .unwrap();

    // Submit contract with commit: None -> automatically captures snapshot
    let stored = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    let mut ev = evidence(&stored);
    ev.commit = None;
    let snapshot_refs = root.0.join(".git/refs/forge/snapshots");
    std::fs::create_dir_all(&snapshot_refs).unwrap();
    let ref_blocker = snapshot_refs.join("forge-mcp");
    std::fs::write(&ref_blocker, "block the nested snapshot ref path").unwrap();
    let failed_pin = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "contract".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        failed_pin.contains("TASK_SNAPSHOT_REF_FAILED"),
        "{failed_pin}"
    );
    let still_claimed = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    assert_eq!(still_claimed.status, "in_progress");
    assert_eq!(still_claimed.gate, 0);
    std::fs::remove_file(ref_blocker).unwrap();

    let contract_res = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "contract".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap();

    let contract_snap = pinned_candidate(&root.0, task_id);
    assert_eq!(contract_snap.len(), 40);
    assert_eq!(contract_res["snapshot"]["commit"], &contract_snap[..7]);
    assert_eq!(
        contract_res["snapshot"]["inspect_cmd"],
        format!("git show {}", &contract_snap[..7])
    );

    // Commit a sibling change outside the task scope while keeping scoped content fixed.
    root.write("README.md", "# sibling change\n");
    std::process::Command::new("git")
        .args(["add", "README.md"])
        .current_dir(&root.0)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "sibling README change"])
        .current_dir(&root.0)
        .status()
        .unwrap();

    // Claim build
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "build".into(),
            worker_id: "builder".into(),
            worktree: root.0.to_str().unwrap().into(),
            ..Default::default()
        },
    )
    .unwrap();

    // Submit build with commit: None -> captures snapshot with the untracked file!
    let stored = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    let mut ev = evidence(&stored);
    ev.commit = None;
    let build_res = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "build".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap();

    let build_snap = pinned_candidate(&root.0, task_id);
    assert_eq!(build_snap.len(), 40);
    assert_eq!(build_res["snapshot"]["commit"], &build_snap[..7]);
    assert_eq!(
        build_snap, contract_snap,
        "an unrelated sibling commit must not change the scoped candidate SHA"
    );

    // Verify git show on build snapshot includes the newly added file
    let show_output = std::process::Command::new("git")
        .args(["show", &build_snap])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let show_text = String::from_utf8_lossy(&show_output.stdout);
    assert!(show_text.contains("new_feature.rs"));
    assert!(show_text.contains("pub fn new_feature() -> i32 { 42 }"));
    assert!(
        !show_text.contains("README.md"),
        "snapshot must contain only scoped files"
    );
    let parents = std::process::Command::new("git")
        .args(["rev-list", "--parents", "-n", "1", &build_snap])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(parents.status.success());
    assert_eq!(
        String::from_utf8_lossy(&parents.stdout)
            .split_whitespace()
            .count(),
        1,
        "root snapshots must not include a branch parent"
    );

    // 3. Independent review
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "review".into(),
            worker_id: "reviewer".into(),
            worktree: root.0.to_str().unwrap().into(),
            ..Default::default()
        },
    )
    .unwrap();

    // Verify: If worktree diverges after build, review fails with TASK_CANDIDATE_MISMATCH!
    root.write("src/new_feature.rs", "pub fn new_feature() -> i32 { 99 }\n");
    let stored = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    let mut ev = evidence(&stored);
    ev.commit = None;
    let mismatch_err = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "review".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap_err();
    assert!(
        mismatch_err.to_string().contains("TASK_CANDIDATE_MISMATCH"),
        "Expected TASK_CANDIDATE_MISMATCH on diverged worktree, got: {mismatch_err}"
    );

    // Restore worktree to match candidate
    root.write("src/new_feature.rs", "pub fn new_feature() -> i32 { 42 }\n");
    let review_res = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "review".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap();

    // Review snapshot matches build candidate snapshot
    assert_eq!(review_res["snapshot"]["commit"], &build_snap[..7]);

    // 4. Delivery
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "deliver".into(),
            worker_id: "delivery".into(),
            worktree: root.0.to_str().unwrap().into(),
            ..Default::default()
        },
    )
    .unwrap();

    // Verify: If worktree diverges after review, delivery fails!
    root.write(
        "src/new_feature.rs",
        "pub fn new_feature() -> i32 { 999 }\n",
    );
    let stored = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    let mut ev = evidence(&stored);
    ev.commit = None;
    let deliver_err = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "deliver".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap_err();
    assert!(
        deliver_err.to_string().contains("TASK_CANDIDATE_MISMATCH"),
        "Expected TASK_CANDIDATE_MISMATCH on diverged delivery worktree, got: {deliver_err}"
    );

    // Restore worktree to match candidate
    root.write("src/new_feature.rs", "pub fn new_feature() -> i32 { 42 }\n");
    let deliver_res = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "deliver".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap();

    assert_eq!(deliver_res["status"], "completed");
    assert_eq!(deliver_res["snapshot"]["commit"], &build_snap[..7]);
    assert_eq!(deliver_res["receipt"]["commit"], &build_snap[..7]);

    let written = std::fs::read_to_string(root.0.join("docs/milestones/010-test.md")).unwrap();
    let milestone = Milestone::parse(&written, "forge-mcp").unwrap();
    assert_eq!(
        milestone.tasks[0]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        Some(build_snap.as_str())
    );

    // Inspect details shows gates with inspect_cmd and latest_snapshot
    let details = tasks::store(&root.0)
        .unwrap()
        .inspect_details(task_id)
        .unwrap();
    assert_eq!(details["receipt"]["commit"], &build_snap[..7]);
    assert_eq!(details["latest_snapshot"]["commit"], &build_snap[..7]);
    assert_eq!(
        details["latest_snapshot"]["inspect_cmd"],
        format!("git show {}", &build_snap[..7])
    );
    for gate in details["gates"].as_array().unwrap() {
        assert_eq!(gate["commit"], &build_snap[..7]);
        let evidence: serde_json::Value =
            serde_json::from_str(gate["evidence"].as_str().unwrap()).unwrap();
        assert_eq!(evidence["commit"], &build_snap[..7]);
    }

    // Verify snapshot refs exist in git before handoff
    let refs_out = std::process::Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname)",
            "refs/forge/snapshots/",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let refs_text = String::from_utf8_lossy(&refs_out.stdout);
    assert!(refs_text.contains("refs/forge/snapshots/"));

    // Complete task second (which transitioned from blocked to ready upon completion of first)
    let ready_tasks = tasks::store(&root.0)
        .unwrap()
        .list(None, "ready", None)
        .unwrap();
    let task_id_2 = ready_tasks[0].task_id.clone();
    for (stage, worker) in [
        ("contract", "b1"),
        ("build", "b1"),
        ("review", "r1"),
        ("deliver", "d1"),
    ] {
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: task_id_2.clone(),
                stage: (*stage).into(),
                worker_id: (*worker).into(),
                worktree: root.0.to_str().unwrap().into(),
                ..Default::default()
            },
        )
        .unwrap();
        let stored = tasks::store(&root.0).unwrap().inspect(&task_id_2).unwrap();
        let mut proof = evidence(&stored);
        proof.commit = None;
        tasks::submit(
            &root.0,
            tasks::Submit {
                task_id: task_id_2.clone(),
                stage: (*stage).into(),
                action: tasks::Action::Pass,
                evidence: serde_json::to_value(proof).unwrap(),
                findings: None,
            },
        )
        .unwrap();
    }

    // Now handoff milestone
    contextunity_forge_mcp::engine::milestones::handoff(
        &root.0,
        "m-test",
        None,
        "cargo test",
        2,
        0,
    )
    .unwrap();

    // Verify snapshot refs were pruned after milestone handoff!
    let refs_after = std::process::Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname)",
            "refs/forge/snapshots/",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let refs_after_text = String::from_utf8_lossy(&refs_after.stdout)
        .trim()
        .to_string();
    assert!(
        refs_after_text.is_empty(),
        "Expected all snapshot refs to be pruned on handoff, got: {refs_after_text}"
    );
}

#[test]
fn scoped_snapshot_omits_deleted_exact_path_during_contract_submit() {
    let root = ScopedWorkspace::new("forge_snapshot_deleted_exact_path");
    root.write("src/deleted.rs", "pub fn removed() {}\n");
    root.write("src/kept.rs", "pub fn kept() -> i32 { 1 }\n");
    root.write("src/unscoped.rs", "pub fn sibling() -> i32 { 2 }\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let spec = "---\nid: m-snapshot\ntitle: Snapshot Tasks\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\ninvariants: [isolated]\n---\n# Snapshot Tasks\n```yaml\ntask_ref: exact-path\ntarget: Capture a scoped candidate with a deleted exact path\nproof_policy: seam-test-first\nscope: [src/deleted.rs, src/kept.rs]\n```\n";
    root.write("docs/milestones/010-snapshot.md", spec);
    tasks::manage(
        &root.0,
        serde_json::from_value(
            json!({"action": "sync", "milestone_ref": "docs/milestones/010-snapshot.md"}),
        )
        .unwrap(),
    )
    .unwrap();

    let run_git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&root.0)
            .output()
            .unwrap()
    };
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Tester"],
        vec!["add", "-A"],
        vec!["commit", "-q", "-m", "initial"],
    ] {
        let output = run_git(&args);
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let ready = tasks::store(&root.0)
        .unwrap()
        .list(None, "ready", None)
        .unwrap();
    let task_id = ready[0].task_id.clone();
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.clone(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            worktree: root.0.to_str().unwrap().into(),
            ..Default::default()
        },
    )
    .unwrap();

    std::fs::remove_file(root.0.join("src/deleted.rs")).unwrap();
    root.write("src/kept.rs", "pub fn kept() -> i32 { 42 }\n");
    root.write("src/unscoped.rs", "pub fn sibling() -> i32 { 99 }\n");

    let stored = tasks::store(&root.0).unwrap().inspect(&task_id).unwrap();
    let mut proof = evidence(&stored);
    proof.commit = None;
    tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.clone(),
            stage: "contract".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(proof).unwrap(),
            findings: None,
        },
    )
    .unwrap();

    let snapshot = pinned_candidate(&root.0, &task_id);
    let task_tree = run_git(&["ls-tree", "-r", "--name-only", &snapshot]);
    assert!(task_tree.status.success());
    assert_eq!(
        String::from_utf8_lossy(&task_tree.stdout).trim(),
        "src/kept.rs"
    );
    let kept = run_git(&["show", &format!("{snapshot}:src/kept.rs")]);
    assert!(kept.status.success());
    assert_eq!(
        String::from_utf8_lossy(&kept.stdout),
        "pub fn kept() -> i32 { 42 }\n"
    );
    let submitted = tasks::store(&root.0).unwrap().inspect(&task_id).unwrap();
    assert_eq!(submitted.gate, 1);
    assert_eq!(submitted.status, "ready");
}

#[test]
fn scope_extension_rejects_paths_owned_by_sibling_tasks_and_admits_unowned_tests_and_src() {
    let root = ScopedWorkspace::new("forge_scope_conflict");
    root.write("src/module_a/foo.rs", "pub fn foo() {}\n");
    root.write("src/module_b/bar.rs", "pub fn bar() {}\n");
    root.write("tests/test_foo.rs", "// test\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let spec = "---\nid: m-conflict\ntitle: Conflict Tasks\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: task_a\ntarget: Deliver A\nproof_policy: seam-test-first\nscope: [src/module_a/foo.rs]\nscope_roots: [src/, tests/]\n```\n```yaml\ntask_ref: task_b\ntarget: Deliver B\nproof_policy: seam-test-first\nscope: [src/module_b/bar.rs]\nscope_roots: [src/, tests/]\n```\n";
    root.write("docs/010-conflict.md", spec);
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-conflict.md", &root.0)
        .unwrap();

    let task_b_id = "forge-mcp/forge-mcp/m-conflict:task_b";
    let task_a_id = "forge-mcp/forge-mcp/m-conflict:task_a";

    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_b_id.into(),
            stage: "contract".into(),
            worker_id: "worker_b".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();

    let assert_scope_conflict = |requester: &str, path: &str, owner: &str, shape: &str| {
        let err = tasks::manage(
            &root.0,
            serde_json::from_value(json!({
                "action": "extend_scope",
                "task_id": requester,
                "paths": [path]
            }))
            .unwrap(),
        )
        .unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("TASK_SCOPE_CONFLICT"),
            "Expected TASK_SCOPE_CONFLICT for {shape}, got: {err_msg}"
        );
        assert!(
            err_msg.contains(owner),
            "Expected {shape} conflict to name owner '{owner}', got: {err_msg}"
        );
    };

    for (path, shape) in [
        ("src/module_a/foo.rs", "exact frozen path"),
        ("src/module_a/foo.rs/child.rs", "child of frozen path"),
        ("src/module_a", "parent of frozen path"),
    ] {
        assert_scope_conflict(task_b_id, path, task_a_id, shape);
    }

    // Unowned tests/src paths are admitted with frozen=0.
    for path in ["tests/test_foo.rs", "src/module_b/extra.rs"] {
        let admitted = tasks::manage(
            &root.0,
            serde_json::from_value(json!({
                "action": "extend_scope",
                "task_id": task_b_id,
                "paths": [path]
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(admitted["status"], "in_progress");
    }

    // Dynamic extension (frozen = 0) by task_b is now owned by task_b; task_a claiming it must conflict
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_a_id.into(),
            stage: "contract".into(),
            worker_id: "worker_a".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();

    let dynamic_count: i64 = store
        .connection
        .query_row(
            "SELECT count(*) FROM task_scope_paths WHERE task_id=?1 AND frozen=0",
            [task_b_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(dynamic_count, 2, "admitted tests/src paths stay unfrozen");

    for (path, shape) in [
        ("src/module_b/extra.rs", "exact unfrozen path"),
        ("src/module_b/extra.rs/child.rs", "child of unfrozen path"),
        ("src/module_b", "parent of unfrozen path"),
        ("tests/test_foo.rs/child.rs", "child of unfrozen test path"),
        ("tests", "parent of unfrozen test path"),
    ] {
        assert_scope_conflict(task_a_id, path, task_b_id, shape);
    }

    let claim_root = ScopedWorkspace::new("forge_claim_scope_conflict");
    claim_root.write("src/shared/owned.rs", "pub fn owned() {}\n");
    claim_root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let claim_spec = "---\nid: m-claim-conflict\ntitle: Claim Conflict Tasks\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: owner\ntarget: Own shared path\nproof_policy: seam-test-first\nscope: [src/shared/owned.rs]\nscope_roots: [src/]\n```\n```yaml\ntask_ref: successor\ntarget: Claim overlapping path\nproof_policy: seam-test-first\nscope: [src/shared/owned.rs]\nscope_roots: [src/]\n```\n";
    let claim_spec = claim_spec.replace(
        "task_ref: successor\ntarget: Claim overlapping path\nproof_policy: seam-test-first\nscope:",
        "task_ref: successor\ntarget: Claim overlapping path\nproof_policy: seam-test-first\ndepends_on: [owner]\nscope:",
    );
    claim_root.write("docs/010-claim-conflict.md", &claim_spec);
    let claim_milestone = Milestone::parse(&claim_spec, "forge-mcp").unwrap();
    let mut claim_store = TasksStore::open(&claim_root.0.join(".forge/tasks.sqlite")).unwrap();
    claim_store
        .sync(&claim_milestone, "docs/010-claim-conflict.md", &claim_root.0)
        .unwrap();
    let owner_id = "forge-mcp/forge-mcp/m-claim-conflict:owner";
    tasks::claim(
        &claim_root.0,
        tasks::Claim {
            task_id: owner_id.into(),
            stage: "contract".into(),
            worker_id: "owner".into(),
            worktree: claim_root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    let claim_spec = claim_spec
        .replace(
            "task_ref: successor\ntarget: Claim overlapping path\nproof_policy: seam-test-first\ndepends_on: [owner]\nscope:",
            "task_ref: successor\ntarget: Claim overlapping path\nproof_policy: seam-test-first\ncontract_revision: 2\nscope:",
        );
    claim_root.write("docs/010-claim-conflict.md", &claim_spec);
    let claim_milestone = Milestone::parse(&claim_spec, "forge-mcp").unwrap();
    claim_store
        .sync(&claim_milestone, "docs/010-claim-conflict.md", &claim_root.0)
        .unwrap();
    let claim_error = tasks::claim(
        &claim_root.0,
        tasks::Claim {
            task_id: "forge-mcp/forge-mcp/m-claim-conflict:successor".into(),
            stage: "contract".into(),
            worker_id: "successor".into(),
            worktree: claim_root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(claim_error.contains("TASK_SCOPE_CONFLICT"), "{claim_error}");
    assert!(claim_error.contains(owner_id), "{claim_error}");

    let release_root = ScopedWorkspace::new("forge_completed_scope_release");
    release_root.write("src/released/owned.rs", "pub fn owned() {}\n");
    release_root.write("src/released/extra.rs", "pub fn extra() {}\n");
    release_root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let release_spec = "---\nid: m-scope-release\ntitle: Scope Release Tasks\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: completed_owner\ntarget: Finish shared scope owner\nproof_policy: seam-test-first\nscope: [src/released/]\nscope_roots: [src/]\n```\n```yaml\ntask_ref: successor\ntarget: Reuse released scope\nproof_policy: seam-test-first\nscope: [src/released/owned.rs]\nscope_roots: [src/]\n```\n";
    release_root.write("docs/010-scope-release.md", release_spec);
    let mut release_milestone = Milestone::parse(release_spec, "forge-mcp").unwrap();
    release_milestone.tasks[0].status = Some("completed".into());
    release_milestone.tasks[0].receipt = Some(Receipt {
        commit: None,
        contract_revision: 1,
        passed_at: "2026-10-10T00:00:00Z".into(),
        evidence: json!({"test_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":1,"tests_failed":0}}),
        review: passing_review_proof(),
        decision: "pass".into(),
        rollup: None,
    });
    let mut release_store =
        TasksStore::open(&release_root.0.join(".forge/tasks.sqlite")).unwrap();
    release_store
        .sync(
            &release_milestone,
            "docs/010-scope-release.md",
            &release_root.0,
        )
        .unwrap();
    let completed_owner_id = "forge-mcp/forge-mcp/m-scope-release:completed_owner";
    let retained_scope_count: i64 = release_store
        .connection
        .query_row(
            "SELECT count(*) FROM task_scope_paths WHERE task_id=?1",
            [completed_owner_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retained_scope_count, 1);
    let successor_id = "forge-mcp/forge-mcp/m-scope-release:successor";
    let claimed = tasks::claim(
        &release_root.0,
        tasks::Claim {
            task_id: successor_id.into(),
            stage: "contract".into(),
            worker_id: "successor".into(),
            worktree: release_root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(claimed["status"], "in_progress");
    let extended = tasks::manage(
        &release_root.0,
        serde_json::from_value(json!({
            "action": "extend_scope",
            "task_id": successor_id,
            "paths": ["src/released/extra.rs"]
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(extended["status"], "in_progress");
}

#[test]
fn claim_returns_bundle_by_default_with_task_and_milestone_blackboard_messages() {
    let (root, store, _) = fixture();
    let task_id = "forge-mcp/forge-mcp/m-test:first";

    // Post message to task
    store
        .blackboard_post(
            task_id,
            "agent_1",
            "hypothesis",
            "Initial hypothesis for task",
        )
        .unwrap();

    // Post message from another task in same milestone
    let task_2_id = "forge-mcp/forge-mcp/m-test:second";
    store
        .blackboard_post(
            task_2_id,
            "agent_2",
            "architectural_notes",
            "Milestone level finding",
        )
        .unwrap();

    // Claim without specifying bundle (bundle defaults to true)
    let claim_res = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();

    // Response should be the full bundle by default
    assert_eq!(claim_res["task_id"], task_id);
    assert_eq!(claim_res["context_bundle"]["contract"]["task_id"], task_id);
    let bb = claim_res["blackboard"]
        .as_array()
        .expect("blackboard array in bundle");
    assert_eq!(
        bb.len(),
        2,
        "Expected both task and milestone sibling blackboard messages"
    );
    assert!(claim_res["workflow_guidance"].is_object());
}

#[test]
fn universal_scope_roots_extend_scope_and_boundary_validation() {
    let root = ScopedWorkspace::new("forge_universal_scope");
    root.write("packages/catalogue/models/product.py", "# product\n");
    root.write(
        "packages/catalogue/services/product_service.py",
        "# service\n",
    );
    root.write("tests/catalogue/test_product.py", "# test\n");
    root.write("docs/architecture/catalogue/overview.md", "# doc\n");
    root.write("packages/auth/models/user.py", "# auth\n");
    root.write("services/payment/handler.py", "# payment\n");
    root.write("services/payment/utils.py", "# payment utils\n");
    root.write("services/billing/invoice.py", "# billing\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [packages, services]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );

    let spec = "---\nid: m-universal\ntitle: Universal Scope Tasks\ndoc_type: contract\nstatus: active\n---\n# Tasks\n```yaml\ntask_ref: catalogue_task\ntarget: Build catalogue service\nproof_policy: seam-test-first\nscope: [packages/catalogue/models/product.py]\nscope_roots:\n  - packages/catalogue/\n  - tests/catalogue/\n  - docs/architecture/catalogue/\n```\n```yaml\ntask_ref: payment_task\ntarget: Build payment handler\nproof_policy: seam-test-first\nscope: [services/payment/handler.py]\n```\n";
    root.write("docs/010-universal.md", spec);
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/010-universal.md", &root.0)
        .unwrap();

    let catalogue_id = "forge-mcp/forge-mcp/m-universal:catalogue_task";
    let payment_id = "forge-mcp/forge-mcp/m-universal:payment_task";

    // 1. Extending within declared scope_roots succeeds
    let ext1 = store
        .extend_scope(
            catalogue_id,
            &[
                "packages/catalogue/services/product_service.py".into(),
                "tests/catalogue/test_product.py".into(),
                "docs/architecture/catalogue/overview.md".into(),
            ],
            &root.0,
        )
        .unwrap();
    assert!(ext1
        .spec
        .scope
        .contains(&"packages/catalogue/services/product_service.py".into()));
    assert!(ext1
        .spec
        .scope
        .contains(&"tests/catalogue/test_product.py".into()));
    assert!(ext1
        .spec
        .scope
        .contains(&"docs/architecture/catalogue/overview.md".into()));

    // 2. Extending outside declared scope_roots fails (even for arbitrary paths or src/)
    let err_auth = store
        .extend_scope(
            catalogue_id,
            &["packages/auth/models/user.py".into()],
            &root.0,
        )
        .unwrap_err();
    assert!(
        err_auth.to_string().contains("TASK_SCOPE_INVALID"),
        "expected TASK_SCOPE_INVALID, got: {err_auth}"
    );

    let err_src = store
        .extend_scope(catalogue_id, &["src/anything.rs".into()], &root.0)
        .unwrap_err();
    assert!(
        err_src.to_string().contains("TASK_SCOPE_INVALID"),
        "expected TASK_SCOPE_INVALID for src outside scope_roots, got: {err_src}"
    );

    // 3. Fallback when scope_roots is omitted: auto-derives base from initial scope
    let ext_payment = store
        .extend_scope(payment_id, &["services/payment/utils.py".into()], &root.0)
        .unwrap();
    assert!(ext_payment
        .spec
        .scope
        .contains(&"services/payment/utils.py".into()));

    let err_billing = store
        .extend_scope(payment_id, &["services/billing/invoice.py".into()], &root.0)
        .unwrap_err();
    assert!(
        err_billing.to_string().contains("TASK_SCOPE_INVALID"),
        "expected TASK_SCOPE_INVALID for outside auto-derived base, got: {err_billing}"
    );

    // 4. Root-level file without scope_roots does not open repository tree
    root.write("install.sh", "#!/bin/sh\n");
    root.write("Cargo.toml", "[package]\n");
    let root_spec = "---\nid: m-root\ntitle: Root File Task\ndoc_type: contract\nstatus: active\n---\n# Tasks\n```yaml\ntask_ref: installer_task\ntarget: Deliver installer\nproof_policy: direct-proof\nscope: [install.sh]\n```\n";
    root.write("docs/010-root.md", root_spec);
    let root_milestone = Milestone::parse(root_spec, "forge-mcp").unwrap();
    store
        .sync(&root_milestone, "docs/010-root.md", &root.0)
        .unwrap();
    let installer_id = "forge-mcp/forge-mcp/m-root:installer_task";

    let err_root_cargo = store
        .extend_scope(installer_id, &["Cargo.toml".into()], &root.0)
        .unwrap_err();
    assert!(
        err_root_cargo.to_string().contains("TASK_SCOPE_INVALID"),
        "expected TASK_SCOPE_INVALID for Cargo.toml from root-level install.sh, got: {err_root_cargo}"
    );

    let err_root_src = store
        .extend_scope(installer_id, &["src/lib.rs".into()], &root.0)
        .unwrap_err();
    assert!(
        err_root_src.to_string().contains("TASK_SCOPE_INVALID"),
        "expected TASK_SCOPE_INVALID for src/lib.rs from root-level install.sh, got: {err_root_src}"
    );

    // 5. Empty scope_roots is omitted from serialization to preserve task digest compatibility
    let serialized_task = serde_json::to_string(&milestone.tasks[1]).unwrap();
    assert!(
        !serialized_task.contains("scope_roots"),
        "expected empty scope_roots to be omitted from serialized JSON, got: {serialized_task}"
    );
    let digest_without_field = milestone.digest(&milestone.tasks[1]).unwrap();
    let mut task_with_empty_vec = milestone.tasks[1].clone();
    task_with_empty_vec.scope_roots = Vec::new();
    let digest_with_empty_vec = milestone.digest(&task_with_empty_vec).unwrap();
    assert_eq!(
        digest_without_field, digest_with_empty_vec,
        "empty scope_roots must produce identical digest to omitted field"
    );

    // 6. Milestone::parse rejects tasks where initial scope does not fall into scope_roots
    let invalid_spec = "---\nid: m-inv\ntitle: Invalid\ndoc_type: contract\n---\n# Tasks\n```yaml\ntask_ref: bad\ntarget: Bad\nproof_policy: direct-proof\nscope: [packages/auth/user.py]\nscope_roots: [packages/catalogue/]\n```\n";
    let parse_err = Milestone::parse(invalid_spec, "forge-mcp").unwrap_err();
    assert!(
        parse_err.to_string().contains("TASK_SCOPE_INVALID"),
        "expected parse error for scope outside scope_roots, got: {parse_err}"
    );
}

#[test]
fn declarative_profile_compiles_defaults_and_respects_role_overrides() {
    use contextunity_forge_mcp::core::tasks::profile::{compiled, load_for_workspace, parse_profile};

    // 1. Embedded default profile has no hardcoded model and contains textual recommendations
    let default_profile = compiled();
    assert_eq!(default_profile.gates.len(), 4);
    assert_eq!(default_profile.gate_id(0), "contract");
    assert_eq!(default_profile.gate_id(1), "build");
    assert_eq!(default_profile.gate_id(2), "review");
    assert_eq!(default_profile.gate_id(3), "deliver");

    let reviewer_role = default_profile
        .roles
        .get("independent_reviewer")
        .expect("reviewer role exists");
    assert!(reviewer_role.model.is_none(), "default reviewer must not hardcode a model");
    assert!(reviewer_role.reasoning.is_none(), "default reviewer must not hardcode reasoning");
    assert!(reviewer_role.recommendation.is_some(), "default reviewer must provide textual recommendation");

    // 2. Strict deny_unknown_fields fails closed on unknown YAML keys
    let invalid_yaml = "unknown_field: true\ngates: []\n";
    let parse_err = parse_profile(invalid_yaml).unwrap_err();
    assert!(
        parse_err.to_string().contains("ACDD_PROFILE_SCHEMA_INVALID"),
        "expected ACDD_PROFILE_SCHEMA_INVALID error on unknown field, got: {parse_err}"
    );

    // 3. Repository override in .forge/acdd/profile.yaml sets sol-6.1 with reasoning high
    let ws = crate::common::Workspace::new();
    ws.write(
        ".forge/acdd/profile.yaml",
        "roles:\n  independent_reviewer:\n    model: sol-6.1\n    reasoning: high\n",
    );

    let loaded = load_for_workspace(ws.root()).unwrap();
    let loaded_reviewer = loaded.roles.get("independent_reviewer").unwrap();
    assert_eq!(loaded_reviewer.model.as_deref(), Some("sol-6.1"));
    assert_eq!(loaded_reviewer.reasoning.as_deref(), Some("high"));
    // Other gates and roles remain preserved from defaults
    assert_eq!(loaded.gates.len(), 4);
    assert!(loaded.roles.contains_key("builder"));

    // 4. Custom profile referenced by path in forge-mcp.yaml
    ws.write(
        "custom/team_profile.yaml",
        "roles:\n  independent_reviewer:\n    model: custom-model\n    reasoning: medium\n",
    );
    ws.write(
        "forge-mcp.yaml",
        "acdd_profile: custom/team_profile.yaml\n",
    );
    let loaded_custom = load_for_workspace(ws.root()).unwrap();
    let custom_reviewer = loaded_custom.roles.get("independent_reviewer").unwrap();
    assert_eq!(custom_reviewer.model.as_deref(), Some("custom-model"));
    assert_eq!(custom_reviewer.reasoning.as_deref(), Some("medium"));
}

