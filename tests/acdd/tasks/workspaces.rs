use super::*;

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
    assert!(id.starts_with("tooling/"));
    let list =
        |arguments| tasks::list(&root.0, serde_json::from_value(arguments).unwrap()).unwrap();
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
    assert!(ids.iter().any(|id| id.starts_with("explicit-api/")));
    assert!(ids.iter().any(|id| id.starts_with("forge-mcp/")));
    assert!(ids.iter().any(|id| id.starts_with("alpha/")));
    assert!(ids.iter().any(|id| id.starts_with("beta/")));
}

#[test]
fn scoped_git_snapshot_captures_untracked_and_modified_files_and_cleans_up_on_handoff() {
    for pending in [false, true] {
        use std::os::unix::fs::PermissionsExt;
        let isolated = ScopedWorkspace::new("forge_provisional_gc");
        isolated.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        isolated.write("src/lib.rs", "pub fn before() {}\n");
        isolated.write("docs/milestones/010-gc.md", "---\nid: m-gc\nstatus: active\n---\n```yaml\ntask_ref: candidate\ntarget: Preserve provisional candidate\nproof_policy: direct-proof\nscope: [src/]\n```\n");
        tasks::manage(
            &isolated.0,
            serde_json::from_value(
                json!({"action":"sync","milestone_ref":"docs/milestones/010-gc.md"}),
            )
            .unwrap(),
        )
        .unwrap();
        let id = "forge-mcp/m-gc:candidate";
        tasks::claim(
            &isolated.0,
            tasks::Claim {
                task_id: id.into(),
                stage: "contract".into(),
                worker_id: "builder".into(),
                worktree: isolated.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let claimed = tasks::store(&isolated.0).unwrap().inspect(id).unwrap();
        tasks::submit(&isolated.0,serde_json::from_value(json!({"task_id":id,"stage":"contract","action":"pass","evidence":evidence(&claimed)})).unwrap()).unwrap();
        tasks::claim(
            &isolated.0,
            tasks::Claim {
                task_id: id.into(),
                stage: "build".into(),
                worker_id: "builder".into(),
                worktree: isolated.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        isolated.write("src/lib.rs", "pub fn unique_provisional_candidate() {}\n");
        let shim = isolated.0.join("fixture-bin/git");
        if pending {
            isolated.write(
                "fixture-publication-fault",
                "isolate post-SQL keeper failure\n",
            );
        }
        isolated.write("fixture-bin/git", r#"#!/usr/bin/python
import subprocess,sys,pathlib,json
r=subprocess.run(['/usr/bin/git',*sys.argv[1:]],capture_output=True)
if r.returncode==0 and 'commit-tree' in sys.argv and pathlib.Path('fixture-publication-fault').exists() and '-m' not in sys.argv:
 state=subprocess.run(['/usr/bin/git','show',r.stdout.decode().strip()+':checkpoint.json'],capture_output=True)
 if state.returncode==0:
  state=json.loads(state.stdout)
  if state.get('intent') is None and any(row[0]=='build' and row[2]=='passed' for row in state.get('gates',[])):
   lock=pathlib.Path('.git/refs/forge/state/forge-mcp/forge-mcp/m-gc/candidate.lock')
   lock.parent.mkdir(parents=True,exist_ok=True);lock.write_text('isolated publication lock')
if r.returncode==0 and len(sys.argv)>1 and sys.argv[1]=='-c' and 'hash-object' in sys.argv and '-t' in sys.argv and 'commit' in sys.argv:
 gc=subprocess.run(['/usr/bin/git','gc','--prune=now'],capture_output=True)
 pathlib.Path('fixture-gc-construction.json').write_text(json.dumps({'gc_attempted':True,'exit':gc.returncode}))
if r.returncode==0 and len(sys.argv)>1 and sys.argv[1]=='commit-tree' and '-m' in sys.argv and any(a.startswith('forge-snapshot:') for a in sys.argv):
 sha=r.stdout.decode().strip()
 subprocess.run(['/usr/bin/git','reflog','expire','--expire=now','--all'],check=True,capture_output=True)
 subprocess.run(['/usr/bin/git','gc','--prune=now'],check=True,capture_output=True)
 check=subprocess.run(['/usr/bin/git','cat-file','-e',sha],capture_output=True)
 pathlib.Path('fixture-gc-window.json').write_text(json.dumps({'candidate':sha,'exists_after_gc':check.returncode==0,'gc_attempted':True}))
sys.stdout.buffer.write(r.stdout);sys.stderr.buffer.write(r.stderr);sys.exit(r.returncode)
"#);
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        let claimed = tasks::store(&isolated.0).unwrap().inspect(id).unwrap();
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--root",
                isolated.0.to_str().unwrap(),
                "--format",
                "json",
                "task",
                "submit",
                id,
                "--stage",
                "build",
                "--action",
                "pass",
                "--evidence",
                &serde_json::to_string(&evidence(&claimed)).unwrap(),
            ])
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    shim.parent().unwrap().display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .current_dir(&isolated.0)
            .output()
            .unwrap();
        let gc: Value = serde_json::from_slice(
            &std::fs::read(isolated.0.join("fixture-gc-window.json")).unwrap(),
        )
        .unwrap();
        let construction: Value = serde_json::from_slice(
            &std::fs::read(isolated.0.join("fixture-gc-construction.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(construction["gc_attempted"], true);
        assert_eq!(
            construction["exit"], 128,
            "Git GC must honor construction exclusion before kept-pack publication"
        );
        assert_eq!(gc["gc_attempted"], true);
        assert_eq!(
            gc["exists_after_gc"], true,
            "actual new parentless candidate must survive real GC before commit-tree stdout"
        );
        if pending {
            assert!(!output.status.success());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("TASK_CHECKPOINT_PENDING"),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                std::fs::read_dir(isolated.0.join(".git/objects/pack"))
                    .unwrap()
                    .any(|entry| entry
                        .unwrap()
                        .path()
                        .extension()
                        .is_some_and(|ext| ext == "keep")),
                "post-SQL failure retains actual provisional object home"
            );
            std::fs::remove_file(
                isolated
                    .0
                    .join(".git/refs/forge/state/forge-mcp/forge-mcp/m-gc/candidate.lock"),
            )
            .unwrap();
            let resumed = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
                .args([
                    "--root",
                    isolated.0.to_str().unwrap(),
                    "--format",
                    "json",
                    "milestone",
                    "db-recover",
                ])
                .output()
                .unwrap();
            assert!(
                resumed.status.success(),
                "{}",
                String::from_utf8_lossy(&resumed.stderr)
            );
            let stored = tasks::store(&isolated.0).unwrap().inspect(id).unwrap();
            assert_eq!(stored.stage, "review");
            assert_eq!(stored.status, "ready");
        } else {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        assert!(
            !std::fs::read_dir(isolated.0.join(".git/objects/pack"))
                .unwrap()
                .any(|entry| entry
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "keep")),
            "keeper durability releases only this task's temporary pack protection"
        );
        let candidate = gc["candidate"].as_str().unwrap();
        assert!(contextunity_forge_mcp::core::git::git_ancestor(
            &isolated.0,
            candidate,
            "refs/forge/state/forge-mcp/forge-mcp/m-gc/candidate"
        )
        .unwrap());
        let parents = contextunity_forge_mcp::core::git::git_stdout(
            &isolated.0,
            &["rev-list", "--parents", "-n", "1", candidate],
            "FIXTURE_PARENTS",
        )
        .unwrap();
        assert_eq!(
            parents.split_whitespace().count(),
            1,
            "candidate remains parentless"
        );
    }
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

    // Initialize git repository without tracking runtime state.
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
        .args(["add", "src", "docs", "forge-mcp.yaml"])
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
    let target = tasks_in_store[0].spec.target.clone();
    tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::SubtaskAdd,
            task_id: Some(task_id.to_string()),
            subtask_ref: Some("release-notes".into()),
            title: Some("Publish release notes\nand notify maintainers".into()),
            ..Default::default()
        },
    )
    .unwrap();
    tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::SubtaskAdd,
            task_id: Some(task_id.to_string()),
            subtask_ref: Some("cleanup-followup".into()),
            title: Some("Optional cleanup follow-up".into()),
            ..Default::default()
        },
    )
    .unwrap();
    tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::SubtaskUpdate,
            task_id: Some(task_id.to_string()),
            subtask_ref: Some("release-notes".into()),
            subtask_status: Some("completed".into()),
            evidence: Some("Release notes reviewed".into()),
            ..Default::default()
        },
    )
    .unwrap();
    tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::SubtaskUpdate,
            task_id: Some(task_id.to_string()),
            subtask_ref: Some("cleanup-followup".into()),
            subtask_status: Some("cancelled".into()),
            evidence: Some("Optional cleanup follow-up deferred beyond this task".into()),
            ..Default::default()
        },
    )
    .unwrap();
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

    // Healthy-first enrollment retains the actual active claim, not a manifest reconstruction.
    let recovery = db_recover(&root.0);
    assert!(
        recovery.is_ok(),
        "active claim checkpoint enrollment: {recovery:?}"
    );
    let enrolled = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    assert_eq!(enrolled.claim_revision, still_claimed.claim_revision);
    assert_eq!(enrolled.worker_id, still_claimed.worker_id);

    // A receipt writer owns the physical fence before its SQL commit. File-backed
    // readers must refuse even while the old SQL snapshot is still observable.
    let database = root.0.join(".forge/tasks.sqlite");
    let writer =
        contextunity_forge_mcp::db::tasks_store::recovery::lock_database(&database).unwrap();
    let manifest_path = root.0.join("docs/milestones/010-test.md");
    let original = std::fs::read_to_string(&manifest_path).unwrap();
    std::fs::write(
        &manifest_path,
        format!("{original}\n<!-- provisional delivery receipt -->\n"),
    )
    .unwrap();
    let composed = contextunity_forge_mcp::engine::milestones::show(&root.0, "m-test", true);
    std::fs::write(&manifest_path, original).unwrap();
    drop(writer);
    assert!(
        composed.is_err(),
        "precommit document must not be acknowledged: {composed:?}"
    );
    assert!(composed
        .unwrap_err()
        .to_string()
        .contains("TASK_CHECKPOINT_PENDING"));

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
    assert_eq!(contract_res["snapshot"]["commit"], contract_snap);
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
    let checkpoint_ref = format!("refs/forge/state/forge-mcp/{}", task_id.replace(':', "/"));
    let checkpoint = contextunity_forge_mcp::db::tasks_store::recovery::read_checkpoint(
        &root.0,
        &checkpoint_ref,
    )
    .unwrap();
    let permanent_pins = std::process::Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname)",
            "refs/forge/evidence/",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(permanent_pins.status.success());
    assert!(
        permanent_pins.stdout.is_empty(),
        "new task gates must not create global evidence pins"
    );
    assert_eq!(checkpoint.workspace, root.0.canonicalize().unwrap());
    assert_eq!(
        checkpoint.task.as_ref().unwrap().claim_revision,
        stored.claim_revision
    );
    // Only the real keeper retains the parentless contract candidate during isolated GC.
    let old_snapshot_ref = format!("refs/forge/snapshots/{}", task_id.replace(':', "/"));
    for args in [
        vec!["update-ref", "-d", old_snapshot_ref.as_str()],
        vec!["reflog", "expire", "--expire=now", "--all"],
        vec!["gc", "--prune=now"],
    ] {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&root.0)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    assert!(std::process::Command::new("git")
        .args(["cat-file", "-e", &contract_snap])
        .current_dir(&root.0)
        .status()
        .unwrap()
        .success());
    let before = tasks::store(&root.0)
        .unwrap()
        .inspect_details(task_id)
        .unwrap();
    let recover = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--root",
                root.0.to_str().unwrap(),
                "--format",
                "json",
                "milestone",
                "db-recover",
            ])
            .output()
            .unwrap()
    };
    // Persisted crash/corruption boundaries: recovery must not promote an old
    // claim from an unresolved intent, an incomplete cohort or a foreign owner.
    let keeper_commit = std::process::Command::new("git")
        .args(["rev-parse", &checkpoint_ref])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let keeper_commit = String::from_utf8(keeper_commit.stdout)
        .unwrap()
        .trim()
        .to_owned();
    for (field, replacement, diagnostic) in [
        (
            "intent",
            json!("unacknowledged isolated process intent"),
            "TASK_RECOVERY_AMBIGUOUS",
        ),
        (
            "cohort_id",
            json!("incomplete isolated publication"),
            "TASK_RECOVERY_AMBIGUOUS",
        ),
        (
            "project",
            json!("foreign"),
            "TASK_RECOVERY_CHECKPOINT_INVALID",
        ),
        ("version", json!(99), "TASK_RECOVERY_AMBIGUOUS"),
    ] {
        let mut state = serde_json::to_value(&checkpoint).unwrap();
        state[field] = replacement;
        let mut mutation = contextunity_forge_mcp::core::git::git_checkpoint(
            &root.0,
            &checkpoint_ref,
            &serde_json::to_vec(&state).unwrap(),
            std::slice::from_ref(&contract_snap),
        )
        .unwrap();
        if field == "intent" {
            assert!(
                contextunity_forge_mcp::engine::milestones::show(&root.0, "m-test", true)
                    .unwrap_err()
                    .to_string()
                    .contains("TASK_CHECKPOINT_PENDING")
            );
            let recovered = recover();
            assert!(
                !recovered.status.success(),
                "an arbitrary intent is not authority to revoke a claim"
            );
            assert_eq!(
                tasks::store(&root.0)
                    .unwrap()
                    .inspect_details(task_id)
                    .unwrap(),
                before
            );
            let mut genuine = serde_json::to_value(&checkpoint).unwrap();
            genuine["intent"] = json!("1-1");
            genuine["max_revision"] = json!(checkpoint.max_revision + 1);
            genuine["cohort"] = json!([]);
            genuine["cohort_id"] = json!("");
            for corrupt in [
                "reservation",
                "worker",
                "claim",
                "claims",
                "gates",
                "cohort",
                "cohort_id",
                "token",
            ] {
                assert!(std::process::Command::new("git")
                    .args(["update-ref", &checkpoint_ref, &keeper_commit])
                    .current_dir(&root.0)
                    .status()
                    .unwrap()
                    .success());
                let mut value = genuine.clone();
                match corrupt {
                    "reservation" => value["max_revision"] = json!(checkpoint.max_revision + 100),
                    "worker" => value["task"]["worker_id"] = json!("foreign-worker"),
                    "claim" => {
                        value["task"]["claim_revision"] = json!(checkpoint.max_revision + 10)
                    }
                    "claims" => value["claims"] = json!([]),
                    "gates" => value["gates"] = json!([]),
                    "cohort" => {
                        value["cohort"] = json!([[root.0.to_string_lossy(), checkpoint_ref]])
                    }
                    "cohort_id" => value["cohort_id"] = json!("foreign-cohort"),
                    "token" => value["intent"] = json!("invalid-token"),
                    _ => unreachable!(),
                }
                let corrupt_tip = contextunity_forge_mcp::core::git::git_checkpoint(
                    &root.0,
                    &checkpoint_ref,
                    &serde_json::to_vec(&value).unwrap(),
                    std::slice::from_ref(&contract_snap),
                )
                .unwrap();
                let refused = recover();
                assert!(!refused.status.success(), "must reject {corrupt}");
                assert_eq!(
                    tasks::store(&root.0)
                        .unwrap()
                        .inspect_details(task_id)
                        .unwrap(),
                    before,
                    "SQL changed after rejected {corrupt}"
                );
                assert_eq!(
                    contextunity_forge_mcp::core::git::git_stdout(
                        &root.0,
                        &["rev-parse", &checkpoint_ref],
                        "FIXTURE_REF"
                    )
                    .unwrap(),
                    corrupt_tip,
                    "ref changed after rejected {corrupt}"
                );
            }
            let peer_ref = format!("{}/second", checkpoint_ref.rsplit_once('/').unwrap().0);
            let peer_keeper = contextunity_forge_mcp::core::git::git_stdout(
                &root.0,
                &["rev-parse", &peer_ref],
                "FIXTURE_REF",
            )
            .unwrap();
            let peer = contextunity_forge_mcp::db::tasks_store::recovery::read_checkpoint(
                &root.0, &peer_ref,
            )
            .unwrap();
            for corrupt in ["token", "acknowledged_cohort"] {
                assert!(std::process::Command::new("git")
                    .args(["update-ref", &checkpoint_ref, &keeper_commit])
                    .current_dir(&root.0)
                    .status()
                    .unwrap()
                    .success());
                let first_tip = contextunity_forge_mcp::core::git::git_checkpoint(
                    &root.0,
                    &checkpoint_ref,
                    &serde_json::to_vec(&genuine).unwrap(),
                    std::slice::from_ref(&contract_snap),
                )
                .unwrap();
                let mut value = serde_json::to_value(&peer).unwrap();
                if corrupt == "token" {
                    value["intent"] = json!("2-2");
                    value["cohort"] = json!([]);
                    value["cohort_id"] = json!("");
                } else {
                    value["cohort_id"] = json!("foreign-cohort");
                }
                let peer_tip = contextunity_forge_mcp::core::git::git_checkpoint(
                    &root.0,
                    &peer_ref,
                    &serde_json::to_vec(&value).unwrap(),
                    &[],
                )
                .unwrap();
                assert!(
                    !recover().status.success(),
                    "must reject conflicting {corrupt}"
                );
                assert_eq!(
                    tasks::store(&root.0)
                        .unwrap()
                        .inspect_details(task_id)
                        .unwrap(),
                    before
                );
                for (reference, tip) in [(&checkpoint_ref, first_tip), (&peer_ref, peer_tip)] {
                    assert_eq!(
                        contextunity_forge_mcp::core::git::git_stdout(
                            &root.0,
                            &["rev-parse", reference],
                            "FIXTURE_REF"
                        )
                        .unwrap(),
                        tip
                    );
                }
                assert!(std::process::Command::new("git")
                    .args(["update-ref", &peer_ref, &peer_keeper])
                    .current_dir(&root.0)
                    .status()
                    .unwrap()
                    .success());
            }
            assert!(std::process::Command::new("git")
                .args(["update-ref", &checkpoint_ref, &keeper_commit])
                .current_dir(&root.0)
                .status()
                .unwrap()
                .success());
            mutation = contextunity_forge_mcp::core::git::git_checkpoint(
                &root.0,
                &checkpoint_ref,
                &serde_json::to_vec(&state).unwrap(),
                std::slice::from_ref(&contract_snap),
            )
            .unwrap();
        }
        let database = root.0.join(".forge/tasks.sqlite");
        let paths = [
            database.clone(),
            std::path::PathBuf::from(format!("{}-wal", database.display())),
            std::path::PathBuf::from(format!("{}-shm", database.display())),
        ];
        let originals = paths.each_ref().map(|path| std::fs::read(path).ok());
        for path in &paths {
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
        let refused = recover();
        assert!(
            !refused.status.success(),
            "{field}: {}",
            String::from_utf8_lossy(&refused.stdout)
        );
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains(diagnostic),
            "{field}: {}",
            String::from_utf8_lossy(&refused.stderr)
        );
        assert!(
            !database.exists(),
            "refusal must not install a partial database"
        );
        for (path, bytes) in paths.iter().zip(originals) {
            if let Some(bytes) = bytes {
                std::fs::write(path, bytes).unwrap();
            }
        }
        let restored = std::process::Command::new("git")
            .args(["update-ref", &checkpoint_ref, &keeper_commit, &mutation])
            .current_dir(&root.0)
            .output()
            .unwrap();
        assert!(restored.status.success());
    }
    let obsolete_archive = "docs/milestones/archive/009-obsolete.md";
    root.write(
        obsolete_archive,
        "---\nid: m-obsolete\nstatus: completed\nhandoff:\n  proof_shas: null\n---\n",
    );
    assert!(
        !recover().status.success(),
        "present malformed proof lists must not be treated as obsolete"
    );
    root.write(obsolete_archive, "---\nid: m-obsolete\nstatus: completed\n---\n```yaml\ntask_ref: obsolete\nunsupported_old_field: true\n```\n");
    let obsolete_active = "docs/milestones/009-obsolete.md";
    root.write(obsolete_active, "---\nid: m-obsolete\nstatus: active\n---\n```yaml\ntask_ref: obsolete\nunsupported_old_field: true\n```\n");
    let obsolete_ref = format!(
        "refs/forge/state/{}/{}/m-obsolete/obsolete",
        checkpoint.repository, checkpoint.project
    );
    let mut obsolete = serde_json::to_value(&checkpoint).unwrap();
    obsolete["milestone_id"] = json!("m-obsolete");
    obsolete["task_id"] = json!("forge-mcp/m-obsolete:obsolete");
    obsolete["task"] = Value::Null;
    for field in [
        "claims",
        "gates",
        "findings",
        "scopes",
        "dependencies",
        "submissions",
        "prepared_proofs",
        "cohort",
    ] {
        obsolete[field] = json!([]);
    }
    obsolete["cohort_id"] = json!("");
    contextunity_forge_mcp::core::git::git_checkpoint(
        &root.0,
        &obsolete_ref,
        &serde_json::to_vec(&obsolete).unwrap(),
        &[],
    )
    .unwrap();
    obsolete["intent"] = json!("2-2");
    let ignored_tip = contextunity_forge_mcp::core::git::git_checkpoint(
        &root.0,
        &obsolete_ref,
        &serde_json::to_vec(&obsolete).unwrap(),
        &[],
    )
    .unwrap();
    assert!(recover().status.success());
    assert_eq!(
        contextunity_forge_mcp::core::git::git_stdout(
            &root.0,
            &["rev-parse", &obsolete_ref],
            "FIXTURE_REF"
        )
        .unwrap(),
        ignored_tip,
        "obsolete intent must not be rewound"
    );
    let ignored_tip = contextunity_forge_mcp::core::git::git_checkpoint(
        &root.0,
        &obsolete_ref,
        b"unsupported obsolete keeper bytes",
        &[],
    )
    .unwrap();
    assert!(
        recover().status.success(),
        "obsolete keeper payload must not be parsed"
    );
    assert_eq!(
        contextunity_forge_mcp::core::git::git_stdout(
            &root.0,
            &["rev-parse", &obsolete_ref],
            "FIXTURE_REF"
        )
        .unwrap(),
        ignored_tip
    );
    assert!(
        std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--root",
                root.0.to_str().unwrap(),
                "task",
                "inspect",
                task_id,
                "--format",
                "json"
            ])
            .output()
            .unwrap()
            .status
            .success(),
        "obsolete keeper must not fence managed reads"
    );
    for corrupt in [false, true] {
        let database = root.0.join(".forge/tasks.sqlite");
        if corrupt {
            std::fs::write(&database, b"confirmed corrupt isolated task database").unwrap();
        } else {
            std::fs::remove_file(&database).unwrap();
        }
        let restored = recover();
        assert!(
            restored.status.success(),
            "{}",
            String::from_utf8_lossy(&restored.stderr)
        );
        let result: Value = serde_json::from_slice(&restored.stdout).unwrap();
        assert_eq!(result["status"], "recovered");
        assert_eq!(
            tasks::store(&root.0)
                .unwrap()
                .inspect_details(task_id)
                .unwrap(),
            before
        );
        if corrupt {
            let backup = std::path::PathBuf::from(result["original_backup"].as_str().unwrap());
            assert_eq!(
                std::fs::read(backup.join("0")).unwrap(),
                b"confirmed corrupt isolated task database"
            );
        }
        let repeated = recover();
        assert!(
            repeated.status.success(),
            "{}",
            String::from_utf8_lossy(&repeated.stderr)
        );
        assert_eq!(
            tasks::store(&root.0)
                .unwrap()
                .inspect_details(task_id)
                .unwrap(),
            before
        );
    }
    std::fs::remove_file(root.0.join(obsolete_archive)).unwrap();
    std::fs::remove_file(root.0.join(obsolete_active)).unwrap();
    assert!(std::process::Command::new("git")
        .args(["update-ref", "-d", &obsolete_ref])
        .current_dir(&root.0)
        .status()
        .unwrap()
        .success());
    let mut ev = evidence(&stored);
    ev.commit = None;
    ev.commit = Some("caller-selected-commit".into());
    let scoped_snapshot_error = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "build".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        scoped_snapshot_error,
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}'. supplied commit caller-selected-commit != scoped snapshot {contract_snap}. Do: resubmit with commit {contract_snap}."
        )
    );
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
    assert_eq!(build_res["snapshot"]["commit"], build_snap);
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

    // 3. Independent review: the gate rejects reusing the builder's worker id.
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "review".into(),
            worker_id: "builder".into(),
            worktree: root.0.to_str().unwrap().into(),
            ..Default::default()
        },
    )
    .unwrap();

    let mut review_store = tasks::store(&root.0).unwrap();
    let same_worker = review_store.inspect(task_id).unwrap();
    let same_worker_evidence = evidence(&same_worker);
    assert_eq!(
        review_store
            .submit(task_id, "review", &same_worker_evidence, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_REVIEW_NOT_INDEPENDENT: task '{task_id}'. reviewer worker 'builder' matches builder 'builder'. Do: submit review with a worker_id other than 'builder'."
        )
    );
    review_store
        .release_failed_claim(task_id, same_worker.claim_revision)
        .unwrap();
    drop(review_store);
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

    let mut review_store = tasks::store(&root.0).unwrap();
    let review_task = review_store.inspect(task_id).unwrap();
    let mut wrong_review_commit = evidence(&review_task);
    wrong_review_commit.commit = Some("caller-selected-commit".into());
    assert_eq!(
        review_store
            .submit(task_id, "review", &wrong_review_commit, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_CANDIDATE_MISMATCH: task '{task_id}'. supplied commit caller-selected-commit != candidate snapshot {build_snap}. Do: resubmit with commit {build_snap}."
        )
    );
    let explicit_review_commit_error = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "review".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&wrong_review_commit).unwrap(),
            findings: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        explicit_review_commit_error,
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}'. supplied commit caller-selected-commit != candidate snapshot {build_snap}. Do: resubmit with commit {build_snap}."
        )
    );

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
    assert_eq!(
        mismatch_err.to_string(),
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}' diverged from snapshot {}. D src/new_feature.rs, ?? src/new_feature.rs. Do: restore those paths to snapshot {}.",
            &build_snap[..7],
            &build_snap[..7]
        )
    );

    // Restore worktree to match candidate
    root.write("src/new_feature.rs", "pub fn new_feature() -> i32 { 42 }\n");
    drop(review_store);
    // A real ref lock acquired after gate validation forces publication to fail
    // AFTER SQL commits. Recovery may publish, but must not replay the gate.
    let keeper_lock = root.0.join(".git").join(format!("{checkpoint_ref}.lock"));
    let mut committed = ev.clone();
    committed.commit = Some(build_snap.clone());
    committed.candidate_baseline_head = Some(git_head(&root.0));
    let mut pending_store = tasks::store(&root.0).unwrap();
    let error = pending_store
        .submit_with_pre_persist(task_id, "review", &mut committed, "pass", None, |_| {
            std::fs::write(&keeper_lock, b"isolated publication rendezvous")?;
            Ok(None)
        })
        .unwrap_err();
    assert!(
        contextunity_forge_mcp::db::tasks_store::recovery::is_pending(&error),
        "{error:#}"
    );
    assert!(pending_store
        .inspect(task_id)
        .unwrap_err()
        .to_string()
        .contains("TASK_CHECKPOINT_PENDING"));
    let counts = |store: &TasksStore| -> (u64, u64, u64) {
        store.connection.query_row("SELECT (SELECT COUNT(*) FROM task_gates WHERE task_id=?1),(SELECT COUNT(*) FROM task_submissions WHERE task_id=?1),(SELECT revision FROM task_revisions WHERE task_id=?1)",[task_id],|row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap()
    };
    let committed_counts = counts(&pending_store);
    let failed_retry = recover();
    assert!(!failed_retry.status.success());
    assert!(String::from_utf8_lossy(&failed_retry.stderr).contains("TASK_CHECKPOINT_PENDING"));
    assert_eq!(counts(&pending_store), committed_counts);
    std::fs::remove_file(&keeper_lock).unwrap();
    drop(pending_store);
    let reconciled = recover();
    assert!(
        reconciled.status.success(),
        "{}",
        String::from_utf8_lossy(&reconciled.stderr)
    );
    let reconciled: Value = serde_json::from_slice(&reconciled.stdout).unwrap();
    assert_eq!(reconciled["checkpoint_reconciled"], true);
    let mut published = tasks::store(&root.0).unwrap();
    assert_eq!(published.inspect(task_id).unwrap().stage, "deliver");
    assert_eq!(counts(&published), committed_counts);
    published
        .submit_with_pre_persist(task_id, "review", &mut committed, "pass", None, |_| {
            panic!("accepted submission retry must not replay pre-persist side effects")
        })
        .unwrap();
    assert_eq!(counts(&published), committed_counts);
    drop(published);
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
    assert_eq!(review_res["snapshot"]["commit"], build_snap);

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

    let mut delivery_store = tasks::store(&root.0).unwrap();
    let delivery_task = delivery_store.inspect(task_id).unwrap();
    let missing_delivery_commit = evidence(&delivery_task);
    let (candidate_snapshot, _) = delivery_store
        .build_snapshot_candidate(task_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        delivery_store
            .submit(task_id, "deliver", &missing_delivery_commit, "pass", None)
            .unwrap_err()
            .to_string(),
        format!(
            "TASK_EVIDENCE_INVALID: task '{task_id}' for stage 'deliver'. delivery evidence missing required commit. Do: set commit to candidate snapshot {candidate_snapshot}."
        )
    );

    let review_evidence_before: String = delivery_store.connection.query_row(
        "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate='review' AND state='passed' ORDER BY revision DESC LIMIT 1",
        [task_id], |row| row.get(0)).unwrap();
    delivery_store
        .connection
        .execute(
            "UPDATE task_gates SET evidence=json_set(evidence, '$.commit', ?1) \
             WHERE task_id=?2 AND gate='review' AND state='passed'",
            rusqlite::params!["tampered-review-commit", task_id],
        )
        .unwrap();
    let review_source_error = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "deliver".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&missing_delivery_commit).unwrap(),
            findings: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        review_source_error,
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}'. review source 'review' commit != candidate snapshot {candidate_snapshot}. Do: resubmit review evidence with commit {candidate_snapshot}."
        )
    );
    delivery_store
        .connection
        .execute(
            "UPDATE task_gates SET evidence=?1 \
             WHERE task_id=?2 AND gate='review' AND state='passed'",
            rusqlite::params![review_evidence_before, task_id],
        )
        .unwrap();
    delivery_store
        .release_failed_claim(task_id, delivery_task.claim_revision)
        .unwrap();
    drop(delivery_store);
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "deliver".into(),
            worker_id: "delivery".into(),
            worktree: root.0.to_string_lossy().into_owned(),
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
    assert_eq!(
        deliver_err.to_string(),
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}' diverged from snapshot {}. D src/new_feature.rs, ?? src/new_feature.rs. Do: restore those paths to snapshot {}.",
            &build_snap[..7],
            &build_snap[..7]
        )
    );

    // Restore worktree to match candidate
    root.write("src/new_feature.rs", "pub fn new_feature() -> i32 { 42 }\n");
    let delivery_baseline = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(delivery_baseline.status.success());
    let delivery_baseline = String::from_utf8_lossy(&delivery_baseline.stdout)
        .trim()
        .to_owned();
    let milestone_path = root.0.join("docs/milestones/010-test.md");
    let milestone_before_delivery = std::fs::read_to_string(&milestone_path).unwrap();

    // Keep an unrelated staged edit in the real index while delivery uses its
    // isolated candidate tree. Delivery must preserve both the staged entry
    // and the committed baseline README in its parented tree.
    root.write("README.md", "# unrelated staged edit\n");
    assert!(std::process::Command::new("git")
        .args(["add", "README.md"])
        .current_dir(&root.0)
        .status()
        .unwrap()
        .success());
    let unrelated_staged_before = std::process::Command::new("git")
        .args(["ls-files", "--stage", "-z", "--", "README.md"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(unrelated_staged_before.status.success());
    let unrelated_staged_before = unrelated_staged_before.stdout;
    let index_before_delivery = std::process::Command::new("git")
        .args(["ls-files", "--stage", "-z"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(index_before_delivery.status.success());
    let index_before_delivery = index_before_delivery.stdout;

    // A rejected hook must leave both the branch and task receipt untouched.
    let hooks = root.0.join(".git/forced-hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 47\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let hook_path = hooks.to_string_lossy().to_string();
    assert!(std::process::Command::new("git")
        .args(["config", "core.hooksPath", &hook_path])
        .current_dir(&root.0)
        .status()
        .unwrap()
        .success());

    // Stop a real delivery process after its provisional receipt is written,
    // inside Git's pre-commit hook and before SQL COMMIT. FIFO rendezvous avoids
    // timing assumptions; the entire isolated child process group is terminated.
    let ready = root.0.join(".forge/crash-ready");
    let resume = root.0.join(".forge/crash-resume");
    assert!(std::process::Command::new("mkfifo")
        .args([&ready, &resume])
        .status()
        .unwrap()
        .success());
    let gates_before = tasks::store(&root.0)
        .unwrap()
        .inspect_details(task_id)
        .unwrap()["gates"]
        .clone();
    std::fs::write(
        &hook,
        "#!/bin/sh\nprintf ready > \"$FORGE_CRASH_READY\"\nread answer < \"$FORGE_CRASH_RESUME\"\n",
    )
    .unwrap();
    let mut child = std::process::Command::new("setsid")
        .arg(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            root.0.to_str().unwrap(),
            "task",
            "submit",
            task_id,
            "--stage",
            "deliver",
            "--action",
            "pass",
            "--evidence",
            &serde_json::to_string(&ev).unwrap(),
        ])
        .env("FORGE_CRASH_READY", &ready)
        .env("FORGE_CRASH_RESUME", &resume)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let (sent, received) = std::sync::mpsc::channel();
    let ready_reader = ready.clone();
    let rendezvous = std::thread::spawn(move || {
        sent.send(std::fs::read_to_string(ready_reader)).unwrap();
    });
    let signal = received.recv_timeout(std::time::Duration::from_secs(30));
    assert!(std::process::Command::new("kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .status()
        .unwrap()
        .success());
    let terminated = child.wait().unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(terminated.signal(), Some(9));
    assert_eq!(signal.unwrap().unwrap(), "ready");
    rendezvous.join().unwrap();
    assert_ne!(
        std::fs::read_to_string(&milestone_path).unwrap(),
        milestone_before_delivery
    );
    assert_eq!(git_head(&root.0), delivery_baseline);
    assert_eq!(
        tasks::store(&root.0)
            .unwrap()
            .inspect(task_id)
            .unwrap()
            .status,
        "in_progress"
    );
    assert!(
        contextunity_forge_mcp::engine::milestones::show(&root.0, "m-test", true)
            .unwrap_err()
            .to_string()
            .contains("TASK_CHECKPOINT_PENDING")
    );
    #[cfg(unix)]
    let released_before_rewind = {
        use std::os::unix::fs::PermissionsExt;
        let real_git = std::process::Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap();
        assert!(real_git.status.success());
        let real_git = String::from_utf8(real_git.stdout)
            .unwrap()
            .trim()
            .to_owned();
        root.write(".forge/rewind-fault-bin/git", "#!/bin/sh\nif [ \"$1\" = update-ref ] && [ \"$2\" = \"$RECOVERY_REF\" ]; then exit 73; fi\nexec \"$RECOVERY_REAL_GIT\" \"$@\"\n");
        let wrapper = root.0.join(".forge/rewind-fault-bin/git");
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let failed_rewind =
            std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
                .args([
                    "--root",
                    root.0.to_str().unwrap(),
                    "milestone",
                    "db-recover",
                    "--format",
                    "json",
                ])
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        wrapper.parent().unwrap().display(),
                        std::env::var("PATH").unwrap()
                    ),
                )
                .env("RECOVERY_REAL_GIT", real_git)
                .env("RECOVERY_REF", &checkpoint_ref)
                .output()
                .unwrap();
        assert!(!failed_rewind.status.success());
        assert!(
            String::from_utf8_lossy(&failed_rewind.stderr).contains("cannot rewind"),
            "{}",
            String::from_utf8_lossy(&failed_rewind.stderr)
        );
        let sql = rusqlite::Connection::open_with_flags(
            root.0.join(".forge/tasks.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let descriptor: String = sql
            .query_row(
                "SELECT descriptor FROM tasks WHERE task_id=?1",
                [task_id],
                |row| row.get(0),
            )
            .unwrap();
        let descriptor: Value = serde_json::from_str(&descriptor).unwrap();
        assert_eq!(descriptor["status"], "ready");
        assert!(descriptor["worker_id"].is_null() && descriptor["worktree"].is_null());
        descriptor["claim_revision"].as_u64()
    };
    #[cfg(not(unix))]
    let released_before_rewind: Option<u64> = None;
    let recovered = recover();
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    let released = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    assert_eq!(released.status, "ready");
    assert_eq!(released.stage, "deliver");
    assert!(released.worker_id.is_none() && released.worktree.is_none());
    assert!(released.claim_revision > ev.claim_revision);
    if let Some(revision) = released_before_rewind {
        assert_eq!(
            released.claim_revision, revision,
            "retry must not release the same claim twice"
        );
    }
    assert!(
        released.receipt.is_none(),
        "recovery must not accept the provisional receipt"
    );
    assert_eq!(
        tasks::store(&root.0)
            .unwrap()
            .inspect_details(task_id)
            .unwrap()["gates"],
        gates_before
    );
    assert!(recover().status.success());
    assert_eq!(
        tasks::store(&root.0)
            .unwrap()
            .inspect(task_id)
            .unwrap()
            .claim_revision,
        released.claim_revision
    );
    std::fs::write(&milestone_path, &milestone_before_delivery).unwrap();
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: task_id.into(),
            stage: "deliver".into(),
            worker_id: ev.worker_id.clone(),
            worktree: root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    ev = evidence(&tasks::store(&root.0).unwrap().inspect(task_id).unwrap());
    std::fs::write(&hook, "#!/bin/sh\nexit 47\n").unwrap();
    let hook_error = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "deliver".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        hook_error.contains("TASK_DELIVERY_HOOK_REJECTED"),
        "{hook_error}"
    );
    assert_eq!(
        std::fs::read_to_string(&milestone_path).unwrap(),
        milestone_before_delivery
    );
    let hook_head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&hook_head.stdout).trim(),
        delivery_baseline
    );
    let hook_index = std::process::Command::new("git")
        .args(["ls-files", "--stage", "-z"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert_eq!(hook_index.stdout, index_before_delivery);
    let after_hook_failure = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    assert_eq!(after_hook_failure.status, "in_progress");
    assert_eq!(after_hook_failure.stage, "deliver");
    assert!(std::process::Command::new("git")
        .args(["config", "--unset", "core.hooksPath"])
        .current_dir(&root.0)
        .status()
        .unwrap()
        .success());

    // A SQLite statement failure after the hooked commit must roll back the
    // candidate branch and restore the original milestone file as well.
    let db_path = root.0.join(".forge/tasks.sqlite");
    let trigger_db = rusqlite::Connection::open(&db_path).unwrap();
    trigger_db
        .execute_batch(
            "CREATE TRIGGER reject_delivery_gate BEFORE INSERT ON task_gates
             WHEN NEW.gate='deliver' BEGIN SELECT RAISE(ABORT,'forced delivery sqlite failure'); END;",
        )
        .unwrap();
    drop(trigger_db);
    let sqlite_error = tasks::submit(
        &root.0,
        tasks::Submit {
            task_id: task_id.into(),
            stage: "deliver".into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(&ev).unwrap(),
            findings: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        sqlite_error.contains("forced delivery sqlite failure"),
        "{sqlite_error}"
    );
    rusqlite::Connection::open(&db_path)
        .unwrap()
        .execute_batch("DROP TRIGGER reject_delivery_gate;")
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&milestone_path).unwrap(),
        milestone_before_delivery
    );
    let sqlite_rollback_head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&sqlite_rollback_head.stdout).trim(),
        delivery_baseline
    );
    let sqlite_rollback_index = std::process::Command::new("git")
        .args(["ls-files", "--stage", "-z"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert_eq!(sqlite_rollback_index.stdout, index_before_delivery);
    let after_sqlite_failure = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    assert_eq!(after_sqlite_failure.status, "in_progress");
    assert_eq!(after_sqlite_failure.stage, "deliver");

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

    let delivered_task = tasks::store(&root.0).unwrap().inspect(task_id).unwrap();
    let delivery_commit = delivered_task
        .receipt
        .as_ref()
        .and_then(|receipt| receipt.commit.as_deref())
        .unwrap()
        .to_owned();
    ev.commit = Some("caller-selected-commit".into());
    assert_eq!(
        tasks::submit(
            &root.0,
            tasks::Submit {
                task_id: task_id.into(),
                stage: "deliver".into(),
                action: tasks::Action::Pass,
                evidence: serde_json::to_value(&ev).unwrap(),
                findings: None,
            },
        )
        .unwrap_err()
        .to_string(),
        format!(
            "TASK_CANDIDATE_MISMATCH: {task_id}. task '{task_id}'. supplied commit caller-selected-commit != accepted receipt commit {delivery_commit}. Do: resubmit with commit {delivery_commit}."
        )
    );

    assert_eq!(deliver_res["status"], "completed");
    assert_eq!(
        deliver_res["snapshot"]["commit"], deliver_res["receipt"]["commit"],
        "delivery returns the auto-created commit SHA"
    );
    assert_eq!(delivery_commit.len(), 40);
    assert_ne!(delivery_commit, build_snap);
    assert_eq!(deliver_res["snapshot"]["commit"], delivery_commit);
    assert_eq!(deliver_res["receipt"]["commit"], delivery_commit);
    assert_eq!(
        deliver_res["receipt"]["commit"], delivery_commit,
        "the returned receipt must carry the delivery commit"
    );
    let delivery_message = std::process::Command::new("git")
        .args(["log", "-1", "--format=%B"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(delivery_message.status.success());
    let delivery_message = String::from_utf8(delivery_message.stdout).unwrap();
    let target_present = delivery_message.contains(&target);
    let completed_subtask_present =
        delivery_message.contains("Publish release notes and notify maintainers");
    let pending_subtask_excluded = !delivery_message.contains("Optional cleanup follow-up");
    let subtask_title_is_single_line =
        completed_subtask_present && !delivery_message.contains("\nand notify maintainers");
    assert!(
        target_present
            && completed_subtask_present
            && pending_subtask_excluded
            && subtask_title_is_single_line,
        "delivery commit must include task target/completed subtasks, exclude pending subtasks, and normalize title newlines; target_present={target_present}, completed_subtask_present={completed_subtask_present}, pending_subtask_excluded={pending_subtask_excluded}, subtask_title_is_single_line={subtask_title_is_single_line}, message={delivery_message:?}"
    );
    let unrelated_staged_after = std::process::Command::new("git")
        .args(["ls-files", "--stage", "-z", "--", "README.md"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert_eq!(unrelated_staged_after.stdout, unrelated_staged_before);
    let clean_task_paths = std::process::Command::new("git")
        .args([
            "status",
            "--short",
            "--untracked-files=no",
            "--",
            "src/new_feature.rs",
            "docs/milestones/010-test.md",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(clean_task_paths.status.success());
    assert!(
        clean_task_paths.stdout.is_empty(),
        "delivery task paths must be clean in the real index and worktree: {}",
        String::from_utf8_lossy(&clean_task_paths.stdout)
    );

    let committed_milestone = std::process::Command::new("git")
        .args([
            "show",
            &format!("{delivery_commit}:docs/milestones/010-test.md"),
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(committed_milestone.status.success());
    let committed_text = String::from_utf8(committed_milestone.stdout).unwrap();
    let committed_spec = Milestone::parse(&committed_text, "forge-mcp").unwrap();
    assert_eq!(committed_spec.tasks[0].status.as_deref(), Some("completed"));
    assert_eq!(
        committed_spec.tasks[0]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        None,
        "delivery commit must not embed its own SHA in the milestone receipt"
    );
    let committed_readme = std::process::Command::new("git")
        .args(["show", &format!("{delivery_commit}:README.md")])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(committed_readme.status.success());
    assert_eq!(committed_readme.stdout, b"# sibling change\n");
    assert!(std::process::Command::new("git")
        .args(["checkout", "HEAD", "--", "README.md"])
        .current_dir(&root.0)
        .status()
        .unwrap()
        .success());

    let written = std::fs::read_to_string(root.0.join("docs/milestones/010-test.md")).unwrap();
    let milestone = Milestone::parse(&written, "forge-mcp").unwrap();
    assert_eq!(
        milestone.tasks[0]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        None,
        "the active milestone receipt stays commitless until handoff"
    );

    // Inspect details shows gates with inspect_cmd and latest_snapshot
    let details = tasks::store(&root.0)
        .unwrap()
        .inspect_details(task_id)
        .unwrap();
    assert_eq!(details["receipt"]["commit"], delivery_commit);
    assert_eq!(details["latest_snapshot"]["commit"], build_snap);
    assert_eq!(
        details["latest_snapshot"]["inspect_cmd"],
        format!("git show {}", &build_snap[..7])
    );
    for gate in details["gates"].as_array().unwrap() {
        let evidence: serde_json::Value =
            serde_json::from_str(gate["evidence"].as_str().unwrap()).unwrap();
        let expected = if gate["stage"] == "deliver" {
            delivery_commit.as_str()
        } else {
            build_snap.as_str()
        };
        assert_eq!(gate["commit"], expected);
        assert_eq!(evidence["commit"], expected);
        if gate["stage"] == "build" {
            assert_eq!(evidence["candidate_baseline_head"], delivery_baseline);
        }
        if gate["stage"] == "deliver" {
            assert_eq!(evidence["candidate_baseline_head"], delivery_baseline);
        }
    }
    let sqlite_store = tasks::store(&root.0).unwrap();
    let build_evidence_json: String = sqlite_store
        .connection
        .query_row(
            "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate='build' AND state='passed'",
            [task_id],
            |row| row.get(0),
        )
        .unwrap();
    let build_evidence: serde_json::Value = serde_json::from_str(&build_evidence_json).unwrap();
    assert_eq!(build_evidence["commit"], build_snap);
    assert_eq!(build_evidence["candidate_baseline_head"], delivery_baseline);
    let delivery_evidence_json: String = sqlite_store
        .connection
        .query_row(
            "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate='deliver' AND state='passed'",
            [task_id],
            |row| row.get(0),
        )
        .unwrap();
    let delivery_evidence: serde_json::Value =
        serde_json::from_str(&delivery_evidence_json).unwrap();
    assert_eq!(delivery_evidence["commit"], delivery_commit);
    assert_eq!(
        delivery_evidence["candidate_baseline_head"],
        delivery_baseline
    );

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
    let milestone_branch = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let milestone_branch = String::from_utf8_lossy(&milestone_branch.stdout)
        .trim()
        .to_owned();
    assert!(std::process::Command::new("git")
        .args(["checkout", "-b", "task-second-delivery"])
        .current_dir(&root.0)
        .status()
        .unwrap()
        .success());
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

    let second_delivery_commit = tasks::store(&root.0)
        .unwrap()
        .inspect(&task_id_2)
        .unwrap()
        .receipt
        .and_then(|receipt| receipt.commit)
        .unwrap();
    assert!(std::process::Command::new("git")
        .args(["checkout", &milestone_branch])
        .current_dir(&root.0)
        .status()
        .unwrap()
        .success());
    assert!(std::process::Command::new("git")
        .args([
            "merge",
            "--no-ff",
            "task-second-delivery",
            "-m",
            "merge second task"
        ])
        .current_dir(&root.0)
        .status()
        .unwrap()
        .success());
    let merge_commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    let merge_commit = String::from_utf8_lossy(&merge_commit.stdout)
        .trim()
        .to_owned();
    for delivered in [&delivery_commit, &second_delivery_commit] {
        assert!(std::process::Command::new("git")
            .args(["merge-base", "--is-ancestor", delivered, &merge_commit])
            .current_dir(&root.0)
            .status()
            .unwrap()
            .success());
    }
    let merge_parent = std::process::Command::new("git")
        .args(["rev-parse", "HEAD^1"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&merge_parent.stdout).trim(),
        delivery_commit,
        "the merge landing's first parent is the fast-forwarded first delivery"
    );

    // A shared physical store is not an enrollment owner. Preserve unrelated
    // legacy provenance and an independent claimed milestone while updating the
    // current active task beside its two genuinely delivered predecessors.
    {
        let reference = "docs/milestones/010-test.md";
        let original = std::fs::read_to_string(root.0.join(reference)).unwrap();
        let extended = format!("{original}\n```yaml\ntask_ref: third\ntarget: Resume current work\nproof_policy: seam-test-first\nscope: [src/]\ndepends_on: [first, second]\n```\n");
        root.write(reference, &extended);
        let mut store = tasks::store(&root.0).unwrap();
        let current = Milestone::parse(&extended, "forge-mcp").unwrap();
        store.sync(&current, reference, &root.0).unwrap();
        let third = current.task_id(&current.tasks[2]);
        store
            .claim(
                &third,
                "contract",
                "current-owner",
                root.0.to_str().unwrap(),
            )
            .unwrap();
        let foreign_reference = "docs/milestones/020-independent.md";
        let foreign_text = "---\nid: m-independent\nstatus: active\ntitle: Independent\n---\n```yaml\ntask_ref: foreign\ntarget: Independent owner\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
        root.write(foreign_reference, foreign_text);
        let foreign = Milestone::parse(foreign_text, "forge-mcp").unwrap();
        store.sync(&foreign, foreign_reference, &root.0).unwrap();
        let foreign_id = foreign.task_id(&foreign.tasks[0]);
        store
            .claim(
                &foreign_id,
                "contract",
                "foreign-owner",
                root.0.to_str().unwrap(),
            )
            .unwrap();
        let raw_state = |store: &TasksStore, id: &str| {
            [
                "tasks",
                "task_revisions",
                "task_claims",
                "task_gates",
                "task_findings",
                "task_scope_paths",
                "task_dependencies",
                "task_subtasks",
                "task_submissions",
            ]
            .into_iter()
            .map(|table| {
                let mut statement = store
                    .connection
                    .prepare(&format!(
                        "SELECT * FROM {table} WHERE task_id=?1 ORDER BY rowid"
                    ))
                    .unwrap();
                let columns = statement.column_count();
                statement
                    .query_map([id], |row| {
                        (0..columns)
                            .map(|index| row.get::<_, rusqlite::types::Value>(index))
                            .collect::<rusqlite::Result<Vec<_>>>()
                    })
                    .unwrap()
                    .map(|row| format!("{table}:{:?}", row.unwrap()))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
        };
        let foreign_before = raw_state(&store, &foreign_id);
        let foreign_ref = format!(
            "refs/forge/state/forge-mcp/{}",
            foreign_id.replace(':', "/")
        );
        let foreign_keeper = || {
            let output = std::process::Command::new("git")
                .args(["rev-parse", &foreign_ref])
                .current_dir(&root.0)
                .output()
                .unwrap();
            assert!(output.status.success());
            output.stdout
        };
        let keeper_before = foreign_keeper();
        // Raw malformed history is confined to this isolated TempDir fixture.
        let legacy_id = "forge-mcp/m-retired:legacy";
        let mut legacy = serde_json::to_value(store.inspect(&third).unwrap()).unwrap();
        legacy["task_id"] = json!(legacy_id);
        legacy["milestone_ref"] = json!("docs/milestones/099-retired.md");
        legacy["authority_snapshot"] = Value::Null;
        store
            .connection
            .execute(
                "INSERT INTO tasks(task_id,milestone_ref,descriptor) VALUES(?1,?2,?3)",
                rusqlite::params![
                    legacy_id,
                    "docs/milestones/099-retired.md",
                    legacy.to_string()
                ],
            )
            .unwrap();
        store
            .connection
            .execute("INSERT INTO task_revisions VALUES(?1,901)", [legacy_id])
            .unwrap();
        store
            .connection
            .execute(
                "INSERT INTO task_findings VALUES(?1,900,'untouched legacy finding')",
                [legacy_id],
            )
            .unwrap();
        let legacy_before = raw_state(&store, legacy_id);
        let legacy_details = tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"inspect","task_id":legacy_id})).unwrap(),
        )
        .unwrap();
        assert_eq!(legacy_details["checkpoint_status"], "UNENROLLED");
        let mutation = store.subtask_add(&third, "owner-isolation", "Owned checkpoint only");
        assert!(
            mutation.is_ok(),
            "current mutation must not enroll unrelated legacy state: {mutation:?}"
        );
        assert_eq!(raw_state(&store, legacy_id), legacy_before);
        assert_eq!(raw_state(&store, &foreign_id), foreign_before);
        assert_eq!(foreign_keeper(), keeper_before);
        for invalid in ["missing-authority", "stale-authority"] {
            if invalid == "stale-authority" {
                legacy["authority_snapshot"] =
                    serde_json::to_value(store.inspect(&third).unwrap().authority_snapshot)
                        .unwrap();
                store
                    .connection
                    .execute(
                        "UPDATE tasks SET descriptor=?2 WHERE task_id=?1",
                        rusqlite::params![legacy_id, legacy.to_string()],
                    )
                    .unwrap();
            }
            let unchanged = raw_state(&store, legacy_id);
            let error = store
                .subtask_add(legacy_id, "refuse", "Never admit stale authority")
                .unwrap_err();
            assert!(
                format!("{error:#}").contains("TASK_RECOVERY_"),
                "{invalid}: {error:#}"
            );
            assert_eq!(raw_state(&store, legacy_id), unchanged);
            store
                .subtask_update(&third, "owner-isolation", "in_progress", Some(invalid))
                .unwrap();
            assert_eq!(raw_state(&store, legacy_id), unchanged);
            assert_eq!(raw_state(&store, &foreign_id), foreign_before);
            assert_eq!(foreign_keeper(), keeper_before);
            store
                .connection
                .execute(
                    "INSERT INTO task_dependencies VALUES(?1,?2,1)",
                    rusqlite::params![legacy_id, third],
                )
                .unwrap();
            let unchanged = raw_state(&store, legacy_id);
            let owned_before = raw_state(&store, &third);
            let mut cancelled = current.clone();
            cancelled.tasks[2].status = Some("cancelled".into());
            let error = store.sync(&cancelled, reference, &root.0).unwrap_err();
            assert!(
                format!("{error:#}").contains("TASK_RECOVERY_"),
                "affected {invalid}: {error:#}"
            );
            assert_eq!(raw_state(&store, legacy_id), unchanged);
            assert_eq!(raw_state(&store, &third), owned_before);
            store
                .connection
                .execute(
                    "DELETE FROM task_dependencies WHERE task_id=?1",
                    [legacy_id],
                )
                .unwrap();
        }
        let coverage = recover();
        assert!(
            coverage.status.success(),
            "{}",
            String::from_utf8_lossy(&coverage.stderr)
        );
        let coverage: Value = serde_json::from_slice(&coverage.stdout).unwrap();
        assert!(coverage["unenrolled_tasks"]
            .as_array()
            .unwrap()
            .contains(&json!(legacy_id)));
        assert_eq!(coverage["legacy_cleanup_complete"], false);
        assert_eq!(raw_state(&store, &foreign_id), foreign_before);
        assert_eq!(foreign_keeper(), keeper_before);
        // A new/forgotten propagation path cannot commit outside the declared
        // owner cohort, even when introduced by a real SQLite trigger.
        store.connection.execute_batch(&format!("CREATE TRIGGER undeclared_owner_delta AFTER INSERT ON task_subtasks WHEN NEW.task_id='{third}' AND NEW.subtask_ref='unexpected-delta' BEGIN UPDATE task_revisions SET revision=revision+1 WHERE task_id='{foreign_id}'; END;")).unwrap();
        let unchanged = raw_state(&store, &third);
        let error = store
            .subtask_add(&third, "unexpected-delta", "Reject an uncovered writer")
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("TASK_CHECKPOINT_COHORT_UNDECLARED"),
            "{error:#}"
        );
        assert_eq!(raw_state(&store, &third), unchanged);
        assert_eq!(raw_state(&store, &foreign_id), foreign_before);
        assert_eq!(foreign_keeper(), keeper_before);
        store
            .connection
            .execute_batch("DROP TRIGGER undeclared_owner_delta")
            .unwrap();
        let members = [task_id.as_str(), task_id_2.as_str(), third.as_str()];
        let before = members.map(|id| store.inspect_details(id).unwrap());
        drop(store);
        std::fs::remove_file(root.0.join(".forge/tasks.sqlite")).unwrap();
        let restored = recover();
        assert!(
            restored.status.success(),
            "{}",
            String::from_utf8_lossy(&restored.stderr)
        );
        let mut store = tasks::store(&root.0).unwrap();
        for (id, expected) in members.into_iter().zip(before) {
            assert_eq!(store.inspect_details(id).unwrap(), expected, "{id}");
        }
        assert_eq!(raw_state(&store, &foreign_id), foreign_before);
        assert!(
            store.inspect(legacy_id).is_err(),
            "unenrolled legacy history is not claimed as recoverable"
        );
        store.delete(Some(&third), None, true, i64::MAX).unwrap();
        store
            .delete(Some(&foreign_id), None, true, i64::MAX)
            .unwrap();
        root.write(reference, &original);
        std::fs::remove_file(root.0.join(foreign_reference)).unwrap();
    }

    // Now handoff milestone
    handoff_with_fixture_evidence(&root.0, "m-test", None, "cargo test", 2, 0).unwrap();

    let archive_path = root.0.join("docs/milestones/archive/010-test.md");
    let archived_text = std::fs::read_to_string(&archive_path).unwrap();
    let archived = Milestone::parse(&archived_text, "forge-mcp").unwrap();
    assert_eq!(
        archived.tasks[0]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        Some(delivery_commit.as_str()),
        "first task keeps its reachable accepted delivery pointer"
    );
    assert_eq!(
        archived.tasks[1]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        Some(second_delivery_commit.as_str()),
        "second task keeps its reachable accepted delivery pointer across merge"
    );
    assert_ne!(delivery_commit, merge_commit);
    assert!(tasks::store(&root.0)
        .unwrap()
        .list(None, "all", None)
        .unwrap()
        .is_empty());

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

    let task_b_id = "forge-mcp/m-conflict:task_b";
    let task_a_id = "forge-mcp/m-conflict:task_a";

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

    // Dynamic extensions stay owned; an inactive requester must still fail closed.
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

    store.reset(task_a_id).unwrap();

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
        .sync(
            &claim_milestone,
            "docs/010-claim-conflict.md",
            &claim_root.0,
        )
        .unwrap();
    let owner_id = "forge-mcp/m-claim-conflict:owner";
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
        .sync(
            &claim_milestone,
            "docs/010-claim-conflict.md",
            &claim_root.0,
        )
        .unwrap();
    let claim_error = tasks::claim(
        &claim_root.0,
        tasks::Claim {
            task_id: "forge-mcp/m-claim-conflict:successor".into(),
            stage: "contract".into(),
            worker_id: "successor".into(),
            worktree: claim_root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(claim_error.contains("TASK_NOT_READY"), "{claim_error}");
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
        evidence: json!({"command_proof":{"command":"cargo test --test acdd","exit_code":0,"tests_passed":1,"tests_failed":0}}),
        review: passing_review_proof(),
        decision: "pass".into(),
        rollup: None,
    });
    let mut release_store = TasksStore::open(&release_root.0.join(".forge/tasks.sqlite")).unwrap();
    release_store
        .sync(
            &release_milestone,
            "docs/010-scope-release.md",
            &release_root.0,
        )
        .unwrap();
    let completed_owner_id = "forge-mcp/m-scope-release:completed_owner";
    let retained_scope_count: i64 = release_store
        .connection
        .query_row(
            "SELECT count(*) FROM task_scope_paths WHERE task_id=?1",
            [completed_owner_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retained_scope_count, 1);
    let successor_id = "forge-mcp/m-scope-release:successor";
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

    let (protected_root, mut protected_store, protected_milestone) = fixture();
    let protected_id = protected_milestone.task_id(&protected_milestone.tasks[0]);
    let before_extension = protected_store.inspect(&protected_id).unwrap();
    let before_paths: i64 = protected_store
        .connection
        .query_row(
            "SELECT count(*) FROM task_scope_paths WHERE task_id=?1",
            [&protected_id],
            |row| row.get(0),
        )
        .unwrap();
    let extension_error = protected_store
        .extend_scope(
            &protected_id,
            &[".forge/acdd/profile.yaml".into()],
            &protected_root.0,
        )
        .unwrap_err();
    assert!(extension_error.to_string().contains("TASK_SCOPE_PROTECTED"));
    let after_extension = protected_store.inspect(&protected_id).unwrap();
    assert_eq!(after_extension.spec.scope, before_extension.spec.scope);
    assert_eq!(after_extension.status, before_extension.status);
    let after_paths: i64 = protected_store
        .connection
        .query_row(
            "SELECT count(*) FROM task_scope_paths WHERE task_id=?1",
            [&protected_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        after_paths, before_paths,
        "rejected extension added scope rows"
    );

    for tracked in [false, true] {
        let root = ScopedWorkspace::new(if tracked {
            "forge_protected_tracked"
        } else {
            "forge_protected_untracked"
        });
        root.write("src/lib.rs", "pub fn protected() {}\n");
        root.write(".forge/acdd/profile.yaml", "\n");
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        let contract = "---\nid: m-protected\ntitle: Protected scope\ndoc_type: contract\nstatus: active\n---\n# Protected scope\n```yaml\ntask_ref: protected\ntarget: Reject dirty protected configuration\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
        root.write("docs/010-protected.md", contract);
        let milestone = Milestone::parse(contract, "forge-mcp").unwrap();
        let task_id = milestone.task_id(&milestone.tasks[0]);
        let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
        store
            .sync(&milestone, "docs/010-protected.md", &root.0)
            .unwrap();
        if tracked {
            let added = std::process::Command::new("git")
                .args(["add", "--", ".forge/acdd/profile.yaml"])
                .current_dir(&root.0)
                .output()
                .unwrap();
            assert!(added.status.success());
            let committed = std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=ACDD test",
                    "-c",
                    "user.email=acdd-test@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "-m",
                    "track protected profile",
                ])
                .current_dir(&root.0)
                .output()
                .unwrap();
            assert!(
                committed.status.success(),
                "{}",
                String::from_utf8_lossy(&committed.stderr)
            );
            root.write(".forge/acdd/profile.yaml", "# changed after commit\n");
        }

        let before = store.inspect(&task_id).unwrap();
        let claims_before: i64 = store
            .connection
            .query_row(
                "SELECT count(*) FROM task_claims WHERE task_id=?1",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();
        let error = tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: task_id.clone(),
                stage: "contract".into(),
                worker_id: "protected-scope-check".into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("TASK_SCOPE_VIOLATION"),
            "tracked={tracked}: {error:#}"
        );
        let after = store.inspect(&task_id).unwrap();
        assert_eq!(after.status, before.status);
        assert_eq!(after.stage, before.stage);
        assert_eq!(after.claim_revision, before.claim_revision);
        let claims_after: i64 = store
            .connection
            .query_row(
                "SELECT count(*) FROM task_claims WHERE task_id=?1",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            claims_after, claims_before,
            "tracked={tracked} claim mutated DB"
        );
    }

    let admitted_root = ScopedWorkspace::new("forge_protected_initial_scope");
    admitted_root.write(".forge/acdd/profile.yaml", "\n");
    admitted_root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let admitted_contract = "---\nid: m-protected-admitted\ntitle: Protected scope admitted\ndoc_type: contract\nstatus: active\n---\n# Protected scope admitted\n```yaml\ntask_ref: protected\ntarget: Admit configuration at planning time\nproof_policy: seam-test-first\nscope: [.forge/acdd/profile.yaml]\nscope_roots: [.forge/acdd/]\n```\n";
    admitted_root.write("docs/010-protected-admitted.md", admitted_contract);
    let admitted_milestone = Milestone::parse(admitted_contract, "forge-mcp").unwrap();
    let admitted_id = admitted_milestone.task_id(&admitted_milestone.tasks[0]);
    let mut admitted_store =
        TasksStore::open(&admitted_root.0.join(".forge/tasks.sqlite")).unwrap();
    admitted_store
        .sync(
            &admitted_milestone,
            "docs/010-protected-admitted.md",
            &admitted_root.0,
        )
        .unwrap();
    let admitted = tasks::claim(
        &admitted_root.0,
        tasks::Claim {
            task_id: admitted_id.clone(),
            stage: "contract".into(),
            worker_id: "admitted-protected-scope".into(),
            worktree: admitted_root.0.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(admitted["status"], "in_progress");
    assert_eq!(
        admitted_store.inspect(&admitted_id).unwrap().initial_scope,
        vec![".forge/acdd/profile.yaml"]
    );
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

    let catalogue_id = "forge-mcp/m-universal:catalogue_task";
    let payment_id = "forge-mcp/m-universal:payment_task";

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
    let installer_id = "forge-mcp/m-root:installer_task";

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
#[cfg(unix)]
fn task_profile_resolution_rejects_file_and_parent_symlink_escapes() {
    use sha2::Digest;
    use std::os::unix::fs::symlink;

    let external = Workspace::new();
    let profile = "roles:\n  independent_reviewer:\n    model: outside-workspace\n";
    external.write("profiles/team.yaml", profile);
    let digest = format!("{:x}", sha2::Sha256::digest(profile.as_bytes()));

    for (name, reference, link_path) in [
        (
            "file",
            "profile-link.yaml",
            external.root().join("profiles/team.yaml"),
        ),
        (
            "parent",
            "profile-dir/team.yaml",
            external.root().join("profiles"),
        ),
    ] {
        let root = ScopedWorkspace::new(&format!("forge_profile_symlink_{name}"));
        root.write("src/lib.rs", "pub fn profile() {}\n");
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        let link = root.0.join(if name == "file" {
            "profile-link.yaml"
        } else {
            "profile-dir"
        });
        symlink(&link_path, &link).unwrap();
        let manifest = format!(
            "---\nid: m-profile-{name}\ntitle: Profile path\ndoc_type: contract\nstatus: active\nacdd_profile: {reference}:{digest}\n---\n# Profile path\n```yaml\ntask_ref: scoped\ntarget: Confine profile resolution\nproof_policy: seam-test-first\nscope: [src/]\n```\n"
        );
        root.write("docs/milestones/010-profile.md", &manifest);

        let error = tasks::manage(
            &root.0,
            serde_json::from_value(json!({
                "action": "sync",
                "milestone_ref": "docs/milestones/010-profile.md"
            }))
            .unwrap(),
        )
        .expect_err("pinned profile symlink escape must fail closed");
        assert!(
            error.to_string().contains(
                "TASK_SCOPE_INVALID: docs/milestones/010-profile.md. symlink escape. Do:"
            ),
            "{name} symlink should be rejected as an escape: {error}"
        );
    }

    let external_profile =
        "roles:\n  contract_author:\n    models:\n      - model: external-profile-escape\n";
    let in_root_profile =
        "roles:\n  contract_author:\n    models:\n      - model: in-root-profile\n";
    let external_default = Workspace::new();
    external_default.write("profiles/profile.yaml", external_profile);
    let mut automatic_default_escapes = Vec::new();

    for (name, link_kind, expected_model) in [
        ("default_file", "file", None),
        ("default_parent", "parent", None),
        ("default_local", "local", Some("in-root-profile")),
    ] {
        let root = ScopedWorkspace::new(&format!("forge_automatic_profile_{name}"));
        root.write("src/lib.rs", "pub fn profile() {}\n");
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nagents_guidance: AGENTS.md\n",
        );
        root.write(
            "AGENTS.md",
            "# Agent instructions\nLoad the contextunity-forge skill.\n",
        );
        root.write(
            ".agents/skills/contextunity-forge/SKILL.md",
            "# ContextUnity Forge\n",
        );
        root.write(".gitignore", CANONICAL_FORGE_GITIGNORE);

        match link_kind {
            "file" => {
                std::fs::create_dir_all(root.0.join(".forge/acdd")).unwrap();
                symlink(
                    external_default.root().join("profiles/profile.yaml"),
                    root.0.join(".forge/acdd/profile.yaml"),
                )
                .unwrap();
            }
            "parent" => {
                std::fs::create_dir_all(root.0.join(".forge")).unwrap();
                symlink(
                    external_default.root().join("profiles"),
                    root.0.join(".forge/acdd"),
                )
                .unwrap();
            }
            "local" => root.write(".forge/acdd/profile.yaml", in_root_profile),
            _ => unreachable!("profile case is declared in the table"),
        }

        root.write(
            "docs/milestones/010-profile.md",
            "---\nid: m-automatic-profile\ntitle: Automatic profile\ndoc_type: contract\nstatus: active\n---\n# Automatic profile\n```yaml\ntask_ref: scoped\ntarget: Confine automatic profile resolution\nproof_policy: seam-test-first\nscope: [src/]\n```\n",
        );

        let sync = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--format",
                "json",
                "--root",
                root.0.to_str().unwrap(),
                "task",
                "sync",
                "docs/milestones/010-profile.md",
            ])
            .output()
            .unwrap();

        if let Some(expected_model) = expected_model {
            assert!(
                sync.status.success(),
                "ordinary in-root default profile should load: {}",
                String::from_utf8_lossy(&sync.stderr)
            );
            let synced: Value = serde_json::from_slice(&sync.stdout).unwrap();
            let task_id = synced["tasks"][0]["task_id"].as_str().unwrap();
            let inspect = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
                .args([
                    "--format",
                    "json",
                    "--root",
                    root.0.to_str().unwrap(),
                    "task",
                    "inspect",
                    task_id,
                ])
                .output()
                .unwrap();
            assert!(
                inspect.status.success(),
                "ordinary in-root task inspection should succeed: {}",
                String::from_utf8_lossy(&inspect.stderr)
            );
            let inspected: Value = serde_json::from_slice(&inspect.stdout).unwrap();
            assert!(inspected["workflow_guidance"]["role_guide"]
                .as_str()
                .unwrap()
                .contains(expected_model));
        } else if sync.status.success() {
            let synced: Value = serde_json::from_slice(&sync.stdout).unwrap();
            let task_id = synced["tasks"][0]["task_id"].as_str().unwrap();
            let inspect = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
                .args([
                    "--format",
                    "json",
                    "--root",
                    root.0.to_str().unwrap(),
                    "task",
                    "inspect",
                    task_id,
                ])
                .output()
                .unwrap();
            let exposed_model = if inspect.status.success() {
                let inspected: Value = serde_json::from_slice(&inspect.stdout).unwrap();
                inspected["workflow_guidance"]["role_guide"]
                    .as_str()
                    .map(str::to_owned)
            } else {
                None
            };
            automatic_default_escapes.push(format!(
                "{name}: sync succeeded, inspect exit={}, exposed model={exposed_model:?}, stderr={}",
                inspect.status,
                String::from_utf8_lossy(&inspect.stderr)
            ));
        } else {
            assert!(
                String::from_utf8_lossy(&sync.stderr).contains(
                    "TASK_SCOPE_INVALID: docs/milestones/010-profile.md. symlink escape. Do:"
                ),
                "automatic default {name} symlink should fail as an escape: {}",
                String::from_utf8_lossy(&sync.stderr)
            );
        }
    }
    assert!(
        automatic_default_escapes.is_empty(),
        "automatic default profile symlink escapes: {automatic_default_escapes:?}"
    );
}

#[test]
fn linked_pinned_tasks_ignore_linked_workspace_profile_overrides() {
    use sha2::Digest;
    let task_profile = "roles:\n  builder:\n    models:\n      - model: linked-task-override\n";
    let digest = format!("{:x}", sha2::Sha256::digest(task_profile.as_bytes()));
    let invalid_linked_base = "gates:\n  - id: linked_only\n    role: builder\n    proof: none\n";

    for (case, linked_base_path) in [
        ("forge_config", "forge-mcp.yaml"),
        ("repository_profile", ".forge/acdd/profile.yaml"),
    ] {
        let root = ScopedWorkspace::new(&format!("forge_linked_profile_{case}"));
        root.write("src/lib.rs", "pub fn primary() {}\n");
        root.write("linked/src/lib.rs", "pub fn linked() {}\n");
        root.write("linked/AGENTS.md", "# Linked instructions\n");
        root.write("linked/profiles/task.yaml", task_profile);
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
            root.write(&format!("linked/{linked_base_path}"), invalid_linked_base);
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
            &root.0,
            serde_json::from_value(json!({"action":"sync","workspace":"linked"})).unwrap(),
        )
        .unwrap_or_else(|error| panic!("{case} linked base profile leaked into task: {error}"));
        let imported_tasks = imported["tasks"].as_array().unwrap();
        assert_eq!(imported_tasks.len(), 1);
        let task_id = imported_tasks[0]["task_id"].as_str().unwrap();
        assert!(task_id.starts_with("linked-project/"));

        tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"inspect","task_id":task_id})).unwrap(),
        )
        .unwrap_or_else(|error| {
            panic!("{case} pinned task inspection used linked base profile: {error}")
        });
    }
}

#[test]
fn scoped_commit_commits_staged_deletion_and_preserves_unrelated_staged_work() {
    use contextunity_forge_mcp::core::git::git_scoped_commit;

    let root = ScopedWorkspace::new("scoped_commit_deletions");
    root.write("src/a.rs", "pub fn a() -> u32 { 1 }\n");
    root.write("src/b.rs", "pub fn b() -> u32 { 2 }\n");
    super::super::support::git(&root.0, &["add", "src/a.rs", "src/b.rs"]);
    super::super::support::git(&root.0, &["commit", "-m", "init"]);

    // Stage unrelated change in the real index
    root.write("README.md", "# Unrelated work\n");
    super::super::support::git(&root.0, &["add", "README.md"]);

    // Delete src/a.rs using git rm (staged deletion in real index)
    super::super::support::git(&root.0, &["rm", "src/a.rs"]);

    // Call git_scoped_commit with scope restricted strictly to src/a.rs
    let commit = git_scoped_commit(&root.0, &["src/a.rs".into()], "remove a")
        .expect("git_scoped_commit should commit staged deletion of scoped file");

    // Verify commit tree: src/b.rs remains, src/a.rs is removed, README.md is not in commit
    let output = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", &commit])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(output.status.success());
    let tree_files = String::from_utf8_lossy(&output.stdout);
    let files: Vec<&str> = tree_files.lines().collect();
    assert_eq!(files, vec!["src/b.rs"], "commit must contain only src/b.rs");

    // Verify that unrelated staged change README.md is still staged in .git/index
    let status_output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(status_output.status.success());
    let status = String::from_utf8_lossy(&status_output.stdout);
    assert!(
        status.contains("A  README.md"),
        "real index must retain unrelated staged README.md: {status}"
    );
}
