use super::*;

#[test]
fn task_selectors_resolve_and_reject_references_with_one_fixture() {
    let h = TaskHarness::selectors();
    {
        for reference in [
            "m-tool-performance-and-storage-compaction",
            "030",
            "030-tool-performance-and-storage-compaction",
            "docs/milestones/030-tool-performance-and-storage-compaction.md",
        ] {
            let synced = h.sync_reference(Some(reference), None).unwrap();
            assert_eq!(synced["tasks"].as_array().unwrap().len(), 1, "{reference}");
        }
        let listed = h
            .list_for("m-tool-performance-and-storage-compaction", Some("test"))
            .unwrap();
        assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(h.list_for("30", None).unwrap()["tasks"], listed["tasks"]);
        assert_eq!(
            h.sync_reference(None, Some("test")).unwrap()["tasks"]
                .as_array()
                .unwrap()
                .len(),
            3
        );

        for alias in [
            "UX-001",
            "UX-001.md",
            "docs/milestones/UX-001",
            "docs/milestones/UX-001.md",
            "milestones/UX-001.md",
        ] {
            for reference in [alias.to_owned(), format!("/{alias}")] {
                let listed = h.list_for(&reference, None).unwrap();
                assert_eq!(listed["tasks"].as_array().unwrap().len(), 1, "{reference}");
                assert_eq!(listed["tasks"][0]["task_id"], h.id, "{reference}");
                assert!(listed.get("resolution").is_none(), "{reference}");
            }
        }

        for reference in [TaskHarness::ALIAS_TASK, &h.id] {
            let listed = h.list_for(reference, Some("test")).unwrap();
            assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
            assert_eq!(listed["tasks"][0]["task_id"], h.id);
            assert_eq!(listed["resolution"]["kind"], "task_parent");
            assert_eq!(listed["resolution"]["input"], reference);
            assert_eq!(listed["resolution"]["task_id"], h.id);
            assert_eq!(
                listed["resolution"]["milestone_id"],
                "m-mcp-response-deduplication-and-agent-efficiency"
            );
            assert_eq!(
                listed["resolution"]["milestone_ref"],
                TaskHarness::ALIAS_PATH
            );
        }

        for (references, id) in [
            (
                [
                    "m-configured-milestone",
                    "040",
                    TaskHarness::CONFIGURED_PATH,
                ],
                "test/m-configured-milestone:configured-task",
            ),
            (
                ["m-archived-milestone", "041", TaskHarness::ARCHIVED_PATH],
                "test/m-archived-milestone:archived-task",
            ),
        ] {
            for reference in references {
                let listed = h.list_for(reference, None).unwrap();
                assert_eq!(listed["tasks"].as_array().unwrap().len(), 1, "{reference}");
                assert_eq!(listed["tasks"][0]["task_id"], id, "{reference}");
            }
        }
        let error = h
            .sync_reference(Some(TaskHarness::ARCHIVED_PATH), None)
            .unwrap_err();
        assert!(format!("{error:#}").contains("STORE_SYNC_FAILED"));
    }

    {
        let slug = TaskHarness::ALIAS_TASK;
        assert!(h
            .sync_reference(Some(slug), None)
            .unwrap_err()
            .to_string()
            .contains("not found"));
        let claim = TaskHarness::claim_at(h.root(), slug, "contract", "slug-is-not-a-task-id");
        assert!(claim.is_err());
        let before = h.store.list(None, "all", None).unwrap();
        assert!(tasks::manage(
            h.root(),
            tasks::Manage {
                action: tasks::ManageAction::Delete,
                milestone_ref: Some(slug.into()),
                ..Default::default()
            }
        )
        .is_err());
        assert_eq!(
            serde_json::to_value(h.store.list(None, "all", None).unwrap()).unwrap(),
            serde_json::to_value(before).unwrap()
        );
        for action in [
            tasks::ManageAction::Inspect,
            tasks::ManageAction::Delete,
            tasks::ManageAction::Reset,
            tasks::ManageAction::Reopen,
            tasks::ManageAction::SubtaskList,
        ] {
            assert!(
                tasks::manage(
                    h.root(),
                    tasks::Manage {
                        action,
                        task_id: Some(slug.into()),
                        ..Default::default()
                    }
                )
                .is_err(),
                "{action:?}"
            );
        }

        for reference in ["../../outside/UX-001.md", "/etc/UX-001.md"] {
            assert!(h.list_for(reference, None).is_err(), "{reference}");
        }
        let path = "docs/milestones/UX-001-foreign.md";
        let foreign = TaskHarness::workspace();
        foreign.write(path, &MilestoneFixtureBuilder::new("m-foreign").build());
        assert!(h
            .list_for(&foreign.path(path).to_string_lossy(), None)
            .is_err());
        for path in ["contracts/UX-001.md", "docs/milestones/UX-001.md"] {
            std::fs::write(h.root().join(path), "---\nid: [invalid\n---\n").unwrap();
            let error = h.list_for(path, None).unwrap_err().to_string();
            assert!(
                error.contains("invalid") || error.contains("parse"),
                "{error}"
            );
        }
        let error = h.sync_reference(Some("m-non-existent"), None).unwrap_err();
        assert!(
            error.to_string().contains("not found in workspace"),
            "{error}"
        );
    }
}

#[test]
fn task_list_alias_collisions_report_both_configured_candidates() {
    let workspace = TaskHarness::workspace();
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\nmilestones: [contracts, proposals]\n",
    );
    for (path, id) in [
        ("contracts/UX-001-primary.md", "m-primary-alias"),
        ("proposals/UX-001-secondary.md", "m-secondary-alias"),
    ] {
        workspace.write(path, &MilestoneFixtureBuilder::new(id).build());
    }
    let error = tasks::list(
        workspace.root(),
        tasks::List {
            milestone_ref: Some("UX-001".into()),
            status: Some(tasks::Status::All),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("ambiguous")
            && error.contains("contracts/UX-001-primary.md")
            && error.contains("proposals/UX-001-secondary.md"),
        "{error}"
    );
}

#[test]
fn linked_task_selectors_share_the_primary_workspace() {
    let (h, _linked) = TaskHarness::linked_selectors(false);
    {
        let primary = h.list_for("shared-task", Some("primary")).unwrap();
        assert_eq!(primary["tasks"].as_array().unwrap().len(), 2);
        assert!(primary["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["task_id"] == h.id));
        assert_eq!(primary["resolution"]["kind"], "task_parent");
        assert_eq!(primary["resolution"]["task_id"], h.id);
        let linked_id = "linked/m-linked:shared-task";
        let linked = h.list_for(linked_id, Some("linked")).unwrap();
        assert_eq!(linked["tasks"].as_array().unwrap().len(), 2);
        assert!(linked["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["task_id"] == linked_id));
        assert_eq!(linked["resolution"]["input"], linked_id);
    }

    {
        let listed = h.list_for("m-direct-match", Some("all")).unwrap();
        assert_eq!(listed["tasks"].as_array().unwrap().len(), 2);
        let ids = listed["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|task| task["task_id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(ids.contains(&"primary/m-direct-match:primary-only"));
        assert!(ids.contains(&h.id.as_str()));
        assert!(!ids.contains(&"linked/m-linked:m-direct-match"));
        assert!(listed.get("resolution").is_none());
        let error = h
            .list_for("missing-task", Some("all"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not found"), "{error}");
    }
}

#[test]
fn task_parent_slug_collisions_report_all_selected_workspace_candidates() {
    let (h, _linked) = TaskHarness::linked_selectors(true);
    let second = "primary/m-primary-second:shared-task";
    for repository in ["primary", "all"] {
        let error = h
            .list_for("shared-task", Some(repository))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("ambiguous") && error.contains(&h.id) && error.contains(second),
            "{error}"
        );
        if repository == "all" {
            assert!(error.contains("linked/m-linked:shared-task"), "{error}");
        }
    }
}

#[test]
fn task_lists_share_filters_pagination_and_selection_fixture() {
    let mut h = TaskHarness::listings();
    {
        assert!(h.store.inspect(&h.id).is_ok());
        let other =
            TasksStore::open_project(&h.root().join(".forge/tasks.sqlite"), "other", "project")
                .unwrap();
        assert!(other.list(None, "all", None).unwrap().is_empty());
        assert!(other.inspect(&h.id).is_err());
    }

    {
        let active = h
            .list(tasks::List {
                status: Some(tasks::Status::All),
                ..Default::default()
            })
            .unwrap();
        let task = &active["tasks"][0];
        assert_eq!(active["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(task["task_id"], h.id);
        let ready = h.list(tasks::List::default()).unwrap();
        assert_eq!(ready["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(ready["tasks"][0]["status"], "ready");
        let subtasks = task["subtasks"].as_array().unwrap();
        assert_eq!(subtasks.len(), 1);
        assert_eq!(subtasks[0]["subtask_ref"], "sub-1");
        assert_eq!(subtasks[0]["status"], "in_progress");
        assert!(subtasks[0].get("title").is_none());
        assert!(subtasks[0].get("evidence").is_none());
        let full = h
            .list(tasks::List {
                status: Some(tasks::Status::All),
                detail: Some(tasks::TaskListDetail::Full),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(full["tasks"][0]["subtasks"][0]["title"], "Active subtask");
        assert_eq!(
            full["tasks"][0]["subtasks"][0]["evidence"],
            "Secret heavy evidence blob"
        );
        for (filter, expected_id) in [
            (
                tasks::MilestoneStatusFilter::Active,
                "forge-mcp/m-active:active-task",
            ),
            (
                tasks::MilestoneStatusFilter::Planned,
                "forge-mcp/m-planned:planned-task",
            ),
            (
                tasks::MilestoneStatusFilter::Completed,
                "forge-mcp/m-archived:archived-task",
            ),
        ] {
            let listed = h
                .list(tasks::List {
                    milestone_status: Some(filter),
                    status: Some(tasks::Status::All),
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
            assert_eq!(listed["tasks"][0]["task_id"], expected_id);
        }
        let all = h
            .list(tasks::List {
                milestone_status: Some(tasks::MilestoneStatusFilter::All),
                status: Some(tasks::Status::All),
                ..Default::default()
            })
            .unwrap();
        let ids = all["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|task| task["task_id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 4);
        assert!(ids.contains(&"forge-mcp/m-cancelled:cancelled-task"));
        for (reference, id) in [
            ("009", "forge-mcp/m-archived:archived-task"),
            ("007", "forge-mcp/m-cancelled:cancelled-task"),
        ] {
            let listed = h.list_for(reference, None).unwrap();
            assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
            assert_eq!(listed["tasks"][0]["task_id"], id);
        }
    }

    {
        let output = TaskHarness::run_cli_for_root(
            h.root(),
            &[
                "--format", "json", "task", "list", "--status", "all", "--limit", "1", "--offset",
                "0",
            ],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let page: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(page["pagination"]["total"], 1);
        assert_eq!(page["pagination"]["limit"], 1);
        assert_eq!(page["pagination"]["offset"], 0);
        assert_eq!(page["pagination"]["returned"], 1);
        assert_eq!(page["pagination"]["has_more"], false);
        assert!(!page["pagination"]["generation"]
            .as_str()
            .unwrap()
            .is_empty());
    }

    {
        let active_path = h.root().join("docs/milestones/010-active.md");
        let active_manifest = std::fs::read_to_string(&active_path).unwrap();
        std::fs::write(
            &active_path,
            MilestoneFixtureBuilder::new("m-active")
                .status("completed")
                .task("active-task", &["src/"])
                .build(),
        )
        .unwrap();
        let listed = h.list(tasks::List::default()).unwrap();
        assert!(listed["tasks"].as_array().unwrap().is_empty());

        std::fs::write(&active_path, active_manifest).unwrap();
        let source = MilestoneFixtureBuilder::new("m-second-active")
            .task("second-active-task", &["src/"])
            .build();
        let path = "docs/milestones/011-second-active.md";
        std::fs::write(h.root().join(path), &source).unwrap();
        let root = h.root().to_owned();
        h.store
            .sync(
                &Milestone::parse(&source, "forge-mcp").unwrap(),
                path,
                &root,
            )
            .unwrap();
        let error = h
            .list(tasks::List {
                status: Some(tasks::Status::All),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("010-active"), "{error}");
        assert!(error.contains("011-second-active"), "{error}");
    }
}
