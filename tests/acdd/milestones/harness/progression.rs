use super::*;

pub(crate) struct SubmittedGate {
    pub stage: String,
    pub is_delivery: bool,
    pub status: String,
}

pub(crate) enum CompletionMode<'a> {
    TaskRef(&'a str),
    Store {
        store: &'a TasksStore,
        task_id: &'a str,
    },
    Project {
        repository: &'a str,
        project: &'a str,
        task_id: &'a str,
    },
    Builder {
        store: &'a TasksStore,
        task_id: &'a str,
        builder: &'a str,
    },
}

impl MilestoneHarness {
    pub(crate) fn complete_task(
        &self,
        mode: CompletionMode<'_>,
    ) -> anyhow::Result<Vec<SubmittedGate>> {
        let (owned_store, store, task_id, builder) = match mode {
            CompletionMode::TaskRef(task_ref) => {
                let store = self.store();
                let task_id = self.task_id_in(&store, task_ref);
                (Some(store), None, task_id, "builder")
            }
            CompletionMode::Store { store, task_id } => {
                (None, Some(store), task_id.to_owned(), "builder")
            }
            CompletionMode::Project {
                repository,
                project,
                task_id,
            } => (
                Some(self.store_in(repository, project)),
                None,
                task_id.to_owned(),
                "builder",
            ),
            CompletionMode::Builder {
                store,
                task_id,
                builder,
            } => (None, Some(store), task_id.to_owned(), builder),
        };
        let store = store.or(owned_store.as_ref()).unwrap();

        use contextunity_forge_mcp::core::tasks::profile::ProofKind;
        let harness = TaskHarness::attach(self, &task_id);
        let mut submissions = Vec::new();
        loop {
            let before = store.inspect(&task_id)?;
            if before.status == "completed" {
                break;
            }
            let profile = harness.profile()?;
            let gate = profile
                .by_stage(&before.stage)
                .ok_or_else(|| anyhow::anyhow!("profile has no gate {}", before.stage))?;
            let kind = gate.proof.kind();
            let worker_id =
                if gate.role.as_deref() == Some("builder") && gate.independent_from.is_none() {
                    builder.to_owned()
                } else {
                    harness.worker_for_stage(&before.stage)?
                };
            TaskHarness::claim_at(self.root(), &task_id, &before.stage, &worker_id)?;
            let task = store.inspect(&task_id)?;
            let mut evidence = harness.evidence_for_gate(&task)?;
            if kind == Some(ProofKind::Review) && !task.spec.extended_scope.is_empty() {
                evidence.proof["review_proof"]["extended_scope_review"] = json!({
                    "approved_paths": task.spec.extended_scope,
                    "reason_verified": true,
                });
            }
            evidence.commit = None;
            let result = tasks::submit(
                self.root(),
                TaskHarness::pass_request(&task_id, &task.stage, &evidence),
            )?;
            submissions.push(SubmittedGate {
                stage: task.stage.clone(),
                is_delivery: kind == Some(ProofKind::Delivery),
                status: result["status"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("submission has no status: {result}"))?
                    .to_owned(),
            });
            let advanced = store.inspect(&task_id)?;
            anyhow::ensure!(
                advanced.status == "completed" || advanced.stage != before.stage,
                "gate stalled: {}",
                before.stage
            );
        }
        Ok(submissions)
    }
}

impl MilestoneHarness {
    pub(crate) fn lifecycle_workspace() -> Self {
        let root = Self::configured_workspace();
        root.write("src/lib.rs", "pub fn lifecycle() {}\n");
        drop(root.store());
        root
    }

    pub(crate) fn context_retention() -> Self {
        let mut root = Self::from_catalog(include_str!(
            "../../fixtures/milestones/context_retention.json"
        ));
        root.milestone_id = "m-context".into();
        root.active_path = "docs/milestones/012-context.md".into();
        root.sync(root.active_path()).unwrap();
        root
    }

    pub(crate) fn inspect_task(&self, task_id: &str) -> Value {
        tasks::manage(
            self.root(),
            tasks::Manage {
                action: tasks::ManageAction::Inspect,
                task_id: Some(task_id.to_owned()),
                ..Default::default()
            },
        )
        .unwrap()
    }

    pub(crate) fn post_task_message(
        &self,
        task_id: &str,
        author: &str,
        topic: &str,
        payload: &str,
    ) -> Value {
        contextunity_forge_mcp::forge_tasks::engine::blackboard::blackboard(self.root(),
            serde_json::from_value(json!({"action":"post", "task_id":task_id, "author":author, "topic":topic, "payload":payload})).unwrap(), "cli").unwrap()
    }

    pub(crate) fn read_task_messages(&self, task_id: &str) -> Value {
        contextunity_forge_mcp::forge_tasks::engine::blackboard::blackboard(
            self.root(),
            serde_json::from_value(json!({"action":"read", "task_id":task_id})).unwrap(),
            "cli",
        )
        .unwrap()
    }

    pub(crate) fn pass_gate_without_commit(
        &self,
        task_id: &str,
        stage: &str,
        worker: &str,
    ) -> GateObservation {
        let before = self.inspect_task(task_id);
        let claimed = TaskHarness::claim_at(self.root(), task_id, stage, worker).unwrap();
        let stored = self.store().inspect(task_id).unwrap();
        let mut proof = evidence(&stored);
        proof.commit = None;
        let submitted = tasks::submit(
            self.root(),
            TaskHarness::pass_request(task_id, stage, &proof),
        )
        .unwrap();
        GateObservation {
            before,
            claimed,
            submitted,
        }
    }
}
