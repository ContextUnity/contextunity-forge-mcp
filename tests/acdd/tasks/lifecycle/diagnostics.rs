use super::*;

#[test]
fn task_manage_selector_diagnostics_report_fields_before_resolution() {
    let root = TaskHarness::workspace();
    let cases = [
        (
            tasks::Manage {
                action: tasks::ManageAction::Inspect,
                ..Default::default()
            },
            "inspect",
            "task_id",
        ),
        (
            tasks::Manage {
                action: tasks::ManageAction::Inspect,
                task_id: Some("forge-mcp/m-missing:task".into()),
                milestone_ref: Some("docs/010-test.md".into()),
                ..Default::default()
            },
            "inspect",
            "milestone_ref",
        ),
        (
            tasks::Manage {
                action: tasks::ManageAction::Sync,
                ..Default::default()
            },
            "sync",
            "milestone_ref, workspace",
        ),
        (
            tasks::Manage {
                action: tasks::ManageAction::ExtendScope,
                task_id: Some("forge-mcp/m-test:first".into()),
                paths: Some(Vec::new()),
                ..Default::default()
            },
            "extend_scope",
            "paths",
        ),
    ];

    for (request, action, fields) in cases {
        let selector = request
            .task_id
            .as_deref()
            .or(request.milestone_ref.as_deref())
            .unwrap_or("unselected")
            .to_owned();
        let error = tasks::manage(root.root(), request).unwrap_err().to_string();
        assert_eq!(
            error,
            format!(
                "TASK_SELECTOR_INVALID: {selector}. action '{action}'. missing or invalid fields: [{fields}]. Do: pass required fields for '{action}'."
            )
        );
    }
    assert!(std::fs::read_dir(root.root()).unwrap().next().is_none());
}

#[test]
fn task_store_diagnostics_cover_missing_stale_and_submission_precedence() {
    let mut h = TaskHarness::memory();
    {
        let mut proof = unobserved_evidence(&h.claim("contract"));
        let id = "forge-mcp/m-test:missing";
        proof.task_id = id.into();
        let root = h.root().to_owned();
        let errors = [
            h.store.inspect(id).unwrap_err(),
            h.store
                .claim(
                    id,
                    "contract",
                    "diagnostic-builder",
                    &proof.worktree,
                    Some(&TaskHarness::session_for("diagnostic-builder")),
                )
                .unwrap_err(),
            h.store.reset(id).unwrap_err(),
            h.store.release_failed_claim(id, 1).unwrap_err(),
            h.store
                .extend_scope(id, &["src/other.rs".into()], &root)
                .unwrap_err(),
            h.store
                .blackboard_post(id, "diagnostic-builder", "notes", "missing task")
                .unwrap_err(),
            h.store
                .submit(
                    id,
                    "contract",
                    &proof,
                    "pass",
                    None,
                    Some(&TaskHarness::session_for(&proof.worker_id)),
                )
                .unwrap_err(),
        ];
        for error in errors {
            assert_eq!(error.to_string(), format!("TASK_NOT_FOUND: task '{id}' not found in tasks database. Do: verify task id and sync milestone."));
        }
    }

    h.store.reset(&h.id).unwrap();
    {
        let id = h.id.clone();
        let ready = h.store.inspect(&id).unwrap();
        let error = h
            .store
            .release_failed_claim(&id, ready.claim_revision)
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "TASK_STALE_CLAIM: task '{id}'. status is 'ready'. Do: leave the claim unchanged."
            )
        );
        let active = h.claim("contract");
        let wrong_revision = active.claim_revision + 1;
        let error = h
            .store
            .release_failed_claim(&id, wrong_revision)
            .unwrap_err();
        assert_eq!(error.to_string(),
                format!("TASK_STALE_CLAIM: task '{id}'. active claim_revision {} != requested {wrong_revision}. Do: release claim_revision {}.", active.claim_revision, active.claim_revision));
        h.store
            .connection
            .execute(
                "UPDATE task_claims SET ended=1 WHERE task_id=?1 AND revision=?2",
                rusqlite::params![id, active.claim_revision],
            )
            .unwrap();
        let error = h
            .store
            .release_failed_claim(&id, active.claim_revision)
            .unwrap_err();
        assert_eq!(error.to_string(),
                format!("TASK_STALE_CLAIM: task '{id}'. claim_revision {} has no open claim row. Do: claim the task again before modifying it.", active.claim_revision));
        h.store.reset(&id).unwrap();
    }

    {
        let id = h.id.clone();
        let active = h.claim("contract");
        let original = unobserved_evidence(&active);
        let session = TaskHarness::session_for(&original.worker_id);
        for field in [
            "claim_revision",
            "contract_revision",
            "task_id",
            "worker_id",
            "worktree",
        ] {
            let mut proof = original.clone();
            let (supplied, expected) = match field {
                "claim_revision" => {
                    proof.claim_revision += 1;
                    (
                        proof.claim_revision.to_string(),
                        original.claim_revision.to_string(),
                    )
                }
                "contract_revision" => {
                    proof.contract_revision += 1;
                    (
                        proof.contract_revision.to_string(),
                        original.contract_revision.to_string(),
                    )
                }
                "task_id" => {
                    proof.task_id = "forge-mcp/m-other:task".into();
                    (proof.task_id.clone(), original.task_id.clone())
                }
                "worker_id" => {
                    proof.worker_id = "other-worker".into();
                    (proof.worker_id.clone(), original.worker_id.clone())
                }
                "worktree" => {
                    proof.worktree = "/other/worktree".into();
                    (proof.worktree.clone(), original.worktree.clone())
                }
                _ => unreachable!(),
            };
            let id = &h.id;
            let diagnostic = format!("TASK_STALE_SUBMISSION: task '{id}'. evidence {field} {supplied} != active {expected}. Do: resubmit with active {field} {expected}.");
            assert_eq!(
                h.store
                    .submit(&h.id, "contract", &proof, "pass", None, Some(&session),)
                    .unwrap_err()
                    .to_string(),
                diagnostic
            );
        }

        let cases = [
                (
                    "evidence stage",
                    "contract",
                    Some("build"),
                    None,
                    false,
                    format!(
                        "TASK_STAGE_INVALID: task '{id}'. evidence.stage 'build' != submitted stage 'contract'. Do: set evidence.stage to 'contract'."
                    ),
                ),
                (
                    "empty commit",
                    "contract",
                    None,
                    Some(" \t"),
                    false,
                    format!(
                        "TASK_EVIDENCE_INVALID: task '{id}' for stage 'contract'. commit field is empty. Do: set commit to candidate snapshot unavailable."
                    ),
                ),
                (
                    "submitted stage",
                    "build",
                    Some("build"),
                    None,
                    false,
                    format!(
                        "TASK_STAGE_INVALID: task '{id}'. submitted stage 'build' does not match active stage 'contract'. Do: submit for active stage 'contract'."
                    ),
                ),
                (
                    "null proof",
                    "contract",
                    None,
                    None,
                    true,
                    format!(
                        "TASK_EVIDENCE_INVALID: task '{id}' for stage 'contract'. proof is null. Do: provide non-empty proof for stage 'contract'."
                    ),
                ),
            ];
        for (case, submitted_stage, evidence_stage, commit, null_proof, expected) in cases {
            let mut evidence = original.clone();
            if let Some(stage) = evidence_stage {
                evidence.stage = stage.into();
            }
            evidence.commit = commit.map(str::to_owned);
            if null_proof {
                evidence.proof = Value::Null;
            }
            let error = h
                .store
                .submit(
                    &id,
                    submitted_stage,
                    &evidence,
                    "pass",
                    None,
                    Some(&session),
                )
                .unwrap_err()
                .to_string();
            assert_eq!(error, expected, "{case}");
        }

        h.set_persisted_status("ready");
        let error = h
            .store
            .submit(&id, "contract", &original, "pass", None, Some(&session))
            .unwrap_err()
            .to_string();
        assert_eq!(
                error,
                format!(
                    "TASK_STALE_SUBMISSION: task '{id}'. status is 'ready'. Do: claim the task before submitting."
                )
            );
    }

    let mut h = TaskHarness::disk();
    {
        let id = "forge-mcp/m-test:missing";
        let diagnostic = format!(
            "task '{id}' not found in tasks database. Do: verify task id and sync milestone."
        );
        let claim =
            TaskHarness::claim_at(h.root(), id, "contract", "diagnostic-builder").unwrap_err();
        assert_eq!(
            claim.to_string(),
            format!("TASK_NOT_FOUND: {id}. {diagnostic}")
        );
        for action in [
            tasks::ManageAction::Inspect,
            tasks::ManageAction::ExtendScope,
        ] {
            let error = tasks::manage(
                h.root(),
                tasks::Manage {
                    action,
                    task_id: Some(id.into()),
                    paths: (action == tasks::ManageAction::ExtendScope)
                        .then(|| vec!["src/other.rs".into()]),
                    ..Default::default()
                },
            )
            .unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("TASK_NOT_FOUND: {id}. {diagnostic}")
            );
        }
        let error = tasks::manage(
            h.root(),
            tasks::Manage {
                action: tasks::ManageAction::Create,
                milestone_ref: Some("docs/010-test.md".into()),
                task_ref: Some("missing".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("TASK_NOT_FOUND: docs/010-test.md. {diagnostic}")
        );
    }

    {
        let id = h.id.clone();
        h.claim("contract");
        let task_root = h.root().to_owned();
        for (value, expected) in [
                (
                    Value::Null,
                    format!(
                        "TASK_EVIDENCE_INVALID: {id}. task '{id}'. evidence is not a valid JSON object. Do: pass evidence as a JSON object."
                    ),
                ),
                (
                    json!({}),
                    format!(
                        "TASK_EVIDENCE_INVALID: {id}. task '{id}'. evidence JSON does not match the evidence object. Do: pass the evidence fields required by stage 'contract'."
                    ),
                ),
            ] {
                assert_eq!(
                    tasks::submit(
                        task_root.as_path(),
                        tasks::Submit {
                            task_id: id.clone(),
                            stage: "contract".into(),
                            action: tasks::Action::Pass,
                            evidence: value,
                            findings: None,
                            session_fingerprint: Some(TaskHarness::session_for("diagnostic-builder")),
                        }
                    )
                    .unwrap_err()
                    .to_string(),
                    expected
                );
            }
    }

    {
        let id = h.id.clone();
        let active = h.store.inspect(&id).unwrap();
        let store = &mut h.store;
        let accepted = TaskHarness::default_evidence(&active);
        store
            .submit(
                &id,
                "contract",
                &accepted,
                "pass",
                None,
                Some(&TaskHarness::session_for(&accepted.worker_id)),
            )
            .unwrap();
        let mut different = accepted;
        different.proof =
            json!({"contract_proof":{"seam_test_ref":"tests/acdd/other.rs","red_exit_code":101}});
        assert_eq!(
                store
                    .submit(&id, "contract", &different, "pass", None, Some(&TaskHarness::session_for(&different.worker_id)))
                    .unwrap_err()
                    .to_string(),
                format!(
                    "TASK_STALE_SUBMISSION: task '{id}'. claim_revision {} already submitted with different evidence. Do: resubmit the accepted evidence for claim_revision {}.",
                    active.claim_revision, active.claim_revision
                )
            );
    }
}
