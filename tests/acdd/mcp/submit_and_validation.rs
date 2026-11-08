use std::fs;

use serde_json::json;

use crate::acdd::runner;
use crate::common::mcp_harness::McpTestHarness;

use super::cli_payload;

#[test]
fn command_submit_validates_evidence_and_uses_cli_mcp_cache() {
    let (root, store, id) = runner::command_fixture(
        "printf run >> .forge/counter; printf checked",
        Some(runner::output_spec(false)),
    );
    let mut client = McpTestHarness::new(&root).expect("MCP new should succeed");
    for (extras, expected_diagnostic) in [
        (json!({"evidence":null}), "evidence is not a valid JSON object"),
        (json!({"evidence":42}), "evidence is not a valid JSON object"),
        (json!({"evidence_ref":"proof.yaml"}), "Unrecognized argument(s): evidence_ref"),
        (json!({"unknown":true}), "Unrecognized argument(s): unknown"),
        (json!({"evidence":{}, "timeout_seconds":5}), "timeout_seconds requires omitted evidence"),
    ] {
        let mut args = json!({"task_id":id, "stage":"contract", "action":"pass"});
        args.as_object_mut()
            .unwrap()
            .extend(extras.as_object().unwrap().clone());
        let (_, result) = client
            .call("task_submit", args)
            .expect("MCP call should succeed");
        assert!(
            result["error"].is_object() || result["result"]["isError"] == true,
            "{result}"
        );
        let error_text = serde_json::to_string(&result).unwrap();
        assert!(
            error_text.contains(expected_diagnostic),
            "expected diagnostic '{expected_diagnostic}' in error response: {error_text}"
        );
    }
    assert!(!root.root().join(".forge/task-traces").exists());
    let verified = cli_payload(&root, &["task", "verify", &id, "--timeout", "5"]).unwrap();
    assert_eq!(verified["verification"]["exit_code"], 0, "{verified}");
    let checked = client
        .payload(
            "task_manage",
            json!({"action":"verify", "task_id":id, "timeout_seconds":5}),
        )
        .expect("MCP payload should succeed");
    assert_eq!(
        checked["verification"]["diagnostic_mode"], "pass",
        "{checked}"
    );
    assert_eq!(store.inspect(&id).unwrap().stage, "contract");
    let catalog = client
        .request_value("tools/list", json!({}))
        .expect("MCP request_value should succeed");
    let submit = catalog["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "task_submit")
        .unwrap();
    assert!(!submit["inputSchema"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|name| name == "evidence"));
    assert!(submit["inputSchema"]["properties"]
        .get("timeout_seconds")
        .is_some());
    let submitted = client
        .payload(
            "task_submit",
            json!({"task_id":id, "stage":"contract", "action":"pass", "timeout_seconds":5}),
        )
        .expect("MCP payload should succeed");
    assert_eq!(submitted["stage"], "build", "{submitted}");
    assert_eq!(
        fs::read_to_string(root.root().join(".forge/counter")).unwrap(),
        "run"
    );
    client.finish().expect("MCP server should finish");
}
