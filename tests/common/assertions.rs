use serde_json::Value;
use std::process::Output;

/// Fails with both output streams when a test command exits unsuccessfully.
pub fn assert_command_succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "command failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Verifies that a JSON-RPC response did not return a protocol error.
pub fn assert_rpc_succeeded(response: &Value) {
    assert!(response.get("error").is_none(), "{response}");
}

/// Parses the first JSON payload in a successful MCP tool response.
pub fn mcp_payload(response: &Value) -> Value {
    assert_rpc_succeeded(response);
    assert_ne!(response["result"]["isError"], true, "{response}");
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("MCP tool response should include a text payload");
    serde_json::from_str(text).expect("MCP tool response text should contain JSON")
}
