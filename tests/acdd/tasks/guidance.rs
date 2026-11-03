use super::*;

#[test]
fn claim_context_intersects_documentation_across_repository_paths() {
    let (root, store, milestone) = fixture();
    drop(store);
    root.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
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
    writer::build(&root.0, &root.0.join(".forge/code-map.sqlite"), None).unwrap();
    let id = milestone.task_id(&milestone.tasks[0]);
    let claim = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: id,
            stage: "contract".into(),
            worker_id: "documentation-author".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
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
        let (root, store, milestone) = fixture();
        drop(store);
        root.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
        root.write(
            "AGENTS.md",
            "# Guidance\nLoad the contextunity-forge skill.\n",
        );
        root.write(
            ".agents/skills/contextunity-forge/SKILL.md",
            "# ContextUnity Forge\n",
        );
        root.write("forge-mcp.yaml", "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nacdd_profile: custom_profile.yaml\n");
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
            contextunity_forge_mcp::core::tasks::profile::load_for_workspace(&root.0).unwrap();
        let role = &profile.roles["contract_author"];
        assert_eq!(role.runner.as_deref(), runner);
        assert_eq!(role.prompt_template.as_deref(), prompt);
        let id = milestone.task_id(&milestone.tasks[0]);
        let claim = tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: id.clone(),
                stage: "contract".into(),
                worker_id: "template-author".into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let assembled = &claim["workflow_guidance"]["role_runner"];
        if runner.is_none() && prompt.is_none() {
            assert!(
                assembled.is_null(),
                "default role without runner or prompt_template must omit role_runner: {assembled}"
            );
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
    }
}

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
        assert_eq!(
            claimed["workflow_guidance"]["active_stage"],
            guidance["active_stage"]
        );
        assert_eq!(claimed["workflow_guidance"]["role"], guidance["role"]);
        assert_eq!(claimed["workflow_guidance"]["steps"], guidance["steps"]);
        assert!(claimed["workflow_guidance"]["blackboard_messages"].is_array());
        if repository == "forge-mcp" && !absent {
            let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
            let task = store.inspect(&id).unwrap();
            store
                .submit(&id, "contract", &evidence(&task), "pass", None)
                .unwrap();
            let build = inspect();
            assert_eq!(build["workflow_guidance"]["active_stage"], "build");
            assert_eq!(build["workflow_guidance"]["role"], "builder");
            assert_ne!(
                build["workflow_guidance"]["role_guide"],
                guidance["role_guide"]
            );
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
            assert_eq!(claimed_build["workflow_guidance"]["role"], "builder");
            let task = store.inspect(&id).unwrap();
            store
                .submit(&id, "build", &evidence(&task), "pass", None)
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

            let claimed_review = tasks::claim(
                server,
                tasks::Claim {
                    task_id: id.clone(),
                    stage: "review".into(),
                    worker_id: "guidance-reviewer".into(),
                    worktree: workspace.to_string_lossy().into_owned(),
                    ..Default::default()
                },
            )
            .unwrap();
            let mut review_evidence = evidence(&store.inspect(&id).unwrap());
            review_evidence.proof = review_proof("reject");
            let findings =
                json!({"decision":"remediate","summary":"preserve rejected review details"});
            store
                .submit(&id, "review", &review_evidence, "reject", Some(&findings))
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

#[test]
fn claim_returns_bundle_by_default_with_task_and_milestone_blackboard_messages() {
    let (root, store, _) = fixture();
    let task_id = "forge-mcp/m-test:first";

    // Post message to task
    store
        .blackboard_post(task_id, "agent_1", "notes", "Initial hypothesis for task")
        .unwrap();

    // Post message from another task in same milestone
    let task_2_id = "forge-mcp/m-test:second";
    store
        .blackboard_post(task_2_id, "agent_2", "decisions", "Milestone level finding")
        .unwrap();

    let milestone_ref = store
        .milestone_scope_ref(&store.inspect(task_id).unwrap().milestone_ref)
        .unwrap();
    store
        .blackboard_post_scoped(
            &milestone_ref,
            None,
            None,
            "architect",
            "draft",
            "RFC: share the task claim breadcrumb with the receiving worker",
        )
        .unwrap();
    store
        .blackboard_post_scoped_with_recipient(
            contextunity_forge_mcp::db::tasks_store::BlackboardPostInput {
                milestone_ref: &milestone_ref,
                task_id: Some(task_2_id),
                subtask_ref: None,
                gate: None,
                author: "architect",
                topic: "draft",
                payload: "RFC addressed to the first task",
            },
            Some(task_id),
        )
        .unwrap();
    store
        .blackboard_post_scoped_with_recipient(
            contextunity_forge_mcp::db::tasks_store::BlackboardPostInput {
                milestone_ref: &milestone_ref,
                task_id: Some(task_2_id),
                subtask_ref: None,
                gate: None,
                author: "architect",
                topic: "draft",
                payload: "RFC addressed to the second task",
            },
            Some(task_2_id),
        )
        .unwrap();

    // The persisted task stage supplies the claim gate when stage is omitted.
    let claim_res = tasks::claim(
        &root.0,
        serde_json::from_value(json!({
            "task_id": task_id,
            "worker_id": "builder",
            "worktree": root.0
        }))
        .unwrap(),
    )
    .unwrap();

    // Response should be the full bundle by default
    assert_eq!(claim_res["task_id"], task_id);
    assert_eq!(claim_res["context_bundle"]["contract"]["task_id"], task_id);
    assert!(
        claim_res["context_bundle"].get("proof_schema").is_none(),
        "built-in proof gates must not expose a scheme proof schema"
    );
    let bb = claim_res["blackboard"]
        .as_array()
        .expect("blackboard array in bundle");
    assert_eq!(
        bb.len(),
        4,
        "the claim sees broadcast and explicitly addressed messages"
    );
    assert!(claim_res["workflow_guidance"].is_object());
    assert_eq!(
        claim_res["breadcrumb"],
        "forge-mcp/forge-mcp/m-test/tasks/first/contract"
    );
    assert_eq!(
        claim_res["workflow_guidance"]["breadcrumb"],
        claim_res["breadcrumb"]
    );
    assert_eq!(
        claim_res["workflow_guidance"]["gate_cursor"]["gate_path"],
        "tasks"
    );
    assert_eq!(
        claim_res["workflow_guidance"]["gate_cursor"]["stage_id"],
        "contract"
    );
    let guidance_messages = claim_res["workflow_guidance"]["blackboard_messages"]
        .as_array()
        .unwrap();
    assert_eq!(guidance_messages.len(), 2);
    assert!(guidance_messages.iter().any(|message| {
        message["payload"]
            .as_str()
            .is_some_and(|payload| payload.contains("RFC: share the task claim breadcrumb"))
    }));
    assert!(guidance_messages.iter().any(|message| {
        message["recipient_task_id"] == task_id
            && message["payload"] == "RFC addressed to the first task"
    }));
    assert!(!guidance_messages
        .iter()
        .any(|message| { message["payload"] == "RFC addressed to the second task" }));

    let checkpoint = store
        .get_checkpoint(claim_res["breadcrumb"].as_str().unwrap())
        .unwrap()
        .expect("claim checkpoint persisted");
    assert_eq!(checkpoint.breadcrumb, claim_res["breadcrumb"]);
    assert_eq!(checkpoint.state_payload["task_id"], task_id);
    assert_eq!(checkpoint.state_payload["stage"], "contract");
}

#[test]
fn task_blackboard_gate_targeting_and_schema_hint() {
    let (root, store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);

    let schema_version: String = store
        .connection
        .query_row(
            "SELECT value FROM task_store_metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(schema_version, "7");
    let gate_column_exists: bool = store
        .connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_blackboard') WHERE name='gate')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(gate_column_exists, "current schema stores gate targeting");
    let gate_indexes: i64 = store
        .connection
        .query_row(
            "SELECT count(*) FROM sqlite_master
             WHERE type='index' AND tbl_name='task_blackboard' AND sql LIKE '%gate%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(gate_indexes > 0, "gate-targeted lookups need an index");
    drop(store);

    let call = |request| {
        tasks::blackboard(
            &root.0,
            serde_json::from_value(request).unwrap(),
            "blackboard-test",
        )
    };
    let contract_post = call(json!({
        "action": "post",
        "task_id": task_id,
        "gate": "contract",
        "topic": "decisions",
        "payload": "Choose the public gate routing shape"
    }))
    .unwrap();
    let contract_id = contract_post["id"].as_u64().unwrap();
    let build_post = call(json!({
        "action": "post",
        "task_id": task_id,
        "gate": "build",
        "topic": "deferred",
        "payload": "Retain the build follow-up"
    }))
    .unwrap();
    let build_id = build_post["id"].as_u64().unwrap();

    let contract_page = call(json!({
        "action": "read",
        "task_id": task_id,
        "gate": "contract",
        "limit": 50
    }))
    .unwrap();
    let contract_messages = contract_page["messages"].as_array().unwrap();
    assert_eq!(contract_messages.len(), 1);
    assert_eq!(contract_messages[0]["id"], contract_id);
    assert_eq!(contract_messages[0]["gate"], "contract");

    let invalid_topic = call(json!({
        "action": "post",
        "task_id": task_id,
        "gate": "contract",
        "topic": "architectural_notes",
        "payload": "This legacy topic must be rejected at runtime"
    }))
    .unwrap_err();
    let schema_hint = format!("{invalid_topic:#}");
    for expected in [
        "draft",
        "notes",
        "findings",
        "blockers",
        "decisions",
        "deferred",
        "contract",
        "build",
        "review",
        "deliver",
        "example",
    ] {
        assert!(
            schema_hint.contains(expected),
            "actionable blackboard schema hint omitted {expected:?}: {schema_hint}"
        );
    }
    let invalid_gate = call(json!({
        "action": "read",
        "task_id": task_id,
        "gate": "unknown-gate"
    }))
    .unwrap_err();
    let gate_hint = format!("{invalid_gate:#}");
    for expected in [
        "unknown-gate",
        "contract",
        "build",
        "review",
        "deliver",
        "example",
    ] {
        assert!(
            gate_hint.contains(expected),
            "actionable blackboard gate hint omitted {expected:?}: {gate_hint}"
        );
    }

    let claim = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.clone(),
            stage: "contract".into(),
            worker_id: "blackboard-claim-test".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    let claim_messages = claim["workflow_guidance"]["blackboard_messages"]
        .as_array()
        .expect("claim guidance carries the active gate's blackboard messages");
    assert_eq!(claim_messages.len(), 1);
    assert_eq!(claim_messages[0]["id"], contract_id);
    assert_eq!(claim_messages[0]["gate"], "contract");
    let blackboard_info = claim["blackboard_info"]
        .as_object()
        .expect("claim returns a compact blackboard_info summary");
    assert!(
        serde_json::to_vec(blackboard_info).unwrap().len() <= 1024,
        "blackboard_info should remain compact"
    );
    assert_ne!(contract_id, build_id);
}

#[test]
fn task_context_preserves_cross_gate_decisions_findings_and_blockers() {
    let (root, store, milestone) = fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    drop(store);

    let mut persistent_ids = Vec::new();
    for topic in ["decisions", "findings", "blockers", "notes"] {
        let posted = tasks::blackboard(
            &root.0,
            serde_json::from_value(json!({
                "action": "post",
                "task_id": task_id,
                "gate": "review",
                "topic": topic,
                "payload": format!("Cross-gate {topic}")
            }))
            .unwrap(),
            "cross-gate-test",
        )
        .unwrap();
        if topic != "notes" {
            persistent_ids.push(posted["id"].as_u64().unwrap());
        }
    }

    let claim = tasks::claim(
        &root.0,
        tasks::Claim {
            task_id,
            stage: "contract".into(),
            worker_id: "cross-gate-test".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    for board in [&claim["blackboard"], &claim["context_bundle"]["blackboard"]] {
        let messages = board
            .as_array()
            .expect("context carries blackboard messages");
        assert_eq!(messages.len(), persistent_ids.len());
        for id in &persistent_ids {
            assert!(messages.iter().any(|message| {
                message["id"].as_u64() == Some(*id) && message["gate"] == "review"
            }));
        }
    }
}

#[test]
fn declarative_profile_compiles_defaults_and_respects_role_overrides() {
    use contextunity_forge_mcp::core::tasks::profile::{
        compiled, load_for_workspace, parse_profile, validate_profile_document, RoleDef,
        RoleExecutionMode,
    };

    let role_defaults = RoleDef::default();
    assert_eq!(role_defaults.mode, None);
    assert_eq!(role_defaults.reuse_on_reject, None);
    assert_eq!(role_defaults.effective_mode(), RoleExecutionMode::Subagent);
    assert!(role_defaults.effective_reuse_on_reject());

    // 1. Embedded roles define the default execution and rejection policies without
    //    duplicating ModelSpec's model and reasoning fields at the role level.
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
    let reviewer_value = serde_json::to_value(reviewer_role).unwrap();
    assert_eq!(reviewer_value["mode"], serde_json::json!("subagent"));
    assert_eq!(reviewer_value["reuse_on_reject"], serde_json::json!(true));
    assert_eq!(
        reviewer_value["models"],
        serde_json::json!([{"model":"sol-6.1","reasoning":"xhigh"}])
    );
    assert!(
        reviewer_value.get("model").is_none(),
        "model belongs to ModelSpec"
    );
    assert!(
        reviewer_value.get("reasoning").is_none(),
        "reasoning belongs to ModelSpec"
    );
    assert!(
        reviewer_role.guidance.is_some(),
        "default reviewer must provide textual guidance"
    );

    // 2. Strict deny_unknown_fields fails closed on unknown YAML keys
    let invalid_yaml = "unknown_field: true\ngates: []\n";
    let parse_err = parse_profile(invalid_yaml).unwrap_err();
    assert!(
        parse_err
            .to_string()
            .contains("ACDD_PROFILE_SCHEMA_INVALID"),
        "expected ACDD_PROFILE_SCHEMA_INVALID error on unknown field, got: {parse_err}"
    );

    // 3. Repository override replaces mode, rejection policy, and ordered models.
    let ws = crate::common::Workspace::new();
    ws.write(
        ".forge/acdd/profile.yaml",
        "roles:\n  independent_reviewer:\n    mode: inline\n    reuse_on_reject: false\n    models:\n      - model: sol-6.1\n        reasoning: high\n      - model: sol-6.0\n        reasoning: medium\n",
    );

    let loaded = load_for_workspace(ws.root()).unwrap();
    let loaded_reviewer = loaded.roles.get("independent_reviewer").unwrap();
    let loaded_reviewer_value = serde_json::to_value(loaded_reviewer).unwrap();
    assert_eq!(loaded_reviewer_value["mode"], serde_json::json!("inline"));
    assert_eq!(
        loaded_reviewer_value["reuse_on_reject"],
        serde_json::json!(false)
    );
    assert_eq!(
        loaded_reviewer_value["models"][0]["model"],
        serde_json::json!("sol-6.1")
    );
    assert_eq!(
        loaded_reviewer_value["models"][0]["reasoning"],
        serde_json::json!("high")
    );
    assert_eq!(
        loaded_reviewer_value["models"][1]["model"],
        serde_json::json!("sol-6.0")
    );
    assert_eq!(
        loaded_reviewer_value["models"][1]["reasoning"],
        serde_json::json!("medium")
    );
    assert!(loaded_reviewer_value.get("model").is_none());
    assert!(loaded_reviewer_value.get("reasoning").is_none());
    assert!(
        loaded_reviewer.guidance.is_some(),
        "the base guidance is inherited"
    );
    // Other gates and roles remain preserved from defaults
    assert_eq!(loaded.gates.len(), 4);
    assert!(loaded.roles.contains_key("builder"));

    // 4. Omitted role overrides inherit the base defaults and a model list replaces as a unit.
    ws.write(
        "custom/team_profile.yaml",
        "roles:\n  independent_reviewer:\n    models:\n      - model: custom-model\n        reasoning: medium\n",
    );
    ws.write("forge-mcp.yaml", "acdd_profile: custom/team_profile.yaml\n");
    let loaded_custom = load_for_workspace(ws.root()).unwrap();
    let custom_reviewer = loaded_custom.roles.get("independent_reviewer").unwrap();
    let custom_reviewer_value = serde_json::to_value(custom_reviewer).unwrap();
    assert_eq!(custom_reviewer_value["mode"], serde_json::json!("subagent"));
    assert_eq!(
        custom_reviewer_value["reuse_on_reject"],
        serde_json::json!(true)
    );
    assert_eq!(custom_reviewer_value["models"].as_array().unwrap().len(), 1);
    assert_eq!(
        custom_reviewer_value["models"][0]["model"],
        serde_json::json!("custom-model")
    );
    assert_eq!(
        custom_reviewer_value["models"][0]["reasoning"],
        serde_json::json!("medium")
    );
    assert!(custom_reviewer_value.get("model").is_none());
    assert!(custom_reviewer_value.get("reasoning").is_none());

    // Direct field-wise merge keeps base fields when an override omits them or
    // supplies an empty model list, then replaces the ordered models as a unit.
    let mut base = parse_profile(
        "roles:\n  builder:\n    mode: inline\n    reuse_on_reject: false\n    models:\n      - model: base-model\n        reasoning: high\n    guidance: base guidance\n",
    )
    .unwrap();
    base.merge_override(
        parse_profile(
            "roles:\n  builder:\n    mode: null\n    reuse_on_reject: null\n    models: []\n",
        )
        .unwrap(),
    );
    let inherited = serde_json::to_value(base.roles.get("builder").unwrap()).unwrap();
    assert_eq!(inherited["mode"], serde_json::json!("inline"));
    assert_eq!(inherited["reuse_on_reject"], serde_json::json!(false));
    assert_eq!(
        inherited["models"][0]["model"],
        serde_json::json!("base-model")
    );
    assert_eq!(inherited["guidance"], serde_json::json!("base guidance"));
    base.merge_override(parse_profile(
        "roles:\n  builder:\n    mode: subagent\n    reuse_on_reject: true\n    models:\n      - model: replacement-model\n        reasoning: medium\n",
    )
    .unwrap());
    let replaced = serde_json::to_value(base.roles.get("builder").unwrap()).unwrap();
    assert_eq!(replaced["mode"], serde_json::json!("subagent"));
    assert_eq!(replaced["reuse_on_reject"], serde_json::json!(true));
    assert_eq!(replaced["models"].as_array().unwrap().len(), 1);
    assert_eq!(
        replaced["models"][0]["model"],
        serde_json::json!("replacement-model")
    );
    assert_eq!(replaced["guidance"], serde_json::json!("base guidance"));

    // String model notation parses model:reasoning and bare model strings
    let string_models = parse_profile(
        "roles:\n  builder:\n    models:\n      - 'gpt6-luna:xhigh'\n      - 'gemini-3.8:high'\n      - 'composer-2.5'\n",
    )
    .unwrap();
    let builder_models = &string_models.roles.get("builder").unwrap().models;
    assert_eq!(builder_models.len(), 3);
    assert_eq!(builder_models[0].model, "gpt6-luna");
    assert_eq!(builder_models[0].reasoning.as_deref(), Some("xhigh"));
    assert_eq!(builder_models[0].formatted(), "gpt6-luna:xhigh");
    assert_eq!(builder_models[1].formatted(), "gemini-3.8:high");
    assert_eq!(builder_models[2].formatted(), "composer-2.5");

    // Invalid string models with empty segments fail closed
    assert!(parse_profile("roles:\n  builder:\n    models:\n      - 'model:'\n").is_err());
    assert!(parse_profile("roles:\n  builder:\n    models:\n      - ':high'\n").is_err());
    assert!(parse_profile("roles:\n  builder:\n    models:\n      - ''\n").is_err());
    // Map models with unknown fields fail closed
    assert!(parse_profile(
        "roles:\n  builder:\n    models:\n      - model: foo\n        unknown_field: bar\n"
    )
    .is_err());

    // The documented reference profile example loads and validates cleanly
    let example_profile_text =
        std::fs::read_to_string("skills/contextunity-forge/references/acdd_profile.yaml.example")
            .expect("example profile file must exist");
    let parsed_example = parse_profile(&example_profile_text)
        .expect("example profile must parse and validate without error");
    assert!(!parsed_example.roles.is_empty());
    // exactly one terminal delivery gate instead of accepting an incomplete workflow.
    let invalid_workflow = crate::common::Workspace::new();
    invalid_workflow.write(
        ".forge/acdd/profile.yaml",
        "gates:\n  - id: inspect\n    role: builder\n    proof: none\n",
    );
    let invalid_profile = load_for_workspace(invalid_workflow.root());
    assert!(
        invalid_profile.is_err(),
        "a merged profile without exactly one terminal delivery gate must fail closed"
    );

    let valid_gates = r#"gates:
  - id: work
    role: builder
    proof: none
  - id: review
    role: independent_reviewer
    proof: review
    reject_to: work
    independent_from: work
    contours: guidance.contours
  - id: deliver
    role: independent_reviewer
    proof: delivery
    receipt_review: review
    review_sources: [review]
    reject_to: work
"#;
    let later_gate = "  - id: later\n    role: builder\n    proof: none\n";
    let assert_profile_error = |case: &str, yaml: &str, expected: &str| {
        let workspace = crate::common::Workspace::new();
        workspace.write(".forge/acdd/profile.yaml", yaml);
        let error = match load_for_workspace(workspace.root()) {
            Ok(_) => panic!("{case} should fail merged profile compilation"),
            Err(error) => error,
        };
        let detail = format!("{error:#}");
        assert!(
            detail.contains(expected),
            "{case} expected diagnostic {expected:?}, got: {detail}"
        );
    };
    let invalid_token = valid_gates
        .replacen("id: work", "id: work/v1", 1)
        .replacen("reject_to: work", "reject_to: work/v1", 2)
        .replacen("independent_from: work", "independent_from: work/v1", 1);
    assert_profile_error(
        "invalid gate token",
        &invalid_token,
        "gate id 'work/v1' must be a non-empty token",
    );
    let gates_without_receipt_review = valid_gates.replace("    receipt_review: review\n", "");
    let missing_review_source = gates_without_receipt_review.replacen(
        "review_sources: [review]",
        "review_sources: [absent]",
        1,
    );
    assert_profile_error(
        "missing review source",
        &missing_review_source,
        "review_sources must reference strictly prior review gates",
    );
    let non_review_source = gates_without_receipt_review.replacen(
        "review_sources: [review]",
        "review_sources: [work]",
        1,
    );
    assert_profile_error(
        "non-review review source",
        &non_review_source,
        "review source 'work' is not a review gate",
    );
    let cases = [
        (
            "duplicate gate id",
            valid_gates.replacen(
                "  - id: review\n    role: independent_reviewer\n    proof: review",
                "  - id: work\n    role: builder\n    proof: none\n  - id: review\n    role: independent_reviewer\n    proof: review",
                1,
            ),
        ),
        (
            "forward reject_to",
            valid_gates
                .replacen("reject_to: work", "reject_to: later", 1)
                .replacen("  - id: deliver", &format!("{later_gate}  - id: deliver"), 1),
        ),
        (
            "missing reject_to",
            valid_gates.replacen("reject_to: work", "reject_to: absent", 1),
        ),
        (
            "forward independent_from",
            valid_gates
                .replacen("independent_from: work", "independent_from: later", 1)
                .replacen("  - id: deliver", &format!("{later_gate}  - id: deliver"), 1),
        ),
        (
            "missing independent_from",
            valid_gates.replacen("independent_from: work", "independent_from: absent", 1),
        ),
        (
            "missing receipt review",
            valid_gates.replacen("receipt_review: review", "receipt_review: absent", 1),
        ),
        (
            "non-review receipt review",
            valid_gates.replacen("receipt_review: review", "receipt_review: work", 1),
        ),
        (
            "undefined role",
            valid_gates.replacen("role: builder", "role: missing_role", 1),
        ),
        (
            "undefined contour set",
            valid_gates.replacen("contours: guidance.contours", "contours: missing_contours", 1),
        ),
        (
            "empty command value",
            format!("commands:\n  blank: '  '\n{valid_gates}"),
        ),
        (
            "empty command registry name",
            format!("commands:\n  '': echo\n{valid_gates}"),
        ),
        (
            "array schema without items",
            valid_gates.replacen(
                "    proof: none",
                "    proof:\n      scheme:\n        type: array",
                1,
            ),
        ),
        (
            "schema required property is undefined",
            valid_gates.replacen(
                "    proof: none",
                "    proof:\n      scheme:\n        type: object\n        required: [missing]\n        properties:\n          nested:\n            type: string",
                1,
            ),
        ),
        (
            "invalid nested schema type",
            valid_gates.replacen(
                "    proof: none",
                "    proof:\n      scheme:\n        type: object\n        properties:\n          nested:\n            type: unsupported",
                1,
            ),
        ),
    ];
    for (case, yaml) in cases {
        let workspace = crate::common::Workspace::new();
        workspace.write(".forge/acdd/profile.yaml", &yaml);
        let error = match load_for_workspace(workspace.root()) {
            Ok(_) => panic!("{case} should fail merged profile compilation"),
            Err(error) => error,
        };
        let detail = format!("{error:#}");
        assert!(
            detail.contains("ACDD_PROFILE_INVALID")
                || detail.contains("ACDD_PROFILE_SCHEMA_INVALID")
                || detail.contains("GATE_INVALID_CONFIGURATION"),
            "{case} failed for an unexpected reason: {detail}"
        );
    }

    let empty_registry = parse_profile(
        "roles:\n  builder: {}\n  independent_reviewer: {}\ncommands: {}\ngates:\n  - id: work\n    role: builder\n    proof: command\n  - id: review\n    role: independent_reviewer\n    proof: review\n  - id: deliver\n    role: independent_reviewer\n    proof: delivery\n    review_sources: [review]\n",
    )
    .unwrap();
    let error = empty_registry.validate().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("requires a non-empty commands registry"),
        "unexpected empty-registry validation error: {error:#}"
    );

    let compiled_empty_catalog = validate_profile_document(
        "commands: {}\ngates:\n  - id: build\n    role: builder\n    proof: none\n  - id: review\n    role: independent_reviewer\n    proof: review\n    reject_to: build\n    independent_from: build\n    contours: guidance.contours\n  - id: deliver\n    role: independent_reviewer\n    proof: delivery\n    review_sources: [review]\n",
    )
    .expect("an explicitly empty catalog should compile when no gate uses a command");
    assert!(
        compiled_empty_catalog.commands.is_empty(),
        "an explicit empty command catalog must clear embedded commands"
    );

    let clean_room_yaml = r#"
inherit: none
version: 2
roles:
  worker: {}
commands: {}
gates:
  - id: build
    role: worker
    proof: none
  - id: review
    role: worker
    proof: review
    reject_to: build
    independent_from: build
  - id: deliver
    role: worker
    proof: delivery
    receipt_review: review
    review_sources: [review]
    reject_to: build
"#;
    let clean_room_workspace = crate::common::Workspace::new();
    clean_room_workspace.write(".forge/acdd/profile.yaml", clean_room_yaml);

    let clean_room = load_for_workspace(clean_room_workspace.root());
    assert!(
        clean_room.is_ok(),
        "inherit: none should compile an isolated profile with an empty commands catalog: {clean_room:?}"
    );
    let clean_room = clean_room.unwrap();
    assert_eq!(
        clean_room
            .roles
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["worker"]
    );
    assert!(clean_room.commands.is_empty());
    assert!(clean_room.proofs.is_empty());
    assert!(clean_room.guidance.is_empty());
    assert!(clean_room.policies.is_empty());
    assert!(clean_room.contour_set(None).is_none());
    assert!(clean_room.gate_templates.is_empty());

    let compiled_clean_room = validate_profile_document(clean_room_yaml)
        .expect("the production profile compiler should honor inherit: none");
    assert!(compiled_clean_room.commands.is_empty());
    assert!(!compiled_clean_room.roles.contains_key("contract_author"));
}

#[test]
fn claim_context_exposes_recursive_scheme_schema_from_custom_profile() {
    let workspace = Workspace::new();
    workspace.write("src/lib.rs", "pub fn schema_gate() {}\n");
    workspace.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nacdd_profile: custom/profile.yaml\n",
    );
    workspace.write(".gitignore", CANONICAL_FORGE_GITIGNORE);
    workspace.write(
        "AGENTS.md",
        "# Task instructions\nLoad the contextunity-forge skill.\n",
    );
    workspace.write(
        ".agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    workspace.write(
        "custom/profile.yaml",
        r#"
gates:
  - id: schema_input
    role: builder
    proof:
      scheme:
        type: object
        required: [report]
        additionalProperties: false
        properties:
          report:
            type: object
            required: [entries]
            additionalProperties: false
            properties:
              entries:
                type: array
                items:
                  type: object
                  required: [name]
                  additionalProperties: false
                  properties:
                    name:
                      type: string
                      enum: [accepted, rejected]
  - id: review
    role: independent_reviewer
    proof: review
    independent_from: schema_input
    reject_to: schema_input
    contours: guidance.contours
  - id: deliver
    role: independent_reviewer
    proof: delivery
    receipt_review: review
    review_sources: [review]
    reject_to: schema_input
    auto_commit: false
"#,
    );
    workspace.write(
        "docs/010-scheme.md",
        "---\nid: m-scheme\ntitle: Scheme proof\ndoc_type: contract\nstatus: active\ninvariants: [isolated]\n---\n# Scheme proof\n```yaml\ntask_ref: schema\ntarget: Deliver schema evidence\nproof_policy: seam-test-first\nscope: [src/]\n```\n",
    );

    tasks::manage(
        workspace.root(),
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/010-scheme.md"
        }))
        .unwrap(),
    )
    .unwrap();

    let task_id = "forge-mcp/m-scheme:schema";
    let claim = tasks::claim(
        workspace.root(),
        tasks::Claim {
            task_id: task_id.into(),
            stage: "schema_input".into(),
            worker_id: "schema-author".into(),
            worktree: workspace.root().to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        claim["context_bundle"]["proof_schema"],
        json!({
            "type": "object",
            "properties": {
                "report": {
                    "type": "object",
                    "properties": {
                        "entries": {
                            "type": "array",
                            "properties": {},
                            "items": {
                                "type": "object",
                                "properties": {
                                    "name": {
                                        "type": "string",
                                        "properties": {},
                                        "required": [],
                                        "additionalProperties": null,
                                        "items": null,
                                "enum": ["accepted", "rejected"]
                                    }
                                },
                                "required": ["name"],
                                "additionalProperties": false,
                                "items": null,
                                "enum": []
                            },
                            "required": [],
                            "additionalProperties": null,
                            "enum": []
                        }
                    },
                    "required": ["entries"],
                    "additionalProperties": false,
                    "items": null,
                    "enum": []
                }
            },
            "required": ["report"],
            "additionalProperties": false,
            "items": null,
            "enum": []
        })
    );

    let properties = claim["context_bundle"]["proof_schema"]
        .pointer("/properties/report/properties/entries/items/additionalProperties")
        .expect("array item schema preserves the forbidden-property contract");
    assert_eq!(properties, false);

    let task: contextunity_forge_mcp::db::tasks_store::Task = {
        let store = tasks::store(workspace.root()).unwrap();
        let descriptor: String = store
            .connection
            .query_row(
                "SELECT descriptor FROM tasks WHERE task_id=?1",
                [task_id],
                |row| row.get(0),
            )
            .unwrap();
        serde_json::from_str(&descriptor).unwrap()
    };
    let before_state = || {
        let store = tasks::store(workspace.root()).unwrap();
        let descriptor: String = store
            .connection
            .query_row(
                "SELECT descriptor FROM tasks WHERE task_id=?1",
                [task_id],
                |row| row.get(0),
            )
            .unwrap();
        let current: contextunity_forge_mcp::db::tasks_store::Task =
            serde_json::from_str(&descriptor).unwrap();
        let gates = store
            .connection
            .prepare("SELECT gate,revision,state,evidence FROM task_gates WHERE task_id=?1 ORDER BY gate,revision")
            .unwrap()
            .query_map([task_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let claims = store
            .connection
            .prepare("SELECT revision,ended FROM task_claims WHERE task_id=?1 ORDER BY revision")
            .unwrap()
            .query_map([task_id], |row| {
                Ok((row.get::<_, u64>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let submissions: i64 = store
            .connection
            .query_row(
                "SELECT count(*) FROM task_submissions WHERE task_id=?1",
                [task_id],
                |row| row.get(0),
            )
            .unwrap();
        (
            current.status,
            current.stage,
            current.claim_revision,
            current.worker_id,
            gates,
            claims,
            submissions,
        )
    };
    assert_eq!(task.stage, "schema_input");
    let unchanged = before_state();
    let invalid_proofs = [
        ("nested required field missing", json!({"report":{}})),
        (
            "wrong nested type",
            json!({"report":{"entries":"not-an-array"}}),
        ),
        (
            "enum value rejected",
            json!({"report":{"entries":[{"name":"unknown"}]}}),
        ),
        (
            "array element type rejected",
            json!({"report":{"entries":[42]}}),
        ),
        (
            "forbidden nested property rejected",
            json!({"report":{"entries":[],"extra":true}}),
        ),
    ];
    for (case, proof) in invalid_proofs {
        let mut invalid_evidence = evidence(&task);
        invalid_evidence.proof = json!({"scheme_proof":proof});
        let error = tasks::submit(
            workspace.root(),
            serde_json::from_value(json!({
                "task_id": task_id,
                "stage": "schema_input",
                "action": "pass",
                "evidence": invalid_evidence
            }))
            .unwrap(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("TASK_EVIDENCE_INVALID"),
            "{case} produced an unexpected error: {error:#}"
        );
        assert_eq!(
            before_state(),
            unchanged,
            "{case} mutated task/gate/claim state"
        );
    }

    let mut accepted_evidence = evidence(&task);
    accepted_evidence.proof = json!({
        "scheme_proof":{"report":{"entries":[{"name":"accepted"}]}}
    });
    let accepted = tasks::submit(
        workspace.root(),
        serde_json::from_value(json!({
            "task_id": task_id,
            "stage": "schema_input",
            "action": "pass",
            "evidence": accepted_evidence
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(accepted["stage"], "review");
    let submitted = before_state();
    assert_eq!(submitted.0, "ready");
    assert_eq!(submitted.1, "review");
    assert_eq!(submitted.6, unchanged.6 + 1);
    let stored_gate: (String, Option<String>) = tasks::store(workspace.root())
        .unwrap()
        .connection
        .query_row(
            "SELECT state,evidence FROM task_gates WHERE task_id=?1 AND gate='schema_input' ORDER BY revision DESC LIMIT 1",
            [task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored_gate.0, "passed");
    let stored_evidence: Value = serde_json::from_str(stored_gate.1.as_deref().unwrap()).unwrap();
    assert_eq!(
        stored_evidence["proof"]["scheme_proof"]["report"]["entries"][0]["name"],
        "accepted"
    );
}

#[test]
fn inline_role_mode_adapts_guidance_text() {
    let root = ScopedWorkspace::new("forge_inline_role_mode");
    root.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nagents_guidance: docs/AGENTS.md\nacdd_profile: custom_profile.yaml\n",
    );
    root.write(
        "docs/AGENTS.md",
        "# Agent Guidance\nUse ContextUnity Forge.\n",
    );
    root.write("src/lib.rs", "pub fn work() -> bool { true }\n");

    let profile_yaml = r#"
gates:
  - id: contract
    role: contract_author
    proof: contract
  - id: build
    role: builder
    proof: command
  - id: review
    role: independent_reviewer
    proof: review
    reject_to: build
    independent_from: build
    contours: guidance.contours
  - id: deliver
    role: independent_reviewer
    proof: delivery
    review_sources: [review]
    reject_to: build
roles:
  independent_reviewer:
    mode: inline
    reuse_on_reject: true
    guidance: "Spawn a dedicated reviewer subagent to review candidate diff across contours."
"#;
    root.write("custom_profile.yaml", profile_yaml);

    let milestone_content = r#"---
id: m-inline-role
title: Inline role test
doc_type: contract
status: active
---
# Inline role test

```yaml
task_ref: inline-task
target: Verify inline role guidance
proof_policy: seam-test-first
scope:
  - src/lib.rs
```
"#;
    root.write("docs/milestones/010-inline.md", milestone_content);

    let sync_res = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "docs/milestones/010-inline.md"
        }))
        .unwrap(),
    )
    .unwrap();
    let task_id = sync_res["tasks"][0]["task_id"].as_str().unwrap();

    let inspected = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "inspect",
            "task_id": task_id
        }))
        .unwrap(),
    )
    .unwrap();
    // At contract stage, mode is subagent
    assert_eq!(inspected["workflow_guidance"]["mode"], "subagent");

    // Advance to review stage
    let store = TasksStore::open_project(
        &root.0.join(".forge/tasks.sqlite"),
        "forge-mcp",
        "forge-mcp",
    )
    .unwrap();
    contextunity_forge_mcp::engine::tasks::claim(
        &root.0,
        contextunity_forge_mcp::engine::tasks::Claim {
            task_id: task_id.to_string(),
            stage: "contract".into(),
            worker_id: "author".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    let mut claimed_task = store.inspect(task_id).unwrap();
    let evidence_val = evidence(&claimed_task);
    contextunity_forge_mcp::engine::tasks::submit(
        &root.0,
        serde_json::from_value(json!({
            "task_id": task_id,
            "stage": "contract",
            "action": "pass",
            "evidence": evidence_val
        }))
        .unwrap(),
    )
    .unwrap();

    contextunity_forge_mcp::engine::tasks::claim(
        &root.0,
        contextunity_forge_mcp::engine::tasks::Claim {
            task_id: task_id.to_string(),
            stage: "build".into(),
            worker_id: "builder".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    claimed_task = store.inspect(task_id).unwrap();
    let build_evidence = evidence(&claimed_task);
    contextunity_forge_mcp::engine::tasks::submit(
        &root.0,
        serde_json::from_value(json!({
            "task_id": task_id,
            "stage": "build",
            "action": "pass",
            "evidence": build_evidence
        }))
        .unwrap(),
    )
    .unwrap();

    // Now claim review stage: mode must be inline and guidance must be adapted!
    let review_claim = contextunity_forge_mcp::engine::tasks::claim(
        &root.0,
        contextunity_forge_mcp::engine::tasks::Claim {
            task_id: task_id.to_string(),
            stage: "review".into(),
            worker_id: "inline-reviewer".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();

    let guidance = &review_claim["workflow_guidance"];
    assert_eq!(guidance["role"], "independent_reviewer");
    assert_eq!(guidance["mode"], "inline");
    let role_guide = guidance["role_guide"].as_str().unwrap();
    assert!(
        role_guide.contains(
            "Execute inline in the current agent to review candidate diff across contours."
        ),
        "expected adapted inline role guide, got: {role_guide}"
    );
    assert!(!role_guide.contains("Spawn a dedicated"));
}
