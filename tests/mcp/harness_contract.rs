use crate::cli::run_cli;
use crate::common::{
    assertions::{assert_command_succeeded, assert_rpc_succeeded, mcp_payload},
    mcp_client::in_process_server,
    Workspace,
};
use crate::harness::McpTestHarness;

#[test]
fn malformed_handshake_preserves_the_real_server_failure_through_finish() {
    use crate::harness::HarnessError;
    let workspace = Workspace::with_files(&[]);
    let mut client = McpTestHarness::spawn(workspace.root(), &workspace.db(), "handshake-failure")
        .expect("real server fixture");
    let error = client
        .request("tools/list", json!({}))
        .expect_err("server must reject missing initialize");
    assert!(
        matches!(error, HarnessError::Server { phase: "serve", ref cause } if cause.contains("expect initialized request")),
        "{error}"
    );
    let finished = client
        .finish()
        .expect_err("finish must retain the same server failure");
    assert!(
        matches!(finished, HarnessError::Server { phase: "serve", ref cause } if cause.contains("expect initialized request")),
        "{finished}"
    );
}

#[test]
fn cancellation_after_the_last_response_is_reported_by_finish() {
    use crate::harness::HarnessError;
    let workspace = Workspace::with_files(&[]);
    let mut client =
        McpTestHarness::spawn(workspace.root(), &workspace.db(), "cancellation-failure")
            .expect("real server fixture");
    let initialized = client
        .request_value(
            "initialize",
            json!({
                "protocolVersion":"2025-03-26", "capabilities":{},
                "clientInfo":{"name":"in-process-test","version":"1"}
            }),
        )
        .expect("initialize real server");
    assert!(initialized.get("error").is_none(), "{initialized}");
    client
        .notify("notifications/initialized", json!({}))
        .expect("notify initialized");
    let response = client
        .request_value("tools/list", json!({}))
        .expect("real tools response");
    assert!(response["result"]["tools"].is_array(), "{response}");
    let receiver = client
        .server_control
        .take()
        .expect("server exposes its cancellation token");
    let cancellation = client.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(5), receiver)
            .await
            .expect("server cancellation token should arrive")
            .expect("server cancellation channel should remain open")
    });
    cancellation.cancel();
    client.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while client
                .server_task
                .as_ref()
                .is_some_and(|task| !task.is_finished())
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cancelled service should finish");
    });
    let error = client
        .finish()
        .expect_err("late cancellation must not be green");
    assert!(
        matches!(error, HarnessError::Server { phase: "waiting", ref cause } if cause == "Cancelled"),
        "{error}"
    );
}
use serde_json::{json, Value};

#[test]
fn shared_mcp_harness_supports_server_rpc_and_cli_seams() {
    let workspace = Workspace::with_files(&[("service.py", "def entry(): return 1\n")]);
    let database = workspace.path(".forge/index.sqlite");
    workspace.build_to(&database);

    let server =
        in_process_server(&workspace).with_cache_dir(workspace.path(".forge/extraction-cache"));
    let stored_nodes = server
        .read(|conn| Ok(json!(crate::sqlite_fixture::stored_node_count(conn)?)))
        .unwrap();
    assert!(stored_nodes.as_i64().unwrap() > 0);

    let mut client = McpTestHarness::new(&workspace).expect("MCP initialize");
    let response = client
        .request_value("tools/list", json!({}))
        .expect("MCP transport");
    assert_rpc_succeeded(&response);
    assert!(!response["result"]["tools"].as_array().unwrap().is_empty());
    let (_, response) = client
        .call(
            "code_map_search",
            json!({"pattern":"entry","exact":true,"format":"json"}),
        )
        .expect("MCP transport");
    let payload: Value = mcp_payload(&response);
    assert_eq!(payload["nodes"]["total"], 1);
    client.finish().expect("server must finish successfully");

    let output = run_cli(&workspace, &["build"]);
    assert_command_succeeded(&output);
}

#[test]
fn trusted_mcp_sessions_preserve_claim_and_next_independence() {
    for next in [false, true] {
        let workspace =
            crate::harness::session_workspace().expect("real session fixture should initialize");
        let mut builder =
            McpTestHarness::with_session(&workspace, "builder-session").expect("MCP initialize");
        builder
            .payload(
                "task_manage",
                json!({"action":"sync","milestone_ref":"010"}),
            )
            .expect("MCP tool should succeed");
        let build_claim = builder
            .payload(
                "task_claim",
                json!({"task_id":"test/m-session:first","stage":"build","worker_id":"builder","worktree":workspace.root(),"bundle":false}),
            )
            .expect("MCP tool should succeed");
        let claimed_task_id = build_claim["task_id"]
            .as_str()
            .expect("initial task claim includes its task id");
        builder
            .payload(
                "task_submit",
                json!({
                    "task_id":claimed_task_id,
                    "stage":"build",
                    "action":"pass",
                    "evidence": {
                        "task_id":claimed_task_id,
                        "stage":"build",
                        "claim_revision":build_claim["claim_revision"],
                        "contract_revision":build_claim["contract_revision"],
                        "worker_id":"builder",
                        "worktree":workspace.root(),
                        "proof":{"none_proof":{}}
                    }
                }),
            )
            .expect("MCP tool should succeed");
        let args = if next {
            json!({"action":"next","worker_id":"reviewer","worktree":workspace.root()})
        } else {
            json!({"task_id":"test/m-session:first","stage":"review","worker_id":"reviewer","worktree":workspace.root(),"bundle":false})
        };
        let tool = if next { "task_manage" } else { "task_claim" };
        let (_, same) = builder.call(tool, args.clone()).expect("MCP transport");
        assert!(
            same.to_string().contains("TASK_REVIEW_NOT_INDEPENDENT"),
            "{same}"
        );
        let mut reviewer =
            McpTestHarness::with_session(&workspace, "review-session").expect("MCP initialize");
        let claimed = reviewer
            .payload(tool, args)
            .expect("MCP tool should succeed");
        assert_eq!(claimed["stage"], "review", "{claimed}");
        let persisted = crate::sqlite_fixture::claim_session_fingerprint(
            &workspace,
            "test/m-session:first",
            claimed["claim_revision"].as_u64().unwrap(),
        )
        .unwrap();
        assert_eq!(persisted, "review-session");
        builder.finish().expect("server must finish successfully");
        reviewer.finish().expect("server must finish successfully");
    }
}

#[test]
fn trusted_mcp_sessions_reject_builder_submit_with_typed_or_automatic_evidence() {
    for automatic in [false, true] {
        let workspace =
            crate::harness::session_workspace().expect("real session fixture should initialize");
        let mut builder =
            McpTestHarness::with_session(&workspace, "builder-session").expect("MCP initialize");
        builder
            .payload(
                "task_manage",
                json!({"action":"sync","milestone_ref":"010"}),
            )
            .expect("MCP tool should succeed");
        let build_claim = builder
            .payload(
                "task_claim",
                json!({"task_id":"test/m-session:first","stage":"build","worker_id":"builder","worktree":workspace.root(),"bundle":false}),
            )
            .expect("MCP tool should succeed");
        let claimed_task_id = build_claim["task_id"]
            .as_str()
            .expect("initial task claim includes its task id");
        builder
            .payload(
                "task_submit",
                json!({
                    "task_id":claimed_task_id,
                    "stage":"build",
                    "action":"pass",
                    "evidence": {
                        "task_id":claimed_task_id,
                        "stage":"build",
                        "claim_revision":build_claim["claim_revision"],
                        "contract_revision":build_claim["contract_revision"],
                        "worker_id":"builder",
                        "worktree":workspace.root(),
                        "proof":{"none_proof":{}}
                    }
                }),
            )
            .expect("MCP tool should succeed");
        let mut reviewer =
            McpTestHarness::with_session(&workspace, "review-session").expect("MCP initialize");
        reviewer.payload("task_claim", json!({"task_id":"test/m-session:first","stage":"review","worker_id":"reviewer","worktree":workspace.root(),"bundle":false})).expect("MCP tool should succeed");
        let checked = reviewer
            .payload(
                "task_manage",
                json!({"action":"verify","task_id":"test/m-session:first"}),
            )
            .expect("MCP tool should succeed");
        let mut args = json!({"task_id":"test/m-session:first","stage":"review","action":"pass"});
        if !automatic {
            args["evidence"] = checked["evidence"].clone();
        }
        let (_, denied) = builder
            .call("task_submit", args.clone())
            .expect("MCP transport");
        assert!(
            denied.to_string().contains("TASK_REVIEW_NOT_INDEPENDENT"),
            "automatic={automatic}: {denied}"
        );
        assert_eq!(
            reviewer
                .payload(
                    "task_manage",
                    json!({"action":"inspect","task_id":"test/m-session:first"})
                )
                .expect("MCP tool should succeed")["stage"],
            "review"
        );
        assert_eq!(
            reviewer
                .payload("task_submit", args)
                .expect("MCP tool should succeed")["stage"],
            "deliver"
        );
        builder.finish().expect("server must finish successfully");
        reviewer.finish().expect("server must finish successfully");
    }
}

#[test]
fn mcp_session_context_is_absent_from_schemas_and_rejected_in_json() {
    let workspace = Workspace::with_files(&[]);
    workspace.build();
    let mut client =
        McpTestHarness::with_session(&workspace, "trusted-session").expect("MCP initialize");
    let catalog = client
        .request_value("tools/list", json!({}))
        .expect("MCP transport");
    for name in ["task_claim", "task_submit", "task_manage"] {
        let schema = catalog["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap()["inputSchema"]
            .clone();
        assert!(
            !schema.to_string().contains("session_fingerprint"),
            "{name}: {schema}"
        );
        let mut arguments = match name {
            "task_claim" => {
                json!({"task_id":"test/m-session:first","stage":"build","worker_id":"builder","worktree":workspace.root()})
            }
            "task_submit" => {
                json!({"task_id":"test/m-session:first","stage":"build","action":"pass"})
            }
            _ => json!({"action":"next","worker_id":"builder","worktree":workspace.root()}),
        };
        arguments["session_fingerprint"] = json!("spoof");
        let (_, response) = client.call(name, arguments).expect("MCP transport");
        assert!(
            response["error"].is_object() || response["result"]["isError"] == true,
            "{response}"
        );
        assert!(
            response.to_string().contains("rejected_arguments")
                && response.to_string().contains("session_fingerprint"),
            "{response}"
        );
    }
    client.finish().expect("server must finish successfully");
}
