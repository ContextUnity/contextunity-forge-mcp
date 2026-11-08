use super::*;

impl MilestoneHarness {
    pub(crate) fn receipt_workspace(
        milestone_path: &str,
        milestone_content: &str,
        extra_config: &str,
    ) -> MilestoneHarness {
        let milestone = Milestone::parse(milestone_content, "forge-mcp")
            .expect("receipt workspace requires a valid milestone fixture");
        let mut root = MilestoneHarness::workspace(DEFAULT_SEAM, "");
        root.milestone_id = milestone.id;
        root.active_path = milestone_path.to_owned();
        root.write(
            "forge-mcp.yaml",
            &format!(
                "tasks_db: .forge/tasks.sqlite\nagents_guidance: docs/AGENTS.md\n{extra_config}"
            ),
        );
        root.write("docs/AGENTS.md", "# Agent Guidance\n");
        root.write("src/lib.rs", "pub fn work() -> bool { true }\n");
        root.write(milestone_path, milestone_content);
        root.stage_paths(&["."]);
        root.commit_staged("Initial milestone contract");
        root
    }

    pub(crate) fn overwrite_passed_gate_evidence(
        store: &TasksStore,
        task: &str,
        gate: &str,
        evidence: &str,
    ) -> anyhow::Result<usize> {
        Ok(store.connection.execute(
            "UPDATE task_gates SET evidence=?1 WHERE task_id=?2 AND gate=?3 AND state='passed'",
            rusqlite::params![evidence, task, gate],
        )?)
    }

    pub(crate) fn read_task(&self, id: &str) -> contextunity_forge_mcp::db::tasks_store::Task {
        tasks::store_for_task(self.root(), id)
            .unwrap()
            .inspect(id)
            .unwrap()
    }

    pub(crate) fn tombstone(store: &TasksStore, milestone_id: &str) -> TombstoneFacts {
        let (terminal, completed): (i64, String) = store.connection.query_row(
            "SELECT all_tasks_terminal, completed_task_ids FROM milestone_tombstones WHERE milestone_id=?1", [milestone_id],
            |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        let total = store
            .connection
            .query_row("SELECT COUNT(*) FROM milestone_tombstones", [], |row| {
                row.get(0)
            })
            .unwrap();
        TombstoneFacts {
            terminal,
            completed_tasks: serde_json::from_str(&completed).unwrap(),
            total,
        }
    }

    pub(crate) fn task_snapshot(store: &TasksStore, id: &str) -> Value {
        serde_json::to_value(store.inspect(id).unwrap()).unwrap()
    }

    pub(crate) fn task_id(&self, task_ref: &str) -> String {
        self.task_id_in(&self.store(), task_ref)
    }

    pub(crate) fn task_id_in(&self, store: &TasksStore, task_ref: &str) -> String {
        store
            .list(None, "all", None)
            .unwrap()
            .into_iter()
            .find(|task| task.milestone_ref == self.active_path && task.spec.task_ref == task_ref)
            .unwrap()
            .task_id
    }

    pub(crate) fn store(&self) -> TasksStore {
        tasks::store(self.root()).unwrap()
    }

    pub(crate) fn store_in(&self, repository: &str, project: &str) -> TasksStore {
        TasksStore::open_project(&self.path(".forge/tasks.sqlite"), repository, project).unwrap()
    }

    pub(crate) fn sync_document(&self, path: &str, source: &str) -> (Milestone, TasksStore) {
        let milestone = Milestone::parse(source, "forge-mcp").unwrap();
        let mut store = TasksStore::open(&self.path(".forge/tasks.sqlite")).unwrap();
        store.sync(&milestone, path, self.root()).unwrap();
        (milestone, store)
    }

    pub(crate) fn post_blackboard_in(
        &self,
        repository: &str,
        project: &str,
        path: &str,
        topic: &str,
        payload: &str,
    ) {
        let store = TasksStore::open_project(
            &tasks::database_path(self.root()).unwrap(),
            repository,
            project,
        )
        .unwrap();
        let scope = store.milestone_scope_ref(path).unwrap();
        store
            .blackboard_post_scoped(&scope, None, None, "fixture", topic, payload)
            .unwrap();
    }

    pub(crate) fn active_path(&self) -> &str {
        &self.active_path
    }

    pub(crate) fn task_rows(store: &TasksStore, table: &str, task_id: &str) -> anyhow::Result<i64> {
        anyhow::ensure!(
            [
                "tasks",
                "task_claims",
                "task_gates",
                "task_findings",
                "task_scope_paths",
                "task_dependencies",
                "task_submissions",
                "task_subtasks",
                "task_revisions"
            ]
            .contains(&table),
            "unsupported task table: {table}"
        );
        Ok(store.connection.query_row(
            &format!("SELECT count(*) FROM {table} WHERE task_id=?1"),
            [task_id],
            |row| row.get(0),
        )?)
    }

    pub(crate) fn persist_descriptor(
        store: &TasksStore,
        task: &contextunity_forge_mcp::db::tasks_store::Task,
    ) {
        store
            .connection
            .execute(
                "UPDATE tasks SET descriptor=?1 WHERE task_id=?2",
                rusqlite::params![serde_json::to_string(task).unwrap(), task.task_id],
            )
            .unwrap();
    }

    pub(crate) fn seed_dependency(store: &TasksStore, task_id: &str, dependency: &str) {
        store.connection.execute("INSERT INTO task_dependencies(task_id, dependency_id, satisfied) VALUES (?1, ?2, 0)", rusqlite::params![task_id, dependency]).unwrap();
    }

    pub(crate) fn post_scoped_message(
        store: &TasksStore,
        scope: &str,
        task_id: Option<&str>,
        topic: &str,
        payload: &str,
    ) {
        store
            .blackboard_post_scoped(scope, task_id, None, "reviewer", topic, payload)
            .unwrap();
    }

    pub(crate) fn dependency_satisfied(store: &TasksStore, task_id: &str, dependency: &str) -> i64 {
        store
            .connection
            .query_row(
                "SELECT satisfied FROM task_dependencies WHERE task_id=?1 AND dependency_id=?2",
                rusqlite::params![task_id, dependency],
                |row| row.get(0),
            )
            .unwrap()
    }

    pub(crate) fn stored_reviews(store: &TasksStore, task_id: &str) -> Vec<(String, Value)> {
        store.connection.prepare("SELECT gate,evidence FROM task_gates WHERE task_id=?1 AND gate IN ('review','security_review') AND state='passed' ORDER BY gate").unwrap()
            .query_map([task_id], |row| {
                let evidence: String = row.get(1)?;
                Ok((row.get(0)?, serde_json::from_str(&evidence).unwrap()))
            }).unwrap()
            .collect::<rusqlite::Result<_>>().unwrap()
    }

    pub(crate) fn sync(&self, path: &str) -> anyhow::Result<Value> {
        self.sync_in(path, None)
    }

    pub(crate) fn sync_in(&self, path: &str, workspace: Option<&str>) -> anyhow::Result<Value> {
        self.sync_selection(Some(path), workspace)
    }

    pub(crate) fn sync_selection(
        &self,
        path: Option<&str>,
        workspace: Option<&str>,
    ) -> anyhow::Result<Value> {
        tasks::manage(
            self.root(),
            tasks::Manage {
                action: tasks::ManageAction::Sync,
                milestone_ref: path.map(str::to_owned),
                workspace: workspace.map(str::to_owned),
                ..Default::default()
            },
        )
    }

    pub(crate) fn tasks_in(&self, repository: &str, milestone: &str) -> Value {
        self.list_tasks(repository, milestone).unwrap()
    }

    pub(crate) fn list_tasks(&self, repository: &str, milestone: &str) -> anyhow::Result<Value> {
        self.list_tasks_request(tasks::List {
            repository: Some(repository.to_owned()),
            milestone_ref: Some(milestone.to_owned()),
            status: Some(tasks::Status::All),
            ..Default::default()
        })
    }

    pub(crate) fn list_tasks_request(&self, request: tasks::List) -> anyhow::Result<Value> {
        tasks::list(self.root(), request)
    }
}
