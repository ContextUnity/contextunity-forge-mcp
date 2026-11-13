use super::*;

fn case_nested_container_gates_claim_and_execute_cleanly() {
    let profile = r#"version: 2
roles:
  worker:
    mode: subagent
gates:
  - id: pipeline
    gates:
      - id: a
        role: worker
        proof: none
      - id: b
        role: worker
        proof: none
  - id: deliver
    role: worker
    proof: delivery
"#;
    let manifest = "---\nid: m-pipeline\ntitle: Pipeline Test\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: nested-task\ntarget: Test nested gates\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    let mut harness = GateAstHarness::fixture(
        profile,
        "docs/milestones/010-pipe.md",
        manifest,
        "test/m-pipeline:nested-task",
        GateAstHarness::CONDITIONAL_CONFIG,
        &[("src/lib.rs", "pub fn ok() {}\n")],
    );

    let claim_a = harness.task_mut().claim_gate("pipeline.a", "worker");
    assert_eq!(claim_a.status, "in_progress");
    let submit_a = harness.task_mut().submit_pass().unwrap();
    assert_eq!(submit_a["status"], "ready");
    assert_eq!(submit_a["stage"], "pipeline.b");

    let claim_b = harness.task_mut().claim_gate("pipeline.b", "worker");
    assert_eq!(claim_b.status, "in_progress");
    let submit_b = harness.task_mut().submit_pass().unwrap();
    assert_eq!(submit_b["status"], "ready");
    assert_eq!(submit_b["stage"], "deliver");

    assert_eq!(
        harness
            .task()
            .store
            .list_checkpoints()
            .unwrap()
            .into_iter()
            .map(|checkpoint| checkpoint.breadcrumb)
            .collect::<Vec<_>>(),
        vec![
            "test/test/m-pipeline/tasks.pipeline/nested-task/a",
            "test/test/m-pipeline/tasks.pipeline/nested-task/b",
        ],
        "nested gate breadcrumbs should record leaf gate without dots in final token"
    );
}

fn case_conditional_gate_successors_and_exhaustion_survive_recovery() {
    let cases = [("gates", "seam"), ("tasks", "doc")];
    let manifest = "---\nid: m-replay\ntitle: Conditional replay\nstatus: active\n---\n```yaml\ntask_ref: first\ntarget: Replay conditional stages\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    let mut harness: Option<GateAstHarness> = None;
    for (container, condition) in cases {
        let profile = if container == "tasks" {
            format!(
                "version: 2\nroles:\n  worker: {{mode: subagent}}\ngates:\n  - tasks:\n      when: code\n      gates:\n        - id: start\n          role: worker\n          proof: none\n        - id: retry\n          fail: start\n          max_attempts: 1\n          gates:\n            - id: attempt\n              role: worker\n              proof: none\n  - tasks:\n      when: {condition}\n      gates:\n        - id: skipped\n          when: code\n          role: worker\n          proof: none\n"
            )
        } else {
            format!(
                "version: 2\nroles:\n  worker: {{mode: subagent}}\ngates:\n  - tasks:\n      gates:\n        - id: start\n          role: worker\n          proof: none\n        - id: conditional\n          when: {condition}\n          gates:\n            - id: skipped\n              when: code\n              role: worker\n              proof: none\n        - id: retry\n          fail: start\n          max_attempts: 1\n          gates:\n            - id: attempt\n              role: worker\n              proof: none\n"
            )
        };
        let profile = GateAstHarness::with_delivery(&profile);
        if let Some(harness) = harness.as_mut() {
            harness.restore_profile(&profile);
        } else {
            harness = Some(GateAstHarness::fixture(
                &profile,
                "docs/milestones/010-replay.md",
                manifest,
                "test/m-replay:first",
                GateAstHarness::CONDITIONAL_CONFIG,
                &[("src/lib.rs", "pub fn ok() {}\n")],
            ));
        }
        let harness = harness.as_mut().unwrap();
        let passed = harness.pass_gate("start").unwrap();
        assert_eq!(passed.stage, "retry.attempt");
        let before = harness.current_task().unwrap();
        harness.recover().unwrap();
        let after = harness.current_task().unwrap();
        assert_eq!(after.stage, before.stage);
        assert_eq!(after.status, before.status);

        let worker = "worker";
        let session = TaskHarness::session_for(worker);
        let claim = harness
            .task_mut()
            .claim_gate_in_session("retry.attempt", worker, Some(&session))
            .unwrap();
        let evidence = harness.task().evidence_for_gate(&claim).unwrap();
        let findings = serde_json::json!({"kind":"scope"});
        let error = harness
            .task_mut()
            .submit_rejection_in_session(&evidence, Some(&session), Some(&findings))
            .unwrap_err();
        assert!(error.to_string().contains("TASK_ATTEMPTS_EXHAUSTED"));
        let before = harness.current_task().unwrap();
        harness.recover().unwrap();
        let after = harness.current_task().unwrap();
        assert_eq!(after.stage, before.stage);
        assert_eq!(after.status, before.status);
    }
}

struct GateAstHarness {
    workspace: crate::common::Workspace,
    task: Option<TaskHarness>,
    baseline: FixtureSnapshot,
    id: String,
}

impl GateAstHarness {
    const CONDITIONAL_ID: &'static str = "test/m-conditional:first";
    const CONDITIONAL_PATH: &'static str = "docs/milestones/010-conditional.md";
    const CONDITIONAL_CONFIG: &'static str =
        "task_repository: test\ntask_project: test\nroots: [src/]\nmilestones: [docs/milestones]\n";

    fn conditional(profile: &str, source: &str, scope: &str) -> Self {
        let manifest = format!(
            "---\nid: m-conditional\ntitle: Conditional gates\nstatus: active\n---\n```yaml\ntask_ref: first\ntarget: Exercise conditional gates\nproof_policy: direct-proof\nscope: [{scope}]\n```\n"
        );
        let profile = Self::with_delivery(profile);
        Self::fixture(
            &profile,
            Self::CONDITIONAL_PATH,
            &manifest,
            Self::CONDITIONAL_ID,
            Self::CONDITIONAL_CONFIG,
            &[("src/lib.rs", source)],
        )
    }

    fn fixture(
        profile: &str,
        manifest_path: &str,
        manifest: &str,
        id: &str,
        config: &str,
        files: &[(&str, &str)],
    ) -> Self {
        use super::support::*;

        let workspace =
            crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
        workspace.write("AGENTS.md", "# Rules\n");
        workspace.write("forge-mcp.yaml", config);
        let profile = if profile.starts_with("inherit:") {
            profile.to_owned()
        } else {
            format!("inherit: none\n{profile}")
        };
        workspace.write(".forge/acdd/profile.yaml", &profile);
        workspace.write(manifest_path, manifest);
        for (path, content) in files {
            workspace.write(path, content);
        }
        git(workspace.root(), &["add", "-A"]);
        git(workspace.root(), &["commit", "-m", "init"]);
        tasks::manage(
            workspace.root(),
            tasks::Manage {
                action: tasks::ManageAction::Sync,
                milestone_ref: Some(manifest_path.into()),
                ..Default::default()
            },
        )
        .unwrap();

        drop(TaskHarness::attach(&workspace, id));
        let baseline = FixtureSnapshot::capture(&workspace);
        let task = TaskHarness::attach(&workspace, id);
        Self {
            workspace,
            task: Some(task),
            baseline,
            id: id.into(),
        }
    }

    fn with_delivery(profile: &str) -> String {
        let mut document: serde_yaml::Value =
            serde_yaml::from_str(profile).expect("gate fixture profile should be valid YAML");
        let gates = document["gates"]
            .as_sequence_mut()
            .expect("gate fixture profile should declare a gates sequence");
        let delivery: serde_yaml::Value = serde_yaml::from_str(
            "id: deliver\nrole: worker\nreceipt: true\nneed: []\nsave: snapshot\n",
        )
        .expect("delivery fixture should be valid YAML");
        let mut found_tasks_container = false;
        for (index, gate) in gates.iter_mut().enumerate() {
            let Some(task_gates) = gate
                .get_mut("tasks")
                .and_then(|tasks| tasks.get_mut("gates"))
                .and_then(serde_yaml::Value::as_sequence_mut)
            else {
                continue;
            };
            let mut track_delivery = delivery.clone();
            if index > 0 {
                track_delivery["id"] = serde_yaml::Value::String(format!("deliver_{index}"));
            }
            task_gates.push(track_delivery);
            found_tasks_container = true;
        }
        if !found_tasks_container {
            gates.push(serde_yaml::from_str(
                "tasks:\n  gates:\n    - id: deliver\n      role: worker\n      receipt: true\n      need: []\n      save: snapshot\n",
            ).expect("delivery task container should be valid YAML"));
        }
        serde_yaml::to_string(&document).expect("updated gate fixture profile should serialize")
    }

    fn task(&self) -> &TaskHarness {
        self.task.as_ref().expect("task store is attached")
    }

    fn task_mut(&mut self) -> &mut TaskHarness {
        self.task.as_mut().expect("task store is attached")
    }

    fn write(&self, path: &str, content: &str) {
        self.workspace.write(path, content);
    }

    fn restore_profile(&mut self, profile: &str) {
        drop(self.task.take());
        self.baseline.restore(&self.workspace);
        let profile = if profile.starts_with("inherit:") {
            profile.to_owned()
        } else {
            format!("inherit: none\n{profile}")
        };
        self.workspace.write(".forge/acdd/profile.yaml", &profile);
        self.task = Some(TaskHarness::attach(&self.workspace, &self.id));
    }

    fn pass_gate(
        &mut self,
        stage: &str,
    ) -> anyhow::Result<contextunity_forge_mcp::db::tasks_store::Task> {
        let worker = "worker";
        let session = TaskHarness::session_for(worker);
        let claim = self
            .task_mut()
            .claim_gate_in_session(stage, worker, Some(&session))?;
        let evidence = self.task().evidence_for_gate(&claim)?;
        self.task_mut()
            .submit_evidence_in_session(&evidence, Some(&session))
    }

    fn current_task(&self) -> anyhow::Result<contextunity_forge_mcp::db::tasks_store::Task> {
        self.task().store.inspect(&self.id)
    }

    fn advance_to(
        &mut self,
        target: &str,
    ) -> anyhow::Result<contextunity_forge_mcp::db::tasks_store::Task> {
        self.task_mut().advance_to(target)
    }

    fn advance_to_completion(
        &mut self,
    ) -> anyhow::Result<contextunity_forge_mcp::db::tasks_store::Task> {
        self.task_mut().advance_to_completion()
    }

    fn recover(&mut self) -> anyhow::Result<()> {
        drop(self.task.take());
        std::fs::remove_file(self.workspace.root().join(".forge/tasks.sqlite"))?;
        contextunity_forge_mcp::engine::milestones::recovery::db_recover(self.workspace.root())?;
        self.task = Some(TaskHarness::attach(&self.workspace, &self.id));
        Ok(())
    }
}

fn case_container_pass_uses_last_applicable_child() {
    let pairs = [("code", "doc"), ("doc", "code")];
    let mut harness: Option<GateAstHarness> = None;
    for (first, last) in pairs {
        let profile = format!(
            "version: 2\nroles:\n  worker: {{mode: subagent}}\ngates:\n  - tasks:\n      gates:\n        - id: cycle\n          pass: done\n          gates:\n            - id: first\n              when: {first}\n              role: worker\n              proof: none\n            - id: last\n              when: {last}\n              role: worker\n              proof: none\n        - id: unwanted\n          role: worker\n          proof: none\n        - id: done\n          role: worker\n          proof: none\n"
        );
        if let Some(harness) = harness.as_mut() {
            let profile = GateAstHarness::with_delivery(&profile);
            harness.restore_profile(&profile);
        } else {
            harness = Some(GateAstHarness::conditional(
                &profile,
                "pub fn value() {}\n",
                "src/",
            ));
        }
        let harness = harness.as_mut().unwrap();
        let stage = if first == "code" {
            "cycle.first"
        } else {
            "cycle.last"
        };
        assert_eq!(harness.pass_gate(stage).unwrap().stage, "done");
    }
}

fn case_initial_docs_gate_does_not_require_an_unrelated_code_snapshot() {
    let profile = "version: 2\nroles:\n  worker: {mode: subagent}\ngates:\n  - id: code\n    tasks:\n      when: code\n      gates:\n        - id: build\n          role: worker\n          proof: none\n          save: snapshot\n  - id: docs\n    tasks:\n      when: doc\n      gates:\n        - id: build\n          role: worker\n          proof: none\n        - id: review\n          role: worker\n          proof: none\n";
    let mut harness = GateAstHarness::conditional(profile, "pub fn value() {}\n", "docs/guide.md");
    harness.write("docs/guide.md", "# Guide\n");
    assert_eq!(
        harness.pass_gate("docs.build").unwrap().stage,
        "docs.review"
    );
}

fn case_same_track_gate_rejects_a_missing_accepted_snapshot() {
    let profile = "version: 2\nroles:\n  worker: {mode: subagent}\ngates:\n  - id: code\n    tasks:\n      when: code\n      gates:\n        - id: build\n          role: worker\n          proof: none\n          save: snapshot\n        - id: review\n          role: worker\n          proof: none\n";
    let mut harness = GateAstHarness::conditional(profile, "pub fn value() {}\n", "src/");
    assert_eq!(
        harness.pass_gate("code.build").unwrap().stage,
        "code.review"
    );
    let id = harness.id.clone();
    harness
        .task_mut()
        .store
        .connection
        .execute("DELETE FROM task_gates WHERE task_id=?1", [&id])
        .unwrap();
    let error = harness.pass_gate("code.review").unwrap_err();
    assert!(
        error.to_string().contains("TASK_SNAPSHOT_NOT_FOUND"),
        "{error:#}"
    );
}

fn case_specialized_dispatch_skips_reviews_owned_by_the_builder() {
    let profile = "version: 2\nroles:\n  worker: {mode: subagent}\ngates:\n  - id: code\n    tasks:\n      when: code\n      gates:\n        - id: build\n          role: worker\n          proof: none\n        - id: review\n          role: worker\n          proof: none\n          independent_from: build\n  - id: docs\n    tasks:\n      when: doc\n      gates:\n        - id: build\n          role: worker\n          proof: none\n        - id: review\n          role: worker\n          proof: none\n          independent_from: build\n";
    let mut harness = GateAstHarness::conditional(profile, "pub fn value() {}\n", "src/");
    harness.pass_gate("build").unwrap();
    let worktree = harness.workspace.root().to_string_lossy().into_owned();
    let worker_session = TaskHarness::session_for("worker");
    assert!(harness
        .task_mut()
        .store
        .next("worker", &worktree, Some(&worker_session), |_, _, _| Ok(()))
        .unwrap()
        .is_none());
    let reviewer_session = TaskHarness::session_for("reviewer");
    assert_eq!(
        harness
            .task_mut()
            .store
            .next(
                "reviewer",
                &worktree,
                Some(&reviewer_session),
                |_, _, _| Ok(()),
            )
            .unwrap()
            .unwrap()
            .stage,
        "code.review"
    );
}

fn case_specialized_rejection_routes_recover_within_track() {
    let profile = "version: 2\nroles:\n  worker: {mode: subagent}\ngates:\n  - id: code\n    tasks:\n      when: code\n      gates:\n        - id: build\n          role: worker\n          proof: none\n          pass: review\n        - id: review\n          role: worker\n          proof: none\n          fail: build\n          loop: {scope: build, symbols: build}\n  - id: docs\n    tasks:\n      when: doc\n      gates:\n        - id: build\n          role: worker\n          proof: none\n        - id: review\n          role: worker\n          proof: none\n          fail: build\n          loop: {scope: build, symbols: build}\n";
    let mut harness = GateAstHarness::conditional(profile, "pub fn value() {}\n", "src/");
    let profile = GateAstHarness::with_delivery(profile);
    for (index, kind) in ["scope", "symbols"].into_iter().enumerate() {
        if index > 0 {
            harness.restore_profile(&profile);
        }
        let passed = harness.pass_gate("build").unwrap();
        assert_eq!(passed.stage, "code.review");
        assert_eq!(harness.current_task().unwrap().stage, "code.review");
        let worker = "reviewer";
        let session = TaskHarness::session_for(worker);
        let claim = harness
            .task_mut()
            .claim_gate_in_session("code.review", worker, Some(&session))
            .unwrap();
        let evidence = harness.task().evidence_for_gate(&claim).unwrap();
        let findings = serde_json::json!({"kind":kind});
        let result = harness
            .task_mut()
            .submit_rejection_in_session(&evidence, Some(&session), Some(&findings))
            .unwrap();
        assert_eq!(result.stage, "code.build");
        harness.recover().unwrap();
        assert_eq!(harness.current_task().unwrap().stage, "code.build");
    }
}

fn case_conditional_container_pass_survives_database_recovery() {
    let mut harness = GateAstHarness::conditional(
        "version: 2\nroles:\n  worker: {mode: subagent}\ngates:\n  - tasks:\n      gates:\n        - id: cycle\n          pass: done\n          gates:\n            - id: active\n              role: worker\n              proof: none\n            - id: skipped\n              when: doc\n              role: worker\n              proof: none\n        - id: unwanted\n          role: worker\n          proof: none\n        - id: done\n          role: worker\n          proof: none\n",
        "pub fn value() {}\n",
        "src/",
    );
    assert_eq!(harness.pass_gate("cycle.active").unwrap().stage, "done");
    harness.recover().unwrap();
    assert_eq!(harness.current_task().unwrap().stage, "done");
}

fn case_public_api_condition_uses_extracted_rust_visibility() {
    let profile = "version: 2\nroles:\n  worker: {mode: subagent}\ngates:\n  - tasks:\n      gates:\n        - id: api\n          when: public_api\n          role: worker\n          proof: none\n        - id: fallback\n          role: worker\n          proof: none\n";
    let mut harness = GateAstHarness::conditional(profile, "fn hidden() {}\n", "src/");
    for (source, expected) in [
        ("pub fn visible() {}\n", "api"),
        (
            "pub struct Item;\nimpl Item { pub fn visible() {} }\n",
            "api",
        ),
        ("pub trait Api { fn visible(&self); }\n", "api"),
        ("fn hidden() {}\n", "fallback"),
    ] {
        harness.write("src/lib.rs", source);
        if harness.current_task().unwrap().status != "ready" {
            let id = harness.id.clone();
            harness.task_mut().store.reset(&id).unwrap();
        }
        let claim = harness.task_mut().claim_next().unwrap();
        assert_eq!(claim.stage, expected, "{source}: {claim:?}");
        let id = harness.id.clone();
        harness.task_mut().store.reset(&id).unwrap();
    }
}

fn case_durable_template_receipt_selects_canonical_candidate_and_review_paths() {
    use super::support::{json, Receipt};
    use contextunity_forge_mcp::core::tasks::{
        gates::validate_durable_receipt_with_profile, profile::GateProfile,
    };
    let profile: GateProfile = serde_yaml::from_str("gate_templates:\n  code_cycle:\n    gates:\n      - {id: candidate, proof: command, sha_snapshot: true}\n      - {id: review, proof: none}\n      - {id: deliver, proof: delivery, review_sources: [review]}\n  docs_cycle:\n    gates:\n      - {id: candidate, proof: none, sha_snapshot: true}\n      - {id: review, proof: none}\n      - {id: deliver, proof: delivery, review_sources: [review]}\ngates:\n  - {id: code, use: code_cycle}\n  - {id: docs, use: docs_cycle}\n").unwrap();
    let receipt: Receipt = serde_json::from_value(json!({"delivery_gate":"docs.deliver","candidate_gate":"docs.candidate","contract_revision":1,"passed_at":"2026-11-04T12:00:00Z","evidence":{"none_proof":{}},"review":{"none_proof":{}},"decision":"pass"})).unwrap();
    validate_durable_receipt_with_profile(&receipt, 1, &profile).unwrap();
    let mut missing = receipt.clone();
    missing.delivery_gate = None;
    assert!(validate_durable_receipt_with_profile(&missing, 1, &profile).is_err());
    let mut wrong = receipt;
    wrong.candidate_gate = Some("code.candidate".into());
    assert!(validate_durable_receipt_with_profile(&wrong, 1, &profile).is_err());
}

fn case_specialized_delivery_receipt_retains_track_through_sync_and_recovery() {
    let mut profile = "inherit: none\nversion: 2\nroles:\n  worker: {mode: subagent}\ncommands:\n  test:\n    command: 'true'\nguidance:\n  contour.paths: {description: Paths, criteria: [Review path changes]}\n  contour.claims: {description: Claims, criteria: [Review claim behavior]}\n  contour.concurrency: {description: Concurrency, criteria: [Review concurrent updates]}\n  contour.project_isolation: {description: Project isolation, criteria: [Review project boundaries]}\n  contour.administration: {description: Administration, criteria: [Review administrative behavior]}\ngates:\n".to_owned();
    for (track, condition, proof) in [("code", "code", "command"), ("docs", "doc", "none")] {
        let command = if proof == "command" {
            "          command: test\n"
        } else {
            ""
        };
        profile.push_str(&format!("  - id: {track}\n    tasks:\n      when: {condition}\n      gates:\n        - id: candidate\n          role: worker\n          proof: {proof}\n{command}          save: snapshot\n        - id: review\n          role: worker\n          proof: review\n          contours: guidance.contours\n          independent_from: candidate\n          fail: candidate\n        - id: deliver\n          role: worker\n          proof: delivery\n          receipt: true\n          review_sources: [review]\n          auto_commit: false\n"));
    }
    let manifest = "---\nid: m-tracks\ntitle: Track receipts\nstatus: active\n---\n```yaml\ntask_ref: first\ntarget: Deliver documentation\nproof_policy: direct-proof\nscope: [docs/guide.md]\n```\n";
    let mut harness = GateAstHarness::fixture(
        &profile,
        "docs/milestones/010-tracks.md",
        manifest,
        "test/m-tracks:first",
        GateAstHarness::CONDITIONAL_CONFIG,
        &[("docs/guide.md", "# Guide\n")],
    );
    harness.advance_to_completion().unwrap();
    let details = harness.task().store.inspect_details(&harness.id).unwrap();
    let passed_gate_stages = details["gates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|gate| gate["state"] == "passed")
        .map(|gate| gate["stage"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        passed_gate_stages,
        ["docs.candidate", "docs.review", "docs.deliver"]
    );
    drop(harness.task.take());
    contextunity_forge_mcp::engine::tasks::manage(
        harness.workspace.root(),
        contextunity_forge_mcp::engine::tasks::Manage {
            action: contextunity_forge_mcp::engine::tasks::ManageAction::Sync,
            milestone_ref: Some("docs/milestones/010-tracks.md".into()),
            ..Default::default()
        },
    )
    .unwrap();
    harness.task = Some(TaskHarness::attach(&harness.workspace, &harness.id));
    let task = harness.current_task().unwrap();
    assert_eq!(task.stage, "docs.deliver");
    assert_eq!(
        task.receipt.unwrap().delivery_gate.as_deref(),
        Some("docs.deliver")
    );
    drop(harness.task.take());
    let handoff = contextunity_forge_mcp::engine::milestones::handoff_context(
        harness.workspace.root(),
        "m-tracks",
        "HEAD",
    );
    harness.task = Some(TaskHarness::attach(&harness.workspace, &harness.id));
    handoff.unwrap();
    harness.recover().unwrap();
    let task = harness.current_task().unwrap();
    assert_eq!(task.stage, "docs.deliver");
    assert_eq!(
        task.receipt.unwrap().delivery_gate.as_deref(),
        Some("docs.deliver")
    );
}

fn case_delivery_preserves_multiple_review_source_keys_after_candidate_substitution() {
    let profile = r#"inherit: none
version: 2
roles:
  worker: {mode: subagent}
guidance:
  contour.paths: {description: Paths, criteria: [Review path changes]}
  contour.claims: {description: Claims, criteria: [Review claim behavior]}
  contour.concurrency: {description: Concurrency, criteria: [Review concurrent updates]}
  contour.project_isolation: {description: Project isolation, criteria: [Review project boundaries]}
  contour.administration: {description: Administration, criteria: [Review administrative behavior]}
gates:
  - tasks:
      gates:
        - id: candidate
          role: worker
          proof: none
          save: snapshot
        - id: review
          role: worker
          proof: review
          contours: guidance.contours
        - id: security
          role: worker
          proof: review
          contours: guidance.contours
        - id: seam
          gates:
            - id: integrate
              role: worker
              proof: none
              save: snapshot
            - id: review
              role: worker
              proof: review
              contours: guidance.contours
            - id: security
              role: worker
              proof: review
              contours: guidance.contours
        - id: deliver
          role: worker
          proof: delivery
          receipt: true
          need: [review, security]
          auto_commit: false
 "#;
    contextunity_forge_mcp::core::tasks::profile::validate_profile_document(profile).unwrap();
    let manifest = "---\nid: m-reviews\ntitle: Multiple reviews\nstatus: active\n---\n```yaml\ntask_ref: first\ntarget: Preserve review identities\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    let mut harness = GateAstHarness::fixture(
        profile,
        "docs/milestones/010-reviews.md",
        manifest,
        "forge-mcp/m-reviews:first",
        "task_repository: forge-mcp\ntask_project: forge-mcp\nroots: [src/]\nmilestones: [docs/milestones]\n",
        &[("src/lib.rs", "pub fn value() -> u8 { 0 }\n")],
    );
    harness.advance_to("seam.integrate").unwrap();
    harness.write("src/lib.rs", "pub fn value() -> u8 { 1 }\n");
    harness.advance_to_completion().unwrap();
    let task = harness.current_task().unwrap();
    assert_eq!(task.status, "completed");
    let receipt = task.receipt.unwrap();
    let sources = receipt.review["review_sources"].as_object().unwrap();
    assert_eq!(
        sources.keys().map(String::as_str).collect::<Vec<_>>(),
        ["review", "security"]
    );
}

#[test]
fn nested_and_conditional_gate_cases() {
    case_nested_container_gates_claim_and_execute_cleanly();
    case_conditional_gate_successors_and_exhaustion_survive_recovery();
    case_container_pass_uses_last_applicable_child();
    case_specialized_dispatch_skips_reviews_owned_by_the_builder();
    case_specialized_rejection_routes_recover_within_track();
    case_conditional_container_pass_survives_database_recovery();
    case_public_api_condition_uses_extracted_rust_visibility();
}

#[test]
fn track_snapshot_gate_cases() {
    case_initial_docs_gate_does_not_require_an_unrelated_code_snapshot();
    case_same_track_gate_rejects_a_missing_accepted_snapshot();
}

#[test]
fn delivery_receipt_gate_cases() {
    case_durable_template_receipt_selects_canonical_candidate_and_review_paths();
    case_specialized_delivery_receipt_retains_track_through_sync_and_recovery();
}

#[test]
fn nested_seam_delivery_preserves_review_sources_after_candidate_substitution() {
    case_delivery_preserves_multiple_review_source_keys_after_candidate_substitution();
}
