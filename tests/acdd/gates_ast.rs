//! Test module reserved for the AC-001 gate AST contract.

use contextunity_forge_mcp::core::tasks::{
    catalog::{filter_subsystem_tools, SubsystemPreset, SubsystemRegistry},
    profile::{parse_profile, validate_profile_document, GateDriver},
    Milestone,
};

#[test]
fn profile_parser_accepts_polymorphic_drivers_and_catalogs() {
    let cases = [
        (
            "command gate driver",
            r#"
commands:
  check_task: "check {scope_target}"
gates:
  - id: verify
    command: check_task
    proof: command
"#,
        ),
        (
            "by_pattern command catalog",
            r#"
commands:
  check_task:
    by_pattern:
      "**/*.py": "typecheck {scope_target} && test {scope_target}"
      "**/*.rs": "check {scope_target}"
"#,
        ),
        (
            "shared diagnostic catalog",
            r#"
error_codes:
  FEATURE_DISABLED:
    description: "A requested Forge subsystem is disabled."
"#,
        ),
        (
            "hierarchical v2 profile",
            include_str!("../../docs/future/references/acdd_profile.v2.yaml.example"),
        ),
    ];

    for (case, yaml) in cases {
        let parsed = parse_profile(yaml);
        assert!(
            parsed.is_ok(),
            "{case} should deserialize through the production profile parser: {parsed:?}"
        );
    }

    let v2 = validate_profile_document(include_str!(
        "../../docs/future/references/acdd_profile.v2.yaml.example"
    ))
    .expect("the v2 profile example should satisfy the production validators");
    assert_eq!(v2.version, Some(2));
    assert_eq!(
        v2.commands["check_task"].command_for_path("src/lib.rs"),
        Some("cargo clippy --lib --test {scope_target} && cargo test --test {scope_target} -- --nocapture")
    );
    assert_eq!(
        v2.commands["check_task"].command_for_path("web/pages/home.tsx"),
        Some("npm run typecheck && npm test -- {scope_target}")
    );
    assert_eq!(
        v2.commands["check_task"].command_for_path("README.md"),
        None
    );
    assert_eq!(v2.gates[0].driver(), Some(GateDriver::Agent));
    assert!(v2
        .validate_guidance_bindings(&[("hitl".to_owned(), "pro".to_owned())].into())
        .expect("a bound HITL slot should resolve")
        .is_empty());
    assert!(v2
        .validate_guidance_bindings(&Default::default())
        .expect("an unbound HITL slot should fall back to its profile default")
        .iter()
        .any(|warning| warning.contains("hitl")));
    assert!(v2
        .validate_guidance_bindings(&[("missing".to_owned(), "pro".to_owned())].into())
        .is_err());
    assert!(validate_profile_document(
        "version: 2\nroles:\n  worker: {}\ngates:\n  - id: symbols\n    role: worker\n    proof: none\n    symbols: [not-a-symbol-coordinate]\n"
    )
    .is_err());

    let alias_conditions = r#"
version: 2
roles:
  worker: {}
gates:
  - id: docs
    when: docs
    role: worker
    proof: none
  - id: database
    when: db
    role: worker
    proof: none
  - id: handoff
    role: worker
    proof: delivery
"#;
    assert!(validate_profile_document(alias_conditions).is_ok());

    let invalid_profiles = [
        (
            "duplicate hierarchical gate ids",
            "version: 2\nroles: {worker: {}}\ngates:\n  - {id: build, role: worker, proof: none}\n  - {id: build, role: worker, proof: none}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "strictly prior gate references",
            "version: 2\nroles: {worker: {}}\ngates:\n  - {id: build, role: worker, proof: none, reject_to: future}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "review sources must point to a prior review gate",
            "version: 2\nroles: {worker: {}}\ngates:\n  - {id: build, role: worker, proof: none}\n  - {id: review, role: worker, proof: review, review_sources: [build]}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "reject routing targets must resolve in the profile",
            "version: 2\nroles: {worker: {}}\ngates:\n  - {id: review, role: worker, proof: review, reject_routing: {symbols: missing}}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "registered command references",
            "version: 2\nroles: {worker: {}}\ncommands: {check: \"echo ok\"}\ngates:\n  - {id: verify, command: missing, proof: command}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "guidance slot and catalog references",
            "version: 2\nroles: {worker: {}}\ngates:\n  - {id: build, role: worker, proof: none, guidance: [missing]}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "contour references",
            "version: 2\nroles: {worker: {}}\ngates:\n  - {id: review, role: worker, proof: review, contours: missing}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "typed gate conditions",
            "version: 2\nroles: {worker: {}}\ngates:\n  - {id: build, role: worker, proof: none, when: rust}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
        (
            "terminal hierarchical delivery",
            "version: 2\nroles: {worker: {}}\ngates:\n  - {id: handoff, role: worker, proof: delivery}\n  - {id: after, role: worker, proof: none}\n",
        ),
        (
            "contour definitions require usable criteria",
            "version: 2\nroles: {worker: {}}\ncontours:\n  standard:\n    paths: {description: \"\", criteria: []}\ngates:\n  - {id: review, role: worker, proof: review, contours: standard}\n  - {id: handoff, role: worker, proof: delivery}\n",
        ),
    ];
    for (case, yaml) in invalid_profiles {
        assert!(
            validate_profile_document(yaml).is_err(),
            "hierarchical validation must reject {case}"
        );
    }

    let milestone = Milestone::parse(
        r#"---
id: m-linked
required_skills: [contextunity-forge]
repositories:
  - name: forge-mcp
  - name: tools
    path: tools
linked_to: [m-upstream]
guidance:
  hitl: pro
concurrency:
  mode: speculative
  sync_on: snapshot
---
```yaml
task_ref: consumer
target: Consume an upstream seam
proof_policy: seam-test-first
scope: [src/lib.rs]
extended_scope: [tests/lib.rs]
symbols: [struct:GateDef]
extended_symbols: [field:GateProfile.error_codes]
task_depends_on: [m-upstream:producer]
concurrency:
  mode: speculative
  sync_on: snapshot
```
"#,
        "forge-mcp",
    )
    .expect("the production milestone parser should accept the task schema");
    assert_eq!(milestone.required_skills, ["contextunity-forge"]);
    assert_eq!(milestone.tasks[0].depends_on, ["m-upstream:producer"]);
    assert_eq!(milestone.tasks[0].extended_scope, ["tests/lib.rs"]);

    let registry = SubsystemRegistry {
        profile: SubsystemPreset::Navigator,
        ..SubsystemRegistry::default()
    };
    let filtered = filter_subsystem_tools(
        [
            "code_map_overview",
            "get_code_snippet",
            "task_list",
            "task_blackboard",
            "search_docs",
            "session_checkpoint",
        ],
        &registry,
    );
    assert_eq!(
        filtered,
        ["code_map_overview", "get_code_snippet", "search_docs"]
    );
    assert!(
        contextunity_forge_mcp::core::tasks::catalog::disabled_tool_error("task_list", &registry)
            .is_some_and(|error| error.starts_with("FEATURE_DISABLED:"))
    );

    let adapter_workspace = super::support::ScopedWorkspace::new("forge_module_registry_config");
    adapter_workspace.write("forge-mcp.yaml", "profile: minimal\n");
    let minimal = contextunity_forge_mcp::engine::scanner::load_adapter(&adapter_workspace.0, None)
        .expect("top-level minimal preset should load");
    assert!(
        ["codemap", "acdd", "blackboard", "docs", "checkpoints"]
            .iter()
            .all(|module| !minimal.modules.is_enabled(module)),
        "minimal should leave only the unassigned forge_guide tool enabled"
    );
    assert_eq!(
        filter_subsystem_tools(
            [
                "forge_guide",
                "task_list",
                "code_map_overview",
                "get_code_snippet"
            ],
            &minimal.modules
        ),
        ["forge_guide"]
    );
    assert!(
        contextunity_forge_mcp::core::tasks::catalog::disabled_tool_error(
            "get_code_snippet",
            &minimal.modules
        )
        .is_some_and(|error| error.starts_with("FEATURE_DISABLED:"))
    );

    adapter_workspace.write("forge-mcp.yaml", "profile: full\n");
    let full = contextunity_forge_mcp::engine::scanner::load_adapter(&adapter_workspace.0, None)
        .expect("top-level full preset should load");
    assert!(["codemap", "acdd", "blackboard", "docs", "checkpoints"]
        .iter()
        .all(|module| full.modules.is_enabled(module)));

    adapter_workspace.write(
        "forge-mcp.yaml",
        "profile: minimal\nmodules:\n  acdd: true\n  docs: true\n",
    );
    let toggles = contextunity_forge_mcp::engine::scanner::load_adapter(&adapter_workspace.0, None)
        .expect("top-level preset should accept explicit subsystem overrides");
    assert!(toggles.modules.is_enabled("acdd"));
    assert!(toggles.modules.is_enabled("docs"));
    assert!(!toggles.modules.is_enabled("codemap"));

    adapter_workspace.write(
        "forge-mcp.yaml",
        "modules:\n  profile: navigator\n  enabled: [blackboard]\n",
    );
    let legacy = contextunity_forge_mcp::engine::scanner::load_adapter(&adapter_workspace.0, None)
        .expect("legacy nested subsystem registry should remain supported");
    assert!(legacy.modules.is_enabled("codemap"));
    assert!(legacy.modules.is_enabled("docs"));
    assert!(legacy.modules.is_enabled("blackboard"));

    let validated = contextunity_forge_mcp::cli::validate::run(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
        Some(std::path::Path::new(
            "docs/future/references/acdd_profile.v2.yaml.example",
        )),
    )
    .expect("the public CLI validation path should validate the example");
    assert_eq!(validated["valid"], true);

    let workspace = super::support::ScopedWorkspace::new("forge_validate_workspace_scan");
    workspace.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nmilestones: [docs/milestones]\n",
    );
    workspace.write(
        "docs/milestones/010-valid.md",
        "---\nid: m-valid\nstatus: active\n---\n```yaml\ntask_ref: one\ntarget: First task\nproof_policy: seam-test-first\nscope: [src/one.rs]\n```\n",
    );
    let scanned = contextunity_forge_mcp::cli::validate::run(&workspace.0, None)
        .expect("zero-argument validation should scan a workspace");
    assert_eq!(scanned["valid"], true, "workspace validation: {scanned}");
    assert_eq!(scanned["scope"], "workspace");

    let overlap = super::support::ScopedWorkspace::new("forge_validate_scope_overlap");
    overlap.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nmilestones: [docs/milestones]\n",
    );
    overlap.write(
        "docs/milestones/020-overlap.md",
        "---\nid: m-overlap\nstatus: planned\n---\n```yaml\ntask_ref: one\ntarget: First task\nproof_policy: seam-test-first\nscope: [src/shared.rs]\n```\n```yaml\ntask_ref: two\ntarget: Second task\nproof_policy: seam-test-first\nscope: [src/shared.rs/subfile.rs]\n```\n",
    );
    let overlap_result = contextunity_forge_mcp::cli::validate::run(&overlap.0, None)
        .expect("scope overlap should produce structured validation diagnostics");
    assert_eq!(overlap_result["valid"], false);
    assert!(overlap_result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| {
            diagnostic["code"] == "MILESTONE_SCOPE_OVERLAP"
                && diagnostic["status"] == "FAIL"
                && diagnostic["line"].as_u64().is_some()
                && diagnostic["column"].as_u64().is_some()
        }));

    let cycle = super::support::ScopedWorkspace::new("forge_validate_dag_cycle");
    cycle.write(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\nmilestones: [docs/milestones]\n",
    );
    cycle.write(
        "docs/milestones/030-a.md",
        "---\nid: m-a\nstatus: active\n---\n```yaml\ntask_ref: task-a\ntarget: A\nproof_policy: seam-test-first\nscope: [src/a.rs]\ntask_depends_on: [m-b:task-b]\n```\n",
    );
    cycle.write(
        "docs/milestones/040-b.md",
        "---\nid: m-b\nstatus: planned\n---\n```yaml\ntask_ref: task-b\ntarget: B\nproof_policy: seam-test-first\nscope: [src/b.rs]\ntask_depends_on: [m-a:task-a]\n```\n",
    );
    let cycle_result = contextunity_forge_mcp::cli::validate::run(&cycle.0, None)
        .expect("cross-milestone cycle should produce structured validation diagnostics");
    assert_eq!(cycle_result["valid"], false);
    assert!(cycle_result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["code"] == "MILESTONE_DAG_CYCLE"));
}

#[test]
fn linked_milestone_context_reports_upstream_task_status_and_blockers() {
    let root = super::support::ScopedWorkspace::new("forge_linked_milestone_context");
    root.write("src/lib.rs", "pub fn example() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\nmilestones: [docs/milestones]\ntasks_db: .forge/tasks.sqlite\n",
    );

    let upstream = r#"---
id: m-upstream
title: Upstream
doc_type: contract
status: active
---
```yaml
task_ref: producer
target: Produce the linked task
proof_policy: seam-test-first
scope: [src/lib.rs]
```
"#;
    let consumer = r#"---
id: m-consumer
title: Consumer
doc_type: contract
status: active
linked_to: [m-upstream]
---
```yaml
task_ref: consumer
target: Consume the linked task
proof_policy: seam-test-first
scope: [src/lib.rs]
task_depends_on: [m-upstream:producer]
```
"#;
    root.write("docs/milestones/010-upstream.md", upstream);
    root.write("docs/milestones/020-consumer.md", consumer);

    for milestone_ref in [
        "docs/milestones/010-upstream.md",
        "docs/milestones/020-consumer.md",
    ] {
        contextunity_forge_mcp::engine::tasks::manage(
            &root.0,
            serde_json::from_value(serde_json::json!({
                "action": "sync",
                "milestone_ref": milestone_ref,
            }))
            .expect("sync action should deserialize"),
        )
        .expect("fixture milestone should sync");
    }

    let upstream_tasks = contextunity_forge_mcp::engine::tasks::list(
        &root.0,
        serde_json::from_value(serde_json::json!({
            "milestone_ref": "docs/milestones/010-upstream.md",
            "milestone_status": "all",
            "status": "all",
        }))
        .expect("list action should deserialize"),
    )
    .expect("upstream task should be listable");

    let context = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(serde_json::json!({
            "action": "context",
            "task_id": "forge-mcp/m-consumer:consumer",
        }))
        .expect("context action should deserialize"),
    )
    .expect("consumer context should build");

    assert_eq!(
        context["linked_milestones"][0]["milestone_id"],
        "m-upstream"
    );
    assert_eq!(
        context["linked_milestones"][0]["task_count"], 1,
        "upstream list: {upstream_tasks}; linked milestone context: {context}"
    );
    assert_eq!(
        context["linked_milestones"][0]["tasks"][0]["status"],
        "ready"
    );
    assert_eq!(
        context["linked_milestones"][0]["dependency_states"][0]["blocking"],
        true
    );

    root.write(
        "forge-mcp.yaml",
        "profile: minimal\nroots: [src]\ndocs: [docs]\nmilestones: [docs/milestones]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let minimal_error = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(serde_json::json!({
            "action": "context",
            "task_id": "forge-mcp/m-consumer:consumer",
        }))
        .expect("context action should deserialize"),
    )
    .expect_err("minimal must reject a gate that requires disabled code navigation");
    assert!(
        format!("{minimal_error:#}").contains(
            "active gate 'contract' requires tool 'code_map_overview' from disabled Forge subsystem 'codemap'"
        ),
        "unexpected minimal-profile result: {minimal_error:#}"
    );

    let mut no_tool_profile: serde_yaml::Value =
        serde_yaml::from_str(include_str!("../../src/core/tasks/acdd.default.yaml"))
            .expect("the embedded ACDD profile should deserialize for the context fixture");
    for gate in no_tool_profile["gates"]
        .as_sequence_mut()
        .expect("the embedded profile should define gates")
    {
        gate["tools"] = serde_yaml::Value::Sequence(Vec::new());
    }
    let no_tool_profile = serde_yaml::to_string(&no_tool_profile)
        .expect("the no-tool context profile should serialize");
    root.write(".forge/acdd/no-tools.yaml", &no_tool_profile);
    root.write(
        "forge-mcp.yaml",
        "profile: minimal\nacdd_profile: .forge/acdd/no-tools.yaml\nroots: [src]\ndocs: [docs]\nmilestones: [docs/milestones]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let minimal_context = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(serde_json::json!({
            "action": "context",
            "task_id": "forge-mcp/m-consumer:consumer",
        }))
        .expect("context action should deserialize"),
    )
    .expect("a gate without disabled tool requirements should run under minimal");
    assert!(minimal_context["context_bundle"]["adrs"].is_null());
    assert!(minimal_context["context_bundle"]["scope_symbols"].is_null());
    assert!(minimal_context["context_bundle"]["covering_tests"].is_null());
    assert!(minimal_context["context_bundle"]["external_callers"].is_null());
    assert!(minimal_context["blackboard"].is_null());
    assert!(minimal_context["blackboard_info"].is_null());
    assert!(minimal_context["workflow_guidance"]["blackboard_messages"].is_null());
    assert!(minimal_context["workflow_guidance"]["recommended_tools"]
        .as_array()
        .is_some_and(Vec::is_empty));
    let minimal_guidance = minimal_context["workflow_guidance"].to_string();
    assert!(
        !minimal_guidance.contains("task_blackboard"),
        "disabled blackboard guidance leaked: {minimal_guidance}"
    );

    root.write(
        "forge-mcp.yaml",
        "profile: minimal\nmodules:\n  codemap: true\n  docs: true\n  acdd: true\nroots: [src]\ndocs: [docs]\nmilestones: [docs/milestones]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let overridden_context = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(serde_json::json!({
            "action": "context",
            "task_id": "forge-mcp/m-consumer:consumer",
        }))
        .expect("context action should deserialize"),
    )
    .expect("explicit module overrides should satisfy the active gate");
    assert_eq!(
        overridden_context["workflow_guidance"]["recommended_tools"],
        serde_json::json!([
            "code_map_overview",
            "get_doc",
            "search_docs",
            "code_map_inspect"
        ])
    );
    assert!(overridden_context["blackboard"].is_null());
    assert!(overridden_context["workflow_guidance"]["blackboard_messages"].is_null());
    assert!(!overridden_context["workflow_guidance"]
        .to_string()
        .contains("task_blackboard"));
}

#[test]
fn task_manage_named_snapshot_restores_worktree_and_guards_head_changes() {
    use std::process::Command;

    let root = super::support::ScopedWorkspace::new("forge_task_manage_named_snapshot");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\nmilestones: [docs/milestones]\ntasks_db: runtime/tasks.sqlite\n",
    );
    root.write("src/lib.rs", "snapshot baseline\n");
    root.write(".forge/managed-state", "preserve Forge state\n");
    let exclude = root.0.join(".git/info/exclude");
    let mut ignored = std::fs::read_to_string(&exclude).unwrap_or_default();
    ignored.push_str("\n.forge/\n");
    std::fs::write(&exclude, ignored).unwrap();

    let run_git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(&root.0)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    run_git(&["add", "forge-mcp.yaml", "src/lib.rs"]);
    run_git(&["commit", "-m", "snapshot fixture baseline"]);
    let head = super::support::git_head(&root.0);

    let manage = |action: &str, name: &str| {
        contextunity_forge_mcp::engine::tasks::manage(
            &root.0,
            serde_json::from_value(serde_json::json!({
                "action": action,
                "name": name,
            }))
            .expect("named snapshot action should deserialize"),
        )
    };

    let snapshot = manage("snapshot", "before-change")
        .expect("snapshot action should create a named shadow snapshot");
    assert_eq!(snapshot["action"], "snapshot");
    assert_eq!(snapshot["snapshot_name"], "before-change");
    assert_eq!(super::support::git_head(&root.0), head);
    assert!(manage("snapshot", "before-change")
        .unwrap_err()
        .to_string()
        .contains("TASK_SNAPSHOT_ALREADY_EXISTS"));
    assert!(contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(serde_json::json!({
            "action": "snapshot",
            "name": "../outside",
        }))
        .unwrap(),
    )
    .unwrap_err()
    .to_string()
    .contains("TASK_SELECTOR_INVALID"));

    root.write("src/lib.rs", "experiment changes\n");
    root.write("after-snapshot.txt", "remove on rollback\n");
    #[cfg(unix)]
    {
        use std::{fs::OpenOptions, os::fd::AsRawFd, path::PathBuf};

        let git_dir = String::from_utf8(run_git(&["rev-parse", "--absolute-git-dir"]).stdout)
            .unwrap()
            .trim()
            .to_owned();
        let lock_path = PathBuf::from(git_dir).join("forge-task-manage-snapshot.lock");
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .unwrap();
        let locked = unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        assert_eq!(locked, 0, "test should acquire snapshot-operation lock");
        let busy = manage("rollback", "before-change").unwrap_err();
        assert!(busy.to_string().contains("TASK_SNAPSHOT_BUSY"));
        assert_eq!(
            std::fs::read_to_string(root.0.join("src/lib.rs")).unwrap(),
            "experiment changes\n",
            "lock contention must fail before touching the worktree"
        );
        assert!(root.0.join("after-snapshot.txt").exists());
        let unlocked = unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_UN) };
        assert_eq!(unlocked, 0, "test should release snapshot-operation lock");
        drop(lock_file);
    }
    let rollback = manage("rollback", "before-change")
        .expect("rollback action should restore the named worktree snapshot");
    assert_eq!(rollback["action"], "rollback");
    assert_eq!(rollback["restored"], true);
    assert_eq!(
        std::fs::read_to_string(root.0.join("src/lib.rs")).unwrap(),
        "snapshot baseline\n"
    );
    assert!(!root.0.join("after-snapshot.txt").exists());
    assert_eq!(super::support::git_head(&root.0), head);
    let status_output = run_git(&["status", "--porcelain"]);
    let status = String::from_utf8_lossy(&status_output.stdout);
    assert_eq!(
        status.trim(),
        "?? runtime/",
        "rollback should remove later experiment files and preserve the task database"
    );
    assert!(root.0.join("runtime/tasks.sqlite").exists());
    assert_eq!(
        std::fs::read_to_string(root.0.join(".forge/managed-state")).unwrap(),
        "preserve Forge state\n"
    );

    manage("snapshot", "head-guard").expect("second named snapshot should succeed");
    root.write("src/lib.rs", "committed after snapshot\n");
    run_git(&["add", "src/lib.rs"]);
    run_git(&["commit", "-m", "advance branch after snapshot"]);
    let advanced_head = super::support::git_head(&root.0);
    root.write("src/lib.rs", "uncommitted after branch advance\n");
    assert!(manage("rollback", "head-guard")
        .unwrap_err()
        .to_string()
        .contains("TASK_SNAPSHOT_HEAD_CHANGED"));
    assert_eq!(super::support::git_head(&root.0), advanced_head);
    assert_eq!(
        std::fs::read_to_string(root.0.join("src/lib.rs")).unwrap(),
        "uncommitted after branch advance\n"
    );
}
