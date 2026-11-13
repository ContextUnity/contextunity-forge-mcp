#[test]
fn linked_milestone_context_reports_upstream_task_status_and_blockers() {
    let root = crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
    root.write("src/lib.rs", "pub fn example() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\nmilestones: [docs/milestones]\ntasks_db: .forge/tasks.sqlite\n",
    );

    let upstream = r#"---
id: m-upstream
title: Upstream
doc_type: contract
status: active
---
```yaml
task_ref: producer
target: Produce the linked task
proof_policy: seam-test-first
scope: [src/lib.rs]
```
"#;
    let consumer = r#"---
id: m-consumer
title: Consumer
doc_type: contract
status: active
linked_to: [m-upstream]
---
```yaml
task_ref: consumer
target: Consume the linked task
proof_policy: seam-test-first
scope: [src/lib.rs]
task_depends_on: [m-upstream:producer]
```
"#;
    root.write("docs/milestones/010-upstream.md", upstream);
    root.write("docs/milestones/020-consumer.md", consumer);

    for milestone_ref in [
        "docs/milestones/010-upstream.md",
        "docs/milestones/020-consumer.md",
    ] {
        contextunity_forge_mcp::engine::tasks::manage(
            root.root(),
            serde_json::from_value(serde_json::json!({
                "action": "sync",
                "milestone_ref": milestone_ref,
            }))
            .expect("sync action should deserialize"),
        )
        .expect("fixture milestone should sync");
    }

    let upstream_tasks = contextunity_forge_mcp::engine::tasks::list(
        root.root(),
        serde_json::from_value(serde_json::json!({
            "milestone_ref": "docs/milestones/010-upstream.md",
            "milestone_status": "all",
            "status": "all",
        }))
        .expect("list action should deserialize"),
    )
    .expect("upstream task should be listable");

    let context = contextunity_forge_mcp::engine::tasks::manage(
        root.root(),
        serde_json::from_value(serde_json::json!({
            "action": "context",
            "task_id": "forge-mcp/m-consumer:consumer",
        }))
        .expect("context action should deserialize"),
    )
    .expect("consumer context should build");

    assert_eq!(
        context["linked_milestones"][0]["milestone_id"],
        "m-upstream"
    );
    assert_eq!(
        context["linked_milestones"][0]["task_count"], 1,
        "upstream list: {upstream_tasks}; linked milestone context: {context}"
    );
    assert_eq!(
        context["linked_milestones"][0]["tasks"][0]["status"],
        "ready"
    );
    assert_eq!(
        context["linked_milestones"][0]["dependency_states"][0]["blocking"],
        true
    );

    let mut tool_profile: serde_yaml::Value = serde_yaml::from_str(include_str!(
        "../../../crates/forge-tasks/src/tasks/acdd.default.yaml"
    ))
    .expect("the embedded ACDD profile should deserialize for the context fixture");
    tool_profile["inherit"] = serde_yaml::Value::String("none".to_owned());
    tool_profile["gates"][1]["tasks"]["gates"][0]["tools"] = serde_yaml::Value::Sequence(vec![
        serde_yaml::Value::String("code_map_overview".to_owned()),
        serde_yaml::Value::String("get_doc".to_owned()),
        serde_yaml::Value::String("search_docs".to_owned()),
        serde_yaml::Value::String("code_map_inspect".to_owned()),
    ]);
    let tool_profile =
        serde_yaml::to_string(&tool_profile).expect("the tool context profile should serialize");
    root.write(".forge/acdd/tool-profile.yaml", &tool_profile);
    root.write(
        "forge-mcp.yaml",
        "profile: minimal\nacdd_profile: .forge/acdd/tool-profile.yaml\nroots: [src]\ndocs: [docs]\nmilestones: [docs/milestones]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let minimal_error = contextunity_forge_mcp::engine::tasks::manage(
        root.root(),
        serde_json::from_value(serde_json::json!({
            "action": "context",
            "task_id": "forge-mcp/m-consumer:consumer",
        }))
        .expect("context action should deserialize"),
    )
    .expect_err("minimal must reject a gate that requires disabled code navigation");
    assert!(
        format!("{minimal_error:#}").contains(
            "active gate 'contract' requires tool 'code_map_overview' from disabled Forge subsystem 'codemap'"
        ),
        "unexpected minimal-profile result: {minimal_error:#}"
    );

    for (preset, expected_tools) in [
        ("minimal", serde_json::json!(["forge_read"])),
        (
            "orchestrator",
            serde_json::json!(["forge_read", "task_blackboard"]),
        ),
    ] {
        root.write(
            "forge-mcp.yaml",
            &format!("profile: {preset}\nroots: [src]\ndocs: [docs]\nmilestones: [docs/milestones]\ntasks_db: .forge/tasks.sqlite\n"),
        );
        let context = contextunity_forge_mcp::engine::tasks::manage(
            root.root(),
            serde_json::from_value(serde_json::json!({
                "action": "context",
                "task_id": "forge-mcp/m-consumer:consumer",
            }))
            .expect("context action should deserialize"),
        )
        .expect("default gates must work with navigation disabled");
        assert!(context["context_bundle"]["adrs"].is_null());
        assert!(context["context_bundle"]["scope_symbols"].is_null());
        assert!(context["context_bundle"]["covering_tests"].is_null());
        assert!(context["context_bundle"]["external_callers"].is_null());
        assert_eq!(
            context["workflow_guidance"]["recommended_tools"],
            expected_tools
        );
        if preset == "minimal" {
            assert!(context["blackboard"].is_null());
            assert!(context["blackboard_info"].is_null());
            assert!(context["workflow_guidance"]["blackboard_messages"].is_null());
            let guidance = context["workflow_guidance"].to_string();
            assert!(
                !guidance.contains("task_blackboard"),
                "disabled blackboard guidance leaked: {guidance}"
            );
        }
    }

    root.write(
        "forge-mcp.yaml",
        "profile: minimal\nacdd_profile: .forge/acdd/tool-profile.yaml\nmodules:\n  codemap: true\n  docs: true\n  acdd: true\nroots: [src]\ndocs: [docs]\nmilestones: [docs/milestones]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let overridden_context = contextunity_forge_mcp::engine::tasks::manage(
        root.root(),
        serde_json::from_value(serde_json::json!({
            "action": "context",
            "task_id": "forge-mcp/m-consumer:consumer",
        }))
        .expect("context action should deserialize"),
    )
    .expect("explicit module overrides should satisfy the active gate");
    assert_eq!(
        overridden_context["workflow_guidance"]["recommended_tools"],
        serde_json::json!([
            "code_map_overview",
            "get_doc",
            "search_docs",
            "code_map_inspect"
        ])
    );
    assert!(overridden_context["blackboard"].is_null());
    assert!(overridden_context["workflow_guidance"]["blackboard_messages"].is_null());
    assert!(!overridden_context["workflow_guidance"]
        .to_string()
        .contains("task_blackboard"));
}

#[test]
fn task_manage_named_snapshot_restores_worktree_and_guards_head_changes() {
    let snapshot = crate::acdd::snapshot::NamedSnapshotHarness::new();
    let head = snapshot.head();
    let created = snapshot
        .manage("snapshot", "before-change")
        .expect("snapshot action should create a named shadow snapshot");
    assert_eq!(created["action"], "snapshot");
    assert_eq!(created["snapshot_name"], "before-change");
    assert_eq!(snapshot.head(), head);
    assert!(snapshot
        .manage("snapshot", "before-change")
        .unwrap_err()
        .to_string()
        .contains("TASK_SNAPSHOT_ALREADY_EXISTS"));
    assert!(snapshot
        .manage("snapshot", "../outside")
        .unwrap_err()
        .to_string()
        .contains("TASK_SELECTOR_INVALID"));

    snapshot.write("src/lib.rs", "experiment changes\n");
    snapshot.write("after-snapshot.txt", "remove on rollback\n");
    #[cfg(unix)]
    {
        let _lock = snapshot
            .lock_snapshot_operation()
            .expect("test should acquire snapshot-operation lock");
        let busy = snapshot.manage("rollback", "before-change").unwrap_err();
        assert!(busy.to_string().contains("TASK_SNAPSHOT_BUSY"));
        assert_eq!(
            snapshot.read("src/lib.rs"),
            "experiment changes\n",
            "lock contention must fail before touching the worktree"
        );
        assert!(snapshot.exists("after-snapshot.txt"));
    }
    let rollback = snapshot
        .manage("rollback", "before-change")
        .expect("rollback action should restore the named worktree snapshot");
    assert_eq!(rollback["action"], "rollback");
    assert_eq!(rollback["restored"], true);
    assert_eq!(snapshot.read("src/lib.rs"), "snapshot baseline\n");
    assert!(!snapshot.exists("after-snapshot.txt"));
    assert_eq!(snapshot.head(), head);
    assert_eq!(
        snapshot.status_porcelain().trim(),
        "?? runtime/",
        "rollback should remove later experiment files and preserve the task database"
    );
    assert!(snapshot.exists("runtime/tasks.sqlite"));
    assert_eq!(
        snapshot.read(".forge/managed-state"),
        "preserve Forge state\n"
    );

    snapshot
        .manage("snapshot", "head-guard")
        .expect("second named snapshot should succeed");
    snapshot.write("src/lib.rs", "committed after snapshot\n");
    snapshot.commit_file("src/lib.rs", "advance branch after snapshot");
    let advanced_head = snapshot.head();
    snapshot.write("src/lib.rs", "uncommitted after branch advance\n");
    assert!(snapshot
        .manage("rollback", "head-guard")
        .unwrap_err()
        .to_string()
        .contains("TASK_SNAPSHOT_HEAD_CHANGED"));
    assert_eq!(snapshot.head(), advanced_head);
    assert_eq!(
        snapshot.read("src/lib.rs"),
        "uncommitted after branch advance\n"
    );
    #[cfg(target_os = "linux")]
    case_named_snapshot_releases_its_lock_with_an_inherited_descriptor_open(&snapshot);
}

#[cfg(target_os = "linux")]
fn case_named_snapshot_releases_its_lock_with_an_inherited_descriptor_open(
    snapshot: &crate::acdd::snapshot::NamedSnapshotHarness,
) {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::time::{Duration, Instant};

    let lock_path = snapshot.snapshot_lock_path();
    let inherited = std::thread::scope(|scope| {
        let operation = scope.spawn(|| snapshot.manage("snapshot", "inherited"));
        let started = Instant::now();
        let descriptor = 'capture: loop {
            for entry in std::fs::read_dir("/proc/self/fd").unwrap().flatten() {
                if std::fs::read_link(entry.path()).ok().as_deref() != Some(lock_path.as_path()) {
                    continue;
                }
                let Some(number) = entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.parse().ok())
                else {
                    continue;
                };
                // SAFETY: F_DUPFD_CLOEXEC creates an owned descriptor or returns an error.
                let duplicate = unsafe { libc::fcntl(number, libc::F_DUPFD_CLOEXEC, 0) };
                if duplicate < 0 {
                    continue;
                }
                // SAFETY: a successful fcntl returns a newly owned descriptor.
                let file = unsafe { std::fs::File::from_raw_fd(duplicate) };
                let link = format!("/proc/self/fd/{}", file.as_raw_fd());
                if std::fs::read_link(link).ok().as_deref() == Some(lock_path.as_path()) {
                    break 'capture file;
                }
            }
            assert!(
                !operation.is_finished() && started.elapsed() < Duration::from_secs(5),
                "snapshot finished before its lock descriptor could be duplicated"
            );
            std::thread::yield_now();
        };
        operation
            .join()
            .unwrap()
            .expect("named snapshot should succeed");
        descriptor
    });

    let lock = snapshot
        .lock_snapshot_operation()
        .expect("completed snapshot must release its lock even with an inherited descriptor");
    drop(lock);
    drop(inherited);
}
