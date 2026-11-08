use serde_json::{json, Value};

use contextunity_forge_mcp::core::tasks::SubtaskSpec;

use crate::acdd::support::MilestoneFixtureBuilder;
use crate::acdd::tasks::harness::TaskHarness;
use crate::common::mcp_harness::McpTestHarness;
use crate::common::Workspace;

use super::{cli_payload, in_process_cli, LargeTaskFixture, TaskListFixture};

pub(super) fn task_list_fixture() -> TaskListFixture {
    let workspace = TaskHarness::task_list_workspace();
    workspace.write("src/lib.rs", "pub fn task_list_fixture() {}\n");
    let subtasks = [
        (
            "explain",
            "Detailed subtask",
            "completed",
            Some("Verified through the listing seam"),
        ),
        ("ready-case", "Ready subtask", "ready", None),
        ("progress-case", "In progress subtask", "in_progress", None),
        ("blocked-case", "Blocked subtask", "blocked", None),
        ("cancelled-case", "Cancelled subtask", "cancelled", None),
    ]
    .map(|(subtask_ref, title, status, evidence)| SubtaskSpec {
        subtask_ref: subtask_ref.into(),
        title: title.into(),
        status: status.into(),
        evidence: evidence.map(str::to_owned),
    });
    workspace.write(
        "docs/milestones/010-active.md",
        &MilestoneFixtureBuilder::new("m-task-list")
            .title("Task list")
            .without_started_at()
            .task("visible", &["src/"])
            .task_target("Visible task")
            .task_proof_policy("direct-proof")
            .task_subtasks(&subtasks)
            .build(),
    );
    workspace.write(
        "docs/milestones/011-planned.md",
        &MilestoneFixtureBuilder::new("m-planned-list")
            .title("Planned task list")
            .without_status()
            .task("planned", &["src/"])
            .task_target("Planned task")
            .task_proof_policy("direct-proof")
            .build(),
    );
    workspace.write(
        "docs/milestones/009-completed.md",
        &MilestoneFixtureBuilder::new("m-completed-list")
            .title("Completed task list")
            .status("completed")
            .without_started_at()
            .task("completed", &["src/"])
            .task_target("Completed task")
            .task_proof_policy("direct-proof")
            .build(),
    );
    let mut client = McpTestHarness::new(&workspace).expect("MCP new should succeed");
    let synced = client
        .payload(
            "task_manage",
            json!({"action":"sync","milestone_ref":"010-active"}),
        )
        .expect("MCP payload should succeed");
    let task_id = synced["tasks"][0]["task_id"].as_str().unwrap();
    let task_harness = TaskHarness::attach(&workspace, task_id);
    let planned = client
        .payload(
            "task_manage",
            json!({"action":"sync","milestone_ref":"011-planned"}),
        )
        .expect("MCP payload should succeed");
    let planned_task_id = planned["tasks"][0]["task_id"].as_str().unwrap();
    let completed = client
        .payload(
            "task_manage",
            json!({"action":"sync","milestone_ref":"009-completed"}),
        )
        .expect("MCP payload should succeed");
    let completed_task_id = completed["tasks"][0]["task_id"].as_str().unwrap();

    TaskListFixture {
        workspace,
        client,
        task_id: task_id.to_owned(),
        task_harness,
        planned_task_id: planned_task_id.to_owned(),
        completed_task_id: completed_task_id.to_owned(),
    }
}

pub(super) fn add_task_status_fixture(
    workspace: &Workspace,
    task_harness: &mut TaskHarness,
) -> [(&'static str, &'static str); 6] {
    let task_statuses = [
        ("ready", "ready"),
        ("in-progress", "in_progress"),
        ("blocked", "blocked"),
        ("parked", "parked"),
        ("completed", "completed"),
        ("cancelled", "cancelled"),
    ];
    let mut status_fixture = MilestoneFixtureBuilder::new("m-task-statuses")
        .title("Task Statuses")
        .without_started_at();
    for (task_ref, _) in task_statuses {
        let scope = format!("src/status-{task_ref}.rs");
        workspace.write(&scope, "pub fn task_status_fixture() {}\n");
        let milestone_task_ref = format!("status-{task_ref}");
        let target = format!("{task_ref} task");
        status_fixture = status_fixture
            .task(&milestone_task_ref, &[scope.as_str()])
            .task_target(&target)
            .task_proof_policy("direct-proof");
    }
    let status_manifest = status_fixture.build();
    let status_path = "docs/milestones/012-statuses.md";
    workspace.write(status_path, &status_manifest);
    task_harness
        .sync_reference(Some(status_path), None)
        .unwrap();
    for (task_ref, status) in task_statuses {
        let task_id = format!("forge-mcp/m-task-statuses:status-{task_ref}");
        task_harness.set_persisted_task_status(&task_id, status);
    }
    task_statuses
}

pub(super) fn large_task_fixture(
    workspace: &Workspace,
    client: &mut McpTestHarness,
    task_harness: &mut TaskHarness,
) -> LargeTaskFixture {
    let large_target = format!("{} \"quoted\" \\slash", "λ".repeat(36_000));
    let large_subtasks = [
        ("done", "Completed nested row", "completed"),
        ("ready-case", "Ready nested row", "ready"),
        ("progress-case", "In progress nested row", "in_progress"),
        ("blocked-case", "Blocked nested row", "blocked"),
        ("cancelled-case", "Cancelled nested row", "cancelled"),
    ]
    .map(|(subtask_ref, title, status)| SubtaskSpec {
        subtask_ref: subtask_ref.into(),
        title: title.into(),
        status: status.into(),
        evidence: None,
    });
    let mut large_fixture = MilestoneFixtureBuilder::new("m-large")
        .title("Large Task Pages")
        .without_started_at();
    for index in 0..4 {
        let scope = format!("src/large-{index}.rs");
        workspace.write(&scope, "pub fn large_task_fixture() {}\n");
        let task_ref = format!("large-{index}");
        large_fixture = large_fixture
            .task(&task_ref, &[scope.as_str()])
            .task_target(&large_target)
            .task_proof_policy("direct-proof")
            .task_subtasks(&large_subtasks);
    }
    let large_manifest = large_fixture.build();
    let large_path = "docs/milestones/013-large.md";
    workspace.write(large_path, &large_manifest);
    task_harness.sync_reference(Some(large_path), None).unwrap();

    let (large_frame_bytes, large_response) = client
        .call(
            "task_list",
            json!({
                "milestone_ref":"013-large",
                "status":"all",
                "detail":"full",
                "limit":3,
                "offset":0,
                "format":"json"
            }),
        )
        .expect("MCP call should succeed");
    let large_page: Value = serde_json::from_str(
        large_response["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let large_generation = large_page["pagination"]["generation"]
        .as_str()
        .unwrap()
        .to_owned();
    LargeTaskFixture {
        target: large_target,
        manifest: large_manifest,
        frame_bytes: large_frame_bytes,
        response: large_response,
        page: large_page,
        generation: large_generation,
    }
}

#[test]
fn stdio_tool_catalog_uses_object_schemas_for_every_property() {
    let workspace = TaskHarness::task_list_workspace();
    let mut client = McpTestHarness::new(&workspace).expect("MCP new should succeed");
    let response = client
        .request_value("tools/list", json!({}))
        .expect("MCP request_value should succeed");
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 20);
    for tool in tools {
        assert!(
            tool["inputSchema"]["properties"]["worktree"].is_object(),
            "{} does not advertise worktree selection",
            tool["name"]
        );
        for (name, schema) in tool["inputSchema"]["properties"].as_object().unwrap() {
            assert!(schema.is_object(), "{}.{name}: {schema}", tool["name"]);
        }
    }
    let names: Vec<_> = tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .filter(|n| n.starts_with("task_"))
        .collect();
    assert_eq!(names.len(), 5);
    for name in [
        "task_list",
        "task_claim",
        "task_submit",
        "task_manage",
        "task_blackboard",
    ] {
        assert!(names.contains(&name));
        let schema = &tools.iter().find(|t| t["name"] == name).unwrap()["inputSchema"];
        assert_eq!(schema["additionalProperties"], false);
    }
    let task_manage_schema = &tools
        .iter()
        .find(|tool| tool["name"] == "task_manage")
        .unwrap()["inputSchema"];
    let manage_schema = task_manage_schema.to_string();
    assert!(
        manage_schema.contains("\"snapshot\""),
        "task_manage schema does not advertise snapshot: {manage_schema}"
    );
    assert!(
        manage_schema.contains("\"rollback\""),
        "task_manage schema does not advertise rollback: {manage_schema}"
    );
    assert!(task_manage_schema["properties"]["name"].is_object());
    let content = &tools
        .iter()
        .find(|tool| tool["name"] == "session_checkpoint")
        .unwrap()["inputSchema"]["properties"]["content"];
    assert!(content.is_object());
    let checkpoint_tool = tools
        .iter()
        .find(|tool| tool["name"] == "session_checkpoint")
        .unwrap();
    assert!(checkpoint_tool["description"]
        .as_str()
        .unwrap()
        .contains("full breadcrumb"));
    let breadcrumb = "forge-mcp/forge-mcp/m-test/tasks.build/checkpoint-sample/build";
    for value in [
        json!(true),
        json!(42),
        json!("text"),
        json!([1]),
        json!({"key":1}),
    ] {
        client
            .payload(
                "session_checkpoint",
                json!({"action":"save","name":breadcrumb,"content":value.clone()}),
            )
            .expect("MCP payload should succeed");
        assert_eq!(
            client
                .payload(
                    "session_checkpoint",
                    json!({"action":"get","name":breadcrumb})
                )
                .expect("MCP payload should succeed"),
            value
        );
    }
    client.finish().expect("MCP server should finish");
}

#[test]
fn task_list_status_and_subtask_detail_matrix() {
    let mut fixture = task_list_fixture();
    let task_id = fixture.task_id.as_str();
    let planned_task_id = fixture.planned_task_id.as_str();
    let completed_task_id = fixture.completed_task_id.as_str();
    let workspace = &fixture.workspace;
    let task_harness = &mut fixture.task_harness;
    let mut client = fixture.client;
    let (_, catalog) = client
        .request("tools/list", json!({}))
        .expect("MCP request should succeed");
    let task_list = catalog["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "task_list")
        .unwrap();
    let (_, raw_page) = client
        .call("task_list", json!({"limit":1,"offset":0,"format":"json"}))
        .expect("MCP call should succeed");
    assert!(
        raw_page["result"].is_object() && raw_page["result"]["isError"] != true,
        "task_list rejected raw pagination parameters: {}",
        raw_page
    );
    let raw_page: Value =
        serde_json::from_str(raw_page["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(raw_page["pagination"]["total"], 1);
    assert_eq!(raw_page["pagination"]["limit"], 1);
    assert_eq!(raw_page["pagination"]["returned"], 1);
    assert!(!raw_page["pagination"]["generation"]
        .as_str()
        .unwrap()
        .is_empty());
    let status_schema =
        serde_json::to_string(&task_list["inputSchema"]["properties"]["milestone_status"]).unwrap();
    for status in ["active", "planned", "completed", "all"] {
        assert!(status_schema.contains(status));
    }
    assert!(!status_schema.contains("cancelled"));
    let task_status_schema =
        serde_json::to_string(&task_list["inputSchema"]["properties"]["status"]).unwrap();
    for status in [
        "ready",
        "in_progress",
        "blocked",
        "parked",
        "completed",
        "cancelled",
        "all",
    ] {
        assert!(task_status_schema.contains(status));
    }
    for parameter in ["limit", "offset", "generation"] {
        assert!(
            task_list["inputSchema"]["properties"]
                .get(parameter)
                .is_some(),
            "task_list schema omitted {parameter}: {}",
            task_list["inputSchema"]
        );
    }
    assert!(task_list["inputSchema"]["properties"]
        .get("include_archived")
        .is_none());
    let detail_schema =
        serde_json::to_string(&task_list["inputSchema"]["properties"]["detail"]).unwrap();
    assert!(detail_schema.contains("compact"));
    assert!(detail_schema.contains("full"));

    let (_, invalid_status) = client
        .call("task_list", json!({"milestone_status":"cancelled"}))
        .expect("MCP call should succeed");
    assert!(invalid_status.get("error").is_some() || invalid_status["result"]["isError"] == true);
    for (milestone_status, expected_id) in [
        ("active", task_id),
        ("planned", planned_task_id),
        ("completed", completed_task_id),
    ] {
        let listed = client
            .payload("task_list", json!({"milestone_status":milestone_status}))
            .expect("MCP payload should succeed");
        assert_eq!(listed["tasks"][0]["task_id"], expected_id);
    }
    let mut mcp_all: Vec<_> = client
        .payload("task_list", json!({"milestone_status":"all"}))
        .expect("MCP payload should succeed")["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["task_id"].as_str().unwrap().to_owned())
        .collect();
    mcp_all.sort();
    let mut expected_all = vec![
        task_id.to_owned(),
        planned_task_id.to_owned(),
        completed_task_id.to_owned(),
    ];
    expected_all.sort();
    assert_eq!(mcp_all, expected_all);

    let cli_list = |flags: &[&str]| -> anyhow::Result<Value> {
        let mut args = vec!["task", "list"];
        args.extend_from_slice(flags);
        cli_payload(workspace, &args)
    };
    let cli_filter = |flags: &[&str]| {
        let payload = cli_list(flags).unwrap();
        payload["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|task| task["task_id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    for (status, expected_id) in [
        ("active", task_id),
        ("planned", planned_task_id),
        ("completed", completed_task_id),
    ] {
        assert_eq!(cli_filter(&["--milestone-status", status]), [expected_id]);
    }
    let mut cli_all = cli_filter(&["--milestone-status", "all"]);
    cli_all.sort();
    assert_eq!(cli_all, expected_all);
    assert_eq!(cli_filter(&["--planned"]), [planned_task_id]);
    assert_eq!(cli_filter(&["--completed"]), [completed_task_id]);
    let mut cli_shorthand_all = cli_filter(&["--all"]);
    cli_shorthand_all.sort();
    assert_eq!(cli_shorthand_all, expected_all);

    use clap::Parser;
    let cli_help = contextunity_forge_mcp::cli::Cli::try_parse_from([
        "contextunity-forge-mcp",
        "task",
        "list",
        "--help",
    ])
    .unwrap_err();
    assert_eq!(cli_help.exit_code(), 0);
    let cli_help = cli_help.to_string();
    assert!(cli_help.contains("--all"));
    assert!(cli_help.contains("cancelled milestones pending sync pruning"));

    let compact = client
        .payload("task_list", json!({}))
        .expect("MCP payload should succeed");
    assert_eq!(compact["tasks"].as_array().unwrap().len(), 1);
    let compact_subtask = &compact["tasks"][0]["subtasks"][0];
    assert_eq!(compact_subtask["subtask_ref"], "explain");
    assert_eq!(compact_subtask["status"], "completed");
    assert_eq!(compact["tasks"][0]["subtasks"].as_array().unwrap().len(), 5);
    assert!(compact_subtask.get("title").is_none());
    assert!(compact_subtask.get("evidence").is_none());

    let full = client
        .payload("task_list", json!({"detail":"full"}))
        .expect("MCP payload should succeed");
    assert_eq!(full["tasks"][0]["subtasks"][0]["title"], "Detailed subtask");
    assert_eq!(
        full["tasks"][0]["subtasks"][0]["evidence"],
        "Verified through the listing seam"
    );
    let (_, full_markdown_response) = client
        .call("task_list", json!({"detail":"full"}))
        .expect("MCP call should succeed");
    let full_markdown = full_markdown_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(full_markdown.contains(task_id));
    assert!(full_markdown.contains("Detailed subtask"));
    assert!(full_markdown.contains("Verified through the listing seam"));
    assert!(full_markdown.contains("completed"));
    assert_eq!(full_markdown.matches("- [x]").count(), 1, "{full_markdown}");
    assert_eq!(full_markdown.matches("- [ ]").count(), 5, "{full_markdown}");
    for (subtask_ref, status) in [
        ("explain", "completed"),
        ("ready-case", "ready"),
        ("progress-case", "in_progress"),
        ("blocked-case", "blocked"),
        ("cancelled-case", "cancelled"),
    ] {
        let subtask = full["tasks"][0]["subtasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|subtask| subtask["subtask_ref"] == subtask_ref)
            .unwrap();
        assert_eq!(subtask["status"], status);
        assert!(full_markdown.contains(subtask_ref));
    }

    let task_statuses = add_task_status_fixture(workspace, task_harness);

    let all_task_states = client
        .payload(
            "task_list",
            json!({"milestone_ref":"012-statuses","status":"all"}),
        )
        .expect("MCP payload should succeed");
    let listed_states = all_task_states["tasks"].as_array().unwrap();
    assert_eq!(listed_states.len(), task_statuses.len());
    for (_, status) in task_statuses {
        assert!(listed_states.iter().any(|task| task["status"] == status));
        let filtered = client
            .payload(
                "task_list",
                json!({"milestone_ref":"012-statuses","status":status}),
            )
            .expect("MCP payload should succeed");
        assert_eq!(filtered["tasks"].as_array().unwrap().len(), 1, "{status}");
        assert_eq!(filtered["tasks"][0]["status"], status);
    }
    let (_, task_status_markdown_response) = client
        .call(
            "task_list",
            json!({"milestone_ref":"012-statuses","status":"all"}),
        )
        .expect("MCP call should succeed");
    let task_status_markdown = task_status_markdown_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert_eq!(task_status_markdown.matches("- [x]").count(), 1);
    assert_eq!(task_status_markdown.matches("- [ ]").count(), 5);
    for (task_ref, status) in task_statuses {
        let task_id = format!("forge-mcp/m-task-statuses:status-{task_ref}");
        let heading = format!("### `{task_id}`");
        let section = task_status_markdown
            .split_once(&heading)
            .unwrap_or_else(|| panic!("missing task heading {heading}: {task_status_markdown}"))
            .1
            .split("\n### ")
            .next()
            .unwrap();
        let checkbox = if status == "completed" {
            "- [x]"
        } else {
            "- [ ]"
        };
        assert!(section.contains(checkbox), "{status}: {section}");
        assert!(section.contains(status), "{status}: {section}");
    }

    let subtask_details = client
        .payload(
            "task_manage",
            json!({"action":"subtask_list","task_id":task_id}),
        )
        .expect("MCP payload should succeed");
    assert_eq!(subtask_details["subtasks"][0]["title"], "Detailed subtask");
    assert_eq!(
        subtask_details["subtasks"][0]["evidence"],
        "Verified through the listing seam"
    );
    let inspected = client
        .payload("task_manage", json!({"action":"inspect","task_id":task_id}))
        .expect("MCP payload should succeed");
    assert_eq!(inspected["subtasks"][0]["title"], "Detailed subtask");
    assert_eq!(
        inspected["subtasks"][0]["evidence"],
        "Verified through the listing seam"
    );

    let cli_full_json = cli_payload(
        workspace,
        &[
            "task",
            "list",
            "--milestone",
            "010-active",
            "--milestone-status",
            "active",
            "--full",
        ],
    )
    .unwrap();
    assert_eq!(cli_full_json["tasks"][0]["task_id"], task_id);
    assert_eq!(
        cli_full_json["tasks"][0]["subtasks"][0]["evidence"],
        "Verified through the listing seam"
    );

    for conflicting_flags in [
        vec!["--planned", "--completed"],
        vec!["--planned", "--all"],
        vec!["--completed", "--all"],
        vec!["--milestone-status", "active", "--planned"],
        vec!["--milestone-status", "all", "--all"],
    ] {
        let mut args = vec!["task", "list"];
        args.extend_from_slice(&conflicting_flags);
        let conflict = in_process_cli::run_cli_json(workspace, &args);
        assert_eq!(conflict.status.code(), Some(2));
    }

    let oversize_target = format!("oversized {}", "界".repeat(40_000));
    let oversized_manifest = MilestoneFixtureBuilder::new("m-oversized")
        .title("Oversized Task")
        .without_started_at()
        .task("one-record", &["src/oversized.rs"])
        .task_target(&oversize_target)
        .task_proof_policy("direct-proof")
        .build();
    workspace.write("src/oversized.rs", "pub fn oversized_fixture() {}\n");
    let oversized_path = "docs/milestones/014-oversized.md";
    workspace.write(oversized_path, &oversized_manifest);
    task_harness
        .sync_reference(Some(oversized_path), None)
        .unwrap();
    let (oversized_frame_bytes, oversized_response) = client
        .call(
            "task_list",
            json!({
                "milestone_ref":"014-oversized",
                "status":"all",
                "limit":1,
                "offset":0,
                "format":"json"
            }),
        )
        .expect("MCP call should succeed");
    assert!(oversized_frame_bytes <= 98_304);
    assert_eq!(oversized_response["result"]["isError"], true);
    let oversized_message = oversized_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(
        oversized_message.to_ascii_lowercase().contains("compact")
            || oversized_message.to_ascii_lowercase().contains("narrow")
    );
    assert!(
        oversized_message.to_ascii_lowercase().contains("task")
            || oversized_message.to_ascii_lowercase().contains("record")
    );

    client.finish().expect("MCP server should finish");
}
