use super::*;

impl TaskHarness {
    pub fn linked_selectors(include_primary_slug_collision: bool) -> (Self, Workspace) {
        let primary = Workspace::new();
        let linked = Workspace::new();
        primary.write("forge-mcp.yaml", &format!(
            "task_repository: primary\ntask_project: primary\nroots: []\nlinked_workspaces:\n  - name: linked\n    path: {}\n    tasks: {{enabled: true}}\n", linked.root().display()
        ));
        linked.write(
            "forge-mcp.yaml",
            "task_repository: linked\ntask_project: linked\nroots: []\n",
        );
        for (root, path, id, second) in [
            (
                &primary,
                "docs/milestones/010-primary-long-name.md",
                "m-direct-match",
                "primary-only",
            ),
            (
                &linked,
                "docs/milestones/020-linked-long-name.md",
                "m-linked",
                "m-direct-match",
            ),
        ] {
            root.write(
                path,
                &MilestoneFixtureBuilder::new(id)
                    .task("shared-task", &["src/"])
                    .task_proof_policy("direct-proof")
                    .task(second, &["src/"])
                    .task_proof_policy("direct-proof")
                    .build(),
            );
            tasks::manage(
                primary.root(),
                tasks::Manage {
                    action: tasks::ManageAction::Sync,
                    milestone_ref: Some(path.into()),
                    workspace: Some(
                        if id == "m-linked" {
                            "linked"
                        } else {
                            "primary"
                        }
                        .into(),
                    ),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        if include_primary_slug_collision {
            let path = "docs/milestones/011-primary-second.md";
            primary.write(
                path,
                &MilestoneFixtureBuilder::new("m-primary-second")
                    .task("shared-task", &["src/"])
                    .task_proof_policy("direct-proof")
                    .build(),
            );
            tasks::manage(
                primary.root(),
                tasks::Manage {
                    action: tasks::ManageAction::Sync,
                    milestone_ref: Some(path.into()),
                    workspace: Some("primary".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let h = Self {
            id: "primary/m-direct-match:shared-task".into(),
            store: tasks::store(primary.root()).unwrap(),
            root: primary.root().to_owned(),
            _workspace: Some(primary),
            in_memory: false,
        };
        (h, linked)
    }

    pub fn selectors() -> Self {
        let workspace = Workspace::new();
        workspace.write("AGENTS.md", "# Rules\n");
        workspace.write("forge-mcp.yaml", "task_repository: test\ntask_project: test\nroots: []\nmilestones: [docs/milestones, contracts]\n");
        for (path, milestone, task, status) in [
            (
                "docs/milestones/030-tool-performance-and-storage-compaction.md",
                "m-tool-performance-and-storage-compaction",
                "compaction",
                "active",
            ),
            (
                Self::ALIAS_PATH,
                "m-mcp-response-deduplication-and-agent-efficiency",
                Self::ALIAS_TASK,
                "active",
            ),
            (
                Self::CONFIGURED_PATH,
                "m-configured-milestone",
                "configured-task",
                "active",
            ),
            (
                Self::ARCHIVED_PATH,
                "m-archived-milestone",
                "archived-task",
                "completed",
            ),
        ] {
            let source = MilestoneFixtureBuilder::new(milestone)
                .status(status)
                .task(task, &["src/"])
                .task_proof_policy("direct-proof")
                .build();
            workspace.write(path, &source);
            // Retained history can contain archived tasks; admission itself is tested separately.
            if path == Self::ARCHIVED_PATH {
                tasks::store(workspace.root())
                    .unwrap()
                    .sync(
                        &Milestone::parse(&source, "test").unwrap(),
                        path,
                        workspace.root(),
                    )
                    .unwrap();
            } else {
                tasks::manage(
                    workspace.root(),
                    tasks::Manage {
                        action: tasks::ManageAction::Sync,
                        milestone_ref: Some(path.into()),
                        ..Default::default()
                    },
                )
                .unwrap();
            }
        }
        Self {
            id: "test/m-mcp-response-deduplication-and-agent-efficiency:normalize-and-resolve-milestone-references".into(),
            store: tasks::store(workspace.root()).unwrap(),
            root: workspace.root().to_owned(), _workspace: Some(workspace), in_memory: false,
        }
    }

    pub const ALIAS_PATH: &str =
        "docs/milestones/UX-001-mcp-response-deduplication-and-agent-efficiency.md";
    pub const ALIAS_TASK: &str = "normalize-and-resolve-milestone-references";
    pub const CONFIGURED_PATH: &str = "contracts/040-configured-milestone-with-a-long-name.md";
    pub const ARCHIVED_PATH: &str =
        "docs/milestones/archive/041-archived-milestone-with-a-long-name.md";

    pub fn list_for(&self, reference: &str, repository: Option<&str>) -> anyhow::Result<Value> {
        tasks::list(
            self.root(),
            tasks::List {
                repository: repository.map(str::to_owned),
                milestone_ref: Some(reference.into()),
                status: Some(tasks::Status::All),
                ..Default::default()
            },
        )
    }

    pub fn sync_reference(
        &self,
        reference: Option<&str>,
        workspace: Option<&str>,
    ) -> anyhow::Result<Value> {
        tasks::manage(
            self.root(),
            tasks::Manage {
                action: tasks::ManageAction::Sync,
                milestone_ref: reference.map(str::to_owned),
                workspace: workspace.map(str::to_owned),
                ..Default::default()
            },
        )
    }

    pub fn memory() -> Self {
        Self::memory_with_manifest(SPEC, "docs/010-test.md")
    }

    pub fn memory_with_manifest(source: &str, path: &str) -> Self {
        let root = unconfigured_test_root();
        let milestone = Milestone::parse(source, "forge-mcp").unwrap();
        let id = milestone.task_id(&milestone.tasks[0]);
        let mut store = TasksStore::open_in_memory("forge-mcp", "forge-mcp").unwrap();
        store.sync(&milestone, path, &root).unwrap();
        Self {
            store,
            id,
            root,
            _workspace: None,
            in_memory: true,
        }
    }

    pub fn memory_with_profile(source: &str, path: &str, profile: GateProfile) -> Self {
        let root = unconfigured_test_root();
        let milestone = Milestone::parse(source, "forge-mcp").unwrap();
        let id = milestone.task_id(&milestone.tasks[0]);
        let mut store =
            TasksStore::open_in_memory_with_profile("forge-mcp", "forge-mcp", Arc::new(profile))
                .unwrap();
        store.sync(&milestone, path, &root).unwrap();
        Self {
            store,
            id,
            root,
            _workspace: None,
            in_memory: true,
        }
    }

    pub fn disk_with_profile(source: &str, path: &str, profile: GateProfile) -> Self {
        let workspace = acdd_workspace(DEFAULT_SEAM, "");
        workspace.write("src/lib.rs", "pub fn example() {}\n");
        workspace.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        workspace.write(path, source);
        workspace.write(
            ".forge/acdd/profile.yaml",
            &serde_yaml::to_string(&profile).unwrap(),
        );
        Self::commit_handoff_fixture_inputs(
            &workspace,
            &[
                "src/lib.rs",
                "forge-mcp.yaml",
                path,
                ".forge/acdd/profile.yaml",
            ],
        );

        let milestone = Milestone::parse(source, "forge-mcp").unwrap();
        let id = milestone.task_id(&milestone.tasks[0]);
        let mut store = tasks::store_for_task(workspace.root(), &id).unwrap();
        store.sync(&milestone, path, workspace.root()).unwrap();
        Self {
            store,
            id,
            root: workspace.root().to_owned(),
            _workspace: Some(workspace),
            in_memory: false,
        }
    }

    pub fn disk() -> Self {
        let (workspace, store, milestone) = fixture(DEFAULT_SEAM);
        Self {
            id: milestone.task_id(&milestone.tasks[0]),
            root: workspace.root().to_owned(),
            store,
            _workspace: Some(workspace),
            in_memory: false,
        }
    }

}
