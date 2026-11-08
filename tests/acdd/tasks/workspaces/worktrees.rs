use super::*;
use crate::acdd::tasks::harness::TaskHarness;

#[test]
fn infer_project_from_path_supports_arbitrary_monorepo_structures_universally() {
    use contextunity_forge_mcp::core::tasks::infer_project_from_path;
    use std::path::Path;

    let path_cases = [
        (
            "extensions/commerce/docs/milestones/010.md",
            Some("extensions.commerce"),
        ),
        ("packages/cli/docs/milestones/010.md", Some("packages.cli")),
        ("services/brain/docs/plans/010.md", Some("services.brain")),
        ("apps/web/docs/milestones/010.md", Some("apps.web")),
        (
            "custom_dir/my-plugin/milestones/010.md",
            Some("custom_dir.my-plugin"),
        ),
        (
            "team_alpha/backend/analytics/docs/milestones/020.md",
            Some("team_alpha.backend.analytics"),
        ),
        ("user_service/docs/milestones/010.md", Some("user_service")),
        (
            "packages/my app/docs/milestones/010.md",
            Some("packages.my~u32~app"),
        ),
        (
            "packages/über/docs/milestones/010.md",
            Some("packages.~u252~ber"),
        ),
        (
            "microservices/billing/docs/plans/030.md",
            Some("microservices.billing"),
        ),
        ("standalone_tool/milestones/010.md", Some("standalone_tool")),
        ("docs/milestones/010.md", None),
        ("milestones/010.md", None),
        ("docs/plans/010.md", None),
        ("plans/010.md", None),
    ];
    for (path, expected) in path_cases {
        assert_eq!(
            infer_project_from_path(Path::new(path)).as_deref(),
            expected,
            "project inferred from {path}"
        );
    }

    for (left, right) in [
        (
            "foo.bar/docs/milestones/010.md",
            "foo/bar/docs/milestones/010.md",
        ),
        ("a~d/docs/milestones/010.md", "a.b/docs/milestones/010.md"),
        (
            "packages/api/docs/milestones/010.md",
            "services/api/docs/milestones/010.md",
        ),
    ] {
        assert_ne!(
            infer_project_from_path(Path::new(left)),
            infer_project_from_path(Path::new(right)),
            "distinct workspace paths must not alias: {left} and {right}"
        );
    }

    let root = acdd_workspace(DEFAULT_SEAM, "");
    root.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\nmilestones:\n  - docs/milestones\n  - packages/api/docs/milestones\n  - services/api/docs/milestones\n  - plugins/api/docs/milestones\n");
    let task_block = "# Tasks\n```yaml\ntask_ref: shared\ntarget: Resolve one project\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    for (dir, project) in [
        ("packages/api", ""),
        ("services/api", ""),
        ("plugins/api", "project: explicit-api\n"),
    ] {
        root.write(
            format!("{dir}/docs/milestones/010-shared.md"),
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
            format!("docs/milestones/{filename}"),
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
            root.root(),
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
    assert!(ids.iter().any(|id| id.starts_with("explicit-api/")));
    assert!(ids.iter().any(|id| id.starts_with("forge-mcp/")));
    assert!(ids.iter().any(|id| id.starts_with("alpha/")));
    assert!(ids.iter().any(|id| id.starts_with("beta/")));
}

#[test]
fn linked_task_workspaces_share_storage_and_confine_local_scope() {
    let (root, _, _) = fixture(DEFAULT_SEAM);
    let linked = acdd_workspace(DEFAULT_SEAM, "");
    let missing = acdd_workspace(DEFAULT_SEAM, "");
    let empty = acdd_workspace(DEFAULT_SEAM, "");
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
    root.write("forge-mcp.yaml", &format!("roots: [src]\nlinked_workspaces:\n  - name: traverse\n    path: {}\n    tasks:\n      enabled: true\n      milestones_dir: contracts\n      agents_guidance: docs/AGENTS.md\n  - name: missing\n    path: {}\n    tasks: {{enabled: true}}\n  - name: empty\n    path: {}\n    tasks: {{enabled: true}}\n  - name: disabled\n    path: {}\n    tasks: {{enabled: false}}\n  - name: index-only\n    path: {}\n",linked.root().display(),missing.root().display(),empty.root().display(),linked.root().display(),linked.root().display()));
    let sync = |workspace: &str| {
        tasks::manage(
            root.root(),
            serde_json::from_value(json!({"action":"sync","workspace":workspace})).unwrap(),
        )
        .unwrap()
    };
    assert_eq!(sync("missing")["tasks"], json!([]));
    assert_eq!(sync("empty")["tasks"], json!([]));
    let imported = sync("traverse");
    assert_eq!(imported["tasks"].as_array().unwrap().len(), 2);
    let id = imported["tasks"][0]["task_id"].as_str().unwrap();
    assert!(id.starts_with("tooling/"));
    let list =
        |arguments| tasks::list(root.root(), serde_json::from_value(arguments).unwrap()).unwrap();
    let linked_by_milestone = list(json!({"milestone_ref":"m-linked","status":"all"}));
    let linked_by_milestone_tasks = linked_by_milestone["tasks"].as_array().unwrap();
    assert_eq!(linked_by_milestone_tasks.len(), 2);
    assert!(linked_by_milestone_tasks
        .iter()
        .all(|task| { task["task_id"].as_str().unwrap().starts_with("tooling/") }));
    assert_eq!(list(json!({}))["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(
        list(json!({"repository":"all","milestone_status":"all"}))["tasks"]
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
        list(json!({"repository":"all","status":"all","milestone_status":"all"}))["tasks"]
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
        root.root(),
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
        root.root(),
        serde_json::from_value(json!({"action":"inspect","task_id":id})).unwrap(),
    )
    .unwrap();
    assert_eq!(inspect["workspace_root"], linked.root().to_str().unwrap());
    assert_eq!(
        inspect["agents_guidance"],
        linked.root().join("docs/AGENTS.md").to_str().unwrap()
    );
    assert_eq!(inspect["applicable_invariants"], json!(["linked-rules"]));
    let extend = |path: &str| {
        tasks::manage(
            root.root(),
            serde_json::from_value(json!({"action":"extend_scope","task_id":id,"paths":[path]}))
                .unwrap(),
        )
    };
    assert!(extend("src/new.rs").is_ok());
    std::os::unix::fs::symlink(root.root().join("src"), linked.root().join("src/foreign")).unwrap();
    assert!(extend("src/foreign/main.rs").is_err());
    assert!(extend("../outside.rs").is_err());
    let claim = |worktree: &std::path::Path| {
        tasks::claim(
            root.root(),
            tasks::Claim {
                task_id: id.into(),
                stage: "contract".into(),
                worker_id: "linked-builder".into(),
                session_fingerprint: Some(TaskHarness::session_for("linked-builder")),
                worktree: worktree.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
    };
    assert!(claim(root.root()).is_err());
    let claimed = claim(linked.root()).unwrap();
    assert_eq!(claimed["workspace_root"], inspect["workspace_root"]);
    assert_eq!(claimed["agents_guidance"], inspect["agents_guidance"]);
    let namespaces = TaskHarness::database_task_count(root.root()).unwrap();
    assert_eq!(namespaces, 4);
}

#[test]
fn linked_scope_extension_validates_owner_and_claimed_worktree() {
    let (root, _, _) = fixture(DEFAULT_SEAM);
    let owner = acdd_workspace(DEFAULT_SEAM, "");
    let worktree = acdd_workspace(DEFAULT_SEAM, "");
    for workspace in [&owner, &worktree] {
        workspace.write("src/lib.rs", "pub fn local() {}\n");
        workspace.write("docs/010-test.md", SPEC);
    }
    root.write(
        "forge-mcp.yaml",
        &format!(
            "linked_workspaces:\n  - name: traverse\n    path: {}\n    tasks: {{enabled: true}}\n",
            owner.root().display()
        ),
    );
    let synced = tasks::manage(
        root.root(),
        serde_json::from_value(
            json!({"action":"sync","workspace":"traverse","milestone_ref":"docs/010-test.md"}),
        )
        .unwrap(),
    )
    .unwrap();
    let id = synced["tasks"][0]["task_id"].as_str().unwrap();
    tasks::claim(
        root.root(),
        tasks::Claim {
            task_id: id.into(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            session_fingerprint: Some(TaskHarness::session_for("builder")),
            worktree: worktree.root().to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    let extend = |path: &str| {
        tasks::manage(
            root.root(),
            serde_json::from_value(json!({"action":"extend_scope","task_id":id,"paths":[path]}))
                .unwrap(),
        )
    };
    std::os::unix::fs::symlink(root.root().join("src"), owner.root().join("src/foreign")).unwrap();
    assert!(extend("src/foreign/new.rs").is_err());
    std::fs::remove_file(owner.root().join("src/foreign")).unwrap();
    std::os::unix::fs::symlink(root.root().join("src"), worktree.root().join("src/foreign"))
        .unwrap();
    assert!(extend("src/foreign/new.rs").is_err());
    assert!(extend("src/local.rs").is_ok());
}

#[test]
fn nested_repository_tasks_preserve_distinct_write_perimeters() {
    let main = acdd_workspace(DEFAULT_SEAM, "");
    main.write("src/main.rs", "pub fn main_owner() {}\n");
    main.write(
        "docs/010-main.md",
        &SPEC.replace("scope: [src/]", "scope: [src/main.rs]"),
    );
    main.write("src/library/src/lib.rs", "pub fn library_owner() {}\n");
    main.write("src/library/docs/milestones/010-lib.md", SPEC);
    main.write("forge-mcp.yaml", "linked_workspaces:\n  - name: library\n    path: src/library\n    tasks: {enabled: true}\n");
    let imported = tasks::manage(
        main.root(),
        serde_json::from_value(json!({"action":"sync","milestone_ref":"docs/010-main.md"}))
            .unwrap(),
    )
    .unwrap();
    let primary_id = imported["tasks"][0]["task_id"].as_str().unwrap();
    let imported_linked = tasks::manage(
        main.root(),
        serde_json::from_value(json!({"action":"sync","workspace":"library"})).unwrap(),
    )
    .unwrap();
    let library_id = imported_linked["tasks"][0]["task_id"].as_str().unwrap();
    let extend = |path: &str| {
        tasks::manage(
            main.root(),
            serde_json::from_value(
                json!({"action":"extend_scope","task_id":primary_id,"paths":[path]}),
            )
            .unwrap(),
        )
    };
    assert!(extend("src/library/src/foreign.rs").is_err());
    assert!(extend("src/main_helpers.rs").is_ok());
    let claimed = tasks::claim(
        main.root(),
        tasks::Claim {
            task_id: library_id.into(),
            stage: "contract".into(),
            worker_id: "library-builder".into(),
            session_fingerprint: Some(TaskHarness::session_for("library-builder")),
            worktree: main
                .root()
                .join("src/library")
                .to_string_lossy()
                .into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        claimed["workspace_root"],
        main.root().join("src/library").to_str().unwrap()
    );
    main.write(
        "docs/010-wide.md",
        &SPEC.replace("id: m-test", "id: m-wide"),
    );
    assert!(tasks::manage(
        main.root(),
        serde_json::from_value(json!({"action":"sync","milestone_ref":"docs/010-wide.md"}))
            .unwrap()
    )
    .is_err());
}

#[test]
fn task_configuration_defaults_and_crlf_preserve_operational_identity() {
    let (root, _, milestone) = fixture(DEFAULT_SEAM);
    root.write("forge-mcp.yaml", "roots: [src]\n");
    assert_eq!(
        tasks::database_path(root.root()).unwrap(),
        root.root().join(".forge/tasks.sqlite")
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
    let missing = root.root().join("not-created");
    let error = tasks::claim(
        root.root(),
        tasks::Claim {
            task_id: id.clone(),
            stage: "contract".into(),
            worker_id: "builder".into(),
            session_fingerprint: Some(TaskHarness::session_for("builder")),
            worktree: missing.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().starts_with("WORKTREE_NOT_FOUND"));
    let task = tasks::store(root.root()).unwrap().inspect(&id).unwrap();
    assert_eq!(task.status, "ready");
    assert!(task.worker_id.is_none());
}

#[test]
fn linked_worktree_resolves_tasks_db_to_primary_worktree_root() {
    let primary = TaskHarness::runner_workspace(DEFAULT_SEAM, "");
    let wt = TaskHarness::workspace();
    primary.write("README.md", "# test\n");
    primary.write("forge-mcp.yaml", "roots: [src]\n");
    TaskHarness::stage_changes(primary.root(), &["."]);
    TaskHarness::commit_fixture(primary.root(), "initial");
    let wt_out = TaskHarness::create_worktree(primary.root(), wt.root(), "test-wt");
    assert!(
        wt_out.status.success(),
        "{}",
        String::from_utf8_lossy(&wt_out.stderr)
    );
    let expected = primary
        .root()
        .canonicalize()
        .unwrap()
        .join(".forge/tasks.sqlite");
    let resolved = tasks::database_path(wt.root()).unwrap();
    assert_eq!(resolved, expected);
    // Fallback when forge-mcp.yaml is absent in worktree:
    let _ = std::fs::remove_file(wt.root().join("forge-mcp.yaml"));
    let resolved_fallback = tasks::database_path(wt.root()).unwrap();
    assert_eq!(resolved_fallback, expected);
    let _ = TaskHarness::remove_worktree(primary.root(), wt.root());
}

#[test]
fn linked_pinned_tasks_ignore_linked_workspace_profile_overrides() {
    use sha2::Digest;
    let task_profile = "roles:\n  builder:\n    models:\n      - model: linked-task-override\n";
    let invalid_linked_base = "gates:\n  - id: linked_only\n    role: builder\n    proof: none\n";

    for (case, linked_base_path) in [
        ("forge_config", "forge-mcp.yaml"),
        ("repository_profile", ".forge/acdd/profile.yaml"),
    ] {
        let root = acdd_workspace(DEFAULT_SEAM, "");
        root.write("src/lib.rs", "pub fn primary() {}\n");
        root.write("linked/src/lib.rs", "pub fn linked() {}\n");
        root.write("linked/AGENTS.md", "# Linked instructions\n");
        root.write("linked/profiles/task.yaml", task_profile);
        let digest = format!(
            "{:x}",
            sha2::Sha256::digest(
                std::fs::read(root.root().join("linked/profiles/task.yaml")).unwrap()
            )
        );
        if linked_base_path == "forge-mcp.yaml" {
            root.write("linked/invalid-base.yaml", invalid_linked_base);
            root.write(
                "linked/forge-mcp.yaml",
                "task_repository: linked-repository\ntask_project: linked-project\nacdd_profile: invalid-base.yaml\n",
            );
        } else {
            root.write(
                "linked/forge-mcp.yaml",
                "task_repository: linked-repository\ntask_project: linked-project\n",
            );
            root.write(format!("linked/{linked_base_path}"), invalid_linked_base);
        }
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nlinked_workspaces:\n  - name: linked\n    path: linked\n    tasks:\n      enabled: true\n      milestones_dir: contracts\n      agents_guidance: AGENTS.md\n",
        );
        let manifest = format!(
            "---\nid: m-linked-{case}\ntitle: Linked profile\ndoc_type: contract\nstatus: active\n---\n# Linked profile\n```yaml\ntask_ref: pinned\ntarget: Ignore linked base profile\nacdd_profile: profiles/task.yaml:{}\nproof_policy: seam-test-first\nscope: [src/]\n```\n",
            &digest[..16]
        );
        root.write("linked/contracts/010-pinned.md", &manifest);

        let imported = tasks::manage(
            root.root(),
            serde_json::from_value(json!({"action":"sync","workspace":"linked"})).unwrap(),
        )
        .unwrap_or_else(|error| panic!("{case} linked base profile leaked into task: {error}"));
        let imported_tasks = imported["tasks"].as_array().unwrap();
        assert_eq!(imported_tasks.len(), 1);
        let task_id = imported_tasks[0]["task_id"].as_str().unwrap();
        assert!(task_id.starts_with("linked-project/"));

        tasks::manage(
            root.root(),
            serde_json::from_value(json!({"action":"inspect","task_id":task_id})).unwrap(),
        )
        .unwrap_or_else(|error| {
            panic!("{case} pinned task inspection used linked base profile: {error}")
        });
    }
}
