use super::*;

#[test]
fn contract_readmission_and_reset_fence_old_evidence() {
    let root = unconfigured_test_root();
    let mut milestone = Milestone::parse(SPEC, "forge-mcp").unwrap();
    let mut store = TasksStore::open_in_memory("forge-mcp", "forge-mcp").unwrap();
    store.sync(&milestone, "docs/010-test.md", &root).unwrap();
    let id = milestone.task_id(&milestone.tasks[0]);
    let first = store
        .claim(
            &id,
            "contract",
            "builder",
            root.to_str().unwrap(),
            Some(&TaskHarness::session_for("builder")),
        )
        .unwrap();
    let stale = unobserved_evidence(&first);
    let reset = store.reset(&id).unwrap();
    let reset_error = store
        .submit(
            &id,
            "contract",
            &stale,
            "pass",
            None,
            Some(&TaskHarness::session_for(&stale.worker_id)),
        )
        .unwrap_err()
        .to_string();
    assert!(
        reset_error.contains("TASK_STALE_SUBMISSION"),
        "{reset_error}"
    );
    let second = store
        .claim(
            &id,
            "contract",
            "builder",
            root.to_str().unwrap(),
            Some(&TaskHarness::session_for("builder")),
        )
        .unwrap();
    assert!(second.claim_revision > reset.claim_revision);
    milestone.tasks[0].target = "Amended target".into();
    assert!(store.sync(&milestone, "docs/010-test.md", &root).is_err());
    assert_eq!(
        store.inspect(&id).unwrap().worker_id.as_deref(),
        Some("builder")
    );
    milestone.tasks[0].contract_revision = 2;
    let synced = store.sync(&milestone, "docs/010-test.md", &root).unwrap();
    assert_eq!(synced[0].contract_revision, 2);
    assert_eq!(synced[0].gate, 0);
    assert!(synced[0].claim_revision > second.claim_revision);
    let readmission_error = store
        .submit(
            &id,
            "contract",
            &unobserved_evidence(&second),
            "pass",
            None,
            Some(&TaskHarness::session_for(
                second.worker_id.as_deref().unwrap(),
            )),
        )
        .unwrap_err()
        .to_string();
    assert!(
        readmission_error.contains("TASK_STALE_SUBMISSION"),
        "{readmission_error}"
    );
}

#[test]
fn profile_session_and_independence_paths_share_a_harness() {
    let mut h = in_memory_profile_harness();
    {
        let gates = h.executable_gates().unwrap();
        assert_eq!(gates[0].id, "draft");
        assert_eq!(gates[1].id, "peer_check");
        assert_eq!(gates[1].independent_from.as_deref(), Some("draft"));
        h.advance_to("peer_check").unwrap();

        let before = h.store.inspect(&h.id).unwrap();
        let builder_session = TaskHarness::session_for("builder");
        let error = h
            .claim_gate_in_session("peer_check", "builder", Some("fresh-reviewer-session"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("TASK_REVIEW_NOT_INDEPENDENT"), "{error}");
        assert!(error.contains("'draft'"), "{error}");
        assert!(error.contains("'peer_check'"), "{error}");
        let after = h.store.inspect(&h.id).unwrap();
        assert_eq!(after.status, "ready", "builder session");
        assert_eq!(
            after.claim_revision, before.claim_revision,
            "builder session"
        );

        let reviewer_session = builder_session;
        let review = h.claim_next_with_session(Some(&reviewer_session)).unwrap();
        let ev = h.evidence_for_gate(&review).unwrap();
        assert_eq!(
            h.persisted_claim_session(ev.claim_revision),
            reviewer_session
        );
        let accepted = h
            .submit_evidence_in_session(&ev, Some(&reviewer_session))
            .unwrap();
        assert_eq!(accepted.status, "ready");
        assert_eq!(accepted.stage, "assurance");
        assert_eq!(h.persisted_gate_proof("peer_check"), ev.proof);
    }

    {
        assert_eq!(
            h.profile().unwrap().gates[0].proof.kind(),
            Some(contextunity_forge_mcp::core::tasks::profile::ProofKind::None)
        );
        let gates = h.executable_gates().unwrap();
        assert_eq!(
            gates
                .iter()
                .map(|gate| gate.id.as_str())
                .collect::<Vec<_>>(),
            ["draft", "peer_check", "assurance", "ship_it"]
        );

        let task = h.advance_to("assurance").unwrap();
        assert_eq!(task.stage, "assurance");
        assert_eq!(task.status, "ready");
        let peer_revision = h.persisted_gate_evidence("peer_check")["claim_revision"]
            .as_u64()
            .unwrap();
        assert_eq!(
            h.persisted_claim_session(peer_revision),
            TaskHarness::session_for("builder")
        );

        let assurance = h.claim_next().unwrap();
        assert_eq!(assurance.stage, "assurance");
        assert_eq!(
            assurance.worker_id.as_deref(),
            Some("independent_reviewer-independent-assurance")
        );
        assert_eq!(
            h.persisted_claim_session(assurance.claim_revision),
            TaskHarness::session_for("independent_reviewer-independent-assurance")
        );
        let ship_it = h.submit_pass().unwrap();
        assert_eq!(ship_it["status"], "ready");
        assert_eq!(ship_it["stage"], "ship_it");
    }

    {
        let (root, _store, milestone) = fixture(DEFAULT_SEAM);
        let id = milestone.task_id(&milestone.tasks[0]);
        let mut h = TaskHarness::attach(&root, &id);
        h.advance_to("build").unwrap();
        let builder_session = TaskHarness::session_for("builder");
        h.claim_next_with_session(Some(&builder_session)).unwrap();
        let build_evidence = h.observed_evidence();
        h.submit_evidence_in_session(&build_evidence, Some(&builder_session))
            .unwrap();
        let review = h.advance_to("review").unwrap();
        assert_eq!(review.stage, "review");
        assert_eq!(review.status, "ready");
        let build = h.persisted_gate_evidence("build");
        assert_eq!(build["proof"]["command_proof"]["exit_code"], 0);
        assert_eq!(build["commit"].as_str().unwrap().len(), 40);
        assert!(build["candidate_baseline_head"].as_str().is_some());
        let reviewer_session = TaskHarness::session_for("reviewer");
        h.claim_next_with_session(Some(&reviewer_session)).unwrap();
        let ev = h.observed_evidence();
        assert_eq!(
            h.submit_evidence_in_session(&ev, Some(&builder_session))
                .unwrap()
                .stage,
            "deliver"
        );
    }

    {
        let claim_schema = serde_json::to_value(schemars::schema_for!(tasks::Claim)).unwrap();
        let manage_schema = serde_json::to_value(schemars::schema_for!(tasks::Manage)).unwrap();
        let submit_schema = serde_json::to_value(schemars::schema_for!(tasks::Submit)).unwrap();
        for schema in [claim_schema, manage_schema, submit_schema] {
            assert!(
                schema["properties"].get("session_fingerprint").is_none(),
                "trusted session context appeared in a wire schema: {schema}"
            );
        }

        let claim_error = serde_json::from_value::<tasks::Claim>(json!({
            "task_id":"forge-mcp/m-test:first",
            "stage":"contract",
            "worker_id":"builder",
            "worktree":"/tmp/worktree",
            "session_fingerprint":"wire-spoof"
        }))
        .unwrap_err()
        .to_string();
        assert!(claim_error.contains("session_fingerprint"), "{claim_error}");
        let manage_error = serde_json::from_value::<tasks::Manage>(json!({
            "action":"next",
            "worker_id":"builder",
            "worktree":"/tmp/worktree",
            "session_fingerprint":"wire-spoof"
        }))
        .unwrap_err()
        .to_string();
        assert!(
            manage_error.contains("session_fingerprint"),
            "{manage_error}"
        );
        let submit_error = serde_json::from_value::<tasks::Submit>(json!({
            "task_id":"forge-mcp/m-test:first",
            "stage":"contract",
            "action":"pass",
            "evidence":{},
            "session_fingerprint":"wire-spoof"
        }))
        .unwrap_err()
        .to_string();
        assert!(
            submit_error.contains("session_fingerprint"),
            "{submit_error}"
        );
    }

    let mut h = in_memory_profile_harness();
    assert_eq!(
        h.profile().unwrap().gates[0].proof.kind(),
        Some(contextunity_forge_mcp::core::tasks::profile::ProofKind::None)
    );
    {
        let builder_session = TaskHarness::session_for("builder");
        let draft = h.claim_next_with_session(Some(&builder_session)).unwrap();
        let draft_evidence = h.evidence_for_gate(&draft).unwrap();
        h.submit_evidence_in_session(&draft_evidence, Some(&builder_session))
            .unwrap();
        h.advance_to("peer_check").unwrap();
        let reviewer_session = TaskHarness::session_for("reviewer");
        let claimed = h.claim_next_with_session(Some(&reviewer_session)).unwrap();
        // Legacy/restored matching metadata does not override the claimed worker.
        let changed = h
            .store
            .connection
            .execute(
                "UPDATE task_claims SET session_fingerprint=?2 WHERE task_id=?1",
                rusqlite::params![h.id, "persisted-builder-session"],
            )
            .unwrap();
        assert!(changed > 0, "TASK_CLAIM_NOT_FOUND: {}", h.id);
        let mut review = h.evidence_for_gate(&claimed).unwrap();
        review.proof = passing_review_proof();
        let accepted = h
            .submit_evidence_in_session(&review, Some(&TaskHarness::session_for(&review.worker_id)))
            .unwrap();
        assert_eq!(accepted.stage, "assurance");
        assert_eq!(accepted.status, "ready");
        assert_eq!(
            h.persisted_gate_evidence("peer_check")["worker_id"],
            review.worker_id
        );
    }
}

#[test]
fn profile_gates_fail_closed_and_resolve_qualified_stages() {
    {
        let manifest = single_direct_task_manifest("m-unobserved-proof", "unobserved");
        let profile = profile_with_gates(
            "- id: renamed_build\n  role: builder\n  proof: command\n  command: rust-check-and-test\n- id: finish\n  role: builder\n  proof: delivery\n  auto_commit: false\n",
        );
        let mut harness =
            TaskHarness::memory_with_profile(&manifest, "docs/010-unobserved-proof.md", profile);
        let error = harness.advance_to_completion().unwrap_err().to_string();
        assert!(error.contains("production-observed evidence"), "{error}");
        assert_eq!(
            harness.store.inspect(&harness.id).unwrap().stage,
            "renamed_build"
        );
        assert_eq!(
            harness.store.inspect(&harness.id).unwrap().status,
            "in_progress"
        );
    }

    {
        let manifest = single_direct_task_manifest("m-nested-profile", "universal");
        let profile = profile_with_gates(
            "- id: assessment\n  gates:\n    - id: work_item\n      role: builder\n      proof: none\n    - id: peer_audit\n      role: independent_reviewer\n      proof: review\n      independent_from: assessment.work_item\n      reject_to: assessment.work_item\n      contours: guidance.contours\n    - id: final_audit\n      role: independent_reviewer\n      proof: review\n      independent_from: assessment.peer_audit\n      reject_to: assessment.work_item\n      contours: guidance.contours\n",
        );
        let mut harness =
            TaskHarness::memory_with_profile(&manifest, "docs/010-nested-profile.md", profile);
        let gates = harness.executable_gates().unwrap();
        assert_eq!(
            gates
                .iter()
                .map(|gate| gate.id.as_str())
                .collect::<Vec<_>>(),
            [
                "assessment.work_item",
                "assessment.peer_audit",
                "assessment.final_audit"
            ]
        );
        let current = harness.advance_to("final_audit").unwrap();
        assert_eq!(current.stage, "assessment.final_audit");
        let final_gate = &gates[2];
        let worker = harness.worker_for_gate(final_gate);
        let claimed = harness.claim_next().unwrap();
        assert_eq!(claimed.stage, "assessment.final_audit");
        assert_eq!(claimed.worker_id.as_deref(), Some(worker.as_str()));
        assert_ne!(
            claimed.worker_id.as_deref(),
            harness.persisted_gate_evidence("assessment.peer_audit")["worker_id"].as_str()
        );
    }
}

fn conditional_first_profile() -> contextunity_forge_mcp::core::tasks::profile::GateProfile {
    let mut profile = fixture_profile();
    profile.gates = serde_yaml::from_str(
        "- id: code_gate\n  role: builder\n  when: code\n  proof: none\n- id: docs_gate\n  role: builder\n  proof: none\n- id: finish\n  role: builder\n  proof: delivery\n  auto_commit: false\n",
    )
    .unwrap();
    profile.validate().unwrap();
    profile
}

fn docs_only_manifest() -> String {
    MilestoneFixtureBuilder::new("m-conditional-first")
        .task("docs-only", &["docs/"])
        .task_proof_policy("direct-proof")
        .build()
}

#[test]
fn conditional_gate_claim_and_submit_paths_share_profile_fixtures() {
    let source = docs_only_manifest();
    let mut memory = TaskHarness::memory_with_profile(
        &source,
        "docs/010-conditional-first.md",
        conditional_first_profile(),
    );
    let mut h = TaskHarness::disk_with_profile(
        &source,
        "docs/010-conditional-first.md",
        conditional_first_profile(),
    );
    for (disk_backed, harness) in [(false, &mut memory), (true, &mut h)] {
        let before = harness.store.inspect(&harness.id).unwrap();
        assert_eq!(before.stage, "code_gate", "disk_backed={disk_backed}");
        assert_eq!(before.status, "ready", "disk_backed={disk_backed}");

        let error = harness
            .claim_gate_in_session("code_gate", "explicit-builder", Some("explicit-session"))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("TASK_STAGE_INVALID"),
            "disk_backed={disk_backed}: {error}"
        );
        assert!(
            error.contains("code_gate"),
            "disk_backed={disk_backed}: {error}"
        );
        assert!(
            error.contains("docs_gate"),
            "disk_backed={disk_backed}: {error}"
        );
        let unchanged = harness.store.inspect(&harness.id).unwrap();
        assert_eq!(unchanged.stage, before.stage, "disk_backed={disk_backed}");
        assert_eq!(unchanged.status, before.status, "disk_backed={disk_backed}");
        assert_eq!(
            unchanged.claim_revision, before.claim_revision,
            "disk_backed={disk_backed}"
        );

        let claimed = harness.claim_next().unwrap();
        assert_eq!(claimed.stage, "docs_gate", "disk_backed={disk_backed}");
        assert_eq!(claimed.status, "in_progress", "disk_backed={disk_backed}");
        assert_eq!(
            claimed.worker_id.as_deref(),
            Some("builder"),
            "disk_backed={disk_backed}"
        );
        assert_eq!(
            harness.persisted_claim_session(claimed.claim_revision),
            TaskHarness::session_for("builder"),
            "disk_backed={disk_backed}"
        );
    }

    let h = TaskHarness::disk_with_profile(
        &source,
        "docs/010-conditional-first.md",
        conditional_first_profile(),
    );
    let worktree = h.root().to_string_lossy().into_owned();
    let next = tasks::manage(
        h.root(),
        tasks::Manage {
            action: tasks::ManageAction::Next,
            worker_id: Some("next-worker".into()),
            worktree: Some(worktree),
            session_fingerprint: Some("next-session".into()),
            ..Default::default()
        },
    )
    .unwrap();

    let claimed = h.store.inspect(&h.id).unwrap();
    assert_eq!(claimed.stage, "docs_gate");
    assert_eq!(claimed.status, "in_progress");
    assert_eq!(claimed.worker_id.as_deref(), Some("next-worker"));
    assert_eq!(next["active_stage_id"], "docs_gate");
    assert_eq!(
        h.persisted_claim_session(claimed.claim_revision),
        "next-session"
    );

    let mut auto_harness = TaskHarness::disk_with_profile(
        &source,
        "docs/010-conditional-first.md",
        conditional_first_profile(),
    );
    let claimed = auto_harness.claim_next().unwrap();
    assert_eq!(claimed.stage, "docs_gate");
    let expected_session = TaskHarness::session_for("builder");
    assert_eq!(
        auto_harness.persisted_claim_session(claimed.claim_revision),
        expected_session
    );

    let accepted = auto_harness.submit_pass().unwrap();
    assert_eq!(accepted["status"], "ready");
    assert_eq!(accepted["stage"], "finish");
    assert_eq!(
        auto_harness.persisted_claim_session(claimed.claim_revision),
        expected_session
    );
    assert_eq!(
        auto_harness.persisted_gate_evidence("docs_gate")["worker_id"],
        "builder"
    );
}

#[test]
fn nested_independence_and_worker_api_share_profile_store() {
    let manifest = MilestoneFixtureBuilder::new("m-nested-independent-leaf")
        .task("universal", &["src/"])
        .task_proof_policy("direct-proof")
        .build();
    let mut h = TaskHarness::memory_with_profile(
        &manifest,
        "docs/010-nested-independent-leaf.md",
        nested_independent_profile(),
    );
    {
        let a_work = h.claim_next().unwrap();
        assert_eq!(a_work.stage, "a.work");
        assert_eq!(a_work.worker_id.as_deref(), Some("architect"));
        assert_eq!(h.submit_pass().unwrap()["stage"], "b.work");
        let b_work = h.claim_next().unwrap();
        assert_eq!(b_work.stage, "b.work");
        assert_eq!(b_work.worker_id.as_deref(), Some("builder"));
        assert_eq!(h.submit_pass().unwrap()["stage"], "b.audit");
        assert_eq!(
            h.persisted_gate_evidence("a.work")["worker_id"],
            "architect"
        );
        assert_eq!(h.persisted_gate_evidence("b.work")["worker_id"], "builder");

        let audit = h.claim_next().unwrap();
        assert_eq!(audit.stage, "b.audit");
        assert_eq!(
            audit.worker_id.as_deref(),
            Some("builder-independent-b.audit")
        );
        assert_eq!(
            h.persisted_claim_session(audit.claim_revision),
            TaskHarness::session_for("builder-independent-b.audit")
        );
    }

    let manifest = MilestoneFixtureBuilder::new("m-nested-worker-api")
        .task("universal", &["src/"])
        .task_proof_policy("direct-proof")
        .build();
    let mut h = TaskHarness::memory_with_profile(
        &manifest,
        "docs/010-nested-worker-api.md",
        nested_independent_profile(),
    );
    {
        let a_work = h.claim_next().unwrap();
        assert_eq!(a_work.stage, "a.work");
        assert_eq!(a_work.worker_id.as_deref(), Some("architect"));
        assert_eq!(h.submit_pass().unwrap()["stage"], "b.work");
        let b_work = h.claim_next().unwrap();
        assert_eq!(b_work.stage, "b.work");
        assert_eq!(b_work.worker_id.as_deref(), Some("builder"));
        assert_eq!(h.submit_pass().unwrap()["stage"], "b.audit");

        let audit = h
            .executable_gates()
            .unwrap()
            .into_iter()
            .find(|gate| gate.id == "b.audit")
            .unwrap();
        assert_eq!(audit.independent_from.as_deref(), Some("work"));
        let worker = h.worker_for_gate(&audit);
        assert_eq!(worker, "builder-independent-b.audit");

        let session = TaskHarness::session_for(&worker);
        let claimed = h
            .claim_gate_in_session(&audit.id, &worker, Some(&session))
            .unwrap();
        assert_eq!(claimed.stage, "b.audit");
        assert_eq!(claimed.worker_id.as_deref(), Some(worker.as_str()));
        assert_eq!(h.persisted_claim_session(claimed.claim_revision), session);
    }
}
fn single_direct_task_manifest(id: &str, task_ref: &str) -> String {
    MilestoneFixtureBuilder::new(id)
        .task(task_ref, &["src/"])
        .task_proof_policy("direct-proof")
        .build()
}

fn profile_with_gates(gates: &str) -> contextunity_forge_mcp::core::tasks::profile::GateProfile {
    let mut profile = fixture_profile();
    profile.gates = serde_yaml::from_str(gates).unwrap();
    profile.validate().unwrap();
    profile
}

fn nested_independent_profile() -> contextunity_forge_mcp::core::tasks::profile::GateProfile {
    let mut profile = fixture_profile();
    profile.gates = serde_yaml::from_str(
        "- id: a\n  gates:\n    - id: work\n      role: architect\n      proof: none\n- id: b\n  gates:\n    - id: work\n      role: builder\n      proof: none\n    - id: audit\n      role: builder\n      proof: review\n      independent_from: work\n      reject_to: work\n      contours: guidance.contours\n",
    )
    .unwrap();
    profile.validate().unwrap();
    profile
}

fn in_memory_profile_harness() -> TaskHarness {
    use contextunity_forge_mcp::core::tasks::profile::ProofKind;

    let manifest = MilestoneFixtureBuilder::new("m-memory-profile")
        .task("universal-task", &["src/"])
        .task_proof_policy("direct-proof")
        .build();
    let mut profile = fixture_profile();
    profile.gates = serde_yaml::from_str(
        "- id: draft\n  role: builder\n  proof: none\n- id: peer_check\n  role: independent_reviewer\n  proof: review\n  independent_from: draft\n  reject_to: draft\n  contours: guidance.contours\n- id: assurance\n  role: independent_reviewer\n  proof: review\n  independent_from: peer_check\n  reject_to: draft\n  contours: guidance.contours\n- id: ship_it\n  role: independent_reviewer\n  proof: delivery\n  review_sources: [peer_check, assurance]\n  auto_commit: false\n",
    )
    .unwrap();
    profile.validate().unwrap();
    assert_eq!(profile.gates[0].proof.kind(), Some(ProofKind::None));
    TaskHarness::memory_with_profile(&manifest, "docs/010-memory-profile.md", profile)
}
