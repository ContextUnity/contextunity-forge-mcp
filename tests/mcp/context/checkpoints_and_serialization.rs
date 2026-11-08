use super::*;

fn checkpoint_breadcrumb(subject: &str) -> String {
    format!(
        "forge-mcp/checkpoint-fixture/AC-001-hierarchical-acdd-gates-and-milestone-orchestration/tasks.build/checkpoint-{subject}/build"
    )
}

fn checkpoint_snapshot(workspace: &Workspace) -> Vec<u8> {
    let store = contextunity_forge_mcp::engine::tasks::store(workspace.root()).unwrap();
    serde_json::to_vec(&store.list_checkpoints().unwrap()).unwrap()
}

fn checkpoint_payload(workspace: &Workspace, breadcrumb: &str) -> Option<Value> {
    let store = contextunity_forge_mcp::engine::tasks::store(workspace.root()).unwrap();
    store
        .get_checkpoint(breadcrumb)
        .unwrap()
        .map(|checkpoint| checkpoint.state_payload)
}

#[test]
fn repair_checkpoint_page_shapes_are_opaque_and_persistence_is_unchanged() {
    let (workspace, harness) = McpTestHarness::with_files(&[(
        "forge-mcp.yaml",
        "tasks_db: .forge/tasks.sqlite\ntask_project: checkpoint-fixture\n",
    )]);
    let mut client = harness.expect("MCP initialize");
    let page = json!({"total":2,"offset":0,"limit":2,"generation":"saved-snapshot","next_offset":null,"has_more":false,"items":[{"text":"x".repeat(60_000)},{"text":"y".repeat(60_000)}]});
    for (subject, value) in [
        ("page", page.clone()),
        ("nested", json!({"snapshot":{"matches":page}})),
    ] {
        let name = checkpoint_breadcrumb(subject);
        client
            .payload(
                "session_checkpoint",
                json!({"action":"save","name":name,"content":value}),
            )
            .expect("MCP tool should succeed");
        let stored = checkpoint_snapshot(&workspace);
        assert_eq!(checkpoint_payload(&workspace, &name), Some(value.clone()));
        for format in [None, Some("markdown"), Some("json")] {
            let mut arguments = json!({"action":"get","name":name});
            if let Some(format) = format {
                arguments["format"] = json!(format);
            }
            let (_, response) = client
                .call("session_checkpoint", arguments)
                .expect("MCP transport");
            assert_eq!(
                response["result"]["isError"], true,
                "oversized {name} {format:?} get created a fabricated page"
            );
            assert!(response["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("checkpoint"));
            assert_eq!(checkpoint_snapshot(&workspace), stored);
        }
        let listed = client
            .payload("session_checkpoint", json!({"action":"list"}))
            .expect("MCP tool should succeed");
        assert_eq!(
            listed[&name]["bytes"],
            serde_json::to_vec(&value).unwrap().len()
        );
        let listed_markdown = client
            .text(
                "session_checkpoint",
                json!({"action":"list","format":"markdown"}),
            )
            .expect("MCP tool should succeed");
        assert!(listed_markdown.contains(&format!("**{name}**")));
        assert!(listed_markdown.contains(&listed[&name]["bytes"].to_string()));
        let cli_list = run_cli(&workspace, &["--format", "markdown", "checkpoint", "list"]);
        assertions::assert_command_succeeded(&cli_list);
        let cli_list = std::str::from_utf8(&cli_list.stdout).unwrap();
        assert!(cli_list.contains(&format!("**{name}**")));
        assert!(cli_list.contains(&listed[&name]["bytes"].to_string()));
        assert_eq!(listed_markdown.trim_end(), cli_list.trim_end());
        assert_eq!(checkpoint_snapshot(&workspace), stored);
    }
    let small = json!({"total":2,"offset":0,"limit":2,"generation":"saved","next_offset":null,"items":[1,2]});
    let small_breadcrumb = checkpoint_breadcrumb("small");
    client
        .payload(
            "session_checkpoint",
            json!({"action":"save","name":small_breadcrumb,"content":small}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(
        client
            .payload(
                "session_checkpoint",
                json!({"action":"get","name":small_breadcrumb})
            )
            .expect("MCP tool should succeed"),
        small
    );

    let opaque = json!({
        "generation":"checkpoint-generation-must-remain-complete",
        "freshness":{"output_root":"user-root-must-remain-complete","refresh":"none"},
        "metadata":{},"items":[],"count":0,"complete":false,
        "nested":{"total":0,"offset":0,"limit":1,"generation":"user-page-token","items":[]}
    });
    let format_breadcrumb = checkpoint_breadcrumb("format-contract");
    let (_, saved) = client
        .call(
            "session_checkpoint",
            json!({
                "action":"save","name":format_breadcrumb,"content":opaque,"format":"markdown"
            }),
        )
        .expect("MCP transport");
    assert_ne!(saved["result"]["isError"], true, "{saved}");
    let save_markdown = saved["result"]["content"][0]["text"].as_str().unwrap();
    assert!(save_markdown.contains("**action**: `\"save\"`"));
    assert!(save_markdown.contains(&format!("**name**: `\"{format_breadcrumb}\"`")));
    let cli_saved_content = json!({"cli":"checkpoint","generation":"user-token"});
    let cli_saved_content_argument = serde_json::to_string(&cli_saved_content).unwrap();
    let cli_save_breadcrumb = checkpoint_breadcrumb("cli-save-markdown");
    let cli_saved = run_cli(
        &workspace,
        &[
            "--format",
            "markdown",
            "checkpoint",
            "save",
            "--name",
            &cli_save_breadcrumb,
            "--content",
            &cli_saved_content_argument,
        ],
    );
    assertions::assert_command_succeeded(&cli_saved);
    let cli_save_text = std::str::from_utf8(&cli_saved.stdout).unwrap();
    assert!(cli_save_text.contains("**action**: `\"save\"`"));
    assert!(cli_save_text.contains(&format!("**name**: `\"{cli_save_breadcrumb}\"`")));
    let persisted = checkpoint_snapshot(&workspace);
    assert_eq!(
        checkpoint_payload(&workspace, &format_breadcrumb),
        Some(opaque.clone())
    );
    assert_eq!(
        checkpoint_payload(&workspace, &cli_save_breadcrumb),
        Some(cli_saved_content)
    );

    // Invalid presentation must be rejected before a real persisted mutation.
    for invalid in ["yaml", "", "JSON"] {
        let (_, rejected) = client.call(
            "session_checkpoint",
            json!({
                "action":"save","name":format_breadcrumb,"content":{"changed":true},"format":invalid
            }),
        ).expect("MCP transport");
        assert!(
            rejected.get("error").is_some() || rejected["result"]["isError"] == true,
            "invalid MCP format {invalid:?}: {rejected}"
        );
        assert_eq!(checkpoint_snapshot(&workspace), persisted);
        let rejected = run_cli(
            &workspace,
            &[
                "--format",
                invalid,
                "checkpoint",
                "save",
                "--name",
                &format_breadcrumb,
                "--content",
                "{\"changed\":true}",
            ],
        );
        assert!(!rejected.status.success(), "invalid CLI format {invalid:?}");
        assert_eq!(checkpoint_snapshot(&workspace), persisted);
    }

    let mut markdown = Vec::new();
    for format in [None, Some("markdown"), Some("json")] {
        let mut arguments = json!({"action":"get","name":format_breadcrumb});
        let mut cli_args = Vec::new();
        if let Some(format) = format {
            arguments["format"] = json!(format);
            cli_args.extend(["--format", format]);
        }
        cli_args.extend(["checkpoint", "get", "--name", &format_breadcrumb]);
        let (_, result) = client
            .call("session_checkpoint", arguments)
            .expect("MCP transport");
        assert_ne!(result["result"]["isError"], true, "{result}");
        let mcp_text = result["result"]["content"][0]["text"].as_str().unwrap();
        let cli = run_cli(&workspace, &cli_args);
        assertions::assert_command_succeeded(&cli);
        let cli_text = std::str::from_utf8(&cli.stdout).unwrap().trim_end();
        if format == Some("json") {
            for text in [mcp_text, cli_text] {
                assert_eq!(serde_json::from_str::<Value>(text).unwrap(), opaque);
            }
        } else {
            for (surface, text) in [("MCP", mcp_text), ("CLI", cli_text)] {
                assert!(
                    serde_json::from_str::<Value>(text).is_err(),
                    "{surface} {format:?} must render Markdown, received raw JSON: {text}"
                );
                for preserved in [
                    "checkpoint-generation-must-remain-complete",
                    "user-root-must-remain-complete",
                    "user-page-token",
                    "false",
                    "0",
                    "**metadata**: `{}`",
                    "**items**: `[]`",
                    "**nested**",
                    "  - **generation**: `\"user-page-token\"`",
                    "  - **items**: `[]`",
                ] {
                    assert!(
                        text.contains(preserved),
                        "{surface} lost {preserved}: {text}"
                    );
                }
            }
            assert_eq!(mcp_text.trim_end(), cli_text);
            markdown.push(mcp_text.to_owned());
        }
    }
    assert_eq!(
        markdown[0], markdown[1],
        "omission and explicit Markdown differ"
    );
    assert_eq!(checkpoint_snapshot(&workspace), persisted);

    let mut missing_renderings = Vec::new();
    for (subject, value, rendered) in [
        ("empty-array", json!([]), "[]"),
        ("empty-object", json!({}), "{}"),
    ] {
        let name = checkpoint_breadcrumb(subject);
        let (_, saved) = client
            .call(
                "session_checkpoint",
                json!({"action":"save","name":name,"content":value}),
            )
            .expect("MCP transport");
        assert_ne!(saved["result"]["isError"], true, "{saved}");
        let persisted = checkpoint_snapshot(&workspace);
        assert_eq!(checkpoint_payload(&workspace, &name), Some(value.clone()));

        let mut markdown = Vec::new();
        for format in [None, Some("markdown"), Some("json")] {
            let mut arguments = json!({"action":"get","name":name});
            let mut cli_args = Vec::new();
            if let Some(format) = format {
                arguments["format"] = json!(format);
                cli_args.extend(["--format", format]);
            }
            cli_args.extend(["checkpoint", "get", "--name", &name]);
            let (_, response) = client
                .call("session_checkpoint", arguments)
                .expect("MCP transport");
            assert_ne!(response["result"]["isError"], true, "{response}");
            let mcp_text = response["result"]["content"][0]["text"].as_str().unwrap();
            let cli = run_cli(&workspace, &cli_args);
            assertions::assert_command_succeeded(&cli);
            let cli_text = std::str::from_utf8(&cli.stdout).unwrap().trim_end();
            if format == Some("json") {
                for text in [mcp_text, cli_text] {
                    assert_eq!(serde_json::from_str::<Value>(text).unwrap(), value);
                }
            } else {
                for (surface, text) in [("MCP", mcp_text), ("CLI", cli_text)] {
                    if !text.contains(rendered) {
                        missing_renderings.push(format!(
                            "{surface} {format:?} omitted root checkpoint {name}: {text}"
                        ));
                    }
                }
                assert_eq!(mcp_text.trim_end(), cli_text);
                markdown.push(mcp_text.to_owned());
            }
            assert_eq!(checkpoint_snapshot(&workspace), persisted);
        }
        assert_eq!(
            markdown[0], markdown[1],
            "empty {name} omission and explicit Markdown differ"
        );
    }
    assert!(
        missing_renderings.is_empty(),
        "empty checkpoint roots were omitted: {}",
        missing_renderings.join("; ")
    );

    for (subject, content) in [
        ("mcp-delete-markdown", json!({"keep":false})),
        ("cli-delete-markdown", json!({"keep":0})),
    ] {
        let name = checkpoint_breadcrumb(subject);
        client
            .payload(
                "session_checkpoint",
                json!({"action":"save","name":name,"content":content}),
            )
            .expect("MCP tool should succeed");
    }
    let mcp_delete_breadcrumb = checkpoint_breadcrumb("mcp-delete-markdown");
    let (_, mcp_deleted) = client
        .call(
            "session_checkpoint",
            json!({"action":"delete","name":mcp_delete_breadcrumb,"format":"markdown"}),
        )
        .expect("MCP transport");
    assert_ne!(mcp_deleted["result"]["isError"], true, "{mcp_deleted}");
    let mcp_delete_text = mcp_deleted["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(mcp_delete_text.contains("**action**: `\"delete\"`"));
    assert!(mcp_delete_text.contains(&format!("**name**: `\"{mcp_delete_breadcrumb}\"`")));
    assert_eq!(checkpoint_payload(&workspace, &mcp_delete_breadcrumb), None);

    let cli_delete_breadcrumb = checkpoint_breadcrumb("cli-delete-markdown");
    let cli_deleted = run_cli(
        &workspace,
        &[
            "--format",
            "markdown",
            "checkpoint",
            "delete",
            "--name",
            &cli_delete_breadcrumb,
        ],
    );
    assertions::assert_command_succeeded(&cli_deleted);
    let cli_delete_text = std::str::from_utf8(&cli_deleted.stdout).unwrap();
    assert!(cli_delete_text.contains("**action**: `\"delete\"`"));
    assert!(cli_delete_text.contains(&format!("**name**: `\"{cli_delete_breadcrumb}\"`")));
    assert_eq!(checkpoint_payload(&workspace, &cli_delete_breadcrumb), None);
    assert!(!workspace.path(".forge/checkpoints.json").exists());

    let shared_database = workspace.path(".forge/tasks.sqlite");
    let other_workspace = Workspace::with_files(&[(
        "forge-mcp.yaml",
        &format!(
            "tasks_db: {}\ntask_project: checkpoint-other\n",
            shared_database.display()
        ),
    )]);
    let mut other_client = McpTestHarness::new(&other_workspace).expect("MCP initialize");
    let other_breadcrumb = "forge-mcp/checkpoint-other/AC-001-hierarchical-acdd-gates-and-milestone-orchestration/tasks.build/checkpoint-shared/build".to_owned();
    other_client
        .payload(
            "session_checkpoint",
            json!({"action":"save","name":other_breadcrumb,"content":{"project":"other"}}),
        )
        .expect("MCP tool should succeed");
    let other_list = other_client
        .payload("session_checkpoint", json!({"action":"list"}))
        .expect("MCP tool should succeed");
    assert_eq!(other_list.as_object().unwrap().len(), 1);
    assert_eq!(
        other_list[&other_breadcrumb]["bytes"],
        serde_json::to_vec(&json!({"project":"other"}))
            .unwrap()
            .len()
    );
    let primary_list = client
        .payload("session_checkpoint", json!({"action":"list"}))
        .expect("MCP tool should succeed");
    assert!(primary_list.get(&other_breadcrumb).is_none());
    assert!(primary_list.get(checkpoint_breadcrumb("shared")).is_none());
    let (_, cross_project) = other_client
        .call(
            "session_checkpoint",
            json!({"action":"get","name":checkpoint_breadcrumb("page")}),
        )
        .expect("MCP transport");
    assert_eq!(cross_project["result"]["isError"], true);
    client.finish().expect("server must finish successfully");
    other_client
        .finish()
        .expect("server must finish successfully");
}

#[tokio::test]
async fn transport_accepts_exact_bound_and_replaces_oversized_frames() {
    use tokio::io::AsyncWriteExt;

    let frame_for = |size: usize| {
        let prefix = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"text\":\"";
        let suffix = "\"}}\n";
        format!(
            "{prefix}{}{suffix}",
            "x".repeat(size - prefix.len() - suffix.len())
        )
    };
    for size in [MAX_OUTPUT_BYTES - 1, MAX_OUTPUT_BYTES] {
        let mut writer = response::BoundedWriter::new(tokio::io::sink());
        let frame = frame_for(size);
        assert_eq!(frame.len(), size);
        serde_json::from_str::<Value>(frame.trim_end()).unwrap();
        writer.write_all(frame.as_bytes()).await.unwrap();
        writer.flush().await.unwrap();
    }
    let mut output = Vec::new();
    let mut writer = response::BoundedWriter::new(&mut output);
    let oversized = frame_for(MAX_OUTPUT_BYTES + 1);
    writer.write_all(oversized.as_bytes()).await.unwrap();
    writer.flush().await.unwrap();
    assert!(output.len() < MAX_OUTPUT_BYTES);
    let error: Value = serde_json::from_slice(&output).unwrap();
    assert!(error.get("error").is_some());
}

#[test]
fn serialization_counts_second_escaping_and_bounds_a_single_item() {
    let policy = ResponsePolicy::default();
    let result = response::result_with_format(
        Ok(
            json!({"nodes":{"total":2,"offset":0,"limit":30,"items":[{"id":"first","text":"\"\\\n".repeat(6000)},{"id":"second","text":"\"\\\n".repeat(6000)}],"has_more":false,"next_offset":null,"generation":"snapshot"}}),
        ),
        &policy,
        OutputFormat::Json,
    );
    assert!(response::serialized_bytes(&result) <= MAX_OUTPUT_BYTES - 256);
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let wire = serde_json::to_value(result).unwrap();
    let payload: Value =
        serde_json::from_str(wire["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(crate::json_array(&payload["nodes"]["items"]).len(), 1);
    assert_eq!(payload["nodes"]["total"], 2);
    assert_eq!(payload["nodes"]["limit"], 30);
    assert_eq!(payload["nodes"]["offset"], 0);
    assert_eq!(payload["nodes"]["generation"], "snapshot");
    assert_eq!(payload["nodes"]["has_more"], true);
    assert_eq!(payload["nodes"]["byte_limited"], true);
    assert_eq!(payload["nodes"]["next_offset"], 1);
    let item_hint = payload["nodes"]["continuation_hint"].as_str().unwrap();
    assert!(item_hint.contains("same tool and filters"));
    assert!(item_hint.contains("offset=1"));
    assert!(item_hint.contains("generation"));
    let result = response::result_with_format(
        Ok(json!({"scalar":"\"\\Привіт".repeat(30_000)})),
        &policy,
        OutputFormat::Json,
    );
    assert_eq!(result.is_error, Some(true));
    assert!(response::serialized_bytes(&result) <= MAX_OUTPUT_BYTES - 256);

    let fitting_text = "λ".repeat(36_000);
    assert!(fitting_text.len() > 64 * 1024);
    assert!(fitting_text.len() < 96 * 1024);
    let fitting = response::result_with_format(
        Ok(json!({
            "tasks": [{"task_id":"fit-96kib","target":fitting_text.clone()}],
            "pagination": {
                "total": 1,
                "limit": 1,
                "offset": 0,
                "returned": 1,
                "next_offset": null,
                "has_more": false,
                "generation": "task-view-fit",
                "byte_omitted": 0
            }
        })),
        &policy,
        OutputFormat::Json,
    );
    assert_ne!(fitting.is_error, Some(true), "{fitting:?}");
    assert_eq!(MAX_OUTPUT_BYTES, 98_304);
    assert_eq!(policy.max_output_bytes, 98_304);
    assert!(response::serialized_bytes(&fitting) <= MAX_OUTPUT_BYTES - 256);
    let fitting_wire = serde_json::to_value(fitting).unwrap();
    let fitting_payload: Value =
        serde_json::from_str(fitting_wire["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(fitting_payload["tasks"][0]["target"], fitting_text);

    let lower_policy = ResponsePolicy {
        max_output_bytes: 4_096,
        ..ResponsePolicy::default()
    };
    let lower_limit = response::result_with_format(
        Ok(json!({"items":[{"id":"configured-limit","text":"x".repeat(5_000)}]})),
        &lower_policy,
        OutputFormat::Json,
    );
    assert_eq!(lower_limit.is_error, Some(true));
    assert!(response::serialized_bytes(&lower_limit) <= 4_096 - 256);

    let task_statuses = ["completed", "ready", "in_progress", "blocked", "cancelled"];
    let task_records = (0..3)
        .map(|index| {
            json!({
                "task_id": format!("forge-mcp/m-large:task-{index}"),
                "target": format!("λ{} \"\\", "λ".repeat(32_000)),
                "subtasks": task_statuses.iter().map(|status| json!({
                    "subtask_ref": status,
                    "status": status
                })).collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    let trimmed_tasks = response::result_with_format(
        Ok(json!({
            "tasks": task_records,
            "pagination": {
                "total": 7,
                "limit": 3,
                "offset": 2,
                "returned": 3,
                "next_offset": 5,
                "has_more": true,
                "generation": "task-view-snapshot",
                "byte_omitted": 0,
                "continuation_hint": {
                    "workspace": "main",
                    "milestone_ref": "010-active",
                    "status": "all",
                    "detail": "full",
                    "generation": "task-view-snapshot"
                }
            }
        })),
        &policy,
        OutputFormat::Json,
    );
    assert_ne!(trimmed_tasks.is_error, Some(true), "{trimmed_tasks:?}");
    assert!(response::serialized_bytes(&trimmed_tasks) <= MAX_OUTPUT_BYTES - 256);
    let task_wire = serde_json::to_value(trimmed_tasks).unwrap();
    let task_page: Value =
        serde_json::from_str(task_wire["content"][0]["text"].as_str().unwrap()).unwrap();
    let emitted = crate::json_array(&task_page["tasks"]);
    assert_eq!(emitted.len(), 1);
    assert_eq!(crate::json_array(&emitted[0]["subtasks"]).len(), 5);
    assert_eq!(emitted[0]["subtasks"][0]["status"], "completed");
    assert_eq!(task_page["pagination"]["total"], 7);
    assert_eq!(task_page["pagination"]["limit"], 3);
    assert_eq!(task_page["pagination"]["offset"], 2);
    assert_eq!(task_page["pagination"]["returned"], 1);
    assert_eq!(task_page["pagination"]["byte_omitted"], 2);
    assert_eq!(task_page["pagination"]["next_offset"], 3);
    assert_eq!(task_page["pagination"]["has_more"], true);
    assert_eq!(task_page["pagination"]["generation"], "task-view-snapshot");
    let hint = task_page["pagination"]["continuation_hint"].to_string();
    for filter in ["main", "010-active", "all", "full", "task-view-snapshot"] {
        assert!(
            hint.contains(filter),
            "continuation hint omitted {filter}: {hint}"
        );
    }

    let final_page = response::result_with_format(
        Ok(json!({
            "tasks": [{"task_id":"last","target":"last intact record","subtasks":[]}],
            "pagination": {
                "total": 4,
                "limit": 3,
                "offset": 3,
                "returned": 1,
                "has_more": false,
                "generation": "task-view-final",
                "byte_omitted": 0
            }
        })),
        &policy,
        OutputFormat::Json,
    );
    assert_ne!(final_page.is_error, Some(true), "{final_page:?}");
    let final_wire = serde_json::to_value(final_page).unwrap();
    let final_payload: Value =
        serde_json::from_str(final_wire["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(final_payload["pagination"].get("next_offset").is_none());
    assert_eq!(final_payload["pagination"]["has_more"], false);

    let empty_page = response::result_with_format(
        Ok(json!({
            "tasks": [],
            "pagination": {
                "total": 0,
                "limit": 30,
                "offset": 0,
                "returned": 0,
                "has_more": false,
                "generation": "task-view-empty",
                "byte_omitted": 0
            }
        })),
        &policy,
        OutputFormat::Json,
    );
    assert_ne!(empty_page.is_error, Some(true), "{empty_page:?}");
    let empty_wire = serde_json::to_value(empty_page).unwrap();
    let empty_payload: Value =
        serde_json::from_str(empty_wire["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(crate::json_array(&empty_payload["tasks"]).is_empty());
    assert!(empty_payload["pagination"].get("next_offset").is_none());

    let oversized_task = response::result_with_format(
        Ok(json!({
            "tasks": [{"task_id":"too-large","target":"界".repeat(40_000),"subtasks":[]}],
            "pagination": {
                "total": 1,
                "limit": 1,
                "offset": 0,
                "returned": 1,
                "next_offset": null,
                "has_more": false,
                "generation": "task-view-oversized",
                "byte_omitted": 0
            }
        })),
        &policy,
        OutputFormat::Json,
    );
    assert_eq!(oversized_task.is_error, Some(true));
    assert!(response::serialized_bytes(&oversized_task) <= MAX_OUTPUT_BYTES - 256);
    let oversized_wire = serde_json::to_value(oversized_task).unwrap();
    let oversized_message = oversized_wire["content"][0]["text"].as_str().unwrap();
    assert!(
        oversized_message.to_ascii_lowercase().contains("compact")
            || oversized_message.to_ascii_lowercase().contains("narrow")
    );
    assert!(
        oversized_message.to_ascii_lowercase().contains("task")
            || oversized_message.to_ascii_lowercase().contains("record")
    );
    assert!(!oversized_message.contains("Output exceeds the response byte limit"));
}
