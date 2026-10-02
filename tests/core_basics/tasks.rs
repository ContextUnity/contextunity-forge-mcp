use super::ScopedWorkspace;
use contextunity_forge_mcp::{
    core::tasks::{gates::Evidence, Milestone, Receipt, GATES},
    db::{
        tasks_store::{TasksStore, RETENTION_SECONDS},
        writer,
    },
    engine::tasks,
};
use serde_json::json;

fn passing_review_proof() -> serde_json::Value {
    let contours: serde_json::Map<String, serde_json::Value> =
        contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS
            .iter()
            .map(|name| {
                (
                    (*name).into(),
                    json!({"applicable":true,"evidence":"public-seam verification"}),
                )
            })
            .collect();
    json!({"decision":"pass","evidence_ref":"review.json","contours":contours})
}

fn evidence(task: &contextunity_forge_mcp::db::tasks_store::Task) -> Evidence {
    Evidence {
        task_id: task.task_id.clone(),
        stage: GATES[task.gate].into(),
        claim_revision: task.claim_revision,
        contract_revision: task.contract_revision,
        worker_id: task.worker_id.clone().unwrap(),
        worktree: task.worktree.clone().unwrap(),
        commit: "0123456789abcdef0123456789abcdef01234567".into(),
        proof: json!({"command":"cargo test","result":"passed","artifacts":[]}),
    }
}

const SPEC: &str = "---\nid: m-test\ntitle: Tasks\ndoc_type: contract\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: first\ntarget: Deliver first\nproof_policy: seam-test-first\nscope: [src/]\n```\n```yaml\ntask_ref: second\ntarget: Deliver second\nproof_policy: seam-test-first\nscope: [src/]\ndepends_on: [first]\n```\n";

fn fixture() -> (ScopedWorkspace, TasksStore, Milestone) {
    let root = ScopedWorkspace::new("forge_tasks");
    root.write("src/lib.rs", "pub fn example() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndoc_roots: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("docs/010-test.md", SPEC);
    let milestone = Milestone::parse(SPEC, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store.sync(&milestone, "docs/010-test.md", &root.0).unwrap();
    (root, store, milestone)
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
    root.write("forge-mcp.yaml", &format!("roots: [src]\nlinked_workspaces:\n  - name: traverse\n    path: {}\n    tasks:\n      enabled: true\n      milestones_dir: contracts\n      agents_md: docs/AGENTS.md\n  - name: missing\n    path: {}\n    tasks: {{enabled: true}}\n  - name: empty\n    path: {}\n    tasks: {{enabled: true}}\n  - name: disabled\n    path: {}\n    tasks: {{enabled: false}}\n  - name: index-only\n    path: {}\n",linked.0.display(),missing.0.display(),empty.0.display(),linked.0.display(),linked.0.display()));
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
                stage: "design".into(),
                worker_id: "linked-builder".into(),
                worktree: worktree.to_string_lossy().into_owned(),
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
            stage: "design".into(),
            worker_id: "builder".into(),
            worktree: worktree.0.to_string_lossy().into_owned(),
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
            stage: "design".into(),
            worker_id: "library-builder".into(),
            worktree: main.0.join("src/library").to_string_lossy().into_owned(),
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
        .claim(&id, "design", "builder", root.0.to_str().unwrap())
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
            stage: "design".into(),
            worker_id: "builder".into(),
            worktree: missing.to_string_lossy().into_owned(),
        },
    )
    .unwrap_err();
    assert!(error.to_string().starts_with("WORKTREE_NOT_FOUND"));
    let task = tasks::store(&root.0).unwrap().inspect(&id).unwrap();
    assert_eq!(task.status, "ready");
    assert!(task.worker_id.is_none());
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
            status: None,
            stage: None,
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
                store.claim(&id, "design", &format!("worker-{n}"), &worktree)
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
        .extend_scope(&id, &["src/next.rs".into()], &root.0)
        .unwrap();
    assert!(extended.spec.scope.contains(&"src/next.rs".into()));
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
fn gates_validate_disk_receipts_and_retention_outcomes() {
    let (root, mut store, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let mut build_proof = json!(null);
    let mut review_proof = json!(null);
    for (gate, stage) in GATES.iter().enumerate() {
        let worker = if gate == 3 { "reviewer" } else { "builder" };
        let task = store
            .claim(&id, stage, worker, root.0.to_str().unwrap())
            .unwrap();
        let evidence = Evidence {
            task_id: id.clone(),
            stage: stage.to_string(),
            claim_revision: task.claim_revision,
            contract_revision: task.contract_revision,
            worker_id: worker.into(),
            worktree: root.0.to_str().unwrap().into(),
            commit: "0123456789abcdef0123456789abcdef01234567".into(),
            proof: if gate == 3 {
                passing_review_proof()
            } else {
                json!({"command":"cargo test","result":"passed","artifacts":[]})
            },
        };
        if gate == 2 {
            build_proof = evidence.proof.clone();
        }
        if gate == 3 {
            review_proof = evidence.proof.clone();
        }
        if gate == 4 {
            assert!(store.submit(&id, stage, &evidence, "pass", None).is_err());
            let mut completed = milestone.clone();
            completed.tasks[0].status = Some("completed".into());
            completed.tasks[0].receipt = Some(Receipt {
                commit: evidence.commit.clone(),
                contract_revision: 1,
                passed_at: "2026-10-02T00:00:00Z".into(),
                evidence: build_proof.clone(),
                review: review_proof.clone(),
                decision: "pass".into(),
            });
            let text=format!("---\nid: m-test\ntitle: Tasks\ndoc_type: contract\ninvariants: [isolated]\n---\n```yaml\n{}```\n```yaml\n{}```\n",serde_yaml::to_string(&completed.tasks[0]).unwrap(),serde_yaml::to_string(&completed.tasks[1]).unwrap());
            root.write("docs/010-test.md", &text);
            let mut mismatched = completed.clone();
            mismatched.tasks[0].receipt.as_mut().unwrap().commit =
                "ffffffffffffffffffffffffffffffffffffffff".into();
            let wrong = text.replace(
                &serde_yaml::to_string(&completed.tasks[0]).unwrap(),
                &serde_yaml::to_string(&mismatched.tasks[0]).unwrap(),
            );
            root.write("docs/010-test.md", &wrong);
            assert!(store.submit(&id, stage, &evidence, "pass", None).is_err());
            assert_eq!(store.inspect(&id).unwrap().status, "in_progress");
            root.write("docs/010-test.md", &text);
        }
        let result = store.submit(&id, stage, &evidence, "pass", None).unwrap();
        let retry = store.submit(&id, stage, &evidence, "pass", None).unwrap();
        assert_eq!(retry.claim_revision, result.claim_revision);
        assert_eq!(result.status, if gate == 4 { "completed" } else { "ready" });
    }
    let completed = store.inspect(&id).unwrap();
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
        commit: "candidate".into(),
        contract_revision: 1,
        passed_at: "timestamp".into(),
        evidence: json!({}),
        review: json!({}),
        decision: "pass".into(),
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
        .claim(&id, "design", "builder", root.0.to_str().unwrap())
        .unwrap();
    let stale = evidence(&first);
    let reset = store.reset(&id).unwrap();
    assert!(store.submit(&id, "design", &stale, "pass", None).is_err());
    let second = store
        .claim(&id, "design", "builder", root.0.to_str().unwrap())
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
        .submit(&id, "design", &evidence(&second), "pass", None)
        .is_err());
}

#[test]
fn review_rejection_retains_findings_and_requires_a_new_build() {
    let (root, mut store, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    for stage in ["design", "contract", "build"] {
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
    let rejected = store
        .submit(
            &id,
            "review",
            &evidence(&claimed),
            "reject",
            Some(&findings),
        )
        .unwrap();
    assert_eq!(rejected.gate, 2);
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
    assert_eq!(store.inspect(&id).unwrap().gate, 3);
}

#[test]
fn force_deletion_preserves_project_boundaries_and_revision_fencing() {
    let (root, mut store, milestone) = fixture();
    let id = milestone.task_id(&milestone.tasks[0]);
    let first = store
        .claim(&id, "design", "worker", root.0.to_str().unwrap())
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
    root.write("forge-mcp.yaml", "roots: [src]\ndoc_roots: []\n");
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
fn cli_migration_reconciles_manifests_and_creation_selects_one_task() {
    let (root, mut store, milestone) = fixture();
    let invoke = |action: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--root",
                root.0.to_str().unwrap(),
                "migrate",
                action,
                "docs/010-test.md",
            ])
            .output()
            .unwrap()
    };
    for action in ["preview", "apply", "apply", "verify"] {
        let output = invoke(action);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["mode"], action);
    }
    store
        .delete(None, Some("docs/010-test.md"), true, 0)
        .unwrap();
    let created = tasks::manage(
        &root.0,
        tasks::Manage {
            workspace: None,
            action: tasks::ManageAction::Create,
            task_id: None,
            milestone_ref: Some("docs/010-test.md".into()),
            task_ref: Some("first".into()),
            paths: None,
            force: false,
        },
    )
    .unwrap();
    assert_eq!(created["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(store.list(None, "all", None).unwrap().len(), 1);
    assert_eq!(
        store.list(None, "all", None).unwrap()[0].task_id,
        milestone.task_id(&milestone.tasks[0])
    );
    assert!(!invoke("verify").status.success());
    assert!(invoke("apply").status.success());
    assert!(invoke("verify").status.success());
    root.write("docs/milestones/archive/010-test.md", SPEC);
    for action in ["preview", "apply", "verify"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args(["--root", root.0.to_str().unwrap(), "migrate", action])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["manifests"].as_array().unwrap().len(), 1);
    }
}
