use std::fs;

use serde_json::{json, Value};

use crate::acdd::support::MilestoneFixtureBuilder;
use crate::acdd::tasks::harness::TaskHarness;
use crate::common::mcp_harness::McpTestHarness;

use super::in_process_cli;

#[test]
fn task_blackboard_mcp_and_cli_share_sqlite_messages() {
    let workspace = TaskHarness::mcp_workspace();
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
    );
    workspace.write(
        "docs/milestones/UX-001-mcp-response-deduplication-and-agent-efficiency.md",
        &MilestoneFixtureBuilder::new("m-blackboard")
            .title("Blackboard")
            .without_status()
            .task("first", &["src/"])
            .task_target("Coordinate agents")
            .build(),
    );
    let mut client = McpTestHarness::new(&workspace).expect("MCP new should succeed");
    let synced = client.payload(
        "task_manage",
        json!({"action":"sync","milestone_ref":"docs/milestones/UX-001-mcp-response-deduplication-and-agent-efficiency.md"}),
    ).expect("MCP payload should succeed");
    let task_id = synced["tasks"][0]["task_id"].as_str().unwrap();
    let store = TaskHarness::attach(&workspace, task_id).store;

    let (_, mcp_post) = client.call(
        "task_blackboard",
        json!({"action":"post","task_id":task_id,"topic":"draft","payload":"{\"red\":true}","format":"json"}),
    ).expect("MCP call should succeed");
    let cli_post = in_process_cli::run_cli_json(
        &workspace,
        &[
            "task",
            "blackboard",
            "post",
            task_id,
            "--topic",
            "notes",
            "--payload",
            "cargo test --all-targets passed",
        ],
    );
    assert!(
        mcp_post.get("error").is_none()
            && mcp_post["result"]["isError"] != true
            && cli_post.status.success(),
        "MCP: {mcp_post}; CLI: {}",
        String::from_utf8_lossy(&cli_post.stderr)
    );
    let mcp_post: Value =
        serde_json::from_str(mcp_post["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let cli_post: Value = serde_json::from_slice(&cli_post.stdout).unwrap();
    let mcp_read = client
        .payload(
            "task_blackboard",
            json!({"action":"read","task_id":task_id,"limit":2}),
        )
        .expect("MCP payload should succeed");
    assert_eq!(mcp_read["messages"].as_array().unwrap().len(), 2);
    assert_eq!(mcp_read["messages"][0]["id"], cli_post["id"]);
    assert_eq!(mcp_read["messages"][1]["id"], mcp_post["id"]);

    let cli_read = in_process_cli::run_cli_json(
        &workspace,
        &["task", "blackboard", "read", task_id, "--topic", "draft"],
    );
    assert!(
        cli_read.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_read.stderr)
    );
    let cli_read: Value = serde_json::from_slice(&cli_read.stdout).unwrap();
    assert_eq!(cli_read["messages"].as_array().unwrap().len(), 1);
    assert_eq!(cli_read["messages"][0]["id"], mcp_post["id"]);

    let (_, mcp_markdown) = client
        .call(
            "task_blackboard",
            json!({"action":"read","task_id":task_id,"topic":"draft","format":"markdown"}),
        )
        .expect("MCP call should succeed");
    let mcp_markdown = mcp_markdown["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(mcp_markdown.contains("## draft"));
    assert!(mcp_markdown.contains("task_blackboard(action: \"inspect\", message_id:"));
    let cli_markdown = in_process_cli::run_cli(
        &workspace,
        &[
            "--format",
            "markdown",
            "task",
            "blackboard",
            "read",
            task_id,
            "--topic",
            "draft",
        ],
    );
    assert!(
        cli_markdown.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_markdown.stderr)
    );
    let cli_markdown = std::str::from_utf8(&cli_markdown.stdout).unwrap();
    assert_eq!(mcp_markdown.trim_end(), cli_markdown.trim_end());

    let stored = store.blackboard_read(task_id, None, None).unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].payload.as_deref(), Some("{\"red\":true}"));
    assert_eq!(
        stored[1].payload.as_deref(),
        Some("cargo test --all-targets passed")
    );
    assert_eq!(stored[0].author, "mcp");
    assert_eq!(stored[1].author, "cli");

    let mcp_list = client
        .payload("task_list", json!({"milestone_ref":"first","status":"all"}))
        .expect("MCP payload should succeed");
    assert_eq!(mcp_list["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(mcp_list["resolution"]["kind"], "task_parent");
    assert_eq!(mcp_list["resolution"]["input"], "first");
    assert_eq!(mcp_list["resolution"]["task_id"], task_id);
    assert_eq!(mcp_list["resolution"]["milestone_id"], "m-blackboard");
    assert_eq!(
        mcp_list["resolution"]["milestone_ref"],
        "docs/milestones/UX-001-mcp-response-deduplication-and-agent-efficiency.md"
    );
    let mcp_alias_list = client
        .payload(
            "task_list",
            json!({"milestone_ref":"UX-001","status":"all"}),
        )
        .expect("MCP payload should succeed");
    assert_eq!(mcp_alias_list["tasks"].as_array().unwrap().len(), 1);
    assert!(mcp_alias_list.get("resolution").is_none());
    let cli_alias_list = in_process_cli::run_cli_json(
        &workspace,
        &["task", "list", "--milestone", "UX-001", "--status", "all"],
    );
    assert!(
        cli_alias_list.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_alias_list.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&cli_alias_list.stdout).unwrap(),
        mcp_alias_list
    );
    let cli_list = in_process_cli::run_cli_json(
        &workspace,
        &["task", "list", "--milestone", "first", "--status", "all"],
    );
    assert!(
        cli_list.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_list.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&cli_list.stdout).unwrap(),
        mcp_list
    );

    let (_, mcp_list_markdown) = client
        .call(
            "task_list",
            json!({"milestone_ref":"first","status":"all","format":"markdown"}),
        )
        .expect("MCP call should succeed");
    let mcp_list_markdown = mcp_list_markdown["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(mcp_list_markdown
        .contains("[auto-resolved task 'first' to parent milestone 'm-blackboard']"));
    assert_eq!(
        mcp_list_markdown
            .matches("[auto-resolved task 'first' to parent milestone 'm-blackboard']")
            .count(),
        1,
        "task-list Markdown displays fallback provenance exactly once"
    );
    let cli_list_markdown = in_process_cli::run_cli(
        &workspace,
        &[
            "--format",
            "markdown",
            "task",
            "list",
            "--milestone",
            "first",
            "--status",
            "all",
        ],
    );
    assert!(
        cli_list_markdown.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_list_markdown.stderr)
    );
    let cli_list_markdown = std::str::from_utf8(&cli_list_markdown.stdout).unwrap();
    assert_eq!(mcp_list_markdown.trim_end(), cli_list_markdown.trim_end());

    let canonical_milestone = store
        .milestone_scope_ref(
            "docs/milestones/UX-001-mcp-response-deduplication-and-agent-efficiency.md",
        )
        .unwrap();
    let milestone_message_count =
        || TaskHarness::milestone_message_count(&store, &canonical_milestone).unwrap();
    let before_fallback_posts = milestone_message_count();
    let mcp_alias_read = client
        .payload(
            "task_blackboard",
            json!({
                "action":"read",
                "scope":"milestone",
                "milestone_ref":"UX-001",
                "topic":"decisions"
            }),
        )
        .expect("MCP payload should succeed");
    assert!(mcp_alias_read["messages"].as_array().unwrap().is_empty());
    assert!(mcp_alias_read.get("resolution").is_none());
    let cli_alias_read = in_process_cli::run_cli_json(
        &workspace,
        &[
            "task",
            "blackboard",
            "read",
            "--scope",
            "milestone",
            "--milestone-ref",
            "UX-001",
            "--topic",
            "decisions",
        ],
    );
    assert!(
        cli_alias_read.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_alias_read.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&cli_alias_read.stdout).unwrap(),
        mcp_alias_read
    );

    let (_, mcp_fallback_post) = client
        .call(
            "task_blackboard",
            json!({
                "action":"post",
                "scope":"milestone",
                "milestone_ref":"first",
                "topic":"decisions",
                "payload":"MCP fallback post",
                "format":"json"
            }),
        )
        .expect("MCP call should succeed");
    assert_ne!(mcp_fallback_post["result"]["isError"], true);
    let mcp_fallback_post: Value = serde_json::from_str(
        mcp_fallback_post["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(mcp_fallback_post["resolution"]["kind"], "task_parent");
    assert_eq!(mcp_fallback_post["resolution"]["input"], "first");
    assert_eq!(mcp_fallback_post["resolution"]["task_id"], task_id);
    assert_eq!(
        mcp_fallback_post["resolution"]["milestone_id"],
        "m-blackboard"
    );
    assert_eq!(
        mcp_fallback_post["resolution"]["milestone_ref"],
        "forge-mcp/forge-mcp/docs/milestones/UX-001-mcp-response-deduplication-and-agent-efficiency.md"
    );
    assert_eq!(milestone_message_count(), before_fallback_posts + 1);

    let cli_fallback_post = in_process_cli::run_cli_json(
        &workspace,
        &[
            "task",
            "blackboard",
            "post",
            "--scope",
            "milestone",
            "--milestone-ref",
            "first",
            "--topic",
            "decisions",
            "--payload",
            "CLI fallback post",
        ],
    );
    assert!(
        cli_fallback_post.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_fallback_post.stderr)
    );
    let cli_fallback_post: Value = serde_json::from_slice(&cli_fallback_post.stdout).unwrap();
    assert_eq!(cli_fallback_post["resolution"]["kind"], "task_parent");
    assert_eq!(cli_fallback_post["resolution"]["input"], "first");
    assert_eq!(cli_fallback_post["resolution"]["task_id"], task_id);
    assert_eq!(
        cli_fallback_post["resolution"]["milestone_id"],
        "m-blackboard"
    );
    assert_eq!(
        cli_fallback_post["resolution"]["milestone_ref"],
        "forge-mcp/forge-mcp/docs/milestones/UX-001-mcp-response-deduplication-and-agent-efficiency.md"
    );
    assert_ne!(cli_fallback_post["id"], mcp_fallback_post["id"]);
    assert_eq!(milestone_message_count(), before_fallback_posts + 2);

    let mcp_fallback_read = client
        .payload(
            "task_blackboard",
            json!({
                "action":"read",
                "scope":"milestone",
                "milestone_ref":"first",
                "topic":"decisions"
            }),
        )
        .expect("MCP payload should succeed");
    let cli_fallback_read = in_process_cli::run_cli_json(
        &workspace,
        &[
            "task",
            "blackboard",
            "read",
            "--scope",
            "milestone",
            "--milestone-ref",
            "first",
            "--topic",
            "decisions",
        ],
    );
    assert!(
        cli_fallback_read.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_fallback_read.stderr)
    );
    let cli_fallback_read: Value = serde_json::from_slice(&cli_fallback_read.stdout).unwrap();
    assert_eq!(cli_fallback_read, mcp_fallback_read);
    assert_eq!(mcp_fallback_read["messages"].as_array().unwrap().len(), 2);
    assert_eq!(mcp_fallback_read["resolution"]["kind"], "task_parent");
    assert_eq!(mcp_fallback_read["resolution"]["input"], "first");
    assert_eq!(mcp_fallback_read["resolution"]["task_id"], task_id);
    assert_eq!(
        mcp_fallback_read["resolution"]["milestone_id"],
        "m-blackboard"
    );
    assert_eq!(milestone_message_count(), before_fallback_posts + 2);

    let (_, mcp_fallback_markdown) = client
        .call(
            "task_blackboard",
            json!({
                "action":"read",
                "scope":"milestone",
                "milestone_ref":"first",
                "topic":"decisions",
                "format":"markdown"
            }),
        )
        .expect("MCP call should succeed");
    let mcp_fallback_markdown = mcp_fallback_markdown["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(mcp_fallback_markdown
        .contains("[auto-resolved task 'first' to parent milestone 'm-blackboard']"));
    assert_eq!(
        mcp_fallback_markdown
            .matches("[auto-resolved task 'first' to parent milestone 'm-blackboard']")
            .count(),
        1,
        "blackboard Markdown displays fallback provenance exactly once"
    );
    let cli_fallback_markdown = in_process_cli::run_cli(
        &workspace,
        &[
            "--format",
            "markdown",
            "task",
            "blackboard",
            "read",
            "--scope",
            "milestone",
            "--milestone-ref",
            "first",
            "--topic",
            "decisions",
        ],
    );
    assert!(
        cli_fallback_markdown.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_fallback_markdown.stderr)
    );
    let cli_fallback_markdown = std::str::from_utf8(&cli_fallback_markdown.stdout).unwrap();
    assert_eq!(
        mcp_fallback_markdown.trim_end(),
        cli_fallback_markdown.trim_end()
    );
    client.finish().expect("MCP server should finish");
}

#[test]
fn milestone_blackboard_lists_are_bounded_payload_free_and_inspectable() {
    let workspace = TaskHarness::mcp_workspace();
    fs::create_dir_all(workspace.root().join("docs/milestones")).unwrap();
    workspace.write(
        "forge-mcp.yaml",
        "roots: []\ndocs: []\ntasks_db: tasks.sqlite\n",
    );
    workspace.write(
        "docs/milestones/010-blackboard.md",
        "---\nid: m-blackboard\ntitle: Blackboard\ndoc_type: contract\nstatus: active\n---\n",
    );
    let mut client = McpTestHarness::new(&workspace).expect("MCP new should succeed");

    let mut message_id = 0;
    let mut blocker_id = 0;
    let mut posted_ids = Vec::new();
    let blocker_body = "blocker needs review\nsecond body line";
    for index in 0..12 {
        let is_blocker = index == 10;
        let (_, response) = client.call(
            "task_blackboard",
            json!({
                "action":"post",
                "topic":if is_blocker {"blockers"} else if index == 9 {"notes"} else {"decisions"},
                "author":if is_blocker {"fixture-builder"} else {"fixture-writer"},
                "gate":if is_blocker {Some("build")} else {None},
                "payload":if is_blocker {blocker_body.to_owned()} else {format!("milestone note {index}")},
                "format":"json"
            }),
        ).expect("MCP call should succeed");
        assert!(
            response.get("error").is_none() && response["result"]["isError"] != true,
            "{response}"
        );
        let posted: Value =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        message_id = posted["id"].as_u64().unwrap();
        posted_ids.push(message_id);
        if is_blocker {
            blocker_id = message_id;
        }
    }

    let messages = client
        .payload("task_blackboard", json!({"action":"read"}))
        .expect("MCP payload should succeed");
    assert_eq!(messages["messages"].as_array().unwrap().len(), 10);
    assert_eq!(messages["pagination"]["limit"], 10);
    assert_eq!(messages["pagination"]["offset"], 0);
    assert_eq!(messages["pagination"]["has_more"], true);
    assert_eq!(messages["pagination"]["next_offset"], 10);
    assert_eq!(messages["messages"][0]["id"], message_id);
    assert_eq!(messages["messages"][0]["task_id"], Value::Null);
    assert!(messages["messages"][0].get("payload").is_none());
    assert_eq!(messages["messages"][1]["id"], blocker_id);
    assert_eq!(messages["messages"][1]["topic"], "blockers");
    assert_eq!(messages["messages"][1]["author"], "fixture-builder");
    assert!(messages["messages"][1]["milestone_ref"]
        .as_str()
        .unwrap()
        .ends_with("docs/milestones/010-blackboard.md"));
    assert!(messages["messages"][1]["task_id"].is_null());
    assert!(messages["messages"][1]["subtask_ref"].is_null());
    assert_eq!(messages["messages"][1]["gate"], "build");
    assert!(messages["messages"][1]["created_at"].is_i64());
    assert!(messages["messages"][1].get("payload").is_none());
    assert!(messages["messages"]
        .as_array()
        .unwrap()
        .windows(2)
        .all(|pair| { pair[0]["id"].as_u64().unwrap() > pair[1]["id"].as_u64().unwrap() }));

    let (_, listed_markdown) = client
        .call(
            "task_blackboard",
            json!({"action":"read","limit":10,"format":"markdown"}),
        )
        .expect("MCP call should succeed");
    let listed_markdown = listed_markdown["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(listed_markdown.contains(&format!(
        "[!CAUTION] BLOCKER (#{blocker_id}) by fixture-builder"
    )));
    let listed_lowercase = listed_markdown.to_lowercase();
    assert!(listed_lowercase.contains("scope") && listed_lowercase.contains("milestone"));
    assert!(listed_lowercase.contains("gate") && listed_lowercase.contains("build"));
    let created_at = messages["messages"][1]["created_at"].as_i64().unwrap();
    assert!(listed_markdown.contains(&created_at.to_string()));
    let listed_blocker = listed_markdown
        .split(&format!(
            "> [!CAUTION] BLOCKER (#{blocker_id}) by fixture-builder"
        ))
        .nth(1)
        .and_then(|section| section.split("\n\n").next())
        .unwrap();
    assert!(listed_blocker.contains(&format!(
        "> Scope: `milestone`\n> Milestone ref: `{}`",
        messages["messages"][1]["milestone_ref"].as_str().unwrap()
    )));
    assert!(listed_blocker.contains(&format!("> Timestamp: `{created_at}`")));
    assert!(listed_markdown.contains(&format!(
        "task_blackboard(action: \"inspect\", message_id: {blocker_id})"
    )));
    assert!(listed_markdown.contains("Body not included in this message summary."));
    assert!(!listed_markdown.contains("> Body: \"\""));
    assert!(!listed_markdown.contains("second body line"));
    assert_eq!(listed_markdown.matches("[!CAUTION] BLOCKER").count(), 1);
    assert!(listed_markdown.contains("## Pagination"));
    assert!(listed_markdown.contains("**next_offset**: `10`"));
    assert!(listed_markdown.contains("**has_more**: `true`"));
    assert!(listed_markdown.contains("## decisions"));
    assert!(listed_markdown.contains("## notes"));
    let newest_decision = listed_markdown
        .find(&format!("### Message #{}", posted_ids[11]))
        .unwrap();
    let older_decision = listed_markdown
        .find(&format!("### Message #{}", posted_ids[8]))
        .unwrap();
    assert!(newest_decision < older_decision);

    let second_page = client
        .payload(
            "task_blackboard",
            json!({"action":"read","limit":10,"offset":messages["pagination"]["next_offset"]}),
        )
        .expect("MCP payload should succeed");
    assert_eq!(second_page["messages"].as_array().unwrap().len(), 2);
    assert_eq!(second_page["messages"][0]["id"], posted_ids[1]);
    assert_eq!(second_page["messages"][1]["id"], posted_ids[0]);
    assert_eq!(second_page["pagination"]["offset"], 10);
    assert_eq!(second_page["pagination"]["has_more"], false);
    let (_, second_page_markdown) = client.call(
        "task_blackboard",
        json!({"action":"read","limit":10,"offset":messages["pagination"]["next_offset"],"format":"markdown"}),
    ).expect("MCP call should succeed");
    let second_page_markdown = second_page_markdown["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(second_page_markdown.contains("**has_more**: `false`"));
    assert!(second_page_markdown.contains("**offset**: `10`"));
    assert!(second_page_markdown.contains("**next_offset**: `null`"));
    assert!(!second_page_markdown.contains("[!CAUTION]"));
    assert!(second_page_markdown.contains(&format!("### Message #{}", posted_ids[1])));
    assert!(second_page_markdown.contains(&format!("### Message #{}", posted_ids[0])));

    let inspected = client
        .payload(
            "task_blackboard",
            json!({"action":"inspect","message_id":message_id}),
        )
        .expect("MCP payload should succeed");
    assert_eq!(inspected["message"]["id"], message_id);
    assert_eq!(inspected["message"]["payload"], "milestone note 11");

    let cli_inspect = crate::common::cli::run_cli_json(
        &workspace,
        &["task", "blackboard", "inspect", &message_id.to_string()],
    );
    assert!(
        cli_inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_inspect.stderr)
    );
    let cli_inspect_json: Value = serde_json::from_slice(&cli_inspect.stdout).unwrap();
    assert_eq!(cli_inspect_json["message"]["id"], message_id);
    assert_eq!(cli_inspect_json["message"]["payload"], "milestone note 11");

    let (_, blocker_inspect) = client
        .call(
            "task_blackboard",
            json!({"action":"inspect","message_id":blocker_id,"format":"markdown"}),
        )
        .expect("MCP call should succeed");
    let blocker_inspect = blocker_inspect["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(blocker_inspect.contains(&format!(
        "[!CAUTION] BLOCKER (#{blocker_id}) by fixture-builder"
    )));
    let blocker_inspect_lowercase = blocker_inspect.to_lowercase();
    assert!(
        blocker_inspect_lowercase.contains("scope")
            && blocker_inspect_lowercase.contains("milestone")
    );
    assert!(
        blocker_inspect_lowercase.contains("gate") && blocker_inspect_lowercase.contains("build")
    );
    assert!(blocker_inspect.contains("second body line"));

    let empty_blocker = client
        .payload(
            "task_blackboard",
            json!({
                "action":"post",
                "topic":"blockers",
                "author":"fixture-builder",
                "gate":"build",
                "payload":""
            }),
        )
        .expect("MCP payload should succeed");
    let empty_blocker_id = empty_blocker["id"].as_u64().unwrap();
    let (_, empty_blocker_response) = client
        .call(
            "task_blackboard",
            json!({"action":"inspect","message_id":empty_blocker_id,"format":"markdown"}),
        )
        .expect("MCP call should succeed");
    let empty_blocker_markdown = empty_blocker_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(empty_blocker_markdown.contains("> Body: \"\""));
    assert!(!empty_blocker_markdown.contains(&format!(
        "task_blackboard(action: \"inspect\", message_id: {empty_blocker_id})"
    )));
    client.finish().expect("MCP server should finish");
}
