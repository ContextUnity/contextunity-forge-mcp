use super::*;

#[test]
fn claim_context_intersects_documentation_across_repository_paths() {
    let (root, store, milestone) = fixture(DEFAULT_SEAM);
    drop(store);
    root.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    TaskHarness::commit_handoff_fixture_inputs(&root, &[".forge/acdd/profile.yaml"]);
    root.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    root.write("forge-mcp.yaml", "roots: [src]\ndocs: [docs, README.md, AGENTS.md, TESTS.md, nested/docs]\ntasks_db: .forge/tasks.sqlite\n");
    let paths = [
        "README.md",
        "AGENTS.md",
        "TESTS.md",
        "docs/testing/guide.md",
        "docs/runbooks/guide.md",
    ];
    for path in paths.into_iter().chain([
        "docs/milestones/archive/old.md",
        "docs/plans/archive/old.md",
        "nested/docs/milestones/archive/old.md",
        "docs/milestones/010-active.md",
    ]) {
        // Resolve the same source claim from each document's directory.
        let depth = std::path::Path::new(path)
            .parent()
            .unwrap()
            .components()
            .count();
        root.write(path, &format!("# Task guidance\nLoad the contextunity-forge skill.\nInvariant: [`example`]({}src/lib.rs) stays callable.\n", "../".repeat(depth)));
    }
    root.build_to(root.path(".forge/code-map.sqlite"));
    let id = milestone.task_id(&milestone.tasks[0]);
    let claim =
        TaskHarness::claim_at(root.root(), &id, "contract", "documentation-author").unwrap();
    let bundle = &claim["context_bundle"];
    let documents = bundle["adrs"].as_array().unwrap();
    for path in paths {
        assert!(
            documents.iter().any(|document| document["path"] == path),
            "missing {path}: {documents:?}"
        );
    }
    assert!(!documents.iter().any(|document| {
        let p = document["path"].as_str().unwrap();
        p.contains("/archive/") || p.contains("docs/milestones/")
    }));
    let symbols = bundle["scope_symbols"].as_array().unwrap();
    let example = symbols
        .iter()
        .find(|symbol| symbol["name"] == "example")
        .unwrap();
    let documented_by = example["documented_by"].as_array().unwrap();
    assert!(documented_by
        .iter()
        .any(|path| path.as_str().unwrap().starts_with("README.md#")));
    assert!(!documented_by.iter().any(|path| {
        let p = path.as_str().unwrap();
        p.contains("/archive/") || p.contains("docs/milestones/")
    }));
}
#[test]
fn role_runner_templates_render_active_claim_context() {
    let (root, store, milestone) = fixture(DEFAULT_SEAM);
    drop(store);
    root.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    TaskHarness::commit_handoff_fixture_inputs(&root, &[".forge/acdd/profile.yaml"]);
    root.write(
        "AGENTS.md",
        "# Guidance\nLoad the contextunity-forge skill.\n",
    );
    root.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    root.write("forge-mcp.yaml", "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nacdd_profile: custom_profile.yaml\n");
    let id = milestone.task_id(&milestone.tasks[0]);
    for (mode, runner, prompt) in [
        (
            "subagent",
            Some("launch {task_id} {stage} {role} {worktree} {model} {reasoning}"),
            Some("{goal}|{role_guidance}|{task_id}|{stage}|{role}|{worktree}|{model}|{reasoning}"),
        ),
        ("inline", None, Some("{task_id}: {goal}")),
        ("subagent", Some("launch {task_id}"), None),
        ("subagent", None, None),
    ] {
        let mut definition = json!({"mode":mode,"models":[{"model":"test-model","reasoning":"high"}],"guidance":"Verify the admitted scope."});
        if let Some(runner) = runner {
            definition["runner"] = json!(runner);
        }
        if let Some(prompt) = prompt {
            definition["prompt_template"] = json!(prompt);
        }
        root.write(
            "custom_profile.yaml",
            &serde_yaml::to_string(&json!({"roles":{"contract_author":definition}})).unwrap(),
        );
        let profile =
            contextunity_forge_mcp::core::tasks::profile::load_for_workspace(root.root()).unwrap();
        let role = &profile.roles["contract_author"];
        assert_eq!(role.runner.as_deref(), runner);
        assert_eq!(role.prompt_template.as_deref(), prompt);
        let claim = TaskHarness::claim_at(root.root(), &id, "contract", "template-author").unwrap();
        let assembled = &claim["workflow_guidance"]["role_runner"];
        if runner.is_none() && prompt.is_none() {
            assert!(
                assembled.is_null(),
                "default role without runner or prompt_template must omit role_runner: {assembled}"
            );
            TaskHarness::manage_task(root.root(), &id, tasks::ManageAction::Reset).unwrap();
            continue;
        }
        assert_eq!(assembled["role"], "contract_author");
        assert_eq!(assembled["mode"], mode);
        if runner.is_some() {
            assert!(assembled["runner"].as_str().unwrap().contains(&id));
        } else {
            assert!(assembled["runner"].is_null());
        }
        let rendered = assembled["rendered_prompt"].as_str().unwrap();
        assert!(rendered.contains(&id), "{rendered}");
        assert!(!rendered.contains("{goal}"));
        if prompt.is_none() {
            assert!(rendered.contains("task_submit"));
        }
        if runner.is_some_and(|runner| runner.contains("{model}")) {
            assert!(assembled["runner"]
                .as_str()
                .unwrap()
                .ends_with("test-model high"));
            assert!(rendered.contains("Verify the admitted scope."));
        }
        TaskHarness::manage_task(root.root(), &id, tasks::ManageAction::Reset).unwrap();
    }
}
#[test]
fn task_guidance_follows_agent_metadata_workspace_config_and_active_stage() {
    let specification = |agent_type: &str| {
        format!(
        "---\nid: m-guided\ntitle: Guided tasks\ndoc_type: contract\n---\n# Guided tasks\n```yaml\ntask_ref: guided\ntarget: Deliver guidance\nagent_type: {agent_type}\nproof_policy: seam-test-first\nscope: [src/]\n```\n"
    )
    };
    let root = acdd_workspace(DEFAULT_SEAM, "");
    let linked = acdd_workspace(DEFAULT_SEAM, "");
    let missing = acdd_workspace(DEFAULT_SEAM, "");
    for (workspace, agent_type) in [
        (&root, "gpt-6-sol"),
        (&linked, "reviewer"),
        (&missing, "flash"),
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
            linked.root().display()
        ),
    );
    root.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    linked.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    TaskHarness::commit_handoff_fixture_inputs(&root, &[".forge/acdd/profile.yaml"]);
    TaskHarness::commit_handoff_fixture_inputs(&linked, &[".forge/acdd/profile.yaml"]);
    linked.write(
        "forge-mcp.yaml",
        "task_repository: linked\ntask_project: linked\n",
    );
    missing.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n");

    for (server, workspace, selector, repository, agent_type, expected_path, absent) in [
        (
            root.root(),
            root.root(),
            None,
            "forge-mcp",
            "gpt-6-sol",
            root.root().join("docs/team-guidance.md"),
            false,
        ),
        (
            root.root(),
            linked.root(),
            Some("linked"),
            "linked",
            "reviewer",
            linked.root().join("docs/linked-guidance.md"),
            false,
        ),
        (
            missing.root(),
            missing.root(),
            None,
            "forge-mcp",
            "flash",
            missing.root().join("AGENTS.md"),
            true,
        ),
    ] {
        let id = format!("{repository}/m-guided:guided");
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
        assert!(guidance["rejection"].is_null());
        assert_eq!(guidance["role"], "contract_author");
        assert!(guidance["role_guide"]
            .as_str()
            .unwrap()
            .contains("contract author"));
        assert!(guidance["steps"]
            .as_array()
            .is_some_and(|steps| !steps.is_empty()));
        if absent {
            assert_eq!(guidance["warning"]["code"], "TASK_GUIDANCE_MISSING");
        } else {
            assert!(guidance["warning"].is_null());
        }
        let claimed = TaskHarness::claim_in_worktree(
            server,
            &id,
            "contract",
            "guidance-contract-author",
            workspace,
            Some(&TaskHarness::session_for("guidance-contract-author")),
        )
        .unwrap();
        assert_eq!(
            claimed["workflow_guidance"]["active_stage"],
            guidance["active_stage"]
        );
        assert_eq!(claimed["workflow_guidance"]["role"], guidance["role"]);
        assert_eq!(claimed["workflow_guidance"]["steps"], guidance["steps"]);
        assert!(claimed["workflow_guidance"]["blackboard_messages"].is_array());
        if repository == "forge-mcp" && !absent {
            let mut store = tasks::store_for_task(root.root(), &id).unwrap();
            let task = store.inspect(&id).unwrap();
            store
                .submit(
                    &id,
                    "contract",
                    &TaskHarness::default_evidence(&task),
                    "pass",
                    None,
                    Some(&TaskHarness::session_for(
                        task.worker_id.as_deref().unwrap(),
                    )),
                )
                .unwrap();
            let build = inspect();
            assert_eq!(build["workflow_guidance"]["active_stage"], "build");
            assert_eq!(build["workflow_guidance"]["role"], "builder");
            assert_ne!(
                build["workflow_guidance"]["role_guide"],
                guidance["role_guide"]
            );
            let claimed_build = TaskHarness::claim_in_worktree(
                server,
                &id,
                "build",
                "guidance-builder",
                workspace,
                Some(&TaskHarness::session_for("guidance-builder")),
            )
            .unwrap();
            assert_eq!(claimed_build["workflow_guidance"]["role"], "builder");
            let task = store.inspect(&id).unwrap();
            store
                .submit(
                    &id,
                    "build",
                    &TaskHarness::default_evidence(&task),
                    "pass",
                    None,
                    Some(&TaskHarness::session_for(
                        task.worker_id.as_deref().unwrap(),
                    )),
                )
                .unwrap();
            let review = inspect();
            assert_eq!(review["workflow_guidance"]["active_stage"], "review");
            assert_eq!(review["workflow_guidance"]["role"], "independent_reviewer");
            assert_eq!(
                review["workflow_guidance"]["independent_from_worker_id"],
                "guidance-builder"
            );
            assert!(review["workflow_guidance"]["independence_rule"]
                .as_str()
                .is_some_and(|rule| rule.contains("different")));

            let claimed_review = TaskHarness::claim_in_worktree(
                server,
                &id,
                "review",
                "guidance-reviewer",
                workspace,
                Some(&TaskHarness::session_for("guidance-reviewer")),
            )
            .unwrap();
            let mut review_evidence = TaskHarness::default_evidence(&store.inspect(&id).unwrap());
            review_evidence.proof = review_proof("reject");
            let findings =
                json!({"decision":"remediate","summary":"preserve rejected review details"});
            store
                .submit(
                    &id,
                    "review",
                    &review_evidence,
                    "reject",
                    Some(&findings),
                    Some(&TaskHarness::session_for(&review_evidence.worker_id)),
                )
                .unwrap();
            let returned = inspect();
            assert_eq!(returned["stage"], "build");
            assert_eq!(
                returned["workflow_guidance"]["rejection"],
                json!({
                    "rejected_from":"review",
                    "worker_id":"guidance-builder",
                    "findings":findings,
                })
            );
            assert!(returned["workflow_guidance"]["steps"]
                .as_array()
                .is_some_and(|steps| steps.iter().any(|step| {
                    step.as_str()
                        == Some("Return task to builder 'guidance-builder' with review rejection findings to repair defects.")
                })));
            assert_eq!(
                claimed_review["workflow_guidance"]["active_stage"],
                "review"
            );
        }
    }

    let missing_id = "forge-mcp/m-guided:guided";
    missing.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nagents_guidance: docs/unavailable.md\n",
    );
    let configured_missing = tasks::manage(
        missing.root(),
        serde_json::from_value(json!({"action": "inspect", "task_id": missing_id})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        configured_missing["agents_guidance"],
        missing.root().join("docs/unavailable.md").to_str().unwrap()
    );
    assert_eq!(
        configured_missing["workflow_guidance"]["warning"]["code"],
        "TASK_GUIDANCE_MISSING"
    );
}
#[test]
fn task_guidance_requires_named_and_installed_forge_skill() {
    let specification = "---\nid: m-skill\ntitle: Skill guidance\ndoc_type: contract\n---\n# Skill guidance\n```yaml\ntask_ref: skill\ntarget: Deliver skill guidance\nagent_type: worker\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let root = acdd_workspace(DEFAULT_SEAM, "");
    root.write("src/lib.rs", "pub fn skill() {}\n");
    root.write("docs/010-skill.md", specification);
    root.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n");
    let skill_path = root
        .root()
        .join(".agents/skills/contextunity-forge/SKILL.md");
    let gitignore_path = root.root().join(".gitignore");
    let global_installed = std::env::var_os("HOME").is_some_and(|home| {
        std::path::Path::new(&home)
            .join(".agents/skills/contextunity-forge/SKILL.md")
            .is_file()
    });
    let canonical = CANONICAL_FORGE_GITIGNORE;
    let load = "# Rules\nLoad the contextunity-forge skill.\n";
    let git_protection =
        format!(".*\n*.json\n.forge/frameworks/nested/\n!.forge/scan.sqlite\n{canonical}");
    let git_unmatched = format!("{canonical}*.md\n");
    let git_yaml = format!("{canonical}*.yaml\n");
    let git_dot = format!("{canonical}.*\n");
    let git_tasks = format!("{canonical}!.forge/tasks.sqlite\n");
    let git_nested = format!("{canonical}.forge/frameworks/nested/\n");
    let git_json = format!("{canonical}*.json\n");
    let git_scan = format!("{canonical}!.forge/scan.sqlite\n");
    for (agents, gitignore, install_skill, warning_fragment) in [
        (load, Some(canonical), true, None),
        (
            "# Rules\nLoad contextunity-forge-mcp skill for ACDD.\n",
            Some(canonical),
            true,
            None,
        ),
        (
            "# Rules\nLoad [contextunity-forge-mcp](https://example.com/SKILL.md) skill for ACDD.\n",
            Some(canonical),
            true,
            None,
        ),
        (
            "# Intro\n\ncontextunity-forge skill.\n",
            Some(canonical),
            true,
            None,
        ),
        (load, Some(git_protection.as_str()), true, None),
        (
            load,
            Some(git_unmatched.as_str()),
            true,
            Some("Edit `.gitignore`"),
        ),
        (
            "# Rules\nUse contextunity-forge-mcp for queries.\n",
            Some(canonical),
            true,
            Some("do not name the contextunity-forge skill"),
        ),
        (
            "# Rules\nLoad xcontextunity-forge skill.\n",
            Some(canonical),
            true,
            Some("do not name the contextunity-forge skill"),
        ),
        (
            "# Rules\nLoad contextunity-forge-mcp-tools skill for ACDD.\n",
            Some(canonical),
            true,
            Some("do not name the contextunity-forge skill"),
        ),
        (
            "# Rules\nUse contextunity-forge-mcp for queries.\nThe worker skill is installed.\n",
            Some(canonical),
            true,
            Some("do not name the contextunity-forge skill"),
        ),
        (
            load,
            Some(canonical),
            false,
            (!global_installed).then_some("not installed"),
        ),
        (load, Some("target/\n.forge/\n"), true, Some("Edit `.gitignore`")),
        (
            load,
            Some("target/\n.forge/\n!.forge/*.yaml\n"),
            true,
            Some("Edit `.gitignore`"),
        ),
        (
            load,
            Some(git_yaml.as_str()),
            true,
            Some("Edit `.gitignore`"),
        ),
        (
            load,
            Some(git_dot.as_str()),
            true,
            Some("Edit `.gitignore`"),
        ),
        (
            load,
            Some(git_tasks.as_str()),
            true,
            Some("Edit `.gitignore`"),
        ),
        (
            load,
            Some(git_nested.as_str()),
            true,
            Some("Edit `.gitignore`"),
        ),
        (
            load,
            Some(git_json.as_str()),
            true,
            Some("Edit `.gitignore`"),
        ),
        (
            load,
            Some(git_scan.as_str()),
            true,
            Some("Edit `.gitignore`"),
        ),
        (load, Some("target/\n"), true, Some("Edit `.gitignore`")),
        (load, None, true, Some("Edit `.gitignore`")),
    ] {
        root.write("AGENTS.md", agents);
        if let Some(gitignore) = gitignore {
            root.write(".gitignore", gitignore);
        } else if gitignore_path.exists() {
            std::fs::remove_file(&gitignore_path).unwrap();
        }
        if install_skill {
            root.write(
                ".agents/skills/contextunity-forge/SKILL.md",
                "# ContextUnity Forge\n",
            );
        } else if skill_path.exists() {
            std::fs::remove_file(&skill_path).unwrap();
        }
        tasks::manage(
            root.root(),
            serde_json::from_value(json!({
                "action": "sync",
                "milestone_ref": "docs/010-skill.md"
            }))
            .unwrap(),
        )
        .unwrap();
        let inspected = tasks::manage(
            root.root(),
            serde_json::from_value(json!({
                "action": "inspect",
                "task_id": "forge-mcp/m-skill:skill"
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
