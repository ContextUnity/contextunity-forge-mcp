use serde_json::{json, Value};

use crate::acdd::tasks::harness::TaskHarness;

use crate::common::Workspace;
use crate::common::mcp_harness::McpTestHarness;

pub(super) use crate::common::cli as in_process_cli;
pub(super) use crate::common::mcp_harness as in_process;

pub(super) struct TaskListFixture {
    pub(super) workspace: Workspace,
    pub(super) client: McpTestHarness,
    pub(super) task_id: String,
    pub(super) task_harness: TaskHarness,
    pub(super) planned_task_id: String,
    pub(super) completed_task_id: String,
}

pub(super) struct LargeTaskFixture {
    pub(super) target: String,
    pub(super) manifest: String,
    pub(super) frame_bytes: usize,
    pub(super) response: Value,
    pub(super) page: Value,
    pub(super) generation: String,
}

mod blackboard_and_list;
mod claims_and_leases;
mod manage_and_sync;
mod milestones_and_handoff;
mod profiles_and_gates;
mod submit_and_validation;
pub(super) mod task_list_matrix;
mod worktrees_and_isolation;

use self::task_list_matrix::{add_task_status_fixture, large_task_fixture, task_list_fixture};

pub(super) fn cli_payload(workspace: &Workspace, args: &[&str]) -> anyhow::Result<Value> {
    let output = in_process_cli::run_cli_json(workspace, args);
    anyhow::ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

#[test]
fn task_list_generation_and_continuation_matrix() {
    let mut fixture = task_list_fixture();
    let workspace = &fixture.workspace;
    let task_harness = &mut fixture.task_harness;
    let mut client = fixture.client;
    let _task_statuses = add_task_status_fixture(workspace, task_harness);
    let LargeTaskFixture {
        manifest: large_manifest,
        generation: large_generation,
        ..
    } = large_task_fixture(workspace, &mut client, task_harness);
    let task_list_cli_json = |root: &Workspace, args: &[&str]| -> anyhow::Result<Value> {
        let mut cli_args = vec!["task", "list", "--milestone", "013-large", "--status", "all"];
        cli_args.extend_from_slice(args);
        cli_payload(root, &cli_args)
    };
    for detail in ["compact", "full"] {
        for limit in [1usize, 3] {
            let limit_arg = limit.to_string();
            let mcp_page = client
                .payload(
                    "task_list",
                    json!({
                        "milestone_ref":"013-large",
                        "status":"all",
                        "detail":detail,
                        "limit":limit,
                        "offset":0
                    }),
                )
                .expect("MCP payload should succeed");
            let cli_page = task_list_cli_json(
                workspace,
                &[
                    "--detail",
                    detail,
                    "--offset",
                    "0",
                    "--limit",
                    limit_arg.as_str(),
                ],
            )
            .expect("CLI task list should succeed");
            for field in ["total", "returned", "has_more", "next_offset"] {
                assert_eq!(
                    cli_page["pagination"][field], mcp_page["pagination"][field],
                    "{detail} limit {limit} pagination field {field}"
                );
            }
            let cli_task_ids: Vec<_> = cli_page["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|task| task["task_id"].as_str().unwrap().to_owned())
                .collect();
            let mcp_task_ids: Vec<_> = mcp_page["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|task| task["task_id"].as_str().unwrap().to_owned())
                .collect();
            assert_eq!(
                cli_task_ids, mcp_task_ids,
                "{detail} limit {limit} task IDs"
            );
        }
    }

    let other_generation = client
        .payload(
            "task_list",
            json!({
                "milestone_ref":"012-statuses",
                "status":"all",
                "detail":"full",
                "limit":10,
                "offset":0
            }),
        )
        .expect("MCP payload should succeed")["pagination"]["generation"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        task_list_cli_json(
            workspace,
            &[
                "--detail",
                "full",
                "--offset",
                "1",
                "--limit",
                "10",
                "--generation",
                &other_generation,
            ],
        )
        .is_err(),
        "CLI must reject a task-list generation from another milestone view"
    );

    let default_page = client
        .payload(
            "task_list",
            json!({"milestone_ref":"013-large","status":"all"}),
        )
        .expect("MCP payload should succeed");
    assert_eq!(default_page["pagination"]["limit"], 30);
    for limit in [1, 30, 100] {
        let page = client
            .payload(
                "task_list",
                json!({
                    "milestone_ref":"013-large",
                    "status":"all",
                    "limit":limit,
                    "offset":0
                }),
            )
            .expect("MCP payload should succeed");
        assert_eq!(page["pagination"]["limit"], limit);
        assert!(page["pagination"]["returned"].as_u64().unwrap() <= limit);
    }
    for limit in ["0", "101"] {
        let cli_invalid = in_process_cli::run_cli_json(
            workspace,
            &[
                "task",
                "list",
                "--milestone",
                "013-large",
                "--limit",
                limit,
                "--offset",
                "0",
            ],
        );
        assert!(
            !cli_invalid.status.success(),
            "invalid CLI limit should fail: {limit}"
        );
    }

    for limit in [0, 101] {
        let invalid = client
            .call(
                "task_list",
                json!({
                    "milestone_ref":"013-large",
                    "status":"all",
                    "limit":limit,
                    "offset":0
                }),
            )
            .expect("MCP call should succeed");
        assert!(invalid.1["error"].is_object() || invalid.1["result"]["isError"] == true);
    }

    let first_continuation = client
        .payload(
            "task_list",
            json!({
                "milestone_ref":"013-large",
                "status":"all",
                "detail":"full",
                "limit":3,
                "offset":1,
                "generation":large_generation
            }),
        )
        .expect("MCP payload should succeed");
    assert_eq!(first_continuation["pagination"]["offset"], 1);
    assert_eq!(first_continuation["pagination"]["returned"], 1);
    assert_eq!(first_continuation["pagination"]["next_offset"], 2);
    assert_eq!(first_continuation["pagination"]["has_more"], true);
    let next_generation = first_continuation["pagination"]["generation"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(next_generation, large_generation);

    let final_continuation = client
        .payload(
            "task_list",
            json!({
                "milestone_ref":"013-large",
                "status":"all",
                "detail":"full",
                "limit":3,
                "offset":3,
                "generation":large_generation
            }),
        )
        .expect("MCP payload should succeed");
    assert_eq!(final_continuation["pagination"]["returned"], 1);
    assert!(final_continuation["pagination"]
        .get("next_offset")
        .is_none());
    assert_eq!(final_continuation["pagination"]["has_more"], false);

    let empty_page = client
        .payload(
            "task_list",
            json!({
                "milestone_ref":"013-large",
                "status":"cancelled",
                "limit":30,
                "offset":0
            }),
        )
        .expect("MCP payload should succeed");
    assert!(empty_page["tasks"].as_array().unwrap().is_empty());
    assert_eq!(empty_page["pagination"]["total"], 0);
    assert!(empty_page["pagination"].get("next_offset").is_none());
    assert_eq!(empty_page["pagination"]["has_more"], false);

    let missing_generation = client
        .call(
            "task_list",
            json!({"milestone_ref":"013-large","status":"all","limit":1,"offset":1}),
        )
        .expect("MCP call should succeed");
    assert!(
        missing_generation.1["error"].is_object()
            || missing_generation.1["result"]["isError"] == true
    );
    let negative_offset = client
        .call(
            "task_list",
            json!({"milestone_ref":"013-large","status":"all","limit":1,"offset":-1}),
        )
        .expect("MCP call should succeed");
    assert!(
        negative_offset.1["error"].is_object() || negative_offset.1["result"]["isError"] == true
    );
    let overflow_offset = client
        .call(
            "task_list",
            json!({
                "milestone_ref":"013-large",
                "status":"all",
                "limit":1,
                "offset":u64::MAX,
                "generation":large_generation
            }),
        )
        .expect("MCP call should succeed");
    assert!(
        overflow_offset.1["error"].is_object() || overflow_offset.1["result"]["isError"] == true
    );
    let changed_filter = client
        .call(
            "task_list",
            json!({
                "milestone_ref":"012-statuses",
                "status":"all",
                "detail":"full",
                "limit":3,
                "offset":1,
                "generation":large_generation
            }),
        )
        .expect("MCP call should succeed");
    assert!(changed_filter.1["error"].is_object() || changed_filter.1["result"]["isError"] == true);
    for changed_filters in [
        json!({
            "milestone_ref":"013-large",
            "status":"ready",
            "detail":"full",
            "limit":3,
            "offset":1,
            "generation":large_generation
        }),
        json!({
            "milestone_ref":"013-large",
            "status":"all",
            "detail":"compact",
            "limit":3,
            "offset":1,
            "generation":large_generation
        }),
        json!({
            "milestone_ref":"013-large",
            "status":"all",
            "detail":"full",
            "limit":3,
            "offset":1,
            "generation":"not-a-task-view-token"
        }),
    ] {
        let changed = client
            .call("task_list", changed_filters)
            .expect("MCP call should succeed");
        assert!(changed.1["error"].is_object() || changed.1["result"]["isError"] == true);
    }

    let index_before = client
        .payload(
            "code_map_search",
            json!({"pattern":"task_list_fixture","path":"src/lib.rs"}),
        )
        .expect("MCP payload should succeed");
    assert!(index_before["nodes"]["total"].as_u64().unwrap_or_default() > 0);
    workspace.write("src/lib.rs", "pub fn task_list_fixture_reindexed() {}\n");
    workspace.build_to(workspace.root().join(".forge/index.sqlite"));
    crate::common::mcp_client::invalidate_mcp_connection_cache(
        &workspace.root().join(".forge/index.sqlite"),
    )
    .unwrap();
    let index_after = client
        .payload(
            "code_map_search",
            json!({"pattern":"task_list_fixture_reindexed","path":"src/lib.rs"}),
        )
        .expect("MCP payload should succeed");
    assert!(index_after["nodes"]["total"].as_u64().unwrap_or_default() > 0);
    let index_only_continuation = client
        .payload(
            "task_list",
            json!({
                "milestone_ref":"013-large",
                "status":"all",
                "detail":"full",
                "limit":3,
                "offset":1,
                "generation":large_generation
            }),
        )
        .expect("MCP payload should succeed");
    assert_eq!(index_only_continuation["pagination"]["offset"], 1);
    assert_eq!(
        index_only_continuation["pagination"]["generation"],
        large_generation
    );

    let extended_manifest = format!(
        "{large_manifest}```yaml\ntask_ref: large-extra\ntarget: Extra task after the first page\nproof_policy: direct-proof\nscope: [src/large-extra.rs]\n```\n"
    );
    workspace.write("src/large-extra.rs", "pub fn extra_large_fixture() {}\n");
    let extended_path = "docs/milestones/013-large.md";
    workspace.write(extended_path, &extended_manifest);
    task_harness
        .sync_reference(Some(extended_path), None)
        .unwrap();
    let stale_task_token = client
        .call(
            "task_list",
            json!({
                "milestone_ref":"013-large",
                "status":"all",
                "detail":"full",
                "limit":3,
                "offset":1,
                "generation":large_generation
            }),
        )
        .expect("MCP call should succeed");
    assert!(
        stale_task_token.1["error"].is_object() || stale_task_token.1["result"]["isError"] == true
    );

    let current_page = client
        .payload(
            "task_list",
            json!({
                "milestone_ref":"013-large",
                "status":"all",
                "detail":"full",
                "limit":3,
                "offset":0
            }),
        )
        .expect("MCP payload should succeed");
    let current_generation = current_page["pagination"]["generation"]
        .as_str()
        .unwrap()
        .to_owned();
    task_harness
        .store
        .subtask_update("forge-mcp/m-large:large-0", "ready-case", "blocked", None)
        .unwrap();
    let stale_subtask_token = client
        .call(
            "task_list",
            json!({
                "milestone_ref":"013-large",
                "status":"all",
                "detail":"full",
                "limit":3,
                "offset":1,
                "generation":current_generation
            }),
        )
        .expect("MCP call should succeed");
    assert!(
        stale_subtask_token.1["error"].is_object()
            || stale_subtask_token.1["result"]["isError"] == true
    );
    let consistent_view = client
        .payload(
            "task_list",
            json!({
                "milestone_ref":"013-large",
                "status":"all",
                "detail":"full",
                "limit":1,
                "offset":0
            }),
        )
        .expect("MCP payload should succeed");
    let first_task = &consistent_view["tasks"][0];
    assert_eq!(first_task["task_id"], "forge-mcp/m-large:large-0");
    assert_eq!(first_task["status"], "ready");
    assert_eq!(
        first_task["subtasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|subtask| subtask["subtask_ref"] == "ready-case")
            .unwrap()["status"],
        "blocked"
    );

    client.finish().expect("MCP server should finish");
}


#[test]
fn task_list_byte_budget_matrix() {
    let mut fixture = task_list_fixture();
    let workspace = &fixture.workspace;
    let task_harness = &mut fixture.task_harness;
    let mut client = fixture.client;
    let LargeTaskFixture {
        target: large_target,
        frame_bytes: large_frame_bytes,
        response: large_response,
        page: large_page,
        generation: large_generation,
        ..
    } = large_task_fixture(workspace, &mut client, task_harness);
    assert!(large_frame_bytes <= 98_304, "frame was {large_frame_bytes} bytes");
    assert_ne!(large_response["result"]["isError"], true, "{large_response}");
    for (field, expected) in [("total", json!(4)), ("limit", json!(3)), ("offset", json!(0)), ("returned", json!(1)), ("byte_omitted", json!(2)), ("next_offset", json!(1)), ("has_more", json!(true))] {
        assert_eq!(large_page["pagination"][field], expected, "pagination {field}");
    }
    assert!(!large_generation.is_empty());
    assert_eq!(large_page["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(large_page["tasks"][0]["target"], large_target);
    assert_eq!(
        large_page["tasks"][0]["subtasks"].as_array().unwrap().len(),
        5
    );
    assert_eq!(large_page["tasks"][0]["subtasks"][0]["status"], "completed");
    let continuation_hint = large_page["pagination"]["continuation_hint"].to_string();
    for filter in ["013-large", "all", "full", &large_generation] {
        assert!(continuation_hint.contains(filter), "{continuation_hint}");
    }
    assert!(continuation_hint.contains("workspace"));

    let (large_markdown_bytes, large_markdown_response) = client
        .call(
            "task_list",
            json!({
                "milestone_ref":"013-large",
                "status":"all",
                "detail":"full",
                "limit":1,
                "offset":0,
                "format":"markdown"
            }),
        )
        .expect("MCP call should succeed");
    assert!(large_markdown_bytes > 64 * 1024);
    assert!(large_markdown_bytes <= 98_304);
    assert_ne!(large_markdown_response["result"]["isError"], true);
    let large_markdown = large_markdown_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(large_markdown.contains(&"λ".repeat(36_000)));
    for (subtask_ref, status, marker) in [
        ("done", "completed", "- [x]"),
        ("ready-case", "ready", "- [ ]"),
        ("progress-case", "in_progress", "- [ ]"),
        ("blocked-case", "blocked", "- [ ]"),
        ("cancelled-case", "cancelled", "- [ ]"),
    ] {
        let line = large_markdown
            .lines()
            .find(|line| line.contains(&format!("**Subtask**: `{subtask_ref}`")))
            .unwrap_or_else(|| panic!("missing rendered subtask {subtask_ref}: {large_markdown}"));
        assert!(line.contains(marker), "{subtask_ref}: {line}");
        assert!(line.contains(status), "{subtask_ref}: {line}");
    }

    let cli_markdown = in_process_cli::run_cli(
        workspace,
        &[
            "task",
            "list",
            "--milestone",
            "013-large",
            "--detail",
            "full",
            "--limit",
            "1",
            "--format",
            "markdown",
        ],
    );
    assert!(cli_markdown.status.success());
    assert!(cli_markdown.stdout.len() <= 98_304);
    let cli_markdown_text = String::from_utf8(cli_markdown.stdout).unwrap();
    assert!(cli_markdown_text.contains(&"λ".repeat(36_000)));

    let cli_page = cli_payload(
        workspace,
        &[
            "task",
            "list",
            "--milestone",
            "013-large",
            "--detail",
            "full",
            "--limit",
            "1",
            "--offset",
            "0",
        ],
    )
    .unwrap();
    assert_eq!(cli_page["pagination"]["limit"], 1);
    assert_eq!(cli_page["pagination"]["returned"], 1);
    assert_eq!(cli_page["pagination"]["has_more"], true);
    assert_eq!(cli_page["pagination"]["next_offset"], 1);

    let cli_full_budget_json_output = in_process_cli::run_cli_json(
        workspace,
        &[
            "task",
            "list",
            "--milestone",
            "013-large",
            "--status",
            "all",
            "--detail",
            "full",
            "--limit",
            "3",
            "--offset",
            "0",
        ],
    );
    assert!(
        cli_full_budget_json_output.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_full_budget_json_output.stderr)
    );
    let cli_full_budget_json_raw = cli_full_budget_json_output.stdout;
    assert!(cli_full_budget_json_raw.len() <= 98_304);
    let cli_full_budget_page: Value = serde_json::from_slice(&cli_full_budget_json_raw).unwrap();
    let cli_full_budget_returned = cli_full_budget_page["pagination"]["returned"]
        .as_u64()
        .unwrap();
    assert!(cli_full_budget_returned < 3);
    assert_eq!(cli_full_budget_page["pagination"]["has_more"], true);
    assert_eq!(
        cli_full_budget_page["pagination"]["next_offset"].as_u64(),
        Some(cli_full_budget_returned)
    );

    let cli_full_budget_md_output = in_process_cli::run_cli(
        workspace,
        &[
            "task",
            "list",
            "--milestone",
            "013-large",
            "--status",
            "all",
            "--detail",
            "full",
            "--limit",
            "3",
            "--offset",
            "0",
            "--format",
            "markdown",
        ],
    );
    assert!(
        cli_full_budget_md_output.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_full_budget_md_output.stderr)
    );
    let cli_full_budget_md = cli_full_budget_md_output.stdout;
    assert!(cli_full_budget_md.len() <= 98_304);

    client.finish().expect("MCP server should finish");
}
