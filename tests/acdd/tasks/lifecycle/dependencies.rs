use super::*;

const CROSS_MILESTONE_CONSUMER: &str = "---\nid: m-second\ntitle: Second\ndoc_type: contract\nstatus: planned\ndepends_on: [m-first]\ninvariants: [isolated]\n---\n# Second\n```yaml\ntask_ref: consumer\ntarget: Deliver consumer\nproof_policy: seam-test-first\nscope: [src/]\ndepends_on: [\"m-first:producer\"]\n```\n";

#[test]
fn cross_milestone_dependency_diagnostic_gates_readiness_and_claim() {
    let m1_text = "---\nid: m-first\ntitle: First\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\ninvariants: [isolated]\n---\n# First\n```yaml\ntask_ref: producer\ntarget: Deliver producer\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let mut h = TaskHarness::memory_with_manifest(m1_text, "docs/milestones/m1.md");
    let root = h.root().to_owned();
    let consumer = Milestone::parse(CROSS_MILESTONE_CONSUMER, "forge-mcp").unwrap();
    h.store
        .sync(&consumer, "docs/milestones/m2.md", &root)
        .unwrap();
    let producer_id = h.id.clone();
    let consumer_id = consumer.task_id(&consumer.tasks[0]);
    let listed = h.store.list(None, "all", None).unwrap();
    assert_eq!(
        listed
            .iter()
            .find(|task| task.task_id == producer_id)
            .unwrap()
            .status,
        "ready"
    );
    assert_eq!(
        listed
            .iter()
            .find(|task| task.task_id == consumer_id)
            .unwrap()
            .status,
        "blocked"
    );
    // Persist computed statuses to prove status validation precedes dependency validation.
    h.set_persisted_task_status(&consumer_id, "blocked");
    let worktree = root.to_string_lossy().into_owned();
    let status_message = h
        .store
        .claim(
            &consumer_id,
            "contract",
            "worker-1",
            &worktree,
            Some(&TaskHarness::session_for("worker-1")),
        )
        .unwrap_err()
        .to_string();
    h.set_persisted_task_status(&consumer_id, "ready");
    let dependency_message = h
        .store
        .claim(
            &consumer_id,
            "contract",
            "worker-1",
            &worktree,
            Some(&TaskHarness::session_for("worker-1")),
        )
        .unwrap_err()
        .to_string();
    assert_eq!(status_message, format!(
        "TASK_NOT_READY: task '{consumer_id}'. status is 'blocked' (active stage 'contract'). Do: claim only ready tasks at their active stage."
    ));
    assert_eq!(dependency_message, format!(
        "TASK_NOT_READY: task '{consumer_id}' has unsatisfied dependencies: [{producer_id}]. Do: complete dependencies before claiming stage 'contract'."
    ));
    h.set_persisted_status("completed");
    let listed = h.store.list(None, "all", None).unwrap();
    assert_eq!(
        listed
            .iter()
            .find(|task| task.task_id == consumer_id)
            .unwrap()
            .status,
        "ready"
    );
    let claimed = h
        .store
        .claim(
            &consumer_id,
            "contract",
            "worker-1",
            &worktree,
            Some(&TaskHarness::session_for("worker-1")),
        )
        .unwrap();
    assert_eq!(claimed.task_id, consumer_id);
    assert_eq!(claimed.status, "in_progress");
}

fn check_archived_predecessor_authority(root_mode: &str) {
    let m2_text = CROSS_MILESTONE_CONSUMER;
    // Archived predecessors unlock newly synced consumers without bridge rows.
    let mut authority_failures = Vec::new();
    let root = acdd_workspace(DEFAULT_SEAM, "");
    root.write("src/lib.rs", "pub fn f() {}\n");
    let cases = [
        ("tombstone", "m-first:producer", true),
        ("tombstone", "m-first:cancelled", false),
        ("tombstone", "m-first", true),
        ("nonterminal-tombstone", "m-first", false),
        ("nonterminal-tombstone", "m-first:producer", true),
        ("foreign-tombstone", "m-first:producer", false),
        ("archive", "m-first:producer", true),
        ("archive", "m-first:cancelled", false),
        ("archive", "m-first", true),
        ("archive", "other/m-first:producer", false),
        ("alias-archive", "old/m-first:producer", true),
        ("late-archive", "m-first:producer", true),
        ("torn-archive", "m-first:producer", false),
        ("active-archive", "m-first:producer", false),
        ("nonterminal-archive", "m-first:producer", false),
        ("undated-archive", "m-first:producer", false),
        ("foreign-archive", "m-first:producer", false),
    ];
    let nested = root_mode != "default";
    if nested {
        let dir = if root_mode == "wildcard" {
            "extensions/*/docs/milestones"
        } else {
            "extensions/base/docs/milestones"
        };
        root.write("forge-mcp.yaml", &format!("task_project: old\nmilestones: [docs/milestones, '{dir}']\nowners: {{extensions/base: old}}\naliases: {{old: forge-mcp, forge-mcp: other}}\n"));
    } else {
        root.write(
            "forge-mcp.yaml",
            "task_project: old\naliases: {old: forge-mcp, forge-mcp: other}\n",
        );
    }
    for (authority, dependency, expected_ready) in cases {
        for rel in [
            ".forge/tasks.sqlite",
            ".forge/tasks.sqlite-wal",
            ".forge/tasks.sqlite-shm",
            "docs/milestones/m2.md",
            "docs/milestones/archive/m-first.md",
            "extensions/base/docs/milestones/archive/m-first.md",
        ] {
            let path = root.root().join(rel);
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
        let mut deferred_archive = None;
        let mut store = TasksStore::open(&root.root().join(".forge/tasks.sqlite")).unwrap();
        if authority.ends_with("tombstone") {
            let identity = if authority == "foreign-tombstone" {
                "forge-mcp/other/m-first"
            } else {
                "forge-mcp/forge-mcp/m-first"
            };
            let completed_task_ids = json!(["forge-mcp/m-first:producer"]).to_string();
            store
                .connection
                .execute(
                    "INSERT INTO milestone_tombstones VALUES(?1,0,?2,?3)",
                    rusqlite::params![
                        identity,
                        authority != "nonterminal-tombstone",
                        completed_task_ids
                    ],
                )
                .unwrap();
        } else {
            let archive = "---\nid: m-first\nstatus: completed\nhandoff:\n  completed_at: 2026-10-01T10:00:00Z\n  commit: 0123456789012345678901234567890123456789\n---\n```yaml\ntask_ref: producer\ntarget: Archived producer\nstatus: completed\nscope: [src/]\n```\n```yaml\ntask_ref: cancelled\ntarget: Transferred work\nstatus: cancelled\nscope: [other/]\n```\n";
            let archive = archive
                .replace(
                    "target: Archived producer\n",
                    "target: Archived producer\nproof_policy: seam-test-first\n",
                )
                .replace(
                    "target: Transferred work\n",
                    "target: Transferred work\nproof_policy: seam-test-first\n",
                );
            let archive = match authority {
                "torn-archive" => "---\nid: [torn".to_owned(),
                "active-archive" => archive.replacen("status: completed", "status: active", 1),
                "nonterminal-archive" => archive.replace("status: cancelled", "status: ready"),
                "undated-archive" => archive.replace("2026-10-01T10:00:00Z", "not-a-date"),
                "alias-archive" => archive.replace("id: m-first", "id: m-first\nproject: old"),
                "foreign-archive" => archive.replace("id: m-first", "id: m-other"),
                _ => archive,
            };
            if authority == "late-archive" {
                deferred_archive = Some(archive);
            } else {
                root.write(
                    if nested {
                        "extensions/base/docs/milestones/archive/m-first.md"
                    } else {
                        "docs/milestones/archive/m-first.md"
                    },
                    &archive,
                );
            }
        }
        let consumer = m2_text.replace("m-first:producer", dependency);
        let milestone = Milestone::parse(&consumer, "forge-mcp").unwrap();
        root.write("docs/milestones/m2.md", &consumer);
        store
            .sync(&milestone, "docs/milestones/m2.md", root.root())
            .unwrap();
        if let Some(archive) = deferred_archive {
            root.write(
                if nested {
                    "extensions/base/docs/milestones/archive/m-first.md"
                } else {
                    "docs/milestones/archive/m-first.md"
                },
                &archive,
            );
        }
        let tasks = store.list(None, "all", None).unwrap();
        assert_eq!(
            tasks.len(),
            1,
            "{root_mode}/{authority}/{dependency}: no bridge tasks"
        );
        let claim = store.claim(
            &tasks[0].task_id,
            "contract",
            "archived-consumer",
            root.root().to_str().unwrap(),
            Some(&TaskHarness::session_for("archived-consumer")),
        );
        if (tasks[0].status == "ready") != expected_ready || claim.is_ok() != expected_ready {
            authority_failures.push(format!(
                "{root_mode}/{authority}/{dependency}: status={}, claim={claim:?}",
                tasks[0].status
            ));
        }
        drop(store);
    }
    assert!(
        authority_failures.is_empty(),
        "archive authority matrix: {authority_failures:?}"
    );
}

#[test]
fn archived_predecessor_authority_controls_cross_milestone_readiness() {
    check_archived_predecessor_authority("default");
}

#[test]
fn archived_predecessor_authority_controls_wildcard_milestone_roots() {
    check_archived_predecessor_authority("wildcard");
}

#[test]
fn archived_predecessor_authority_controls_concrete_milestone_roots() {
    check_archived_predecessor_authority("concrete");
}
