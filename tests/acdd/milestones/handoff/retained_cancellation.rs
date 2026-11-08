use super::*;

#[test]
fn handoff_validates_retained_cancelled_task_history() {
    crate::acdd::snapshot::assert_safe_git_environment();
    let active = "docs/milestones/030-terminal.md";
    let archive = "docs/milestones/archive/030-terminal.md";
    let root = MilestoneHarness::workspace(DEFAULT_SEAM, "");
    root.write(".gitignore", ".forge/\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("src/lib.rs", "pub fn finish() -> u8 { 0 }\n");
    root.write("deferred/work.rs", "pub fn transferred() -> u8 { 0 }\n");
    let retained_subtask = contextunity_forge_mcp::core::tasks::SubtaskSpec {
        subtask_ref: "retained-case".into(),
        title: "Retain cancellation history".into(),
        status: "ready".into(),
        evidence: None,
    };
    let source = MilestoneFixtureBuilder::new("m-terminal")
        .title("Retained terminal authority")
        .without_started_at()
        .task("finish", &["src/lib.rs"])
        .task_target("Complete admitted work")
        .task_with_deps("transferred", &["deferred/"], &["finish"])
        .task_target("Work transferred to another milestone")
        .task_contract_revision(1)
        .task_subtasks(&[retained_subtask])
        .build();
    root.write(active, &source);
    root.commit_fixture_inputs(&[".gitignore", "forge-mcp.yaml", "src", "deferred", "docs"]);
    let (milestone, store) = root.sync_document(active, &source);
    let finish_id = milestone.task_id(&milestone.tasks[0]);
    let transferred_id = milestone.task_id(&milestone.tasks[1]);
    root.write("src/lib.rs", "pub fn finish() -> u8 { 1 }\n");
    root.complete_task(CompletionMode::Store {
        store: &store,
        task_id: &finish_id,
    })
    .unwrap();
    root.claim_task(&transferred_id, "contract", "transferred-contract-author")
        .unwrap();
    drop(store);
    let delivered = root.document(active);
    let cancelled = delivered.replace(
        "task_ref: transferred\n",
        "task_ref: transferred\nstatus: cancelled\n",
    );
    root.write(active, &cancelled);
    root.sync(active).unwrap();
    root.commit_fixture_inputs(&["docs", "deferred"]);
    let evidence = root.default_fixture_evidence("m-terminal", "cargo test --test acdd", 1, 0);
    let snapshot = root.checkpoint();
    let cancelled_history = root.store().inspect_details(&transferred_id).unwrap();
    assert_eq!(cancelled_history["status"], "cancelled");
    assert!(!cancelled_history["attempts"].as_array().unwrap().is_empty());
    assert!(root
        .store()
        .inspect(&transferred_id)
        .unwrap()
        .receipt
        .is_none());
    let completed_receipt = root.store().inspect(&finish_id).unwrap().receipt.unwrap();
    let mut needs_restore = false;
    let mut failures = Vec::new();
    for scenario in [
        "archive",
        "scope-tamper",
        "cancelled-receipt",
        "old-ref",
        "rollback",
        "ready",
        "manifest-status",
        "digest",
        "revision",
        "missing-task",
        "cancelled-scope",
        "dependent",
    ] {
        if needs_restore {
            snapshot.restore(&root);
        }
        needs_restore = true;
        let mut store = root.store();
        let mut dependent_claim = None;
        let mut revision_mutation = None;
        match scenario {
            "ready" => {
                let ready = cancelled.replace("status: cancelled\n", "status: ready\n");
                let at = ready.find("task_ref: transferred\n").unwrap();
                let (before, task) = ready.split_at(at);
                let changed = task.replacen("contract_revision: 1", "contract_revision: 2", 1);
                let revision = format!("{before}{changed}");
                revision_mutation = Some((task.to_owned(), changed));
                root.write(active, &revision);
                store
                    .sync(
                        &Milestone::parse(&revision, "forge-mcp").unwrap(),
                        active,
                        root.root(),
                    )
                    .unwrap();
            }
            "manifest-status" => root.write(
                active,
                &cancelled.replace("status: cancelled\n", "status: ready\n"),
            ),
            "digest" => root.write(
                active,
                &cancelled.replace(
                    "Work transferred to another milestone",
                    "Unaccepted changed authority",
                ),
            ),
            "revision" => {
                let at = cancelled.find("task_ref: transferred\n").unwrap();
                let (before, task) = cancelled.split_at(at);
                let changed = task.replacen("contract_revision: 1", "contract_revision: 2", 1);
                revision_mutation = Some((task.to_owned(), changed.clone()));
                root.write(active, &format!("{before}{changed}"));
            }
            "missing-task" => root.write(
                active,
                cancelled.split("\n### task: transferred\n").next().unwrap(),
            ),
            "cancelled-scope" => {
                root.write("deferred/work.rs", "pub fn transferred() -> u8 { 99 }\n")
            }
            "cancelled-receipt" | "old-ref" => {
                let mut task = store.inspect(&transferred_id).unwrap();
                if scenario == "cancelled-receipt" {
                    let receipt = store.inspect(&finish_id).unwrap().receipt.unwrap();
                    task.receipt = Some(receipt.clone());
                    task.spec.receipt = Some(receipt);
                } else {
                    task.milestone_ref = "docs/milestones/foreign.md".into();
                }
                store
                    .connection
                    .execute(
                        "UPDATE tasks SET descriptor=?1, milestone_ref=?2 WHERE task_id=?3",
                        rusqlite::params![
                            serde_json::to_string(&task).unwrap(),
                            task.milestone_ref,
                            task.task_id
                        ],
                    )
                    .unwrap();
            }
            "scope-tamper" => {
                let mut task = store.inspect(&finish_id).unwrap();
                task.spec.scope.push("unadmitted/outside.rs".into());
                MilestoneHarness::persist_descriptor(&store, &task);
            }
            "dependent" => {
                let pending = format!(
                    "{}\n```yaml\ntask_ref: dependent\ntarget: Must remain blocked on cancelled work\nproof_policy: seam-test-first\nscope: [dependent/]\ndepends_on: [transferred]\n```\n",
                    cancelled
                );
                root.write(active, &pending);
                let pending_milestone = Milestone::parse(&pending, "forge-mcp").unwrap();
                store.sync(&pending_milestone, active, root.root()).unwrap();
                let task_id = pending_milestone.task_id(&pending_milestone.tasks[2]);
                dependent_claim = Some(root.claim_task(&task_id, "contract", "dependent-author"));
            }
            "rollback" => {
                MilestoneHarness::install_pruning_fault(&store, PruningFault::RejectCancelled);
            }
            "archive" => {}
            _ => panic!("unknown retained-history scenario: {scenario}"),
        }
        if matches!(
            scenario,
            "ready"
                | "manifest-status"
                | "digest"
                | "revision"
                | "missing-task"
                | "cancelled-scope"
                | "dependent"
        ) {
            root.commit_fixture_inputs(&["docs", "deferred"]);
        }
        let before_doc = root.file_bytes(active);
        let before_tasks = store
            .list(None, "all", None)
            .unwrap()
            .into_iter()
            .map(|task| {
                let details = store.inspect_details(&task.task_id).unwrap();
                (task.task_id, details)
            })
            .collect::<Vec<_>>();
        if scenario == "rollback" {
            root.write(archive, "previous torn archive preserved on rollback\n");
        }
        if let Some((original, changed)) = &revision_mutation {
            assert_ne!(
                original, changed,
                "fixture must change the retained task revision"
            );
        }
        if scenario == "dependent" {
            assert!(
                dependent_claim
                    .as_ref()
                    .expect("dependent fixture must attempt a claim")
                    .is_err(),
                "cancelled task must not satisfy dependencies"
            );
        }
        if scenario == "archive" {
            let context = root.handoff_context("m-terminal", &evidence.target_base);
            if let Ok(context) = context {
                assert_eq!(context["binding"]["builders"], json!(["builder"]));
            } else {
                failures.push(format!(
                    "{scenario}: retained cancellation blocked handoff context: {}",
                    context.unwrap_err()
                ));
            }
        }
        let output = root.handoff_at("m-terminal", None, &evidence);
        let expected_success = scenario == "archive";
        if output.is_ok() != expected_success {
            failures.push(format!(
                "{scenario}: expected success={expected_success}: {output:#?}"
            ));
            continue;
        }
        if scenario == "scope-tamper" {
            assert!(
                format!("{:#}", output.as_ref().unwrap_err()).contains("TASK_AUTHORITY_CHANGED"),
                "unrecorded task-scope mutation must fail authority validation: {output:#?}"
            );
        }
        if expected_success {
            assert!(!root.root().join(active).exists());
            let archived_text = root.document(archive);
            let archived = Milestone::parse(&archived_text, "forge-mcp").unwrap();
            let retained = archived
                .tasks
                .iter()
                .find(|spec| spec.task_ref == "transferred")
                .unwrap();
            assert_eq!(retained.status.as_deref(), Some("cancelled"));
            assert!(retained.receipt.is_none());
            assert_eq!(
                serde_json::to_value(&retained.subtasks).unwrap(),
                cancelled_history["subtasks"]
            );
            assert!(store.list(None, "all", None).unwrap().is_empty());
            let accepted = archived
                .tasks
                .iter()
                .find(|spec| spec.task_ref == "finish")
                .unwrap();
            assert!(completed_receipt.matches_proofs(accepted.receipt.as_ref().unwrap()));
            let dependencies = crate::common::sqlite::query_value::<i64>(
                &store.connection,
                "SELECT count(*) FROM task_dependencies WHERE dependency_id=?1 AND satisfied<>0",
                [&transferred_id],
            );
            assert_eq!(dependencies, 0);
        } else {
            if scenario == "rollback" {
                assert!(!root.root().join(active).exists());
                let prepared = root.document(archive);
                assert_eq!(
                    Milestone::parse(&prepared, "forge-mcp")
                        .unwrap()
                        .tasks
                        .len(),
                    2
                );
                assert!(
                    !root.handoff_journals().is_empty(),
                    "failed pruning must retain the prepared journal"
                );
            } else {
                assert_eq!(root.file_bytes(active), before_doc, "{scenario}");
                assert!(!root.root().join(archive).exists(), "{scenario}");
            }
            assert!(
                !store.milestone_tombstoned("m-terminal").unwrap(),
                "{scenario}"
            );
            for (id, details) in &before_tasks {
                assert_eq!(
                    store.inspect_details(id).unwrap(),
                    details.clone(),
                    "{scenario}: task history changed"
                );
            }
            if scenario == "rollback" {
                MilestoneHarness::remove_pruning_fault(&store, PruningFault::RejectCancelled);
                root.handoff_at("m-terminal", None, &evidence).unwrap();
                assert!(store.list(None, "all", None).unwrap().is_empty());
                assert!(
                    root.handoff_journals().is_empty(),
                    "recovered handoff must remove its journal"
                );
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
