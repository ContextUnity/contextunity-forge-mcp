use super::*;

#[test]
fn subproject_registry_resolves_declared_projects_and_ignores_phantoms() {
    let declared_catalog = include_str!("../../fixtures/milestones/declared_projects.json");
    let root = MilestoneHarness::from_catalog(declared_catalog);
    let workspace_names = root.workspace_names().unwrap();
    for name in ["test", "base"] {
        assert!(
            workspace_names.contains(&name.to_string()),
            "expected workspace {name:?}, got: {workspace_names:?}"
        );
    }
    for name in ["extensions.base", "ext"] {
        assert!(
            !workspace_names.contains(&name.to_string()),
            "phantom workspace {name:?} must not be registered: {workspace_names:?}"
        );
    }
    let sync_root = root.sync("M-ROOT").unwrap();
    assert_eq!(sync_root["tasks"].as_array().unwrap().len(), 1);
    let sync_subproject = root.sync("BS-102").unwrap();
    assert_eq!(sync_subproject["tasks"].as_array().unwrap().len(), 1);
    let list_base = root.tasks_in("base", "BS-102");
    assert_eq!(list_base["tasks"].as_array().unwrap().len(), 1);
}

const OWNER_ALIAS_TASKS: [(&str, &str, &str, &str); 3] = [
    (
        "BS-103",
        "owners-task",
        "BS-103-owners-milestone.md",
        "base",
    ),
    (
        "BS-104",
        "alias-task",
        "BS-104-aliased-milestone.md",
        "base",
    ),
    ("BS-106", "other-task", "BS-106-other-project.md", "other"),
];

#[test]
fn milestone_owner_and_depends_on_aliases_drive_sync_show_and_claim_gating() {
    let root = MilestoneHarness::from_catalog(OWNER_ALIAS_CATALOG);
    {
        let workspace_names = root.workspace_names().unwrap();
        for name in ["test", "base", "other"] {
            assert!(
                workspace_names.contains(&name.to_string()),
                "expected workspace {name:?}, got: {workspace_names:?}"
            );
        }
        for name in [
            "extensions.base",
            "legacy",
            "extensions.readme-only",
            "extensions.empty",
        ] {
            assert!(
                !workspace_names.contains(&name.to_string()),
                "phantom workspace {name:?} must not be registered: {workspace_names:?}"
            );
        }

        // Sync BS-103 (owners without explicit project frontmatter) must succeed without TASK_PROJECT_MISMATCH
        let sync_owners = root.sync("BS-103").unwrap();
        assert_eq!(sync_owners["tasks"].as_array().unwrap().len(), 1);

        // Sync BS-104 (project: legacy aliased to base) must succeed without TASK_PROJECT_MISMATCH
        let sync_alias = root.sync_in("BS-104", Some("all")).unwrap();
        assert_eq!(sync_alias["tasks"].as_array().unwrap().len(), 1);
        let sync_other = root.sync_in("BS-106", Some("all")).unwrap();
        assert_eq!(sync_other["tasks"][0]["task_id"], "other/BS-106:other-task");

        root.write("extensions/base/src/lib.rs", "pub fn extension() {}\n");
        root.commit_fixture_inputs(&["AGENTS.md", "forge-mcp.yaml", "docs", "extensions"]);
        for (index, (milestone_id, task_ref, file, project)) in
            OWNER_ALIAS_TASKS.into_iter().enumerate()
        {
            let listed = root.tasks_in(project, milestone_id);
            let task_id = format!("{project}/{milestone_id}:{task_ref}");
            assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
            assert_eq!(listed["tasks"][0]["task_id"], task_id);
            assert_eq!(listed["tasks"][0]["status"], "ready");
            let store = root.store_in("test", project);
            if index == 0 {
                root.complete_task(CompletionMode::Store {
                    store: &store,
                    task_id: &task_id,
                })
                .unwrap();
            } else {
                drop(store);
                root.complete_task(CompletionMode::Project {
                    repository: "test",
                    project,
                    task_id: &task_id,
                })
                .unwrap();
            }
            let delivered = root.store_in("test", project).inspect(&task_id).unwrap();
            assert_eq!(delivered.status, "completed");
            assert_eq!(delivered.receipt.unwrap().decision, "pass");
            let text = root.document(&format!("extensions/base/docs/milestones/{file}"));
            let parsed = Milestone::parse(&text, "test").unwrap();
            assert_eq!(parsed.tasks[0].status.as_deref(), Some("completed"));
            assert!(parsed.tasks[0].receipt.is_some());
            let shown = root.show_result(milestone_id, false).unwrap();
            assert_eq!(shown["completion"], "1/1");
            let evidence =
                root.default_fixture_evidence(milestone_id, "cargo test --test acdd", 1, 0);
            MilestoneHarness::install_pruning_fault(&root.store(), PruningFault::Capture);
            let result = root.handoff_with_evidence(milestone_id, &evidence);
            let interrupted = result.unwrap_err();
            assert!(format!("{interrupted:#}").contains("capture handoff journal"));
            let journals = root.handoff_journals();
            assert_eq!(journals.len(), 1);
            let journal_path = &journals[0];
            let journal = MilestoneHarness::read_journal(journal_path);
            MilestoneHarness::remove_pruning_fault(&root.store(), PruningFault::Capture);
            root.handoff_with_evidence(milestone_id, &evidence).unwrap();
            assert!(!journal_path.exists(), "recovered journal removed");
            let archive_path = format!("extensions/base/docs/milestones/archive/{file}");
            assert!(root.path(&archive_path).is_file());
            let accepted_archive = root.document(&archive_path);
            let header = MilestoneHarness::frontmatter(&accepted_archive);
            let passport = &header["handoff"]["passport"];
            assert!(passport.get("terminal_tasks").is_none());
            root.write(format!("extensions/base/docs/milestones/{file}"), &text);
            root.write(&archive_path, "---\nid: [torn\n");
            MilestoneHarness::restore_journal(journal_path, &journal);
            for _ in 0..2 {
                root.handoff_with_evidence(milestone_id, &evidence)
                    .unwrap_or_else(|error| panic!("{milestone_id}: {error:#}"));
            }
            let repaired = root.document(&archive_path);
            assert_eq!(
                Milestone::parse(&repaired, "test").unwrap().tasks[0].receipt,
                Milestone::parse(&accepted_archive, "test").unwrap().tasks[0].receipt,
            );
            let shown = root.show_result(milestone_id, false).unwrap();
            assert_eq!(shown["status"], "completed");
            let archived = Milestone::parse(&repaired, "base").unwrap();
            assert!(archived
                .tasks
                .iter()
                .all(|task| task.status.as_deref() == Some("completed")));
            root.commit_fixture_inputs(&["extensions/base/docs/milestones"]);
        }
    }

    {
        root.init(milestone_engine::Init {
            num: Some("105".into()),
            slug: Some("owned-init".into()),
            title: Some("Owned init".into()),
            dir: Some("extensions/base/docs/milestones".into()),
            ..milestone_init_options()
        })
        .unwrap();
        let text = root.document("extensions/base/docs/milestones/105-owned-init.md");
        assert_eq!(Milestone::parse(&text, "test").unwrap().project, "base");
    }

    {
        let left = MilestoneHarness::workspace(DEFAULT_SEAM, "");
        let right = MilestoneHarness::workspace(DEFAULT_SEAM, "");
        for (linked, project) in [(&left, "left"), (&right, "right")] {
            linked.write(
                "forge-mcp.yaml",
                &format!("task_repository: linked\ntask_project: {project}\n"),
            );
            linked.write(
                "docs/milestones/099-shared.md",
                &MilestoneFixtureBuilder::new("m-shared")
                    .title("Shared reference")
                    .without_status()
                    .task("shared-task", &["src/"])
                    .task_target("Shared target")
                    .task_proof_policy("direct-proof")
                    .build(),
            );
        }
        let config = root.document("forge-mcp.yaml");
        root.write(
        "forge-mcp.yaml",
        &format!(
            "{config}linked_workspaces:\n  - name: left\n    path: {}\n    tasks: {{enabled: true}}\n  - name: right\n    path: {}\n    tasks: {{enabled: true}}\n",
            left.root().display(),
            right.root().display()
        ),
    );
        for workspace in [None, Some("all")] {
            let error = root
                .list_tasks_request(tasks::List {
                    milestone_ref: Some("099".into()),
                    repository: workspace.map(str::to_owned),
                    ..Default::default()
                })
                .unwrap_err()
                .to_string();
            assert!(error.contains("ambiguous milestone reference"), "{error}");
            assert!(error.contains("left") && error.contains("right"), "{error}");
        }
    }
}

const DEFAULT_ALIAS_CATALOG: &str = include_str!("../../fixtures/milestones/default_aliases.json");

#[test]
fn milestone_frontmatter_defaults_task_profile_and_accepts_tasks_alias() {
    let root = MilestoneHarness::from_catalog(DEFAULT_ALIAS_CATALOG);
    {
        root.commit_fixture_inputs(&["AGENTS.md", "forge-mcp.yaml", "docs", "src"]);
        let synced = root.sync("001").unwrap();
        let task_id = "base/m-default:first";
        assert_eq!(synced["tasks"][0]["task_id"], task_id);
        let listed = root.list_tasks_request(tasks::List::default()).unwrap();
        assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(listed["tasks"][0]["task_id"], task_id);
        assert_eq!(root.workspace_names().unwrap(), vec!["test"]);
        let store = root.store();
        assert_eq!(store.inspect(task_id).unwrap().task_id, task_id);
        drop(store);
        root.complete_task(CompletionMode::Project {
            repository: "test",
            project: "base",
            task_id,
        })
        .unwrap();
        let store = root.store();
        root.complete_task(CompletionMode::Store {
            store: &store,
            task_id,
        })
        .unwrap();
        assert_eq!(store.inspect(task_id).unwrap().status, "completed");
        assert_eq!(root.show_result("001", false).unwrap()["completion"], "1/1");
        drop(store);

        let evidence = root.default_fixture_evidence("001", "cargo test --test acdd", 1, 0);
        let active_text = root.document("docs/milestones/001-default.md");
        MilestoneHarness::install_pruning_fault(&root.store(), PruningFault::Capture);
        let interrupted = root.handoff_with_evidence("001", &evidence).unwrap_err();
        assert!(format!("{interrupted:#}").contains("capture handoff journal"));
        let journals = root.handoff_journals();
        assert_eq!(
            journals.len(),
            1,
            "expected one interrupted handoff journal"
        );
        let journal_path = &journals[0];
        let journal = MilestoneHarness::read_journal(journal_path);
        MilestoneHarness::remove_pruning_fault(&root.store(), PruningFault::Capture);
        root.handoff_with_evidence("001", &evidence).unwrap();
        assert!(
            !journal_path.exists(),
            "successful recovery removes the journal"
        );
        assert_eq!(
            root.archived_project("docs/milestones/archive/001-default.md", "test", "base"),
            "base"
        );
        root.write("docs/milestones/001-default.md", &active_text);
        root.write("docs/milestones/archive/001-default.md", "---\nid: [torn\n");
        MilestoneHarness::restore_journal(journal_path, &journal);
        for _ in 0..2 {
            root.handoff_with_evidence("001", &evidence).unwrap();
        }
        assert_eq!(
            root.archived_project("docs/milestones/archive/001-default.md", "test", "base"),
            "base"
        );

        root.init(milestone_engine::Init {
            num: Some("002".into()),
            slug: Some("aliased-init".into()),
            title: Some("Aliased init".into()),
            active: true,
            dir: Some("extensions/new/docs/milestones".into()),
            ..milestone_init_options()
        })
        .unwrap();
        let synced = root.sync("002").unwrap();
        let initialized_id = "base/m-aliased-init:aliased-init";
        assert_eq!(synced["tasks"][0]["task_id"], initialized_id);
        let next = root
            .manage(tasks::Manage {
                action: tasks::ManageAction::Next,
                worker_id: Some("builder".into()),
                worktree: Some(root.root().to_string_lossy().into_owned()),
                session_fingerprint: Some(TaskHarness::session_for("builder")),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(next["task_id"], initialized_id);
    }
}

type TopologyCase = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);
const PASSPORT_TOPOLOGY_CASES: [TopologyCase; 6] = [
    ("default", "base", "{}", "docs/milestones", "", "", "base"),
    (
        "single-hop",
        "old",
        "{old: base}",
        "docs/milestones",
        "",
        "",
        "base",
    ),
    (
        "default-chain",
        "old",
        "{old: base, base: other}",
        "docs/milestones",
        "",
        "",
        "base",
    ),
    (
        "configured-chain",
        "fallback",
        "{old: base, base: other}",
        "extensions/new/docs/milestones",
        "",
        "",
        "base",
    ),
    (
        "explicit-old",
        "old",
        "{old: base, base: other}",
        "docs/milestones",
        "",
        "old/",
        "base",
    ),
    (
        "explicit-base",
        "old",
        "{old: base, base: other}",
        "docs/milestones",
        "project: base\n",
        "base/",
        "other",
    ),
];

#[test]
fn milestone_passport_topology_records_resolved_and_unresolved_dependencies() {
    let topology = MilestoneHarness::workspace(DEFAULT_SEAM, "");
    drop(topology.store());
    let snapshot = topology.checkpoint();
    let mut topology_failures = Vec::new();
    for (case_index, &(case, default, aliases, directory, declaration, qualifier, project)) in
        PASSPORT_TOPOLOGY_CASES.iter().enumerate()
    {
        if case_index > 0 {
            snapshot.restore(&topology);
        }
        topology.write(".gitignore", ".forge/\n");
        topology.write("src/lib.rs", "pub fn topology() {}\n");
        let mut profile = fixture_profile();
        profile.commands.insert(
            "check_full_repo".into(),
            serde_yaml::from_str("command: 'true'\noutput: {require_counts: false}\n").unwrap(),
        );
        topology.write(
            "profile.yaml",
            &crate::acdd::support::standalone_profile_yaml(&profile),
        );
        topology.write(
            "forge-mcp.yaml",
            &format!(
                "roots: [src]\ndocs: [docs]\nacdd_profile: profile.yaml\ntasks_db: .forge/tasks.sqlite\ntask_repository: topology-repo\ntask_project: {default}\nmilestones: [docs/milestones, 'extensions/*/docs/milestones']\nowners:\n  extensions/new: old\naliases: {aliases}\n"
            ),
        );
        let manifest = |id: &str, state: &str, dependencies: &[String], task_status: &str| {
            let dependencies: Vec<_> = dependencies.iter().map(String::as_str).collect();
            let mut fixture = MilestoneFixtureBuilder::new(id)
                .title("Passport topology")
                .without_doc_type()
                .repository("topology-repo")
                .status(state)
                .started_at("2026-10-01T00:00:00Z")
                .depends_on(&dependencies)
                .task("work", &["src/lib.rs"])
                .task_target("Preserve topology")
                .task_status(task_status);
            if !declaration.is_empty() {
                fixture = fixture.project(declaration.strip_prefix("project: ").unwrap().trim());
            }
            let source = fixture.build();
            if state == "completed" {
                let completed = source.replacen(
                    "\n---\n",
                    "\nhandoff:\n  completed_at: 2026-10-01T00:00:00Z\n  commit: 0123456789012345678901234567890123456789\n---\n",
                    1,
                );
                assert_ne!(completed, source);
                completed
            } else {
                source
            }
        };
        topology.write(
            format!("{directory}/archive/001-up.md"),
            &manifest("m-up", "completed", &[], "cancelled"),
        );
        let current = format!("{directory}/002-current.md");
        topology.write(
            &current,
            &manifest(
                "m-current",
                "active",
                &[format!("{qualifier}m-up"), format!("{qualifier}m-missing")],
                "cancelled",
            ),
        );
        topology.write(
            format!("{directory}/003-down.md"),
            &manifest(
                "m-down",
                "active",
                &[format!("{qualifier}m-current")],
                "ready",
            ),
        );
        topology.commit_fixture_inputs(&[
            ".gitignore",
            "forge-mcp.yaml",
            "profile.yaml",
            "src",
            directory,
        ]);
        topology
            .sync(&current)
            .unwrap_or_else(|error| panic!("{case}: {error:#}"));
        let context = topology
            .handoff_context("m-current", "HEAD")
            .unwrap_or_else(|error| panic!("{case}: {error:#}"));
        assert_eq!(context["binding"]["project"], project, "{case}");
        let evidence = topology.default_fixture_evidence("m-current", "true", 0, 0);
        let handed_off = topology
            .handoff_at("m-current", None, &evidence)
            .unwrap_or_else(|error| panic!("{case}: {error:#}"));
        let expected = json!({"upstream":[format!("topology-repo/{project}/m-missing"),format!("topology-repo/{project}/m-up")],"downstream":[format!("topology-repo/{project}/m-down")],"unresolved_upstream":[format!("topology-repo/{project}/m-missing")]});
        let archive = topology.document(&format!("{directory}/archive/002-current.md"));
        let header = MilestoneHarness::frontmatter(&archive);
        let (note_output, note) = topology.git_note(context["commit"].as_str().unwrap());
        assert!(note_output.status.success(), "{case}: {note_output:?}");
        assert!(
            note_output.stdout.len() < 2048,
            "{case}: compact note budget"
        );
        let identity = format!("topology-repo/{project}/m-current");
        assert_eq!(
            note["milestone_passports"][&identity]["binding"],
            context["binding"]
        );
        assert!(note["milestone_passports"][&identity]
            .get("topology")
            .is_none());
        for (location, observed) in [
            (
                "response",
                handed_off["handoff"]["passport"]["topology"].clone(),
            ),
            (
                "archive",
                serde_json::to_value(&header["handoff"]["passport"]["topology"]).unwrap(),
            ),
        ] {
            if observed != expected {
                topology_failures.push(format!(
                    "{case}/{location}: {observed}, expected {expected}"
                ));
            }
        }
    }
    assert!(
        topology_failures.is_empty(),
        "passport topology matrix: {topology_failures:?}"
    );
}
