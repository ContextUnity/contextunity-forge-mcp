use crate::common::{
    assertions::{assert_command_succeeded, assert_rpc_succeeded, mcp_payload},
    mcp_client::{in_process_server, run_cli, StdioClient},
    Workspace,
};
use serde_json::{json, Value};

#[test]
fn shared_mcp_harness_supports_in_process_stdio_and_cli_seams() {
    let workspace = Workspace::new();
    workspace.write("service.py", "def entry(): return 1\n");
    let database = workspace.path(".forge/index.sqlite");
    workspace.build_to(&database);

    let server = in_process_server(&workspace);
    let stored_nodes = server
        .read(|conn| Ok(json!(conn.query_row::<i64, _, _>("SELECT count(*) FROM nodes", [], |row| row.get(0))?)))
        .unwrap();
    assert!(stored_nodes.as_i64().unwrap() > 0);

    let mut client = StdioClient::new(&workspace);
    let response = client.request_value("tools/list", json!({}));
    assert_rpc_succeeded(&response);
    assert!(!response["result"]["tools"].as_array().unwrap().is_empty());
    let (_, response) = client.call("code_map_search", json!({"pattern":"entry","exact":true}));
    let payload: Value = mcp_payload(&response);
    assert_eq!(payload["nodes"]["total"], 1);

    let output = run_cli(&workspace, &["build"]);
    assert_command_succeeded(&output);
}
