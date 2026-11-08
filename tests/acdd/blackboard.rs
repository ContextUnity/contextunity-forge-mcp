use super::support::*;

#[test]
fn blackboard_persists_task_scoped_messages_in_chronological_order() {
    let milestone = Milestone::parse(SPEC, "forge-mcp").unwrap();
    let mut store = TasksStore::open_in_memory("forge-mcp", "forge-mcp").unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/acdd");
    assert!(!root.join("forge-mcp.yaml").exists());
    store.sync(&milestone, "docs/010-test.md", &root).unwrap();
    let first = milestone.task_id(&milestone.tasks[0]);
    let second = milestone.task_id(&milestone.tasks[1]);
    let legacy_topic_error = store
        .blackboard_post(&first, "author-legacy", "contract_draft", "must use draft")
        .unwrap_err();
    assert!(legacy_topic_error
        .to_string()
        .contains("draft, notes, findings, blockers, decisions, deferred"));
    let first_id = store
        .blackboard_post(&first, "author-a", "draft", "red test")
        .unwrap();
    let second_id = store
        .blackboard_post(&first, "author-b", "decisions", "use WAL")
        .unwrap();
    store
        .blackboard_post(&second, "author-c", "draft", "other task")
        .unwrap();
    assert!(second_id > first_id);
    let all = store.blackboard_read(&first, None, None).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, first_id);
    assert_eq!(all[0].author, "author-a");
    assert_eq!(all[1].id, second_id);
    assert!(all[0].created_at <= all[1].created_at);
    assert_eq!(
        store
            .blackboard_read(&first, Some("decisions"), Some(1))
            .unwrap()[0]
            .payload
            .as_deref(),
        Some("use WAL")
    );
    assert_eq!(store.blackboard_clear(&first).unwrap(), 2);
    assert!(store
        .blackboard_read(&first, None, None)
        .unwrap()
        .is_empty());
    assert_eq!(store.blackboard_read(&second, None, None).unwrap().len(), 1);
    let index: String = store.connection.query_row(
        "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='task_blackboard' AND sql LIKE '%task_id, created_at%'",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(index, "idx_task_blackboard_task_created");
}
#[test]
fn blackboard_scope_resolution_isolates_milestone_task_and_subtask_messages() {
    use contextunity_forge_mcp::engine::tasks as engine_tasks;

    let root = crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
    let manifest = "---\nid: m-blackboard\ntitle: Blackboard\ndoc_type: contract\nstatus: active\n---\n# Blackboard\n```yaml\ntask_ref: first\ntarget: Coordinate task agents\nproof_policy: seam-test-first\nscope: [src/]\n```\n```yaml\ntask_ref: second\ntarget: Coordinate another task\nproof_policy: seam-test-first\nscope: [src/]\n```\n";
    root.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n");
    let manifest = manifest.replace(
        "task_ref: second\ntarget: Coordinate another task\nproof_policy: seam-test-first\nscope: [src/]",
        "task_ref: second\ntarget: Coordinate another task\nproof_policy: seam-test-first\nscope: [docs/]",
    );
    root.write("docs/milestones/010-blackboard.md", &manifest);
    let milestone = Milestone::parse(&manifest, "forge-mcp").unwrap();
    let (mut store, first, second) =
        sync_blackboard_tasks(&root, "docs/milestones/010-blackboard.md", &milestone);
    store
        .claim(
            &first,
            "contract",
            "worker-one",
            root.root().to_str().unwrap(),
            Some("test-session-worker-one"),
        )
        .unwrap();
    store
        .subtask_add(&first, "active-slice", "Resolve the active subtask")
        .unwrap();
    store
        .subtask_update(&first, "active-slice", "in_progress", None)
        .unwrap();

    let call =
        |args| engine_tasks::blackboard(root.root(), serde_json::from_value(args).unwrap(), "test");
    let milestone_post = call(json!({
        "action":"post","scope":"milestone",
        "milestone_ref":"docs/milestones/010-blackboard.md",
        "topic":"decisions","payload":"milestone coordination"
    }))
    .unwrap();
    let milestone_message_id = milestone_post["id"].as_u64().unwrap();
    let task_post = call(json!({
        "action":"post","task_id":first,
        "topic":"notes","payload":"task coordination"
    }))
    .unwrap();
    let task_message_id = task_post["id"].as_u64().unwrap();
    let subtask_post = call(json!({
        "action":"post","scope":"subtask",
        "topic":"findings","payload":"subtask coordination"
    }))
    .unwrap();
    let subtask_message_id = subtask_post["id"].as_u64().unwrap();

    let milestone_page = call(json!({
        "action":"read","scope":"milestone",
        "milestone_ref":"docs/milestones/010-blackboard.md"
    }))
    .unwrap();
    assert_eq!(milestone_page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(milestone_page["messages"][0]["id"], milestone_message_id);
    assert_eq!(milestone_page["messages"][0]["task_id"], Value::Null);
    assert!(milestone_page["messages"][0].get("payload").is_none());

    let task_page = call(json!({"action":"read","limit":100})).unwrap();
    assert_eq!(task_page["pagination"]["limit"], 50);
    assert_eq!(task_page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(task_page["messages"][0]["id"], task_message_id);
    assert_eq!(task_page["messages"][0]["task_id"], first);
    assert_eq!(task_page["messages"][0]["subtask_ref"], Value::Null);
    assert!(task_page["messages"][0].get("payload").is_none());

    let subtask_page = call(json!({"action":"read","scope":"subtask"})).unwrap();
    assert_eq!(subtask_page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(subtask_page["messages"][0]["id"], subtask_message_id);
    assert_eq!(subtask_page["messages"][0]["task_id"], first);
    assert_eq!(subtask_page["messages"][0]["subtask_ref"], "active-slice");

    let inspected = call(json!({"action":"inspect","message_id":subtask_message_id})).unwrap();
    assert_eq!(inspected["message"]["payload"], "subtask coordination");

    store
        .claim(
            &second,
            "contract",
            "worker-two",
            root.root().to_str().unwrap(),
            Some("test-session-worker-two"),
        )
        .unwrap();
    let ambiguous = call(json!({"action":"read","scope":"task"})).unwrap_err();
    assert!(ambiguous
        .to_string()
        .contains("TASK_BLACKBOARD_AMBIGUOUS_TASK"));
    let still_isolated = call(json!({
        "action":"read","scope":"milestone",
        "milestone_ref":"docs/milestones/010-blackboard.md"
    }))
    .unwrap();
    assert_eq!(still_isolated["messages"][0]["id"], milestone_message_id);

    let snapshot = || {
        let mut statement = store
            .connection
            .prepare(
                "SELECT id, milestone_ref, task_id, subtask_ref FROM task_blackboard ORDER BY id",
            )
            .unwrap();
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
    };
    let before_fallback = snapshot();
    let canonical_milestone = store
        .milestone_scope_ref("docs/milestones/010-blackboard.md")
        .unwrap();
    let task_scope_does_not_fallback = call(json!({
        "action":"read",
        "scope":"task",
        "milestone_ref":"first"
    }))
    .unwrap_err()
    .to_string();
    assert!(
        task_scope_does_not_fallback.contains("not found"),
        "task-scoped milestone filters remain direct milestone selectors: {task_scope_does_not_fallback}"
    );
    assert_eq!(snapshot(), before_fallback);
    for action in ["read", "inspect"] {
        for reference in [first.as_str(), "first"] {
            let resolved = call(json!({
                "action":action,
                "scope":"milestone",
                "milestone_ref":reference
            }))
            .unwrap();
            assert_eq!(resolved["messages"], milestone_page["messages"]);
            assert_eq!(resolved["messages"][0]["id"], milestone_message_id);
            assert_eq!(resolved["resolution"]["kind"], "task_parent");
            assert_eq!(resolved["resolution"]["input"], reference);
            assert_eq!(resolved["resolution"]["task_id"], first);
            assert_eq!(resolved["resolution"]["milestone_ref"], canonical_milestone);
            assert_eq!(
                snapshot(),
                before_fallback,
                "fallback {action} must not migrate, duplicate, or change scoped messages"
            );
        }
    }

    let fallback_post = call(json!({
        "action":"post",
        "scope":"milestone",
        "milestone_ref":"first",
        "topic":"notes",
        "payload":"fallback post goes to the canonical milestone scope"
    }))
    .unwrap();
    assert_eq!(fallback_post["resolution"]["kind"], "task_parent");
    assert_eq!(fallback_post["resolution"]["task_id"], first);
    let after_fallback_post = snapshot();
    assert_eq!(after_fallback_post.len(), before_fallback.len() + 1);
    assert_eq!(
        after_fallback_post.last().unwrap().0 as u64,
        fallback_post["id"]
    );
    assert_eq!(after_fallback_post.last().unwrap().1, canonical_milestone);
    assert_eq!(after_fallback_post.last().unwrap().2, None);
    assert_eq!(after_fallback_post.last().unwrap().3, None);
    assert_eq!(
        call(json!({
            "action":"read",
            "scope":"milestone",
            "milestone_ref":"docs/milestones/010-blackboard.md"
        }))
        .unwrap()["messages"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "one fallback post must add exactly one canonical-scope message"
    );
}

#[test]
fn blackboard_milestone_profile_pin_routes_gates_and_fails_closed_on_tampering() {
    use sha2::Digest;

    let root = crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
    root.write("src/lib.rs", "pub fn primary() {}\n");
    root.write("linked/src/lib.rs", "pub fn linked() {}\n");
    root.write(
        "linked/AGENTS.md",
        "# Linked instructions\nLoad the contextunity-forge skill.\n",
    );
    root.write(
        "linked/.agents/skills/contextunity-forge/SKILL.md",
        "# ContextUnity Forge\n",
    );
    root.write(
        "forge-mcp.yaml",
        "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\nlinked_workspaces:\n  - name: linked\n    path: linked\n    tasks:\n      enabled: true\n      milestones_dir: contracts\n      agents_guidance: AGENTS.md\n",
    );
    root.write(
        "linked/forge-mcp.yaml",
        "task_repository: linked-repository\ntask_project: linked-project\n",
    );

    let with_first_gate = |gate_id: &str| {
        include_str!("../../crates/forge-tasks/src/tasks/acdd.default.yaml")
            .replace("id: \"contract\"", &format!("id: \"{gate_id}\""))
            .replace("symbols: \"contract\"", &format!("symbols: \"{gate_id}\""))
    };
    let milestone_profile = with_first_gate("plan");
    let task_profile = with_first_gate("task_plan");
    let pinned_reference = |path: &str, contents: &str| {
        let digest = format!("{:x}", sha2::Sha256::digest(contents.as_bytes()));
        format!("{path}:{}", &digest[..12])
    };
    root.write("linked/profiles/milestone.yaml", &milestone_profile);
    root.write("linked/profiles/task.yaml", &task_profile);
    let milestone_pin = pinned_reference(
        "profiles/milestone.yaml",
        &std::fs::read_to_string(root.root().join("linked/profiles/milestone.yaml")).unwrap(),
    );
    let task_pin = pinned_reference(
        "profiles/task.yaml",
        &std::fs::read_to_string(root.root().join("linked/profiles/task.yaml")).unwrap(),
    );
    let manifest = format!(
        "---\nid: m-pinned-blackboard\ntitle: Pinned blackboard\ndoc_type: contract\nstatus: active\nacdd_profile: \"{milestone_pin}\"\n---\n# Pinned blackboard\n```yaml\ntask_ref: inherited\ntarget: Inherit the milestone profile\nproof_policy: seam-test-first\nscope: [src/]\n```\n```yaml\ntask_ref: override\ntarget: Use a task-level profile override\nacdd_profile: \"{task_pin}\"\nproof_policy: seam-test-first\nscope: [src/]\n```\n"
    );
    root.write("linked/contracts/010-pinned-blackboard.md", &manifest);
    let synced = tasks::manage(
        root.root(),
        serde_json::from_value(json!({"action":"sync","workspace":"linked"})).unwrap(),
    )
    .unwrap();
    let inherited_task = synced["tasks"][0]["task_id"].as_str().unwrap().to_owned();
    let override_task = synced["tasks"][1]["task_id"].as_str().unwrap().to_owned();
    let milestone_ref = "contracts/010-pinned-blackboard.md";
    let call = |arguments| {
        tasks::blackboard(
            root.root(),
            serde_json::from_value(arguments).unwrap(),
            "test",
        )
    };

    let milestone_post = call(json!({
        "action":"post","scope":"milestone","milestone_ref":milestone_ref,
        "gate":"plan","topic":"draft","payload":"pinned milestone gate"
    }))
    .unwrap();
    let milestone_id = milestone_post["id"].as_u64().unwrap();
    let default_gate_error = call(json!({
        "action":"post","scope":"milestone","milestone_ref":milestone_ref,
        "gate":"contract","topic":"draft","payload":"must reject default gate"
    }))
    .unwrap_err()
    .to_string();
    assert!(default_gate_error.contains("TASK_BLACKBOARD_SCHEMA_INVALID"));
    let milestone_read = call(json!({
        "action":"read","scope":"milestone","milestone_ref":milestone_ref,"gate":"plan"
    }))
    .unwrap();
    assert_eq!(milestone_read["messages"].as_array().unwrap().len(), 1);
    assert_eq!(milestone_read["messages"][0]["id"], milestone_id);
    let default_read_error = call(json!({
        "action":"read","scope":"milestone","milestone_ref":milestone_ref,"gate":"contract"
    }))
    .unwrap_err()
    .to_string();

    let inherited_post = call(json!({
        "action":"post","scope":"task","task_id":inherited_task,
        "gate":"plan","topic":"notes","payload":"inherited task gate"
    }))
    .unwrap();
    let inherited_id = inherited_post["id"].as_u64().unwrap();
    let inherited_read = call(json!({
        "action":"read","scope":"task","task_id":inherited_task,"gate":"plan"
    }))
    .unwrap();
    assert!(inherited_read["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["id"] == inherited_id));
    let inherited_default_error = call(json!({
        "action":"post","scope":"task","task_id":inherited_task,
        "gate":"contract","topic":"notes","payload":"must reject default gate"
    }))
    .unwrap_err()
    .to_string();
    for error in [
        &default_gate_error,
        &default_read_error,
        &inherited_default_error,
    ] {
        for expected in [
            "Active profile gates:",
            "plan",
            "build",
            "review",
            "deliver",
        ] {
            assert!(error.contains(expected), "missing {expected}: {error}");
        }
    }

    let override_post = call(json!({
        "action":"post","scope":"task","task_id":override_task,
        "gate":"task_plan","topic":"notes","payload":"task override gate"
    }))
    .unwrap();
    let override_id = override_post["id"].as_u64().unwrap();
    let override_read = call(json!({
        "action":"read","scope":"task","task_id":override_task,"gate":"task_plan"
    }))
    .unwrap();
    assert!(override_read["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["id"] == override_id));

    root.write(
        "linked/profiles/milestone.yaml",
        &milestone_profile.replace("id: \"plan\"", "id: \"planning\""),
    );
    for request in [
        json!({
            "action":"post","scope":"milestone","milestone_ref":milestone_ref,
            "gate":"plan","topic":"draft","payload":"milestone after tampering"
        }),
        json!({"action":"read","scope":"milestone","milestone_ref":milestone_ref,"gate":"plan"}),
        json!({
            "action":"post","scope":"task","task_id":inherited_task,
            "gate":"plan","topic":"notes","payload":"inherited task after tampering"
        }),
        json!({"action":"read","scope":"task","task_id":inherited_task,"gate":"plan"}),
    ] {
        let error = call(request).unwrap_err().to_string();
        assert!(error.contains("TASK_PROFILE_TAMPERED"), "{error}");
    }
    let override_after_tamper = call(json!({
        "action":"read","scope":"task","task_id":override_task,"gate":"task_plan"
    }))
    .unwrap();
    assert!(override_after_tamper["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["id"] == override_id));
}

#[test]
fn blackboard_automatic_scope_fails_closed_without_one_active_context() {
    let root = crate::common::Workspace::new();
    root.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n");
    for (manifests, expected) in [
        (
            vec![("010-first.md", "planned")],
            "TASK_BLACKBOARD_NO_ACTIVE_CONTEXT",
        ),
        (
            vec![("010-first.md", "active"), ("020-second.md", "active")],
            "TASK_BLACKBOARD_AMBIGUOUS_MILESTONE",
        ),
    ] {
        for (filename, status) in manifests {
            root.write(
                format!("docs/milestones/{filename}"),
                &format!("---\nid: m-{filename}\ntitle: {filename}\ndoc_type: contract\nstatus: {status}\n---\n"),
            );
        }
        let request: tasks::BlackboardRequest =
            serde_json::from_value(json!({"action":"read"})).unwrap();
        let error = tasks::blackboard(root.root(), request, "test").unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn blackboard_concurrent_connections_persist_and_cascade() {
    let (root, store, milestone) = file_backed_blackboard_fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let database = root.path(".forge/tasks.sqlite");
    let workers: Vec<_> = (0..2)
        .map(|worker| {
            let database = database.clone();
            let task_id = task_id.clone();
            std::thread::spawn(move || {
                let connection = TasksStore::open(&database).unwrap();
                for index in 0..20 {
                    connection
                        .blackboard_post(
                            &task_id,
                            &format!("worker-{worker}"),
                            "notes",
                            &index.to_string(),
                        )
                        .unwrap();
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    drop(store);
    let reopened = TasksStore::open(&database).unwrap();
    assert_eq!(
        reopened
            .blackboard_read(&task_id, None, None)
            .unwrap()
            .len(),
        40
    );
    reopened
        .connection
        .execute("DELETE FROM tasks WHERE task_id=?1", [&task_id])
        .unwrap();
    assert!(reopened
        .blackboard_read(&task_id, None, None)
        .unwrap()
        .is_empty());
}

static BLACKBOARD_BUSY_SIGNAL: std::sync::OnceLock<std::sync::mpsc::Sender<()>> =
    std::sync::OnceLock::new();
static BLACKBOARD_BUSY_NOTIFIED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn signal_blackboard_write_contention(_: i32) -> bool {
    if !BLACKBOARD_BUSY_NOTIFIED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        if let Some(signal) = BLACKBOARD_BUSY_SIGNAL.get() {
            let _ = signal.send(());
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(1));
    true
}

#[test]
fn blackboard_post_fails_closed_when_task_completes_during_write() {
    let (root, store, milestone) = file_backed_blackboard_fixture();
    let task_id = milestone.task_id(&milestone.tasks[0]);
    let task = store.inspect(&task_id).unwrap();
    let milestone_ref = store.milestone_scope_ref(&task.milestone_ref).unwrap();
    drop(store);

    let database = root.path(".forge/tasks.sqlite");
    let mut completion = TasksStore::open(&database).unwrap();
    let poster = TasksStore::open(&database).unwrap();
    let (busy_tx, busy_rx) = std::sync::mpsc::channel();
    BLACKBOARD_BUSY_NOTIFIED.store(false, std::sync::atomic::Ordering::SeqCst);
    assert!(BLACKBOARD_BUSY_SIGNAL.set(busy_tx).is_ok());
    poster
        .connection
        .busy_handler(Some(signal_blackboard_write_contention))
        .unwrap();

    let transition = completion
        .connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    transition
        .execute(
            "UPDATE tasks SET descriptor=json_set(descriptor,'$.status','completed') WHERE task_id=?1",
            [&task_id],
        )
        .unwrap();
    let post = std::thread::spawn(move || {
        poster.blackboard_post_scoped(
            &milestone_ref,
            Some(&task_id),
            None,
            "worker",
            "notes",
            "post races task completion",
        )
    });

    busy_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("post reached SQLite write contention");
    transition.commit().unwrap();
    let error = post.join().unwrap().unwrap_err();
    assert!(error.to_string().contains("TASK_TERMINAL"), "{error}");
}

#[test]
fn blackboard_accepts_nested_task_gates_read_aliases_and_unique_in_progress_task() {
    use contextunity_forge_mcp::engine::tasks as engine_tasks;

    let root = crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
    root.write(".gitignore", ".forge/\n");
    root.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n");
    root.write(
        ".forge/acdd/profile.yaml",
        r#"gates:
  - { id: "contract_synthesis", role: "architect", proof: "contract" }
  - tasks:
      gates:
        - { id: "contract", role: "contract_author", proof: "contract" }
        - { id: "build", role: "builder", command: "test", proof: "command" }
        - { id: "review", role: "independent_reviewer", proof: "review" }
        - { id: "deliver", role: "independent_reviewer", proof: "delivery", receipt: true }
 "#,
    );
    let manifest = "---\nid: m-nested-bb\ntitle: Nested blackboard\ndoc_type: contract\nstatus: active\n---\n# Nested blackboard\n```yaml\ntask_ref: first\ntarget: First task\nproof_policy: seam-test-first\nscope: [src/]\n```\n```yaml\ntask_ref: second\ntarget: Second task\nproof_policy: seam-test-first\nscope: [docs/]\n```\n";
    root.write("docs/milestones/010-nested-bb.md", manifest);
    let milestone = Milestone::parse(manifest, "forge-mcp").unwrap();
    let (mut store, first, second) =
        sync_blackboard_tasks(&root, "docs/milestones/010-nested-bb.md", &milestone);
    store
        .claim(
            &first,
            "contract",
            "worker-one",
            root.root().to_str().unwrap(),
            Some("test-session-worker-one"),
        )
        .unwrap();
    store.reset(&first).unwrap();
    store
        .claim(
            &second,
            "contract",
            "worker-two",
            root.root().to_str().unwrap(),
            Some("test-session-worker-two"),
        )
        .unwrap();

    let call =
        |args| engine_tasks::blackboard(root.root(), serde_json::from_value(args).unwrap(), "test");
    let posted = call(json!({
        "action": "post",
        "gate": "build",
        "topic": "draft",
        "payload": "handoff to nested build gate"
    }))
    .unwrap();
    let msg_id = posted["id"].as_u64().unwrap();

    for alias in ["get", "list", "read"] {
        let page = call(json!({
            "action": alias,
            "gate": "build"
        }))
        .unwrap();
        assert_eq!(
            page["messages"].as_array().unwrap().len(),
            1,
            "alias: {alias}"
        );
        assert_eq!(page["messages"][0]["id"], msg_id);
        assert_eq!(page["messages"][0]["task_id"], second);
    }
}
