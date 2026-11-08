use super::*;

#[test]
fn specifications_validate_unique_identities_and_governing_digests() {
    let parsed = Milestone::parse(SPEC, "forge-mcp").unwrap();
    let first = &parsed.tasks[0];
    let digest = parsed.digest(first).unwrap();
    let mut completed = first.clone();
    completed.status = Some("completed".into());
    completed.receipt = Some(Receipt {
        delivery_gate: None,
        candidate_gate: None,
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
fn task_authority_diagnostics_share_the_synced_manifest_fixture() {
    let (root, mut store, milestone) = fixture(DEFAULT_SEAM);
    let id = milestone.task_id(&milestone.tasks[0]);
    {
        let cases = [
                (
                    SPEC.replace(
                        "status: active\n",
                        "status: active\nowners: [docs/AGENTS.md]\n",
                    )
                    .replace("target: Deliver first", "target: Deliver amended first"),
                    format!(
                        "AUTHORITY_GAP: {id}. task '{id}'. file revision 1 <= stored 1; frontmatter fields [milestone.owners], task fields [task.target]. Do: revert [milestone.owners] in docs/010-test.md, set contract_revision above 1, and run contextunity-forge-mcp task sync."
                    ),
                ),
                (
                    SPEC.replace("invariants: [isolated]", "invariants: [changed]"),
                    format!(
                        "AUTHORITY_GAP: {id}. task '{id}'. file revision 1 <= stored 1; digest fields [milestone.invariants]. Do: revert [milestone.invariants] in docs/010-test.md. Declare task invariants inside task.invariants."
                    ),
                ),
                (
                    SPEC.replace(
                        "status: active\n",
                        "status: active\nowners: [docs/AGENTS.md]\n",
                    ),
                    format!(
                        "AUTHORITY_GAP: {id}. task '{id}'. file revision 1 <= stored 1; digest fields [milestone.owners]. Do: revert [milestone.owners] in docs/010-test.md."
                    ),
                ),
            ];
        for (changed, expected) in cases {
            root.write("docs/010-test.md", &changed);
            let error =
                TaskHarness::claim_at(root.root(), &id, "contract", "header-authority-check")
                    .unwrap_err()
                    .to_string();
            assert_eq!(error, expected);
        }
    }

    {
        let changed_text = SPEC.replace("target: Deliver first", "target: Deliver changed");
        let changed = Milestone::parse(&changed_text, "forge-mcp").unwrap();
        let error = store
            .sync(&changed, "docs/010-test.md", root.root())
            .unwrap_err()
            .to_string();
        let id = &id;
        assert_eq!(
                error,
                format!(
                    "AUTHORITY_GAP: task '{id}'. file revision 1 <= stored 1; digest fields [task.target]. Do: set contract_revision above 1 and run contextunity-forge-mcp task sync."
                )
            );
    }

    {
        root.write("docs/010-test.md", SPEC);
        let original_id = id.clone();
        let changed_text = SPEC.replace("id: m-test", "id: m-renamed");
        root.write("docs/010-test.md", &changed_text);
        let changed = Milestone::parse(&changed_text, "forge-mcp").unwrap();
        let sync_error = store
            .sync(&changed, "docs/010-test.md", root.root())
            .unwrap_err()
            .to_string();
        assert_eq!(
                sync_error,
                "AUTHORITY_GAP: milestone 'docs/010-test.md'. milestone id changed from 'm-test' to 'm-renamed'. Do: revert milestone id in docs/010-test.md. Task ids are forge-mcp/m-renamed:first; sync would admit them as new tasks."
            );
        assert!(store.inspect("forge-mcp/m-renamed:first").is_err());

        let claim_error = tasks::claim(
            root.root(),
            tasks::Claim {
                task_id: original_id,
                stage: "contract".into(),
                worker_id: "id-change-check".into(),
                session_fingerprint: Some(TaskHarness::session_for("id-change-check")),
                worktree: root.root().to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert_eq!(
                claim_error,
                "AUTHORITY_GAP: forge-mcp/m-test:first. milestone 'docs/010-test.md'. milestone id changed from 'm-test' to 'm-renamed'. Do: revert milestone id in docs/010-test.md. Task ids are forge-mcp/m-renamed:first; task cannot be resolved."
            );
    }

    {
        root.write("docs/010-test.md", SPEC);
        std::fs::remove_file(root.root().join("docs/010-test.md")).unwrap();
        let missing_file =
            TaskHarness::claim_at(root.root(), &id, "contract", "missing-file-check")
                .unwrap_err()
                .to_string();
        assert_eq!(
                missing_file,
                format!(
                    "TASK_AUTHORITY_MISSING: {id}. milestone file 'docs/010-test.md' is missing. Do: restore milestone file 'docs/010-test.md'."
                )
            );

        root.write(
            "docs/010-test.md",
            &SPEC
                .replace("task_ref: first", "task_ref: renamed")
                .replace("depends_on: [first]", "depends_on: [renamed]"),
        );
        let missing_task =
            TaskHarness::claim_at(root.root(), &id, "contract", "missing-task-check")
                .unwrap_err()
                .to_string();
        assert_eq!(
                missing_task,
                format!(
                    "TASK_AUTHORITY_MISSING: {id}. task '{id}' not found in milestone document 'docs/010-test.md'. Do: restore task '{id}' in 'docs/010-test.md'."
                )
            );
    }

    {
        let changed = SPEC
            .replace(
                "status: active\n",
                "status: active\nowners: [docs/AGENTS.md]\n",
            )
            .replace("task_ref: first", "task_ref: first\ncontract_revision: 2");
        root.write("docs/010-test.md", &changed);
        let error = TaskHarness::claim_at(root.root(), &id, "contract", "unsynced-revision-check")
            .unwrap_err()
            .to_string();
        assert_eq!(
                error,
                format!(
                    "AUTHORITY_GAP: {id}. task '{id}'. file revision 2 > stored 1 (unsynced); digest fields [milestone.owners]. Do: revert [milestone.owners] in docs/010-test.md and run contextunity-forge-mcp task sync."
                )
            );
        assert!(!error.contains("task.contract_revision"));
    }
}

#[test]
fn milestone_id_guard_ignores_foreign_project_at_the_same_manifest_path() {
    let root = acdd_workspace(DEFAULT_SEAM, "");
    let foreign_root = acdd_workspace(DEFAULT_SEAM, "");
    let database = root.root().join(".forge/tasks.sqlite");
    let mut foreign_store = TasksStore::open_project(&database, "forge-mcp", "aaa").unwrap();
    let mut current_store = TasksStore::open_project(&database, "forge-mcp", "zzz").unwrap();
    let specification = |project: &str, id: &str| {
        format!(
            "---\nrepository: forge-mcp\nproject: {project}\nid: {id}\ntitle: Shared path\ndoc_type: contract\nstatus: active\n---\n# Shared path\n```yaml\ntask_ref: first\ntarget: Deliver first\nproof_policy: seam-test-first\nscope: [src/]\n```\n"
        )
    };
    let foreign = Milestone::parse(&specification("aaa", "m-test"), "forge-mcp").unwrap();
    foreign_root.write(
        "docs/milestones/010-shared.md",
        &specification("aaa", "m-test"),
    );
    root.write(
        "docs/milestones/010-shared.md",
        &specification("zzz", "m-test"),
    );
    foreign_store
        .sync(
            &foreign,
            "docs/milestones/010-shared.md",
            foreign_root.root(),
        )
        .unwrap();
    let current = Milestone::parse(&specification("zzz", "m-test"), "forge-mcp").unwrap();
    current_store
        .sync(&current, "docs/milestones/010-shared.md", root.root())
        .unwrap();

    let renamed = Milestone::parse(&specification("zzz", "m-renamed"), "forge-mcp").unwrap();
    let error = current_store
        .sync(&renamed, "docs/milestones/010-shared.md", root.root())
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "AUTHORITY_GAP: milestone 'docs/milestones/010-shared.md'. milestone id changed from 'm-test' to 'm-renamed'. Do: revert milestone id in docs/milestones/010-shared.md. Task ids are zzz/m-renamed:first; sync would admit them as new tasks."
    );
    assert!(current_store.inspect("zzz/m-renamed:first").is_err());
    current_store
        .connection
        .execute_batch(
            "INSERT INTO task_dependencies(task_id,dependency_id,satisfied) VALUES
             ('zzz/m-test:first','aaa/m-test:first',0),
             ('zzz/m-test:first','aaa/m-test:__milestone__',0);",
        )
        .unwrap();

    // Both projects legitimately use this ID and manifest path. A sync that
    // commits first must still be removed by the subsequent public handoff.
    for (store, mut milestone, project) in [
        (&mut foreign_store, foreign, "aaa"),
        (&mut current_store, current, "zzz"),
    ] {
        let workspace = if project == "aaa" {
            foreign_root.root()
        } else {
            root.root()
        };
        assert!(!store.milestone_tombstoned("m-test").unwrap(), "{project}");
        if project == "zzz" {
            root.write("src/lib.rs", "pub fn shared() {}\n");
            root.write(
                "docs/milestones/010-shared.md",
                &specification("zzz", "m-test"),
            );
            root.write("forge-mcp.yaml", "roots: [src]\ndocs: [docs]\ntask_repository: forge-mcp\ntask_project: zzz\ntasks_db: .forge/tasks.sqlite\n");
            store
                .connection
                .execute_batch(
                    "CREATE TRIGGER reject_live_sync BEFORE INSERT ON tasks
                     WHEN NEW.task_id='zzz/m-test:first' BEGIN
                         SELECT RAISE(ABORT,'non-archive admission failure');
                     END;",
                )
                .unwrap();
            let error = tasks::manage(
                root.root(),
                tasks::Manage {
                    action: tasks::ManageAction::Sync,
                    workspace: Some("zzz".into()),
                    ..Default::default()
                },
            )
            .unwrap_err();
            assert!(
                format!("{error:#}").contains("non-archive admission failure"),
                "{error:#}"
            );
            store
                .connection
                .execute_batch("DROP TRIGGER reject_live_sync")
                .unwrap();
            for explicit in [false, true] {
                let result = tasks::manage(
                    root.root(),
                    tasks::Manage {
                        action: tasks::ManageAction::Sync,
                        workspace: Some("zzz".into()),
                        milestone_ref: explicit.then(|| "docs/milestones/010-shared.md".into()),
                        ..Default::default()
                    },
                )
                .unwrap();
                assert_eq!(result["total"], 1);
            }
            store
                .create(
                    &milestone,
                    "docs/milestones/010-shared.md",
                    root.root(),
                    "first",
                )
                .unwrap();
        }
        milestone.tasks[0].status = Some("cancelled".into());
        let terminal = store
            .sync(&milestone, "docs/milestones/010-shared.md", workspace)
            .unwrap();
        store
            .handoff_milestone(
                &format!("{project}/m-test:"),
                "docs/milestones/010-shared.md",
                "docs/archive/010-shared.md",
                &terminal,
                &Default::default(),
            )
            .unwrap();
        assert!(store.milestone_tombstoned("m-test").unwrap());
        assert!(store.list(None, "all", None).unwrap().is_empty());
        if project == "aaa" {
            let mut statement = store
                .connection
                .prepare(
                    "SELECT dependency_id,satisfied FROM task_dependencies WHERE task_id=?1 ORDER BY dependency_id",
                )
                .unwrap();
            let dependencies = statement
                .query_map(["zzz/m-test:first"], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<rusqlite::Result<Vec<(String, bool)>>>()
                .unwrap();
            assert_eq!(
                dependencies,
                vec![
                    ("aaa/m-test:__milestone__".into(), true),
                    ("aaa/m-test:first".into(), false),
                ]
            );
        }
    }
    let mut statement = foreign_store
        .connection
        .prepare(
            "SELECT milestone_id,completed_task_ids FROM milestone_tombstones ORDER BY milestone_id",
        )
        .unwrap();
    let tombstones = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<(String, String)>>>()
        .unwrap();
    assert_eq!(
        tombstones,
        vec![
            ("forge-mcp/aaa/m-test".into(), "[]".into()),
            ("forge-mcp/zzz/m-test".into(), "[]".into()),
        ]
    );
}
