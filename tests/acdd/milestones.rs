use super::support::*;

#[test]
fn cli_migration_reconciles_manifests_and_creation_selects_one_task() {
    let (root, mut store, milestone) = fixture();
    let invoke = |action: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--root",
                root.0.to_str().unwrap(),
                "migrate",
                action,
                "docs/010-test.md",
            ])
            .output()
            .unwrap()
    };
    for action in ["preview", "apply", "apply", "verify"] {
        let output = invoke(action);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["mode"], action);
    }
    store
        .delete(None, Some("docs/010-test.md"), true, 0)
        .unwrap();
    let created = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Create,
            milestone_ref: Some("docs/010-test.md".into()),
            task_ref: Some("first".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(created["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(store.list(None, "all", None).unwrap().len(), 1);
    assert_eq!(
        store.list(None, "all", None).unwrap()[0].task_id,
        milestone.task_id(&milestone.tasks[0])
    );
    assert!(!invoke("verify").status.success());
    assert!(invoke("apply").status.success());
    assert!(invoke("verify").status.success());
    root.write("docs/milestones/archive/010-test.md", SPEC);
    for action in ["preview", "apply", "verify"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args(["--root", root.0.to_str().unwrap(), "migrate", action])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["manifests"].as_array().unwrap().len(), 1);
    }
}
fn milestone_cli(root: &ScopedWorkspace, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args(["--root", root.0.to_str().unwrap(), "milestone"])
        .args(args)
        .output()
        .unwrap()
}

fn complete_task_through_engine(root: &ScopedWorkspace, store: &TasksStore, task_id: &str) {
    for (index, stage) in GATES.iter().enumerate() {
        let worker_id = match index {
            0 => "contract-author",
            1 => "builder",
            2 => "independent-reviewer",
            _ => "delivery-reviewer",
        };
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: task_id.to_owned(),
                stage: (*stage).to_owned(),
                worker_id: worker_id.to_owned(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let task = store.inspect(task_id).unwrap();
        let mut evidence = evidence(&task);
        evidence.commit = None;
        let request: tasks::Submit = serde_json::from_value(json!({
            "task_id":task_id,
            "stage":stage,
            "action":"pass",
            "evidence":evidence,
        }))
        .unwrap();
        let result = tasks::submit(&root.0, request).unwrap();
        assert_eq!(
            result["status"],
            if index + 1 == GATES.len() {
                "completed"
            } else {
                "ready"
            }
        );
    }
}

#[test]
fn milestone_init_scaffolds_numbered_planned_and_active_documents() {
    use std::io::Write;
    let root = ScopedWorkspace::new("forge_milestone_init");
    root.write("docs/milestones/010-first.md", "---\nid: m-first\n---\n");
    root.write(
        "docs/milestones/archive/1050-prior.md",
        "---\nid: m-prior\n---\n",
    );
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            root.0.to_str().unwrap(),
            "milestone",
            "init",
            "--slug",
            "next-work",
            "--title",
            "Next work",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"A concrete outcome.\n\n### task: first\n\n```yaml\ntask_ref: first\ntarget: First task\nproof_policy: seam-test-first\nscope: [src/]\n```\n\nTask notes: preserve this detail.\n\n### task: second\n\n```yaml\ntask_ref: second\ntarget: Second task\nproof_policy: seam-test-first\nscope: [src/]\n```\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = root.0.join("docs/milestones/1060-next-work.md");
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("status: planned"));
    assert!(!text.contains("started_at:"));
    assert!(text.contains("A concrete outcome."));
    assert!(text.contains("## Deferred and out-of-scope defects"));
    assert!(text.contains("deferred_defects: []"));
    assert!(Milestone::parse(&text, "forge-mcp")
        .unwrap()
        .deferred_defects
        .is_empty());
    let note = text.find("Task notes: preserve this detail.").unwrap();
    assert!(text.find("task_ref: first").unwrap() < note);
    assert!(note < text.find("task_ref: second").unwrap());
    let output = milestone_cli(
        &root,
        &[
            "init",
            "--num",
            "011",
            "--slug",
            "active-work",
            "--title",
            "Active work",
            "--active",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(root.0.join("docs/milestones/011-active-work.md")).unwrap();
    assert!(text.contains("status: active"));
    let started = text
        .lines()
        .find_map(|line| line.strip_prefix("started_at: "))
        .unwrap();
    let parsed = chrono::DateTime::parse_from_rfc3339(started).unwrap();
    assert!(
        (chrono::Utc::now() - parsed.with_timezone(&chrono::Utc))
            .num_minutes()
            .abs()
            < 2
    );
    let output = milestone_cli(
        &root,
        &[
            "init",
            "--num",
            "012",
            "--slug",
            "described",
            "--title",
            "Described",
            "--desc",
            "Explicit description",
            "--depends-on",
            "m-first",
        ],
    );
    assert!(output.status.success());
    let text = std::fs::read_to_string(root.0.join("docs/milestones/012-described.md")).unwrap();
    assert!(text.contains("Explicit description"));
    assert!(text.contains("m-first"));
}
#[test]
fn milestone_init_imports_plan_metadata_and_rejects_duplicate_number() {
    let root = ScopedWorkspace::new("forge_milestone_plan");
    root.write("docs/plans/proposal.md", "---\ntitle: Planned title\ndoc_type: plan\ndepends_on: [m-prior]\n---\n# Planned title\n\nA scoped purpose.\n\n## Delivery notes\n\nA separate implementation note.\n");
    let invoke = || {
        milestone_cli(
            &root,
            &[
                "init",
                "--num",
                "025",
                "--slug",
                "planned-title",
                "--plan",
                "docs/plans/proposal.md",
            ],
        )
    };
    let output = invoke();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(response["sync"].as_str().unwrap().contains("task sync"));
    assert!(response["claim"].as_str().unwrap().contains("task claim"));
    let text =
        std::fs::read_to_string(root.0.join("docs/milestones/025-planned-title.md")).unwrap();
    assert!(text.contains("title: Planned title"));
    assert!(text.contains("m-prior"));
    assert!(text.contains("A scoped purpose."));
    assert!(text.contains("## Source plan notes"));
    assert!(text.find("## Source plan notes").unwrap() > text.find("A scoped purpose.").unwrap());
    assert!(text.contains("A separate implementation note."));
    assert!(!invoke().status.success());
    let same_id = milestone_cli(
        &root,
        &[
            "init",
            "--num",
            "035",
            "--slug",
            "planned-title",
            "--title",
            "Another title",
        ],
    );
    assert!(!same_id.status.success());
    root.write(
        "docs/milestones/archive/18446744073709551616-legacy.md",
        "---\nid: m-legacy\n---\n",
    );
    let overflow = milestone_cli(&root, &["init", "--slug", "later", "--title", "Later"]);
    assert!(!overflow.status.success());
}
#[test]
fn milestone_specification_parses_typed_deferred_defects_block() {
    let spec = r#"---
id: m-defects
title: Defects test
doc_type: contract
deferred_defects:
  - id: DEFECT-FRONTMATTER
    source: task_blackboard
    title: "Frontmatter defect stays typed"
    path: src/engine/tasks.rs
    disposition: subsequent_milestone
    notes: "Imported from contract metadata"
---
# Defects test

### task: task-a
```yaml
task_ref: task-a
target: Task A target
proof_policy: direct-proof
scope: [src/]
```

## Deferred and out-of-scope defects

```yaml
deferred_defects:
  - id: DEFECT-001
    source: review_findings
    title: "Unshadowed function resolution bug in edge case"
    path: src/engine/languages/python.rs
    disposition: deferred
    notes: "Follow up in next milestone"
  - id: DEFECT-002
    source: task_blackboard
    title: "Minor syntax warning"
```
"#;
    let milestone = Milestone::parse(spec, "forge-mcp").unwrap();
    assert_eq!(milestone.deferred_defects.len(), 3);
    assert_eq!(milestone.deferred_defects[0].id, "DEFECT-FRONTMATTER");
    assert_eq!(milestone.deferred_defects[0].source, "task_blackboard");
    assert_eq!(
        milestone.deferred_defects[0].title,
        "Frontmatter defect stays typed"
    );
    assert_eq!(
        milestone.deferred_defects[0].path.as_deref(),
        Some("src/engine/tasks.rs")
    );
    assert_eq!(
        milestone.deferred_defects[0].disposition,
        "subsequent_milestone"
    );
    assert_eq!(
        milestone.deferred_defects[0].notes.as_deref(),
        Some("Imported from contract metadata")
    );
    assert_eq!(milestone.deferred_defects[1].id, "DEFECT-001");
    assert_eq!(milestone.deferred_defects[1].source, "review_findings");
    assert_eq!(
        milestone.deferred_defects[1].title,
        "Unshadowed function resolution bug in edge case"
    );
    assert_eq!(
        milestone.deferred_defects[1].path.as_deref(),
        Some("src/engine/languages/python.rs")
    );
    assert_eq!(milestone.deferred_defects[1].disposition, "deferred");
    assert_eq!(
        milestone.deferred_defects[1].notes.as_deref(),
        Some("Follow up in next milestone")
    );
    assert_eq!(milestone.deferred_defects[2].id, "DEFECT-002");
    assert_eq!(milestone.deferred_defects[2].source, "task_blackboard");
    assert_eq!(milestone.deferred_defects[2].disposition, "deferred");
    assert!(milestone.deferred_defects[2].path.is_none());
    assert!(milestone.deferred_defects[2].notes.is_none());
}
#[test]
fn milestone_list_and_show_report_scoped_documents_and_sqlite_progress() {
    let root = ScopedWorkspace::new("forge_milestone_inspection");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let queued = "---\nid: m-queued\ntitle: Queued work\ndoc_type: contract\nstatus: planned\n---\n# Queued work\n### task: draft\n```yaml\ntask_ref: draft\ntarget: Draft work\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    let active = "---\nid: m-live\ntitle: Live work\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\ndepends_on: [m-queued]\ninvariants: [Keep receipts]\n---\n# Live work\n## Outcome and purpose\nDeliver visible progress.\n### task: first\n```yaml\ntask_ref: first\ntarget: First result\nproof_policy: seam-test-first\nscope: [src/]\n```\nImplementation steps for first.\n### task: second\n```yaml\ntask_ref: second\ntarget: Second result\nproof_policy: seam-test-first\nscope: [src/]\n```\nDelivery notes for second.\n";
    let archived = "---\nid: m-prior\ntitle: Prior work\ndoc_type: contract\nstatus: completed\nstarted_at: 2026-09-01T10:00:00Z\n---\n# Prior work\n### task: shipped\n```yaml\ntask_ref: shipped\ntarget: Shipped result\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    for (path, source) in [
        ("docs/milestones/010-queued.md", queued),
        ("docs/milestones/011-live.md", active),
        ("docs/milestones/archive/020-prior.md", archived),
    ] {
        root.write(path, source);
    }
    root.write(
        "docs/milestones/README.md",
        "---\ntitle: Milestone guide\ndoc_type: guide\n---\n# Milestone guide\n",
    );
    root.write(
        "docs/milestones/archive/README.md",
        "---\ntitle: Archive guide\ndoc_type: guide\n---\n# Archive guide\n",
    );
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    for (path, source) in [
        ("docs/milestones/010-queued.md", queued),
        ("docs/milestones/011-live.md", active),
        ("docs/milestones/archive/020-prior.md", archived),
    ] {
        store
            .sync(
                &Milestone::parse(source, "forge-mcp").unwrap(),
                path,
                &root.0,
            )
            .unwrap();
    }
    root.write("docs/milestones/010-queued.md", &format!("{queued}### task: file-only\n```yaml\ntask_ref: file-only\ntarget: File-declared completion\nproof_policy: seam-test-first\nscope: [src/]\nstatus: completed\n```\n"));
    root.write(
        "docs/milestones/archive/020-prior.md",
        &archived.replace("scope: [src/]", "scope: [src/]\nstatus: completed"),
    );
    for task_ref in ["first", "shipped"] {
        let mut task = store
            .list(None, "all", None)
            .unwrap()
            .into_iter()
            .find(|t| t.task_id.ends_with(&format!(":{task_ref}")))
            .unwrap();
        task.status = "completed".into();
        store
            .connection
            .execute(
                "UPDATE tasks SET descriptor=?1 WHERE task_id=?2",
                rusqlite::params![serde_json::to_string(&task).unwrap(), task.task_id],
            )
            .unwrap();
    }
    let run = |args: &[&str]| {
        let output = milestone_cli(&root, args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let listed = run(&["list"]);
    let rows = listed["milestones"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for heading in ["ID", "title", "status", "started_at", "completion"] {
        assert!(listed["table"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains(&heading.to_ascii_lowercase()));
    }
    let queued_row = rows.iter().find(|row| row["id"] == "m-queued").unwrap();
    assert_eq!(
        (
            queued_row["title"].as_str(),
            queued_row["status"].as_str(),
            queued_row["started_at"].as_str(),
            queued_row["completion"].as_str()
        ),
        (Some("Queued work"), Some("planned"), Some("-"), Some("0/2"))
    );
    assert!(queued_row["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["task_ref"] == "file-only" && task["status"] == "unsynced"));
    let live = rows.iter().find(|row| row["id"] == "m-live").unwrap();
    assert!(listed["table"]
        .as_str()
        .unwrap()
        .lines()
        .any(|line| line.contains("m-live") && line.contains("1/2")));
    assert_eq!(
        (live["status"].as_str(), live["completion"].as_str()),
        (Some("active"), Some("1/2"))
    );
    assert_eq!(live["tasks"].as_array().unwrap().len(), 2);
    assert!(live["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["task_ref"] == "first" && task["status"] == "completed"));
    assert!(live["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["task_ref"] == "second" && task["status"] == "ready"));
    for args in [&["list", "--archive"][..], &["list", "--status", "all"][..]] {
        assert_eq!(run(args)["milestones"].as_array().unwrap().len(), 3);
    }
    let completed = run(&["list", "--status", "completed"]);
    assert_eq!(completed["milestones"][0]["id"], "m-prior");
    assert_eq!(completed["milestones"][0]["completion"], "1/1");
    let shown = run(&["show", "m-live"]);
    assert!(shown.get("full_document").is_none());
    assert_eq!(
        (
            shown["id"].as_str(),
            shown["title"].as_str(),
            shown["status"].as_str()
        ),
        (Some("m-live"), Some("Live work"), Some("active"))
    );
    assert_eq!(shown["frontmatter"]["id"], "m-live");
    assert_eq!(shown["depends_on"][0], "m-queued");
    assert_eq!(shown["invariants"][0], "Keep receipts");
    assert!(shown["outcome"]
        .as_str()
        .unwrap()
        .contains("Deliver visible progress."));
    assert_eq!(shown["tasks"].as_array().unwrap().len(), 2);
    assert!(shown["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["task_ref"] == "first" && task["target"] == "First result"));
    let by_prefix = run(&["show", "011", "--full"]);
    assert!(by_prefix.get("full_document").is_some());
    assert_eq!(by_prefix["id"], shown["id"]);
    assert!(by_prefix["full_document"]
        .as_str()
        .unwrap()
        .contains("Implementation steps for first."));
    assert!(by_prefix["full_document"]
        .as_str()
        .unwrap()
        .contains("Delivery notes for second."));
    let by_path = run(&["show", "docs/milestones/011-live.md"]);
    assert_eq!(by_path["id"], shown["id"]);
    assert_eq!(run(&["show", "020"])["status"], "completed");
    let override_source = "---\nid: m-override\ntitle: Override work\ndoc_type: contract\nstatus: active\nrepository: alternate\nproject: special\n---\n# Override work\n### task: delivered\n```yaml\ntask_ref: delivered\ntarget: Delivered override\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    root.write("docs/milestones/025-override.md", override_source);
    let mut alternate =
        TasksStore::open_project(&root.0.join(".forge/tasks.sqlite"), "alternate", "special")
            .unwrap();
    alternate
        .sync(
            &Milestone::parse_with_identity(override_source, "alternate", "special").unwrap(),
            "docs/milestones/025-override.md",
            &root.0,
        )
        .unwrap();
    let mut delivered = alternate.list(None, "all", None).unwrap().pop().unwrap();
    delivered.status = "completed".into();
    alternate
        .connection
        .execute(
            "UPDATE tasks SET descriptor=?1 WHERE task_id=?2",
            rusqlite::params![
                serde_json::to_string(&delivered).unwrap(),
                delivered.task_id
            ],
        )
        .unwrap();
    let overridden = run(&["list"]);
    let override_row = overridden["milestones"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "m-override")
        .unwrap();
    assert_eq!(override_row["completion"], "1/1");
    assert_eq!(override_row["tasks"][0]["status"], "completed");
    root.write("docs/milestones/archive/030-broken.md", "---\nid: m-broken\ntitle: Broken archive\ndoc_type: contract\nstatus: completed\n---\n# Broken archive\n```yaml\ntask_ref: [invalid\n```\n");
    assert_eq!(run(&["list"])["milestones"].as_array().unwrap().len(), 3);
    assert_eq!(run(&["show", "011"])["id"], "m-live");
    root.write(
        "docs/milestones/032-duplicate.md",
        &active
            .replace("m-live", "m-duplicate")
            .replace("Live work", "Duplicate active"),
    );
    root.write(
        "docs/milestones/archive/032-duplicate.md",
        &archived
            .replace("m-prior", "m-duplicate-archived")
            .replace("Prior work", "Duplicate archived"),
    );
    assert_eq!(run(&["show", "032"])["id"], "m-duplicate");
    assert_eq!(
        run(&["show", "docs/milestones/archive/032-duplicate.md"])["id"],
        "m-duplicate-archived"
    );
    root.write(
        "docs/milestones/031-malformed-frontmatter.md",
        "---\nid: m-invalid\ntitle: Invalid frontmatter\ndoc_type: contract\nbroken: [\n---\n",
    );
    let invalid_id = milestone_cli(&root, &["show", "m-does-not-exist"]);
    let invalid_id_output = format!(
        "{}{}",
        String::from_utf8_lossy(&invalid_id.stdout),
        String::from_utf8_lossy(&invalid_id.stderr)
    );
    assert!(
        invalid_id_output.contains("failed to read milestone id from"),
        "milestone ID lookup must preserve frontmatter parse errors: {invalid_id_output}"
    );
}
#[test]
fn milestone_handoff_requires_completed_tasks_and_archives_typed_receipt() {
    let root = ScopedWorkspace::new("forge_milestone_handoff");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let source = "---\nid: m-close\ntitle: Close work\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\n---\n# Close work\n### task: finish\n```yaml\ntask_ref: finish\ntarget: Finish work\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    root.write("docs/milestones/010-close.md", source);
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(
            &Milestone::parse(source, "forge-mcp").unwrap(),
            "docs/milestones/010-close.md",
            &root.0,
        )
        .unwrap();
    let commit = git_head(&root.0);
    let args = [
        "handoff",
        "m-close",
        "--commit",
        commit.as_str(),
        "--verification-command",
        "cargo test --all-targets",
        "--tests-passed",
        "7",
        "--tests-failed",
        "0",
    ];
    let rejected = milestone_cli(&root, &args);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("finish"));
    assert!(root.0.join("docs/milestones/010-close.md").exists());
    let task = store.list(None, "all", None).unwrap().pop().unwrap();
    root.write("src/lib.rs", "pub fn finish() {}\n");
    complete_task_through_engine(&root, &store, &task.task_id);
    drop(store);
    let handoff_commit = git_head(&root.0);
    let completed_args = [
        "handoff",
        "m-close",
        "--commit",
        handoff_commit.as_str(),
        "--verification-command",
        "cargo test --all-targets",
        "--tests-passed",
        "7",
        "--tests-failed",
        "0",
    ];
    let completed = milestone_cli(&root, &completed_args);
    assert!(
        completed.status.success(),
        "{}",
        String::from_utf8_lossy(&completed.stderr)
    );
    assert!(!root.0.join("docs/milestones/010-close.md").exists());
    let reopened = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    assert_eq!(
        reopened.list(None, "all", None).unwrap()[0].milestone_ref,
        "docs/milestones/archive/010-close.md"
    );
    let archived =
        std::fs::read_to_string(root.0.join("docs/milestones/archive/010-close.md")).unwrap();
    let header = archived
        .strip_prefix("---\n")
        .unwrap()
        .split_once("\n---\n")
        .unwrap()
        .0;
    let meta: serde_yaml::Value = serde_yaml::from_str(header).unwrap();
    assert_eq!(meta["status"].as_str(), Some("completed"));
    assert_eq!(
        meta["handoff"]["commit"].as_str(),
        Some(handoff_commit.as_str())
    );
    let started =
        chrono::DateTime::parse_from_rfc3339(meta["started_at"].as_str().unwrap()).unwrap();
    let ended =
        chrono::DateTime::parse_from_rfc3339(meta["handoff"]["completed_at"].as_str().unwrap())
            .unwrap();
    let minutes = (ended - started).num_minutes();
    assert_eq!(
        meta["handoff"]["duration"].as_str().unwrap(),
        format!("{}h {}m", minutes / 60, minutes % 60)
    );
    let verification = &meta["handoff"]["verification"];
    assert_eq!(
        verification["command"].as_str(),
        Some("cargo test --all-targets")
    );
    assert_eq!(verification["status"].as_str(), Some("passed"));
    assert_eq!(verification["tests_passed"].as_i64(), Some(7));
    assert_eq!(verification["tests_failed"].as_i64(), Some(0));
    let fallback = ScopedWorkspace::new("forge_handoff_claim_fallback");
    fallback.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    fallback.write("src/lib.rs", "pub fn fallback() {}\n");
    let source = "---\nid: m-fallback\ntitle: Fallback work\ndoc_type: contract\nstatus: active\n---\n# Fallback work\n### task: finish\n```yaml\ntask_ref: finish\ntarget: Finish fallback\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    fallback.write("docs/milestones/020-fallback.md", source);
    let milestone = Milestone::parse(source, "forge-mcp").unwrap();
    let mut fallback_store = TasksStore::open(&fallback.0.join(".forge/tasks.sqlite")).unwrap();
    fallback_store
        .sync(&milestone, "docs/milestones/020-fallback.md", &fallback.0)
        .unwrap();
    let id = milestone.task_id(&milestone.tasks[0]);
    complete_task_through_engine(&fallback, &fallback_store, &id);
    let claim_time = fallback_store
        .earliest_claim("forge-mcp/forge-mcp/m-fallback:")
        .unwrap()
        .unwrap();
    drop(fallback_store);
    let fallback_commit = git_head(&fallback.0);
    let result = milestone_cli(
        &fallback,
        &[
            "handoff",
            "m-fallback",
            "--commit",
            fallback_commit.as_str(),
            "--verification-command",
            "cargo test",
            "--tests-passed",
            "1",
            "--tests-failed",
            "0",
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = std::fs::read_to_string(fallback.0.join("docs/milestones/archive/020-fallback.md"))
        .unwrap();
    let header = text
        .strip_prefix("---\n")
        .unwrap()
        .split_once("\n---\n")
        .unwrap()
        .0;
    let meta: serde_yaml::Value = serde_yaml::from_str(header).unwrap();
    let started =
        chrono::DateTime::parse_from_rfc3339(meta["started_at"].as_str().unwrap()).unwrap();
    let ended =
        chrono::DateTime::parse_from_rfc3339(meta["handoff"]["completed_at"].as_str().unwrap())
            .unwrap();
    let minutes = (ended - started).num_minutes();
    assert_eq!(started.timestamp(), claim_time);
    assert_eq!(
        meta["handoff"]["duration"].as_str().unwrap(),
        format!("{}h {}m", minutes / 60, minutes % 60)
    );
}

#[test]
fn manual_delivery_handoff_rewrites_markdown_receipt_to_landed_commit() {
    let root = ScopedWorkspace::new("forge_manual_delivery_handoff");
    root.write("src/lib.rs", "pub fn manual_delivery() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nacdd_profile: custom/manual.yaml\n",
    );
    let mut profile = (*contextunity_forge_mcp::core::tasks::profile::compiled()).clone();
    profile.gates.last_mut().unwrap().auto_commit = false;
    root.write(
        "custom/manual.yaml",
        &serde_yaml::to_string(&profile).unwrap(),
    );
    let source = "---\nid: m-manual\ntitle: Manual delivery\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\n---\n# Manual delivery\n### task: finish\n```yaml\ntask_ref: finish\ntarget: Finish manually delivered work\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    root.write("docs/milestones/010-manual.md", source);
    let milestone = Milestone::parse(source, "forge-mcp").unwrap();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/milestones/010-manual.md", &root.0)
        .unwrap();
    let baseline = git_head(&root.0);
    complete_task_through_engine(&root, &store, &task_id);

    let task = store.inspect(&task_id).unwrap();
    let candidate = task
        .receipt
        .as_ref()
        .and_then(|receipt| receipt.commit.as_deref())
        .expect("manual delivery receipt retains its accepted candidate SHA");
    let (snapshot_candidate, candidate_baseline) = store
        .build_snapshot_candidate(&task_id)
        .unwrap()
        .expect("delivery retains the captured snapshot candidate and its baseline");
    assert_eq!(candidate, snapshot_candidate);
    let candidate_parents = std::process::Command::new("git")
        .args(["rev-list", "--parents", "-n", "1", candidate])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(candidate_parents.status.success());
    assert_eq!(
        String::from_utf8_lossy(&candidate_parents.stdout)
            .split_whitespace()
            .count(),
        1,
        "captured candidates are parentless root snapshots"
    );
    assert_eq!(
        candidate_baseline, baseline,
        "the accepted snapshot already stores the branch baseline needed by handoff"
    );
    assert_eq!(
        git_head(&root.0),
        baseline,
        "manual delivery leaves HEAD unchanged"
    );

    let staged = std::process::Command::new("git")
        .args(["add", "--", "src/lib.rs"])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(staged.status.success());
    let commit = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=ACDD test",
            "-c",
            "user.email=acdd-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "manual task landing",
        ])
        .current_dir(&root.0)
        .output()
        .unwrap();
    assert!(
        commit.status.success(),
        "{}",
        String::from_utf8_lossy(&commit.stderr)
    );
    let landed_commit = git_head(&root.0);
    drop(store);

    let result = milestone_cli(
        &root,
        &[
            "handoff",
            "m-manual",
            "--commit",
            landed_commit.as_str(),
            "--verification-command",
            "cargo test --test acdd",
            "--tests-passed",
            "1",
            "--tests-failed",
            "0",
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );

    let archive =
        std::fs::read_to_string(root.0.join("docs/milestones/archive/010-manual.md")).unwrap();
    let archived = Milestone::parse(&archive, "forge-mcp").unwrap();
    assert_eq!(
        archived.tasks[0]
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.commit.as_deref()),
        Some(landed_commit.as_str()),
        "handoff must persist the landed Git commit into Markdown before archival"
    );
    let reopened = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    assert_eq!(
        reopened
            .inspect(&task_id)
            .unwrap()
            .receipt
            .and_then(|receipt| receipt.commit),
        Some(landed_commit)
    );
}

#[test]
fn legacy_parentless_candidate_handoff_resolves_first_parent_landings() {
    let cases = [("direct", false), ("merge", true)];
    for (landing_kind, merge_landing) in cases {
        let root = ScopedWorkspace::new("forge_legacy_parentless_handoff");
        root.write("src/lib.rs", "pub fn legacy_delivery() {}\n");
        let milestone_id = format!("m-legacy-{landing_kind}");
        let milestone_ref = format!("docs/milestones/010-legacy-{landing_kind}.md");
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nacdd_profile: custom/manual.yaml\n",
        );
        let mut profile = (*contextunity_forge_mcp::core::tasks::profile::compiled()).clone();
        profile.gates.last_mut().unwrap().auto_commit = false;
        root.write(
            "custom/manual.yaml",
            &serde_yaml::to_string(&profile).unwrap(),
        );
        let source = format!(
            "---\nid: {milestone_id}\ntitle: Legacy parentless handoff\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\n---\n# Legacy parentless handoff\n### task: finish\n```yaml\ntask_ref: finish\ntarget: Finish manually delivered work\nproof_policy: seam-test-first\nscope: [src/]\n```\n"
        );
        root.write(&milestone_ref, &source);
        let milestone = Milestone::parse(&source, "forge-mcp").unwrap();
        let task_id = milestone.task_id(&milestone.tasks[0]);
        let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
        store.sync(&milestone, &milestone_ref, &root.0).unwrap();
        complete_task_through_engine(&root, &store, &task_id);

        let task = store.inspect(&task_id).unwrap();
        let receipt = task
            .receipt
            .clone()
            .expect("engine delivery leaves a durable receipt for legacy import");
        let candidate = receipt
            .commit
            .as_deref()
            .expect("manual delivery retains its accepted candidate SHA")
            .to_owned();
        let candidate_parents = std::process::Command::new("git")
            .args(["rev-list", "--parents", "-n", "1", &candidate])
            .current_dir(&root.0)
            .output()
            .unwrap();
        assert!(candidate_parents.status.success());
        assert_eq!(
            String::from_utf8_lossy(&candidate_parents.stdout)
                .split_whitespace()
                .count(),
            1,
            "accepted snapshots are parentless root commits"
        );

        let base_branch = std::process::Command::new("git")
            .args(["symbolic-ref", "--short", "HEAD"])
            .current_dir(&root.0)
            .output()
            .unwrap();
        assert!(base_branch.status.success());
        let base_branch = String::from_utf8_lossy(&base_branch.stdout)
            .trim()
            .to_owned();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&root.0)
                .output()
                .unwrap()
        };
        let landed_commit = if merge_landing {
            assert!(git(&["checkout", "-b", "legacy-task-landing"])
                .status
                .success());
            let staged = git(&["add", "--", "src/lib.rs"]);
            assert!(staged.status.success());
            let commit = git(&[
                "-c",
                "user.name=ACDD test",
                "-c",
                "user.email=acdd-test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "legacy task landing",
            ]);
            assert!(
                commit.status.success(),
                "{}",
                String::from_utf8_lossy(&commit.stderr)
            );
            let switched = git(&["checkout", &base_branch]);
            assert!(
                switched.status.success(),
                "{}",
                String::from_utf8_lossy(&switched.stderr)
            );
            let merged = git(&["merge", "--no-ff", "--no-edit", "legacy-task-landing"]);
            assert!(
                merged.status.success(),
                "{}",
                String::from_utf8_lossy(&merged.stderr)
            );
            git_head(&root.0)
        } else {
            let staged = git(&["add", "--", "src/lib.rs"]);
            assert!(staged.status.success());
            let commit = git(&[
                "-c",
                "user.name=ACDD test",
                "-c",
                "user.email=acdd-test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "legacy task landing",
            ]);
            assert!(
                commit.status.success(),
                "{}",
                String::from_utf8_lossy(&commit.stderr)
            );
            git_head(&root.0)
        };

        let first_parent = git(&["rev-parse", &format!("{landed_commit}^1")]);
        assert!(first_parent.status.success());
        let first_parent = String::from_utf8_lossy(&first_parent.stdout)
            .trim()
            .to_owned();
        let candidate_diff = git(&["diff", "--quiet", &candidate, &landed_commit, "--", "src/"]);
        assert_eq!(candidate_diff.status.code(), Some(0));
        let landing_diff = git(&[
            "diff",
            "--quiet",
            &first_parent,
            &landed_commit,
            "--",
            "src/",
        ]);
        assert_eq!(landing_diff.status.code(), Some(1));

        // Legacy completed receipts have no preceding gate evidence carrying a baseline.
        let legacy_db = root.0.join(".forge/legacy.sqlite");
        let receipt_yaml = serde_yaml::to_string(&receipt).unwrap();
        let receipt_yaml = receipt_yaml
            .lines()
            .map(|line| format!("  {line}\n"))
            .collect::<String>();
        let completed_source = source.replace(
            "scope: [src/]\n```",
            &format!("scope: [src/]\nstatus: completed\nreceipt:\n{receipt_yaml}```"),
        );
        root.write(&milestone_ref, &completed_source);
        drop(store);
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/legacy.sqlite\nacdd_profile: custom/manual.yaml\n",
        );
        tasks::manage(
            &root.0,
            tasks::Manage {
                action: tasks::ManageAction::Sync,
                milestone_ref: Some(milestone_ref.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        let legacy_store = TasksStore::open(&legacy_db).unwrap();
        assert_eq!(
            legacy_store.build_snapshot_candidate(&task_id).unwrap(),
            None,
            "legacy receipt imports contain no captured candidate baseline"
        );
        drop(legacy_store);

        let result = milestone_cli(
            &root,
            &[
                "handoff",
                &milestone_id,
                "--commit",
                &landed_commit,
                "--verification-command",
                "cargo test --test acdd",
                "--tests-passed",
                "1",
                "--tests-failed",
                "0",
            ],
        );
        assert!(
            result.status.success(),
            "{landing_kind} parentless handoff should resolve: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let archive = std::fs::read_to_string(root.0.join(format!(
            "docs/milestones/archive/010-legacy-{landing_kind}.md"
        )))
        .unwrap();
        let archived = Milestone::parse(&archive, "forge-mcp").unwrap();
        assert_eq!(
            archived.tasks[0]
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.commit.as_deref()),
            Some(landed_commit.as_str()),
            "handoff must record the first-parent landing for legacy candidates"
        );
    }
}

#[test]
fn terminal_task_delivery_rolls_up_durable_context_and_prunes_blackboard() {
    let root = ScopedWorkspace::new("forge_task_context_rollup");
    root.write("src/lib.rs", "pub fn example() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let source = SPEC
        .replace(
            "task_ref: first\n",
            "task_ref: first\ninvariants: [first-rule]\n",
        )
        .replace(
            "invariants: [isolated]\n",
            "invariants: [isolated]\ndeferred_defects:\n  - id: DEFECT-RECEIPT\n    source: task_blackboard\n    title: Keep this typed finding across receipt writes\n    path: src/engine/tasks.rs\n    disposition: deferred\n    notes: Persist milestone-level defect metadata.\n",
        );
    let milestone_ref = "docs/milestones/010-test.md";
    root.write(milestone_ref, &source);
    let milestone = Milestone::parse(&source, "forge-mcp").unwrap();
    let first = milestone.task_id(&milestone.tasks[0]);
    let second = milestone.task_id(&milestone.tasks[1]);
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store.sync(&milestone, milestone_ref, &root.0).unwrap();

    for (index, stage) in GATES.iter().enumerate() {
        let worker = match index {
            2 => "independent-reviewer",
            3 => "delivery-reviewer",
            _ => "builder",
        };
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: first.clone(),
                stage: (*stage).to_owned(),
                worker_id: worker.into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let claimed = store.inspect(&first).unwrap();
        let mut proof = evidence(&claimed);
        proof.commit = None;
        if index == 3 {
            store
                .blackboard_post(
                    &first,
                    "architect",
                    "decisions",
                    "Keep durable task outcomes in the milestone receipt.",
                )
                .unwrap();
            store
                .blackboard_post(&first, "builder", "notes", "temporary trace")
                .unwrap();
            store
                .blackboard_post(&second, "sibling", "notes", "other task context")
                .unwrap();
            root.write(
                milestone_ref,
                &source.replace("target: Deliver first", "target: Changed without admission"),
            );
            let rejected: tasks::Submit = serde_json::from_value(json!({
                "task_id":first,
                "stage":stage,
                "action":"pass",
                "evidence":proof,
            }))
            .unwrap();
            assert!(tasks::submit(&root.0, rejected).is_err());
            assert_eq!(store.inspect(&first).unwrap().status, "in_progress");
            assert_eq!(store.blackboard_read(&first, None, None).unwrap().len(), 2);
            root.write(milestone_ref, &source);
        }
        let request: tasks::Submit = serde_json::from_value(json!({
            "task_id":first,
            "stage":stage,
            "action":"pass",
            "evidence":proof,
        }))
        .unwrap();
        let result = tasks::submit(&root.0, request).unwrap();
        if index == 3 {
            assert_eq!(result["status"], "completed");
        }
    }

    let delivered = store.inspect(&first).unwrap();
    let receipt = serde_json::to_value(delivered.receipt.unwrap()).unwrap();
    assert_eq!(receipt["commit"], git_head(&root.0));
    assert_eq!(
        receipt["rollup"]["verified_invariants"],
        json!(["isolated", "first-rule"])
    );
    assert_eq!(
        receipt["rollup"]["architectural_notes"],
        json!(["decisions: Keep durable task outcomes in the milestone receipt."])
    );
    assert_eq!(receipt["rollup"]["review_summary"]["decision"], "pass");
    for contour in contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS {
        assert_eq!(
            receipt["rollup"]["review_summary"]["contours"][contour],
            "accepted"
        );
    }
    let written = std::fs::read_to_string(root.0.join(milestone_ref)).unwrap();
    let parsed = Milestone::parse(&written, "forge-mcp").unwrap();
    assert_eq!(parsed.deferred_defects.len(), 1);
    assert_eq!(parsed.deferred_defects[0].id, "DEFECT-RECEIPT");
    assert_eq!(
        parsed.deferred_defects[0].title,
        "Keep this typed finding across receipt writes"
    );
    assert_eq!(
        parsed.deferred_defects[0].notes.as_deref(),
        Some("Persist milestone-level defect metadata.")
    );
    let persisted = parsed
        .tasks
        .iter()
        .find(|task| task.task_ref == "first")
        .unwrap();
    assert_eq!(persisted.status.as_deref(), Some("completed"));
    let document_receipt = persisted.receipt.as_ref().unwrap();
    assert!(document_receipt.commit.is_none());
    let mut database_receipt = receipt.clone();
    database_receipt
        .as_object_mut()
        .unwrap()
        .remove("commit");
    assert_eq!(
        serde_json::to_value(document_receipt).unwrap(),
        database_receipt
    );
    assert!(store
        .blackboard_read(&first, None, None)
        .unwrap()
        .is_empty());
    assert_eq!(store.blackboard_read(&second, None, None).unwrap().len(), 1);
    let reopened = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    let closed = reopened
        .blackboard_post(&first, "late-worker", "notes", "after delivery")
        .unwrap_err();
    assert_eq!(closed.to_string(), "TASK_TERMINAL");
    assert!(reopened
        .blackboard_read(&first, None, None)
        .unwrap()
        .is_empty());

    let mut accepted_build = json!(null);
    let mut accepted_review = json!(null);
    for (index, stage) in GATES.iter().enumerate() {
        let worker = match index {
            2 => "second-independent-reviewer",
            3 => "second-delivery-reviewer",
            _ => "second-builder",
        };
        tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: second.clone(),
                stage: (*stage).to_owned(),
                worker_id: worker.into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let claimed = store.inspect(&second).unwrap();
        let proof = evidence(&claimed);
        if index == 1 {
            accepted_build = proof.proof.clone();
        }
        if index == 2 {
            accepted_review = proof.proof.clone();
        }
        if index == 3 {
            store
                .blackboard_post(&second, "architect", "decisions", "Retain retry notes.")
                .unwrap();
            let current = std::fs::read_to_string(root.0.join(milestone_ref)).unwrap();
            let parsed = Milestone::parse(&current, "forge-mcp").unwrap();
            let mut recovered = Receipt {
                commit: proof.commit.clone(),
                contract_revision: 1,
                passed_at: "2026-10-02T00:00:00Z".into(),
                evidence: accepted_build.clone(),
                review: accepted_review.clone(),
                decision: "pass".into(),
                rollup: Some(ReceiptRollup {
                    verified_invariants: vec!["isolated".into()],
                    architectural_notes: vec!["decisions: Retain retry notes.".into()],
                    review_summary: ReviewSummary {
                        decision: "pass".into(),
                        contours: contextunity_forge_mcp::core::tasks::gates::REVIEW_CONTOURS
                            .into_iter()
                            .map(|name| (name.into(), "accepted".into()))
                            .collect(),
                    },
                }),
            };
            let original_block = "task_ref: second\ntarget: Deliver second\nproof_policy: seam-test-first\nscope: [src/]\ndepends_on: [first]\n";
            assert!(current.contains(original_block));
            let recovery_text = |receipt: &Receipt| {
                let mut completed = parsed.tasks[1].clone();
                completed.status = Some("completed".into());
                completed.receipt = Some(receipt.clone());
                current.replace(original_block, &serde_yaml::to_string(&completed).unwrap())
            };
            recovered.rollup.as_mut().unwrap().architectural_notes[0] =
                "decisions: stale note".into();
            root.write(milestone_ref, &recovery_text(&recovered));
            let rejected: tasks::Submit = serde_json::from_value(json!({
                "task_id":second,
                "stage":stage,
                "action":"pass",
                "evidence":proof,
            }))
            .unwrap();
            assert!(tasks::submit(&root.0, rejected).is_err());
            assert_eq!(store.inspect(&second).unwrap().status, "in_progress");
            assert_eq!(store.blackboard_read(&second, None, None).unwrap().len(), 2);
            recovered.rollup.as_mut().unwrap().architectural_notes[0] =
                "decisions: Retain retry notes.".into();
            root.write(milestone_ref, &recovery_text(&recovered));
        }
        let request: tasks::Submit = serde_json::from_value(json!({
            "task_id":second,
            "stage":stage,
            "action":"pass",
            "evidence":proof,
        }))
        .unwrap();
        let completed = tasks::submit(&root.0, request).unwrap();
        if index == 3 {
            assert_eq!(completed["status"], "completed");
            assert_eq!(completed["receipt"]["passed_at"], "2026-10-02T00:00:00Z");
            assert!(store
                .blackboard_read(&second, None, None)
                .unwrap()
                .is_empty());
        }
    }
}
#[test]
fn first_cli_task_claim_activates_planned_milestone_with_started_at() {
    let root = ScopedWorkspace::new("forge_milestone_claim_start");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write("src/lib.rs", "pub fn task() {}\n");
    let source = "---\nid: m-start\ntitle: Start work\ndoc_type: contract\nstatus: planned\n---\n# Start work\n### task: begin\n```yaml\ntask_ref: begin\ntarget: Begin work\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    root.write("docs/milestones/010-start.md", source);
    let milestone = Milestone::parse(source, "forge-mcp").unwrap();
    let mut store = TasksStore::open(&root.0.join(".forge/tasks.sqlite")).unwrap();
    store
        .sync(&milestone, "docs/milestones/010-start.md", &root.0)
        .unwrap();
    let id = milestone.task_id(&milestone.tasks[0]);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args([
            "--root",
            root.0.to_str().unwrap(),
            "task",
            "claim",
            &id,
            "--stage",
            "contract",
            "--worker",
            "first",
            "--worktree",
            root.0.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(root.0.join("docs/milestones/010-start.md")).unwrap();
    let header = text
        .strip_prefix("---\n")
        .unwrap()
        .split_once("\n---\n")
        .unwrap()
        .0;
    let meta: serde_yaml::Value = serde_yaml::from_str(header).unwrap();
    assert_eq!(meta["status"].as_str(), Some("active"));
    let started =
        chrono::DateTime::parse_from_rfc3339(meta["started_at"].as_str().unwrap()).unwrap();
    assert!(
        (chrono::Utc::now() - started.with_timezone(&chrono::Utc))
            .num_minutes()
            .abs()
            < 2
    );
}

#[test]
fn deferred_final_task_context_survives_delivery_and_prunes_blackboard() {
    let root = ScopedWorkspace::new("forge_milestone_012_e2e");
    let milestone_ref = "docs/milestones/012-context.md";
    let source = "---\nid: m-context\ntitle: Context retention\ndoc_type: contract\nstatus: planned\ninvariants: [durable-context]\n---\n# Context retention\n### task: implement\n```yaml\ntask_ref: implement\ntarget: Deliver context retention\nagent_type: gpt-6-sol\nproof_policy: seam-test-first\nscope: [src/]\ninvariants: [reviewed]\n```\n";
    root.write("src/lib.rs", "pub fn context() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    root.write(milestone_ref, source);
    let milestone = Milestone::parse(source, "forge-mcp").unwrap();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    tasks::manage(
        &root.0,
        serde_json::from_value(json!({"action":"sync","milestone_ref":milestone_ref})).unwrap(),
    )
    .unwrap();

    let blackboard = |args: &[&str]| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args(["--root", root.0.to_str().unwrap(), "task", "blackboard"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let inspect = || {
        tasks::manage(
            &root.0,
            serde_json::from_value(json!({"action":"inspect","task_id":task_id})).unwrap(),
        )
        .unwrap()
    };
    assert_eq!(inspect()["spec"]["agent_type"], "gpt-6-sol");
    let stages = ["contract", "build", "review", "deliver"];
    let mut candidate_commit = None;
    let mut delivery_commit = None;

    for (index, stage) in stages.iter().enumerate() {
        let before = inspect();
        assert_eq!(before["workflow_guidance"]["active_stage"], *stage);
        assert_eq!(before["workflow_guidance"]["agent_type"], "gpt-6-sol");
        assert!(before["workflow_guidance"]["steps"]
            .as_array()
            .is_some_and(|steps| !steps.is_empty()));
        let worker = if index >= 2 {
            "independent-reviewer"
        } else {
            "builder"
        };
        if index == stages.len() - 1 {
            let previous_name = tasks::claim(
                &root.0,
                tasks::Claim {
                    task_id: task_id.clone(),
                    stage: "handoff/v1".into(),
                    worker_id: worker.into(),
                    worktree: root.0.to_string_lossy().into_owned(),
                    ..Default::default()
                },
            )
            .unwrap_err();
            assert!(previous_name.to_string().contains("TASK_STAGE_INVALID"));
            assert_eq!(inspect()["status"], "ready");
        }
        let claimed = tasks::claim(
            &root.0,
            tasks::Claim {
                task_id: task_id.clone(),
                stage: (*stage).into(),
                worker_id: worker.into(),
                worktree: root.0.to_string_lossy().into_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            claimed["workflow_guidance"]["active_stage"],
            before["workflow_guidance"]["active_stage"]
        );
        assert_eq!(
            claimed["workflow_guidance"]["agent_type"],
            before["workflow_guidance"]["agent_type"]
        );
        assert!(claimed["workflow_guidance"]["blackboard_messages"].is_array());
        if index == 2 {
            blackboard(&[
                "post",
                &task_id,
                "--author",
                "architect",
                "--topic",
                "decisions",
                "--payload",
                "Keep task context in the milestone receipt.",
            ]);
            blackboard(&[
                "post",
                &task_id,
                "--author",
                "builder",
                "--topic",
                "notes",
                "--payload",
                "The direct JSON proof passed SQLite verification.",
            ]);
            let messages = blackboard(&["read", &task_id]);
            assert_eq!(messages["messages"].as_array().unwrap().len(), 2);
            assert_eq!(messages["messages"][0]["author"], "builder");
            assert_eq!(messages["messages"][1]["author"], "architect");
        }
        let stored = tasks::store(&root.0).unwrap().inspect(&task_id).unwrap();
        if index == stages.len() - 1 {
            let mut previous_proof = evidence(&stored);
            previous_proof.stage = "handoff/v1".into();
            let previous_submit: tasks::Submit = serde_json::from_value(json!({
                "task_id": task_id,
                "stage": "handoff/v1",
                "action": "pass",
                "evidence": serde_json::to_value(previous_proof).unwrap()
            }))
            .unwrap();
            let rejected = tasks::submit(&root.0, previous_submit).unwrap_err();
            assert!(rejected.to_string().contains("TASK_STAGE_UNKNOWN"));
            assert_eq!(inspect()["status"], "in_progress");
        }
        let mut submitted_evidence = evidence(&stored);
        submitted_evidence.commit = None;
        let submitted = tasks::submit(
            &root.0,
            serde_json::from_value(json!({
                "task_id":task_id,
                "stage":stage,
                "action":"pass",
                "evidence":serde_json::to_value(submitted_evidence).unwrap()
            }))
            .unwrap(),
        )
        .unwrap();
        if index == 1 {
            candidate_commit = Some(pinned_candidate(&root.0, &task_id));
        }
        if let Some(candidate) = candidate_commit.as_deref() {
            if matches!(index, 1 | 2) {
                let short_candidate = &candidate[..7];
                assert_eq!(submitted["snapshot"]["commit"], short_candidate);
                assert_eq!(
                    submitted["snapshot"]["inspect_cmd"],
                    format!("git show {short_candidate}")
                );
            }
        }
        if index == stages.len() - 1 {
            assert_eq!(submitted["status"], "completed");
            let landed = git_head(&root.0);
            let short_landed = &landed[..7];
            assert_eq!(submitted["snapshot"]["commit"], short_landed);
            assert_eq!(submitted["receipt"]["commit"], short_landed);
            delivery_commit = Some(landed);
        }
    }

    let details = tasks::store(&root.0)
        .unwrap()
        .inspect_details(&task_id)
        .unwrap();
    let candidate = candidate_commit.as_deref().unwrap();
    let short_candidate = &candidate[..7];
    assert_eq!(details["latest_snapshot"]["commit"], short_candidate);
    assert_eq!(
        details["latest_snapshot"]["inspect_cmd"],
        format!("git show {short_candidate}")
    );
    let delivery = delivery_commit.as_deref().unwrap();
    assert_ne!(candidate, delivery);

    let written = std::fs::read_to_string(root.0.join(milestone_ref)).unwrap();
    let parsed = Milestone::parse(&written, "forge-mcp").unwrap();
    let completed = &parsed.tasks[0];
    assert_eq!(completed.status.as_deref(), Some("completed"));
    assert_eq!(completed.agent_type.as_deref(), Some("gpt-6-sol"));
    let receipt = completed.receipt.as_ref().unwrap();
    assert!(receipt.commit.is_none());
    let sqlite_receipt = tasks::store(&root.0)
        .unwrap()
        .inspect(&task_id)
        .unwrap()
        .receipt
        .unwrap();
    assert_eq!(sqlite_receipt.commit.as_deref(), Some(delivery));
    let mut sqlite_receipt = serde_json::to_value(sqlite_receipt).unwrap();
    sqlite_receipt.as_object_mut().unwrap().remove("commit");
    assert_eq!(serde_json::to_value(receipt).unwrap(), sqlite_receipt);
    assert!(receipt.evidence.get("command_proof").is_some());
    assert!(receipt.review.get("review_proof").is_some());
    let rollup = receipt.rollup.as_ref().unwrap();
    assert_eq!(rollup.verified_invariants, ["durable-context", "reviewed"]);
    assert_eq!(
        rollup.architectural_notes,
        ["decisions: Keep task context in the milestone receipt."]
    );
    assert_eq!(rollup.review_summary.decision, "pass");
    assert!(blackboard(&["read", &task_id])["messages"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        tasks::store(&root.0)
            .unwrap()
            .inspect(&task_id)
            .unwrap()
            .status,
        "completed"
    );
}

#[test]
fn milestone_and_plan_directories_configured_and_excluded_from_scanner() {
    let root = ScopedWorkspace::new("forge_multi_milestones_and_plans");
    root.write("src/lib.rs", "pub fn platform() {}\n");
    root.write(
        "docs/architecture.md",
        "# Platform Architecture\nGeneral documentation.\n",
    );
    root.write(
        "docs/milestones/README.md",
        "# Milestone Index\nrootmilestoneoverviewtoken\n",
    );
    root.write(
        "docs/milestones/archive/README.md",
        "archivemilestoneoverviewtoken\n",
    );
    root.write(
        "docs/plans/README.md",
        "# Plan Index\nrootplanoverviewtoken\n",
    );
    root.write("docs/plans/archive/README.md", "archiveplanoverviewtoken\n");
    root.write("docs/plans/platform_plan.md", "---\nid: p-platform\ntitle: Platform Plan\ndoc_type: plan\npurpose: Platform\n---\n# Platform Plan\nrootplancontractisolationtoken\n");
    root.write("docs/milestones/010-platform.md", "---\nid: m-platform\ntitle: Platform Milestone\ndoc_type: contract\nstatus: active\n---\n# Platform Milestone\nrootmilestonecontractisolationtoken\n### task: ptask\n```yaml\ntask_ref: ptask\ntarget: Deliver ptask\nproof_policy: direct-proof\nscope: [src/]\n```\n");
    root.write("extensions/commerce/src/models.py", "# Commerce models\n");
    root.write(
        "extensions/commerce/docs/README.md",
        "# Commerce Readme\nDocumentation for commerce.\n",
    );
    root.write(
        "extensions/commerce/docs/milestones/README.md",
        "# Commerce Milestones\ncommercemilestoneoverviewtoken\n",
    );
    root.write(
        "extensions/commerce/docs/plans/README.md",
        "# Commerce Plans\ncommerceplanoverviewtoken\n",
    );
    root.write("extensions/commerce/docs/plans/commerce_plan.md", "---\nid: p-commerce\ntitle: Commerce Plan\ndoc_type: plan\npurpose: Commerce\n---\n# Commerce Plan\ncommerceplancontractisolationtoken\n");
    root.write("extensions/commerce/docs/milestones/010-commerce.md", "---\nid: m-commerce\ntitle: Commerce Milestone\ndoc_type: contract\nstatus: active\nstarted_at: 2026-10-01T10:00:00Z\n---\n# Commerce Milestone\ncommercemilestonecontractisolationtoken\n### task: ctask\n```yaml\ntask_ref: ctask\ntarget: Deliver ctask\nproof_policy: direct-proof\nscope: [extensions/commerce/]\n```\n");

    root.write(
        "forge-mcp.yaml",
        "roots: [src, extensions]\ndocs: [docs]\nmilestones:\n  - docs/milestones\n  - extensions/*/docs/milestones\nplans:\n  - docs/plans\n  - extensions/*/docs/plans\ntasks_db: .forge/tasks.sqlite\n",
    );

    // 1. Verify scanner automatically excludes plans and milestones from code/doc index
    let scan = contextunity_forge_mcp::engine::scanner::scan(&root.0, None).unwrap();
    let scanned_paths: Vec<_> = scan.entries.iter().map(|e| e.path.as_str()).collect();

    assert!(scanned_paths.contains(&"src/lib.rs"));
    assert!(scanned_paths.contains(&"docs/architecture.md"));
    assert!(scanned_paths.contains(&"extensions/commerce/src/models.py"));
    assert!(scanned_paths.contains(&"extensions/commerce/docs/README.md"));
    assert!(scanned_paths.contains(&"docs/milestones/README.md"));
    assert!(scanned_paths.contains(&"docs/plans/README.md"));
    assert!(scanned_paths.contains(&"extensions/commerce/docs/milestones/README.md"));
    assert!(scanned_paths.contains(&"extensions/commerce/docs/plans/README.md"));

    assert!(!scanned_paths.contains(&"docs/plans/platform_plan.md"));
    assert!(!scanned_paths.contains(&"docs/milestones/010-platform.md"));
    assert!(!scanned_paths.contains(&"docs/plans/archive/README.md"));
    assert!(!scanned_paths.contains(&"docs/milestones/archive/README.md"));
    assert!(!scanned_paths.contains(&"extensions/commerce/docs/plans/commerce_plan.md"));
    assert!(!scanned_paths.contains(&"extensions/commerce/docs/milestones/010-commerce.md"));

    // Overview README files reach doc_search through the public cold-build and reader path,
    // while individual contracts and nested archive content remain isolated.
    let db_path = root.0.join("code_map.sqlite");
    writer::build(&root.0, &db_path, None).expect("cold build failed");
    let conn =
        contextunity_forge_mcp::db::reader::open(&db_path, &root.0).expect("reader open failed");
    let page = contextunity_forge_mcp::core::response::QueryOptions::resolve(
        &contextunity_forge_mcp::core::response::ResponsePolicy::default(),
        Some(10),
        0,
        None,
        None,
    )
    .unwrap();
    let search_doc_paths = |query: &str| {
        contextunity_forge_mcp::db::reader::search_docs_with_options(
            &conn,
            query,
            &contextunity_forge_mcp::db::reader::DocSearchOptions {
                doc_type: None,
                component: None,
                include_excerpt: false,
                page: &page,
            },
        )
        .unwrap()["sections"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["path"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert!(search_doc_paths("rootmilestoneoverviewtoken")
        .contains(&"docs/milestones/README.md".to_owned()));
    assert!(search_doc_paths("rootplanoverviewtoken").contains(&"docs/plans/README.md".to_owned()));
    assert!(search_doc_paths("commercemilestoneoverviewtoken")
        .contains(&"extensions/commerce/docs/milestones/README.md".to_owned()));
    assert!(search_doc_paths("commerceplanoverviewtoken")
        .contains(&"extensions/commerce/docs/plans/README.md".to_owned()));
    assert!(search_doc_paths("rootmilestonecontractisolationtoken").is_empty());
    assert!(search_doc_paths("rootplancontractisolationtoken").is_empty());
    assert!(search_doc_paths("commercemilestonecontractisolationtoken").is_empty());
    assert!(search_doc_paths("commerceplancontractisolationtoken").is_empty());
    assert!(search_doc_paths("archivemilestoneoverviewtoken").is_empty());
    assert!(search_doc_paths("archiveplanoverviewtoken").is_empty());

    // 2. Verify milestones::list discovers milestones across all configured milestone directories
    let listed = contextunity_forge_mcp::engine::milestones::list(&root.0, false, None).unwrap();
    let milestones = listed["milestones"].as_array().unwrap();
    assert_eq!(milestones.len(), 2);
    let ids: Vec<_> = milestones
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"m-platform"));
    assert!(ids.contains(&"m-commerce"));

    // 3. Verify milestones::show finds milestone in extension directory
    let show_commerce =
        contextunity_forge_mcp::engine::milestones::show(&root.0, "m-commerce", false).unwrap();
    assert_eq!(show_commerce["id"], "m-commerce");
    assert_eq!(show_commerce["title"], "Commerce Milestone");

    // 4. Verify milestones::init with --plan automatically infers extension milestone directory
    let init_result = contextunity_forge_mcp::engine::milestones::init(
        &root.0,
        contextunity_forge_mcp::engine::milestones::Init {
            num: None,
            slug: Some("commerce-v2".into()),
            title: None,
            plan: Some("extensions/commerce/docs/plans/commerce_plan.md".into()),
            dir: None,
            desc: None,
            depends_on: vec![],
            active: false,
            stdin: String::new(),
        },
    )
    .unwrap();
    assert_eq!(init_result["id"], "m-commerce-v2");
    assert_eq!(
        init_result["path"],
        "extensions/commerce/docs/milestones/020-commerce-v2.md"
    );
    let init_v2_content = std::fs::read_to_string(
        root.0
            .join("extensions/commerce/docs/milestones/020-commerce-v2.md"),
    )
    .unwrap();
    assert!(init_v2_content.contains("project: extensions.commerce"));

    // 5. Verify task sync on extension milestone creates task in subproject namespace
    let synced = contextunity_forge_mcp::engine::tasks::manage(
        &root.0,
        serde_json::from_value(json!({
            "action": "sync",
            "milestone_ref": "extensions/commerce/docs/milestones/010-commerce.md"
        }))
        .unwrap(),
    )
    .unwrap();
    let tasks_array = synced["tasks"].as_array().unwrap();
    assert_eq!(tasks_array.len(), 1);
    let task_id = tasks_array[0]["task_id"].as_str().unwrap();
    assert!(task_id.contains("/extensions.commerce/"));

    // Verify milestones::list now shows task as synced (ready, not unsynced)
    let listed_after_sync =
        contextunity_forge_mcp::engine::milestones::list(&root.0, false, None).unwrap();
    let commerce_m = listed_after_sync["milestones"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "m-commerce")
        .unwrap();
    assert_eq!(commerce_m["tasks"][0]["status"], "ready");

    // Complete the task and verify handoff succeeds and archives into extension's archive
    let store = TasksStore::open_project(
        &root.0.join(".forge/tasks.sqlite"),
        "forge-mcp",
        "extensions.commerce",
    )
    .unwrap();
    let task = store.list(None, "all", None).unwrap().pop().unwrap();
    complete_task_through_engine(&root, &store, &task.task_id);
    drop(store);

    let commit = git_head(&root.0);
    let handoff_res = contextunity_forge_mcp::engine::milestones::handoff(
        &root.0,
        "m-commerce",
        Some(&commit),
        "pytest extensions/commerce/tests",
        5,
        0,
    )
    .unwrap();
    assert_eq!(handoff_res["id"], "m-commerce");
    assert!(root
        .0
        .join("extensions/commerce/docs/milestones/archive/010-commerce.md")
        .exists());
    assert!(!root
        .0
        .join("extensions/commerce/docs/milestones/010-commerce.md")
        .exists());

    // 6. Verify init with --dir rejects path escaping workspace or not in configured milestones
    let escaping_init = contextunity_forge_mcp::engine::milestones::init(
        &root.0,
        contextunity_forge_mcp::engine::milestones::Init {
            num: None,
            slug: Some("escape".into()),
            title: Some("Escape".into()),
            plan: None,
            dir: Some("../outside".into()),
            desc: None,
            depends_on: vec![],
            active: false,
            stdin: String::new(),
        },
    );
    assert!(escaping_init.is_err());

    let unconfigured_init = contextunity_forge_mcp::engine::milestones::init(
        &root.0,
        contextunity_forge_mcp::engine::milestones::Init {
            num: None,
            slug: Some("unconfigured".into()),
            title: Some("Unconfigured".into()),
            plan: None,
            dir: Some("unconfigured/milestones".into()),
            desc: None,
            depends_on: vec![],
            active: false,
            stdin: String::new(),
        },
    );
    assert!(unconfigured_init.is_err());
}

#[test]
fn milestone_resolution_prefers_active_path_components_and_propagates_list_errors() {
    let root = ScopedWorkspace::new("forge_milestone_archive_precedence");
    root.write("AGENTS.md", "# Rules\n");
    root.write(
        "forge-mcp.yaml",
        "task_repository: test\ntask_project: test\nroots: []\n",
    );

    let active_spec = "---\nid: m-active-archive-fix\ntitle: Active Archive Fix\ndoc_type: contract\nstatus: active\n---\n```yaml\ntask_ref: active-match\ntarget: Active manifest\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    let archived_spec = "---\nid: m-archived-archive-fix\ntitle: Archived Archive Fix\ndoc_type: contract\nstatus: completed\n---\n```yaml\ntask_ref: archived-match\ntarget: Archived manifest\nproof_policy: direct-proof\nscope: [src/]\n```\n";
    root.write("docs/milestones/051-archive-fix.md", active_spec);
    root.write("docs/milestones/archive/051-archive-fix.md", archived_spec);

    let by_stem = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("051-archive-fix".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let by_stem_tasks = by_stem["tasks"].as_array().unwrap();
    assert_eq!(by_stem_tasks.len(), 1);
    assert_eq!(
        by_stem_tasks[0]["milestone_ref"],
        "docs/milestones/051-archive-fix.md"
    );

    let by_archive_path = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some("docs/milestones/archive/051-archive-fix.md".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let by_archive_tasks = by_archive_path["tasks"].as_array().unwrap();
    assert_eq!(by_archive_tasks.len(), 1);
    assert_eq!(
        by_archive_tasks[0]["milestone_ref"],
        "docs/milestones/archive/051-archive-fix.md"
    );

    let unscoped_sync = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            ..Default::default()
        },
    );
    assert!(
        unscoped_sync.is_err(),
        "sync requires a workspace or milestone selector"
    );

    root.write("docs/milestones/README.md", "# Milestone index\n");
    let non_manifest_params = serde_json::from_value(serde_json::json!({
        "repository": "test",
        "milestone_ref": "docs/milestones/README.md",
        "status": "all"
    }))
    .unwrap();
    assert!(tasks::list(&root.0, non_manifest_params).is_err());

    let outside = ScopedWorkspace::new("forge_milestone_outside");
    outside.write("010-outside.md", active_spec);
    std::os::unix::fs::symlink(
        outside.0.join("010-outside.md"),
        root.0.join("docs/milestones/053-symlink.md"),
    )
    .unwrap();
    for reference in ["../outside.md", "docs/milestones/053-symlink.md"] {
        let params = serde_json::from_value(serde_json::json!({
            "repository": "test",
            "milestone_ref": reference,
            "status": "all"
        }))
        .unwrap();
        assert!(
            tasks::list(&root.0, params).is_err(),
            "milestone path must remain confined to the workspace: {reference}"
        );
    }

    let list_params = serde_json::from_value(serde_json::json!({
        "repository": "test",
        "milestone_ref": "m-does-not-exist",
        "status": "all",
        "stage": null
    }))
    .unwrap();
    let list_error = tasks::list(&root.0, list_params).unwrap_err();
    assert!(
        list_error.to_string().contains("not found in workspace"),
        "task_list must propagate milestone resolution errors: {list_error}"
    );

    let malformed_spec = "---\nid: m-invalid-frontmatter\ntitle: Invalid frontmatter\ndoc_type: contract\nbroken: [\n---\n";
    let malformed_path = "docs/milestones/nested/052-invalid-frontmatter.md";
    root.write(malformed_path, malformed_spec);

    let malformed_id_params = serde_json::from_value(serde_json::json!({
        "repository": "test",
        "milestone_ref": "m-does-not-exist",
        "status": "all"
    }))
    .unwrap();
    let malformed_id_error = tasks::list(&root.0, malformed_id_params).unwrap_err();
    assert!(
        malformed_id_error.to_string().contains(
            "failed to read milestone id from 'docs/milestones/nested/052-invalid-frontmatter.md'"
        ),
        "ID lookup must preserve manifest parse errors: {malformed_id_error}"
    );

    let malformed_list_params = serde_json::from_value(serde_json::json!({
        "repository": "test",
        "milestone_ref": malformed_path,
        "status": "all"
    }))
    .unwrap();
    let malformed_list_error = tasks::list(&root.0, malformed_list_params).unwrap_err();
    assert!(
        malformed_list_error
            .to_string()
            .contains(&format!("invalid milestone manifest '{malformed_path}'")),
        "task_list must propagate malformed milestone frontmatter: {malformed_list_error}"
    );

    let malformed_sync = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::Sync,
            milestone_ref: Some(malformed_path.into()),
            ..Default::default()
        },
    );
    assert!(
        malformed_sync
            .unwrap_err()
            .to_string()
            .contains(&format!("invalid milestone manifest '{malformed_path}'")),
        "task_manage sync must reject malformed milestone frontmatter"
    );
}

#[test]
fn milestone_lifecycle_sync_validates_status_and_preserves_cancellation_and_subtask_state() {
    let root = ScopedWorkspace::new("forge_milestone_lifecycle_sync");
    root.write("src/lib.rs", "pub fn lifecycle() {}\n");
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
    );
    let database_path = tasks::database_path(&root.0).unwrap();
    std::fs::create_dir_all(database_path.parent().unwrap()).unwrap();
    let initial_store = TasksStore::open(&database_path).unwrap();
    initial_store
        .connection
        .execute_batch(
            "DROP TABLE task_blackboard;
             CREATE TABLE task_blackboard(
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 milestone_ref TEXT NOT NULL,
                 task_id TEXT REFERENCES tasks ON DELETE CASCADE,
                 subtask_ref TEXT,
                 author TEXT NOT NULL,
                 topic TEXT NOT NULL,
                 payload TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             CREATE INDEX idx_task_blackboard_task_created ON task_blackboard(task_id, created_at);
             UPDATE task_store_metadata SET value='1' WHERE key='schema_version';",
        )
        .unwrap();
    drop(initial_store);

    let manifest = |id: &str,
                    status: Option<&str>,
                    closure_reason: Option<&str>,
                    task_ref: &str,
                    revision: u64,
                    target: &str,
                    subtask_ref: &str| {
        let status = status
            .map(|value| format!("status: {value}\n"))
            .unwrap_or_default();
        let closure = match (status.as_str(), closure_reason) {
            (_, Some(reason)) => format!("closure:\n  reason: \"{reason}\"\n"),
            ("status: cancelled\n", None) => "closure:\n  owner: platform\n".into(),
            _ => String::new(),
        };
        format!(
            "---\nid: {id}\ntitle: {id}\ndoc_type: contract\n{status}{closure}---\n# Tasks\n```yaml\ntask_ref: {task_ref}\ntarget: {target}\nproof_policy: seam-test-first\ncontract_revision: {revision}\nscope: [src/]\nsubtasks:\n  - subtask_ref: {subtask_ref}\n    title: {subtask_ref}\n    status: pending\n```\n"
        )
    };
    let sync = |path: &str| {
        tasks::manage(
            &root.0,
            tasks::Manage {
                action: tasks::ManageAction::Sync,
                milestone_ref: Some(path.to_owned()),
                ..Default::default()
            },
        )
    };

    let crlf_path = "docs/milestones/015-crlf-activation.md";
    let crlf_id = "forge-mcp/forge-mcp/m-crlf-activation:activate";
    let crlf_manifest = manifest(
        "m-crlf-activation",
        Some("planned"),
        None,
        "activate",
        1,
        "Activate CRLF milestone on claim",
        "activate-subtask",
    )
    .replace('\n', "\r\n");
    root.write(crlf_path, &crlf_manifest);
    sync(crlf_path).unwrap();
    tasks::claim(
        &root.0,
        tasks::Claim {
            task_id: crlf_id.into(),
            stage: "contract".into(),
            worker_id: "crlf-lifecycle-test".into(),
            worktree: root.0.to_string_lossy().into_owned(),
            bundle: None,
        },
    )
    .unwrap();
    let crlf_activated =
        contextunity_forge_mcp::engine::milestones::show(&root.0, "m-crlf-activation", true)
            .unwrap();
    assert_eq!(crlf_activated["status"], "active");
    assert!(crlf_activated["started_at"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));

    let default_path = "docs/milestones/010-default.md";
    let default_spec = manifest(
        "m-default",
        None,
        None,
        "remaining",
        1,
        "Keep remaining work",
        "preserve-progress",
    );
    root.write(default_path, &default_spec);
    let default_sync = sync(default_path).unwrap();
    assert_eq!(default_sync["tasks"].as_array().unwrap().len(), 1);
    let default_view =
        contextunity_forge_mcp::engine::milestones::show(&root.0, "m-default", false).unwrap();
    assert_eq!(default_view["status"], "planned");

    let invalid_path = "docs/milestones/001-invalid-status.md";
    root.write(
        invalid_path,
        &manifest(
            "m-invalid-status",
            Some("deferred"),
            None,
            "invalid",
            1,
            "Reject invalid lifecycle status",
            "invalid-subtask",
        ),
    );
    let invalid_status = sync(invalid_path);
    assert!(
        invalid_status.is_err(),
        "milestone sync must reject statuses outside active, planned, completed, and cancelled: {invalid_status:?}"
    );

    let archived_default_path = "docs/milestones/archive/011-archived-default.md";
    root.write(
        archived_default_path,
        &manifest(
            "m-archived-default",
            None,
            None,
            "archived-default",
            1,
            "Default archived milestone",
            "archived-subtask",
        ),
    );
    sync(archived_default_path).unwrap();
    let archived_default_view =
        contextunity_forge_mcp::engine::milestones::show(&root.0, "m-archived-default", true)
            .unwrap();
    assert_eq!(archived_default_view["status"], "completed");

    for (id, status, path) in [
        ("m-planned", "planned", "docs/milestones/020-planned.md"),
        (
            "m-completed",
            "completed",
            "docs/milestones/archive/030-completed.md",
        ),
    ] {
        root.write(
            path,
            &manifest(
                id,
                Some(status),
                None,
                status,
                1,
                "Accept a supported lifecycle status",
                "status-subtask",
            ),
        );
        assert_eq!(sync(path).unwrap()["tasks"].as_array().unwrap().len(), 1);
    }

    let cancelled_path = "docs/milestones/040-cancelled.md";
    let cancelled_id = "forge-mcp/forge-mcp/m-cancelled:retired";
    root.write(
        cancelled_path,
        &manifest(
            "m-cancelled",
            Some("active"),
            None,
            "retired",
            1,
            "Retire this task",
            "retired-subtask",
        ),
    );
    assert_eq!(
        sync(cancelled_path).unwrap()["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let remaining_id = "forge-mcp/forge-mcp/m-default:remaining";
    let store = TasksStore::open(&database_path).unwrap();
    let progress = tasks::manage(
        &root.0,
        tasks::Manage {
            action: tasks::ManageAction::SubtaskUpdate,
            task_id: Some(remaining_id.into()),
            subtask_ref: Some("preserve-progress".into()),
            subtask_status: Some("in_progress".into()),
            evidence: Some("verified retained work".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(progress["subtask"]["status"], "in_progress");

    root.write(
        default_path,
        &manifest(
            "m-default",
            None,
            None,
            "remaining",
            2,
            "Keep remaining work after revision",
            "preserve-progress",
        ),
    );
    sync(default_path).unwrap();
    let retained = store
        .subtask_list(remaining_id)
        .unwrap()
        .into_iter()
        .find(|subtask| subtask.subtask_ref == "preserve-progress")
        .unwrap();
    assert_eq!(retained.status, "in_progress");
    assert_eq!(retained.evidence.as_deref(), Some("verified retained work"));

    store
        .connection
        .execute(
            "INSERT INTO task_dependencies(task_id, dependency_id, satisfied) VALUES (?1, ?2, 0)",
            rusqlite::params![remaining_id, cancelled_id],
        )
        .unwrap();
    store
        .connection
        .execute(
            "INSERT INTO task_dependencies(task_id, dependency_id, satisfied) VALUES (?1, ?2, 0)",
            rusqlite::params![cancelled_id, remaining_id],
        )
        .unwrap();
    let cancelled_scope_ref = store.milestone_scope_ref(cancelled_path).unwrap();
    store
        .connection
        .execute(
            "INSERT INTO task_blackboard(milestone_ref, task_id, subtask_ref, author, topic, payload, created_at) \
             VALUES(?1, ?2, NULL, 'reviewer', 'decisions', 'task context', 1)",
            rusqlite::params![cancelled_scope_ref, cancelled_id],
        )
        .unwrap();
    store
        .connection
        .execute(
            "INSERT INTO task_blackboard(milestone_ref, task_id, subtask_ref, author, topic, payload, created_at) \
             VALUES(?1, NULL, NULL, 'reviewer', 'decisions', 'milestone context', 2)",
            [&cancelled_scope_ref],
        )
        .unwrap();

    root.write(
        cancelled_path,
        &manifest(
            "m-cancelled",
            Some("cancelled"),
            None,
            "retired",
            1,
            "Retire this task",
            "retired-subtask",
        ),
    );
    let missing_reason = sync(cancelled_path);
    assert!(
        missing_reason.is_err(),
        "cancelled milestone sync must require closure.reason: {missing_reason:?}"
    );
    assert!(store.inspect(cancelled_id).is_ok());
    assert_eq!(
        store
            .blackboard_read(cancelled_id, None, None)
            .unwrap()
            .len(),
        1
    );

    root.write(
        cancelled_path,
        &manifest(
            "m-cancelled",
            Some("cancelled"),
            Some("   "),
            "retired",
            1,
            "Retire this task",
            "retired-subtask",
        ),
    );
    let blank_reason = sync(cancelled_path);
    assert!(
        blank_reason.is_err(),
        "cancelled milestone sync must reject a blank closure.reason: {blank_reason:?}"
    );
    assert!(store.inspect(cancelled_id).is_ok());

    root.write(
        cancelled_path,
        &manifest(
            "m-cancelled",
            Some("cancelled"),
            Some("Replaced by the remaining milestone"),
            "retired",
            1,
            "Retire this task",
            "retired-subtask",
        ),
    );
    let cancelled_sync = sync(cancelled_path).unwrap();
    assert!(cancelled_sync["tasks"].as_array().unwrap().is_empty());
    assert!(store.inspect(cancelled_id).is_err());
    assert!(store
        .blackboard_read(cancelled_id, None, None)
        .unwrap()
        .is_empty());

    let incoming_satisfied: i64 = store
        .connection
        .query_row(
            "SELECT satisfied FROM task_dependencies WHERE task_id=?1 AND dependency_id=?2",
            rusqlite::params![remaining_id, cancelled_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(incoming_satisfied, 0);
    let cancelled_blackboard_rows: i64 = store
        .connection
        .query_row(
            "SELECT count(*) FROM task_blackboard WHERE milestone_ref=?1",
            [&cancelled_scope_ref],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(cancelled_blackboard_rows, 0);
    let outgoing_count: i64 = store
        .connection
        .query_row(
            "SELECT count(*) FROM task_dependencies WHERE task_id=?1",
            [cancelled_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(outgoing_count, 0);
    let remaining = store
        .list(None, "all", None)
        .unwrap()
        .into_iter()
        .find(|task| task.task_id == remaining_id)
        .unwrap();
    assert_eq!(remaining.status, "blocked");
}
