use super::*;

impl TaskHarness {
    /// Allocate a workspace for a task-domain fixture.
    pub fn workspace() -> Workspace {
        Workspace::new()
    }

    pub fn commit_handoff_fixture_inputs(root: &Workspace, paths: &[&str]) {
        crate::acdd::support::commit_handoff_fixture_inputs(root, paths);
    }

    pub fn workspace_with_file(path: &str, contents: &str) -> Workspace {
        let workspace = Self::workspace();
        workspace.write(path, contents);
        workspace
    }

    pub fn runner_workspace(seam: &str, extra_profile: &str) -> Workspace {
        acdd_workspace(seam, extra_profile)
    }

    pub fn session_for(worker: &str) -> String {
        format!("test-session-{worker}")
    }

    pub fn listings() -> Self {
        let workspace = Workspace::new();
        workspace.write("src/lib.rs", "pub fn example() {}\n");
        workspace.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        let mut store = TasksStore::open(&workspace.path(".forge/tasks.sqlite")).unwrap();
        for (path, id, task, status) in [
            (
                "docs/milestones/010-active.md",
                "m-active",
                "active-task",
                "active",
            ),
            (
                "docs/milestones/archive/009-archived.md",
                "m-archived",
                "archived-task",
                "completed",
            ),
            (
                "docs/milestones/008-planned.md",
                "m-planned",
                "planned-task",
                "planned",
            ),
            (
                "docs/milestones/archive/007-cancelled.md",
                "m-cancelled",
                "cancelled-task",
                "active",
            ),
        ] {
            let mut builder = MilestoneFixtureBuilder::new(id)
                .status(status)
                .task(task, &["src/"]);
            if id == "m-planned" {
                builder = builder.without_status();
            }
            if id == "m-active" {
                let subtask = serde_json::from_value(json!({"subtask_ref":"sub-1", "title":"Active subtask", "status":"in_progress", "evidence":"Secret heavy evidence blob"})).unwrap();
                builder = builder.task_subtasks(&[subtask]);
            } else {
                builder = builder.task_proof_policy("direct-proof");
            }
            let source = builder.build();
            workspace.write(path, &source);
            store
                .sync(
                    &Milestone::parse(&source, "forge-mcp").unwrap(),
                    path,
                    workspace.root(),
                )
                .unwrap();
        }
        workspace.write(
            "docs/milestones/archive/007-cancelled.md",
            &MilestoneFixtureBuilder::new("m-cancelled")
                .status("cancelled")
                .closure_reason("Replaced by a later milestone")
                .task("cancelled-task", &["src/"])
                .task_proof_policy("direct-proof")
                .build(),
        );
        Self {
            id: "forge-mcp/m-active:active-task".into(),
            store,
            root: workspace.root().to_owned(),
            _workspace: Some(workspace),
            in_memory: false,
        }
    }

    pub fn list(&self, options: tasks::List) -> anyhow::Result<Value> {
        tasks::list(self.root(), options)
    }

    pub fn claim_at(root: &Path, id: &str, stage: &str, worker: &str) -> anyhow::Result<Value> {
        Self::claim_in_worktree(
            root,
            id,
            stage,
            worker,
            root,
            Some(&Self::session_for(worker)),
        )
    }

    pub fn claim_in_worktree(
        root: &Path,
        id: &str,
        stage: &str,
        worker: &str,
        worktree: &Path,
        session: Option<&str>,
    ) -> anyhow::Result<Value> {
        tasks::claim(
            root,
            tasks::Claim {
                task_id: id.into(),
                stage: stage.into(),
                worker_id: worker.into(),
                worktree: worktree.to_string_lossy().into_owned(),
                session_fingerprint: session.map(str::to_owned),
                ..Default::default()
            },
        )
    }

    pub fn manage_task(
        root: &Path,
        id: &str,
        action: tasks::ManageAction,
    ) -> anyhow::Result<Value> {
        tasks::manage(
            root,
            tasks::Manage {
                action,
                task_id: Some(id.into()),
                ..Default::default()
            },
        )
    }

    pub fn observed_evidence(&self) -> Evidence {
        evidence(&self.store.inspect(&self.id).unwrap())
    }

    pub fn default_evidence(claim: &Task) -> Evidence {
        evidence(claim)
    }

    pub fn submit_evidence(&self, ev: &Evidence) -> anyhow::Result<Value> {
        tasks::submit(self.root(), Self::pass_request(&self.id, &ev.stage, ev))
    }

    pub fn pass_request(task_id: &str, stage: &str, ev: &Evidence) -> tasks::Submit {
        Self::pass_request_in_session(task_id, stage, ev, Some(&Self::session_for(&ev.worker_id)))
    }

    pub fn pass_request_in_session(
        task_id: &str,
        stage: &str,
        ev: &Evidence,
        session: Option<&str>,
    ) -> tasks::Submit {
        tasks::Submit {
            task_id: task_id.into(),
            stage: stage.into(),
            action: tasks::Action::Pass,
            evidence: serde_json::to_value(ev).unwrap(),
            findings: None,
            session_fingerprint: session.map(str::to_owned),
        }
    }

    pub fn persisted_claim_session(&self, revision: u64) -> String {
        self.store
            .connection
            .query_row(
                "SELECT session_fingerprint FROM task_claims WHERE task_id=?1 AND revision=?2",
                rusqlite::params![self.id, revision],
                |row| row.get(0),
            )
            .unwrap()
    }

    pub fn begin_competing_handoff_tombstone(
        &mut self,
        milestone_id: &str,
    ) -> anyhow::Result<rusqlite::Transaction<'_>> {
        let tx = self
            .store
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch("DELETE FROM task_revisions; DELETE FROM tasks;")?;
        tx.execute(
            "INSERT INTO milestone_tombstones VALUES(?1,1,1,'[]')",
            [milestone_id],
        )?;
        Ok(tx)
    }

    pub fn persisted_subtask_status(&self, subtask_ref: &str) -> anyhow::Result<String> {
        self.store
            .connection
            .query_row(
                "SELECT status FROM task_subtasks WHERE task_id=?1 AND subtask_ref=?2",
                rusqlite::params![self.id, subtask_ref],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Put every normalized row and the first descriptor entry into the legacy state repaired on read.
    pub fn set_all_subtasks_legacy_pending(&self) -> anyhow::Result<()> {
        let changed = self.store.connection.execute(
            "UPDATE task_subtasks SET status='pending' WHERE task_id=?1",
            [&self.id],
        )?;
        anyhow::ensure!(changed > 0, "TASK_SUBTASK_NOT_FOUND: {}", self.id);
        let task = self.store.inspect(&self.id)?;
        anyhow::ensure!(
            !task.spec.subtasks.is_empty(),
            "TASK_SUBTASK_NOT_FOUND: {}",
            self.id
        );
        let changed = self.store.connection.execute(
            "UPDATE tasks SET descriptor=json_set(descriptor, '$.spec.subtasks[0].status', 'pending') WHERE task_id=?1",
            [&self.id],
        )?;
        anyhow::ensure!(changed == 1, "TASK_NOT_FOUND: {}", self.id);
        Ok(())
    }
}
