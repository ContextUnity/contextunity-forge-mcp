use super::*;

#[test]
fn task_specification_and_store_survive_code_index_rebuild() {
    let (root, mut store, milestone) = fixture(DEFAULT_SEAM);
    let first = &milestone.tasks[0];
    let digest = milestone.digest(first).unwrap();
    let mut changed = milestone.clone();
    changed.tasks[1].target = "Sibling amendment".into();
    assert_eq!(digest, changed.digest(first).unwrap());
    let id = milestone.task_id(first);
    let claim = store
        .claim(
            &id,
            "contract",
            "builder",
            root.root().to_str().unwrap(),
            Some(&TaskHarness::session_for("builder")),
        )
        .unwrap();
    root.build_to(root.path(".forge/code-map.sqlite"));
    drop(store);
    let store = TasksStore::open(&root.root().join(".forge/tasks.sqlite")).unwrap();
    assert_eq!(
        store.inspect(&id).unwrap().claim_revision,
        claim.claim_revision
    );
    assert_eq!(store.inspect(&id).unwrap().status, "in_progress");
    let wal: String = store
        .connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    let timeout: i64 = store
        .connection
        .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
        .unwrap();
    assert_eq!(wal, "wal");
    assert_eq!(timeout, 5000);
}

#[test]
fn tasks_coordinate_claims_dependencies_reset_scope_and_claim_diagnostic() {
    let mut h = TaskHarness::disk();
    {
        let external = TaskHarness::workspace();
        let root = h.root().to_owned();
        let extended = h
            .store
            .extend_scope(
                &h.id,
                &["src/next.rs".into(), "tests/fixture.rs".into()],
                &root,
            )
            .unwrap();
        assert!(extended.spec.scope.contains(&"src/next.rs".into()));
        assert!(extended.spec.scope.contains(&"tests/fixture.rs".into()));
        for path in ["../escape", "docs/outside.rs"] {
            assert!(h.store.extend_scope(&h.id, &[path.into()], &root).is_err());
        }
        std::os::unix::fs::symlink(external.root(), root.join("src/escape")).unwrap();
        std::os::unix::fs::symlink(root.join("docs"), root.join("src/module_escape")).unwrap();
        for path in ["src/escape/file.rs", "src/module_escape/file.rs"] {
            assert!(h.store.extend_scope(&h.id, &[path.into()], &root).is_err());
        }
    }

    {
        let listed = h.list(tasks::List::default()).unwrap();
        assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(listed["tasks"][0]["task_id"], h.id);
        let path = h.root().join(".forge/tasks.sqlite");
        let worktree = h.root().to_string_lossy().into_owned();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|n| {
                let (path, worktree, id, barrier) = (
                    path.clone(),
                    worktree.clone(),
                    h.id.clone(),
                    barrier.clone(),
                );
                std::thread::spawn(move || {
                    let mut store = TasksStore::open(&path).unwrap();
                    barrier.wait();
                    store.claim(
                        &id,
                        "contract",
                        &format!("worker-{n}"),
                        &worktree,
                        Some(&TaskHarness::session_for(&format!("worker-{n}"))),
                    )
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let winner = h.store.inspect(&h.id).unwrap();
        let claimed_at: i64 = h
            .store
            .connection
            .query_row(
                "SELECT claimed_at FROM task_claims WHERE task_id=?1 AND ended=0",
                [&h.id],
                |row| row.get(0),
            )
            .unwrap();
        let winner_id = winner.worker_id.as_deref().unwrap();
        let expected_collision = format!(
                "TASK_ALREADY_CLAIMED: {}. Claimed by worker '{winner_id}' in worktree '{}' (revision {}, claimed at {claimed_at}). Do: submit the active claim from worker '{winner_id}' before opening another.",
                h.id, winner.worktree.as_deref().unwrap(), winner.claim_revision,
            );
        assert!(results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .all(|error| error.to_string() == expected_collision));
    }

    {
        let mut h = TaskHarness::memory();
        let milestone = Milestone::parse(SPEC, "forge-mcp").unwrap();
        assert!(milestone.acdd_profile.is_none());
        assert!(milestone.tasks[0].acdd_profile.is_none());
        let before = h.claim("contract");
        assert_eq!(before.profile_pin, None);
        let root = h.root().to_owned();
        assert_eq!(
            h.store
                .sync(&milestone, "docs/010-test.md", &root)
                .unwrap()
                .len(),
            2
        );
        let after = h.store.inspect(&h.id).unwrap();
        assert_eq!(after.profile_pin, None);
        assert_eq!(after.digest, before.digest);
        assert_eq!(after.status, before.status);
        assert_eq!(after.stage, before.stage);
        assert_eq!(after.claim_revision, before.claim_revision);
        let reset = h.store.reset(&h.id).unwrap();
        assert!(reset.claim_revision > before.claim_revision);
        assert_eq!(reset.status, "ready");
    }
}

#[test]
fn gates_write_receipts_and_preserve_retention_outcomes() {
    let (root, mut store, milestone) = fixture(DEFAULT_SEAM);
    let id = milestone.task_id(&milestone.tasks[0]);
    store
        .blackboard_post(
            &id,
            "builder",
            "decisions",
            "Keep the parser boundary stable.",
        )
        .unwrap();
    store
        .blackboard_post(
            &id,
            "builder",
            "deferred",
            "Measure large fixture cost later.",
        )
        .unwrap();
    let mut harness = TaskHarness::attach(&root, &id);
    harness
        .store
        .connection
        .execute(
            "UPDATE task_blackboard SET created_at=?2 WHERE task_id=?1",
            rusqlite::params![harness.id, 1],
        )
        .unwrap();
    let gates = harness.executable_gates().unwrap();
    for definition in &gates {
        let stage = definition.id.as_str();
        harness.advance_to(stage).unwrap();
        let worker = harness.worker_for_gate(definition);
        let session = TaskHarness::session_for(&worker);
        let task = harness
            .claim_gate_in_session(stage, &worker, Some(&session))
            .unwrap();
        let mut evidence = harness.evidence_for_gate(&task).unwrap();
        if definition.proof.kind()
            == Some(contextunity_forge_mcp::core::tasks::profile::ProofKind::Delivery)
        {
            evidence.proof = json!({"stage":stage});
            assert!(harness
                .submit_evidence_in_session(&evidence, Some(&session))
                .is_err());
            assert_eq!(harness.store.inspect(&id).unwrap().status, "in_progress");
            evidence = harness
                .evidence_for_gate(&harness.store.inspect(&id).unwrap())
                .unwrap();
        }
        let result = harness
            .submit_evidence_in_session(&evidence, Some(&session))
            .unwrap();
        assert_eq!(
            result.status,
            if definition.proof.kind()
                == Some(contextunity_forge_mcp::core::tasks::profile::ProofKind::Delivery)
            {
                "completed"
            } else {
                "ready"
            }
        );
    }
    let completed = harness.store.inspect(&id).unwrap();
    let rollup = completed.receipt.as_ref().unwrap().rollup.as_ref().unwrap();
    assert_eq!(
        rollup.architectural_notes,
        vec![
            "decisions: Keep the parser boundary stable.",
            "deferred: Measure large fixture cost later."
        ]
    );
    assert!(completed.receipt.as_ref().unwrap().commit.is_some());
    let remaining_messages: i64 = harness
        .store
        .connection
        .query_row(
            "SELECT COUNT(*) FROM task_blackboard WHERE task_id=?1",
            [&harness.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining_messages, 0);
    let at = completed.completed_at.unwrap();
    assert!(store
        .delete(Some(&id), None, false, at + RETENTION_SECONDS)
        .is_err());
    assert_eq!(store.cleanup(at + RETENTION_SECONDS + 1).unwrap(), vec![id]);
    assert_eq!(store.list(None, "ready", None).unwrap().len(), 1);
    let rows: i64 = harness
        .store
        .connection
        .query_row("SELECT COUNT(*) FROM task_claims", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}
