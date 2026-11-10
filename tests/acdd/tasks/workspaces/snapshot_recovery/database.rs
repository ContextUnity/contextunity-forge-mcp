use super::*;

#[test]
fn db_recovery_rejects_invalid_checkpoints_and_preserves_independent_owners() {
    let fixture = TaskHarness::recovery_contract_fixture().unwrap();
    let proof = fixture.contract_evidence().unwrap();
    let refs = fixture.root.root().join(".git/refs/forge/snapshots");
    std::fs::create_dir_all(&refs).unwrap();
    let blocker = refs.join("forge-mcp");
    std::fs::write(&blocker, "block the nested snapshot ref path").unwrap();
    let failed_pin = fixture.submit_contract(&proof).unwrap_err().to_string();
    assert!(
        failed_pin.contains("TASK_SNAPSHOT_REF_FAILED"),
        "{failed_pin}"
    );
    let still_claimed = tasks::store(fixture.root.root())
        .unwrap()
        .inspect(&fixture.id)
        .unwrap();
    assert_eq!(still_claimed.status, "in_progress");
    assert_eq!(still_claimed.gate, 0);
    std::fs::remove_file(blocker).unwrap();
    let recovery = milestones::recovery::db_recover(fixture.root.root());
    assert!(
        recovery.is_ok(),
        "active claim checkpoint enrollment: {recovery:?}"
    );
    let enrolled = tasks::store(fixture.root.root())
        .unwrap()
        .inspect(&fixture.id)
        .unwrap();
    assert_eq!(enrolled.claim_revision, still_claimed.claim_revision);
    assert_eq!(enrolled.worker_id, still_claimed.worker_id);
    let database = fixture.root.root().join(".forge/tasks.sqlite");
    let writer =
        contextunity_forge_mcp::db::tasks_store::recovery::lock_database(&database).unwrap();
    let manifest = fixture.root.root().join("docs/milestones/010-test.md");
    let original = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        &manifest,
        format!("{original}\n<!-- provisional delivery receipt -->\n"),
    )
    .unwrap();
    let composed = milestones::show(fixture.root.root(), "m-test", true);
    std::fs::write(&manifest, original).unwrap();
    drop(writer);
    assert!(
        composed.is_err(),
        "precommit document must not be acknowledged: {composed:?}"
    );
    assert!(composed
        .unwrap_err()
        .to_string()
        .contains("TASK_CHECKPOINT_PENDING"));
    let (contract_receipt, contract_snap) = fixture.submit_contract(&proof).unwrap();
    assert_eq!(contract_snap.len(), 40);
    assert_eq!(contract_receipt["snapshot"]["commit"], contract_snap);
    assert_eq!(
        contract_receipt["snapshot"]["inspect_cmd"],
        format!("git show {}", &contract_snap[..7])
    );
    fixture.begin_build().unwrap();
    let root = fixture.root;
    let task_id = fixture.id;
    let task_id = &task_id;
    let checkpoint_ref = format!("refs/forge/state/forge-mcp/{}", task_id.replace(':', "/"));
    let checkpoint = contextunity_forge_mcp::db::tasks_store::recovery::read_checkpoint(
        root.root(),
        &checkpoint_ref,
    )
    .unwrap();
    let before = tasks::store(root.root())
        .unwrap()
        .inspect_details(task_id)
        .unwrap();
    let stored = tasks::store(root.root()).unwrap().inspect(task_id).unwrap();
    let permanent_pins = TaskHarness::references(root.root(), "refs/forge/evidence/");
    assert!(permanent_pins.status.success());
    assert!(
        permanent_pins.stdout.is_empty(),
        "new task gates must not create global evidence pins"
    );
    assert_eq!(checkpoint.workspace, root.root().canonicalize().unwrap());
    assert_eq!(
        checkpoint.task.as_ref().unwrap().claim_revision,
        stored.claim_revision
    );
    let recover = || db_recover(root.root());
    // Recovery must not promote old claims from unresolved intents, incomplete cohorts, or foreign owners.
    let keeper_commit =
        String::from_utf8(TaskHarness::revision(root.root(), &checkpoint_ref).stdout)
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
            root.root(),
            &checkpoint_ref,
            &serde_json::to_vec(&state).unwrap(),
            std::slice::from_ref(&contract_snap),
        )
        .unwrap();
        if field == "intent" {
            assert!(
                contextunity_forge_mcp::engine::milestones::show(root.root(), "m-test", true)
                    .unwrap_err()
                    .to_string()
                    .contains("TASK_CHECKPOINT_PENDING")
            );
            let recovered = recover();
            assert!(
                recovered.is_err(),
                "an arbitrary intent is not authority to revoke a claim"
            );
            assert_eq!(
                tasks::store(root.root())
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
                assert!(TaskHarness::update_reference(
                    root.root(),
                    &checkpoint_ref,
                    &keeper_commit,
                    None
                )
                .status
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
                        value["cohort"] = json!([[root.root().to_string_lossy(), checkpoint_ref]])
                    }
                    "cohort_id" => value["cohort_id"] = json!("foreign-cohort"),
                    "token" => value["intent"] = json!("invalid-token"),
                    _ => unreachable!(),
                }
                let corrupt_tip = contextunity_forge_mcp::core::git::git_checkpoint(
                    root.root(),
                    &checkpoint_ref,
                    &serde_json::to_vec(&value).unwrap(),
                    std::slice::from_ref(&contract_snap),
                )
                .unwrap();
                let refused = recover();
                assert!(refused.is_err(), "must reject {corrupt}");
                assert_eq!(
                    tasks::store(root.root())
                        .unwrap()
                        .inspect_details(task_id)
                        .unwrap(),
                    before,
                    "SQL changed after rejected {corrupt}"
                );
                assert_eq!(
                    TaskHarness::revision_text(root.root(), &checkpoint_ref).unwrap(),
                    corrupt_tip,
                    "ref changed after rejected {corrupt}"
                );
            }
            let peer_ref = format!("{}/second", checkpoint_ref.rsplit_once('/').unwrap().0);
            let peer_keeper = TaskHarness::revision_text(root.root(), &peer_ref).unwrap();
            let peer = contextunity_forge_mcp::db::tasks_store::recovery::read_checkpoint(
                root.root(),
                &peer_ref,
            )
            .unwrap();
            for corrupt in ["token", "acknowledged_cohort"] {
                assert!(TaskHarness::update_reference(
                    root.root(),
                    &checkpoint_ref,
                    &keeper_commit,
                    None
                )
                .status
                .success());
                let first_tip = contextunity_forge_mcp::core::git::git_checkpoint(
                    root.root(),
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
                    root.root(),
                    &peer_ref,
                    &serde_json::to_vec(&value).unwrap(),
                    &[],
                )
                .unwrap();
                assert!(recover().is_err(), "must reject conflicting {corrupt}");
                assert_eq!(
                    tasks::store(root.root())
                        .unwrap()
                        .inspect_details(task_id)
                        .unwrap(),
                    before
                );
                for (reference, tip) in [(&checkpoint_ref, first_tip), (&peer_ref, peer_tip)] {
                    assert_eq!(
                        TaskHarness::revision_text(root.root(), reference).unwrap(),
                        tip
                    );
                }
                assert!(
                    TaskHarness::update_reference(root.root(), &peer_ref, &peer_keeper, None)
                        .status
                        .success()
                );
            }
            assert!(TaskHarness::update_reference(
                root.root(),
                &checkpoint_ref,
                &keeper_commit,
                None
            )
            .status
            .success());
            mutation = contextunity_forge_mcp::core::git::git_checkpoint(
                root.root(),
                &checkpoint_ref,
                &serde_json::to_vec(&state).unwrap(),
                std::slice::from_ref(&contract_snap),
            )
            .unwrap();
        }
        let database = root.root().join(".forge/tasks.sqlite");
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
        let refused = recover().expect_err("invalid checkpoint must be refused");
        let diagnostic_text = format!("{refused:#}");
        assert!(
            diagnostic_text.contains(diagnostic),
            "{field}: {diagnostic_text}"
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
        let restored = TaskHarness::update_reference(
            root.root(),
            &checkpoint_ref,
            &keeper_commit,
            Some(&mutation),
        );
        assert!(restored.status.success());
    }
    let obsolete_archive = "docs/milestones/archive/009-obsolete.md";
    root.write(
        obsolete_archive,
        "---\nid: m-obsolete\nstatus: completed\nhandoff:\n  proof_shas: null\n---\n",
    );
    assert!(
        recover().is_err(),
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
        root.root(),
        &obsolete_ref,
        &serde_json::to_vec(&obsolete).unwrap(),
        &[],
    )
    .unwrap();
    obsolete["intent"] = json!("2-2");
    let ignored_tip = contextunity_forge_mcp::core::git::git_checkpoint(
        root.root(),
        &obsolete_ref,
        &serde_json::to_vec(&obsolete).unwrap(),
        &[],
    )
    .unwrap();
    assert!(recover().is_ok());
    assert_eq!(
        TaskHarness::revision_text(root.root(), &obsolete_ref).unwrap(),
        ignored_tip,
        "obsolete intent must not be rewound"
    );
    let ignored_tip = contextunity_forge_mcp::core::git::git_checkpoint(
        root.root(),
        &obsolete_ref,
        b"unsupported obsolete keeper bytes",
        &[],
    )
    .unwrap();
    assert!(
        recover().is_ok(),
        "obsolete keeper payload must not be parsed"
    );
    assert_eq!(
        TaskHarness::revision_text(root.root(), &obsolete_ref).unwrap(),
        ignored_tip
    );
    assert!(
        tasks::manage(
            root.root(),
            serde_json::from_value(json!({"action": "inspect", "task_id": task_id})).unwrap(),
        )
        .is_ok(),
        "obsolete keeper must not fence managed reads"
    );
    let database = root.root().join(".forge/tasks.sqlite");
    std::fs::remove_file(&database).unwrap();
    let result = recover().unwrap();
    assert_eq!(result["status"], "recovered");
    assert_eq!(
        tasks::store(root.root())
            .unwrap()
            .inspect_details(task_id)
            .unwrap(),
        before
    );
    recover().unwrap();
    assert_eq!(
        tasks::store(root.root())
            .unwrap()
            .inspect_details(task_id)
            .unwrap(),
        before
    );
    std::fs::remove_file(root.root().join(obsolete_archive)).unwrap();
    std::fs::remove_file(root.root().join(obsolete_active)).unwrap();
    assert!(TaskHarness::delete_reference(root.root(), &obsolete_ref)
        .status
        .success());
    let mut first_task = TaskHarness::attach(&root, task_id);
    first_task
        .submit_pass()
        .expect("claimed build gate should pass from the shared recovery snapshot");
    first_task
        .advance_to_completion()
        .expect("first recovery task should complete remaining profile gates");
    let task_id_2 = tasks::store(root.root())
        .unwrap()
        .list(None, "ready", None)
        .unwrap()[0]
        .task_id
        .clone();
    let branch = TaskHarness::branch_name(root.root());
    assert!(branch.status.success(), "{branch:?}");
    let milestone_branch = String::from_utf8_lossy(&branch.stdout).trim().to_owned();
    let created = TaskHarness::create_branch(root.root(), "task-second-delivery");
    assert!(created.status.success(), "{created:?}");
    let mut second_task = TaskHarness::attach(&root, &task_id_2);
    let completed_second_task = second_task
        .advance_to_completion()
        .expect("second task should pass its profile-driven gates");
    assert_eq!(completed_second_task.status, "completed");
    let switched = TaskHarness::checkout_branch(root.root(), &milestone_branch);
    assert!(switched.status.success(), "{switched:?}");
    let merged =
        TaskHarness::merge_branch(root.root(), "task-second-delivery", "merge second task");
    assert!(merged.status.success(), "{merged:?}");
    let recover = || db_recover(root.root());
    // A shared physical store is not an enrollment owner. Preserve unrelated
    // legacy provenance and an independent claimed milestone while updating the
    // current active task beside its two genuinely delivered predecessors.
    {
        let reference = "docs/milestones/010-test.md";
        let original = std::fs::read_to_string(root.root().join(reference)).unwrap();
        let extended = format!("{original}\n```yaml\ntask_ref: third\ntarget: Resume current work\nproof_policy: seam-test-first\nscope: [src/]\ndepends_on: [first, second]\n```\n");
        root.write(reference, &extended);
        let mut store = tasks::store(root.root()).unwrap();
        let current = Milestone::parse(&extended, "forge-mcp").unwrap();
        store.sync(&current, reference, root.root()).unwrap();
        let third = current.task_id(&current.tasks[2]);
        store
            .claim(
                &third,
                "contract",
                "current-owner",
                root.root().to_str().unwrap(),
                Some(&TaskHarness::session_for("current-owner")),
            )
            .unwrap();
        let foreign_reference = "docs/milestones/020-independent.md";
        let foreign_text = "---\nid: m-independent\nstatus: active\ntitle: Independent\n---\n```yaml\ntask_ref: foreign\ntarget: Independent owner\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
        root.write(foreign_reference, foreign_text);
        let foreign = Milestone::parse(foreign_text, "forge-mcp").unwrap();
        store
            .sync(&foreign, foreign_reference, root.root())
            .unwrap();
        let foreign_id = foreign.task_id(&foreign.tasks[0]);
        store
            .claim(
                &foreign_id,
                "contract",
                "foreign-owner",
                root.root().to_str().unwrap(),
                Some(&TaskHarness::session_for("foreign-owner")),
            )
            .unwrap();
        let foreign_before = TaskHarness::persisted_task_rows(&store, &foreign_id).unwrap();
        let foreign_ref = format!(
            "refs/forge/state/forge-mcp/{}",
            foreign_id.replace(':', "/")
        );
        let foreign_keeper = || {
            let output = TaskHarness::revision(root.root(), &foreign_ref);
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
        TaskHarness::seed_unenrolled_task(
            &store,
            legacy_id,
            "docs/milestones/099-retired.md",
            &legacy,
            901,
            (900, "untouched legacy finding"),
        )
        .unwrap();
        let legacy_before = TaskHarness::persisted_task_rows(&store, legacy_id).unwrap();
        let legacy_details = tasks::manage(
            root.root(),
            serde_json::from_value(json!({"action":"inspect","task_id":legacy_id})).unwrap(),
        )
        .unwrap();
        assert_eq!(legacy_details["checkpoint_status"], "UNENROLLED");
        let mutation = store.subtask_add(&third, "owner-isolation", "Owned checkpoint only");
        assert!(
            mutation.is_ok(),
            "current mutation must not enroll unrelated legacy state: {mutation:?}"
        );
        assert_eq!(
            TaskHarness::persisted_task_rows(&store, legacy_id).unwrap(),
            legacy_before
        );
        assert_eq!(
            TaskHarness::persisted_task_rows(&store, &foreign_id).unwrap(),
            foreign_before
        );
        assert_eq!(foreign_keeper(), keeper_before);
        for invalid in ["missing-authority", "stale-authority"] {
            if invalid == "stale-authority" {
                legacy["authority_snapshot"] =
                    serde_json::to_value(store.inspect(&third).unwrap().authority_snapshot)
                        .unwrap();
                TaskHarness::set_task_descriptor(&store, legacy_id, &legacy).unwrap();
            }
            let unchanged = TaskHarness::persisted_task_rows(&store, legacy_id).unwrap();
            let error = store
                .subtask_add(legacy_id, "refuse", "Never admit stale authority")
                .unwrap_err();
            assert!(
                format!("{error:#}").contains("TASK_RECOVERY_"),
                "{invalid}: {error:#}"
            );
            assert_eq!(
                TaskHarness::persisted_task_rows(&store, legacy_id).unwrap(),
                unchanged
            );
            store
                .subtask_update(&third, "owner-isolation", "in_progress", Some(invalid))
                .unwrap();
            assert_eq!(
                TaskHarness::persisted_task_rows(&store, legacy_id).unwrap(),
                unchanged
            );
            assert_eq!(
                TaskHarness::persisted_task_rows(&store, &foreign_id).unwrap(),
                foreign_before
            );
            assert_eq!(foreign_keeper(), keeper_before);
            TaskHarness::seed_task_dependency(&store, legacy_id, &third, 1).unwrap();
            let unchanged = TaskHarness::persisted_task_rows(&store, legacy_id).unwrap();
            let owned_before = TaskHarness::persisted_task_rows(&store, &third).unwrap();
            let mut cancelled = current.clone();
            cancelled.tasks[2].status = Some("cancelled".into());
            let error = store.sync(&cancelled, reference, root.root()).unwrap_err();
            assert!(
                format!("{error:#}").contains("TASK_RECOVERY_"),
                "affected {invalid}: {error:#}"
            );
            assert_eq!(
                TaskHarness::persisted_task_rows(&store, legacy_id).unwrap(),
                unchanged
            );
            assert_eq!(
                TaskHarness::persisted_task_rows(&store, &third).unwrap(),
                owned_before
            );
            TaskHarness::remove_task_dependencies(&store, legacy_id).unwrap();
        }
        let coverage = recover().unwrap();
        assert!(coverage["unenrolled_tasks"]
            .as_array()
            .unwrap()
            .contains(&json!(legacy_id)));
        assert_eq!(coverage["legacy_cleanup_complete"], false);
        assert_eq!(
            TaskHarness::persisted_task_rows(&store, &foreign_id).unwrap(),
            foreign_before
        );
        assert_eq!(foreign_keeper(), keeper_before);
        // A new/forgotten propagation path cannot commit outside the declared
        // owner cohort, even when introduced by a real SQLite trigger.
        TaskHarness::install_subtask_revision_trigger(
            &store,
            &third,
            "unexpected-delta",
            &foreign_id,
        )
        .unwrap();
        let unchanged = TaskHarness::persisted_task_rows(&store, &third).unwrap();
        let error = store
            .subtask_add(&third, "unexpected-delta", "Reject an uncovered writer")
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("TASK_CHECKPOINT_COHORT_UNDECLARED"),
            "{error:#}"
        );
        assert_eq!(
            TaskHarness::persisted_task_rows(&store, &third).unwrap(),
            unchanged
        );
        assert_eq!(
            TaskHarness::persisted_task_rows(&store, &foreign_id).unwrap(),
            foreign_before
        );
        assert_eq!(foreign_keeper(), keeper_before);
        TaskHarness::remove_subtask_revision_trigger(&store).unwrap();
        let members = [task_id.as_str(), task_id_2.as_str(), third.as_str()];
        let before = members.map(|id| store.inspect_details(id).unwrap());
        drop(store);
        std::fs::remove_file(root.root().join(".forge/tasks.sqlite")).unwrap();
        recover().unwrap();
        let mut store = tasks::store(root.root()).unwrap();
        for (id, expected) in members.into_iter().zip(before) {
            assert_eq!(store.inspect_details(id).unwrap(), expected, "{id}");
        }
        assert_eq!(
            TaskHarness::persisted_task_rows(&store, &foreign_id).unwrap(),
            foreign_before
        );
        assert!(
            store.inspect(legacy_id).is_err(),
            "unenrolled legacy history is not claimed as recoverable"
        );
        store.delete(Some(&third), None, true, i64::MAX).unwrap();
        store
            .delete(Some(&foreign_id), None, true, i64::MAX)
            .unwrap();
        root.write(reference, &original);
        std::fs::remove_file(root.root().join(foreign_reference)).unwrap();
    }
}
