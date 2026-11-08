#![cfg(feature = "lang-python")]

use crate::common::Workspace;
use contextunity_forge_mcp::{core::commitments, db::reader};

#[test]
fn fastmcp_tool_decorators_link_to_handlers_without_claiming_other_tool_methods() {
    let source = r#"from fastmcp import FastMCP
mcp = FastMCP("Example")
@mcp.tool()
def plain(): return 1
@mcp.tool(name="public_name")
def internal(): return 2
@mcp.tool(name=runtime_name)
def dynamic(): return 3
class Other:
    def tool(self, function): return function
other = Other()
@other.tool()
def unrelated(): return 4
mcp = None
@mcp.tool()
def after_rebind(): return 5
"#;
    let workspace = Workspace::with_files(&[("server.py", source)]);
    workspace.build();
    let mut client = crate::harness::McpTestHarness::new(&workspace).expect("MCP initialize");
    let registrations = client
        .payload(
            "code_map_query",
            serde_json::json!({"operation":"sql","selector":"SELECT name,details FROM nodes WHERE kind='tool_registration' ORDER BY line"}),
        )
        .expect("MCP tool should succeed");
    let registrations = registrations["rows"]["items"].as_array().unwrap();
    assert_eq!(
        registrations
            .iter()
            .map(|row| row["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["plain", "public_name", "tool@7"]
    );
    assert!(registrations[1]["details"]
        .as_str()
        .unwrap()
        .contains("\"advertised_name\":\"public_name\""));
    assert!(registrations[2]["details"]
        .as_str()
        .unwrap()
        .contains("\"name_status\":\"dynamic\""));
    let handlers = client
        .payload(
            "code_map_query",
            serde_json::json!({"operation":"sql","selector":"SELECT dst.name FROM edges e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE src.kind='tool_registration' AND e.kind='handles' ORDER BY src.line"}),
        )
        .expect("MCP tool should succeed");
    let handlers = handlers["rows"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(handlers, ["plain", "internal", "dynamic"]);

    workspace.write(
        "server.py",
        &source.replace("@mcp.tool(name=\"public_name\")\n", ""),
    );
    workspace.delta(&["server.py"]);
    let remaining = client
        .payload(
            "code_map_query",
            serde_json::json!({"operation":"sql","selector":"SELECT count(*) AS total FROM nodes WHERE kind='tool_registration'"}),
        )
        .expect("MCP tool should succeed");
    assert_eq!(remaining["rows"]["items"][0]["total"], 2);
    let delta = workspace.open();
    commitments::verify(&delta).unwrap();
    let cold_path = workspace.path(".forge/cold.sqlite");
    workspace.build_to(&cold_path);
    let cold = reader::open(&cold_path, workspace.root()).unwrap();
    commitments::verify(&cold).unwrap();
    assert_eq!(
        crate::sqlite_fixture::edge_occurrence_snapshot(&delta, "server.py").unwrap(),
        crate::sqlite_fixture::edge_occurrence_snapshot(&cold, "server.py").unwrap()
    );
    client.finish().expect("server must finish successfully");
}

#[test]
fn bare_decorator_and_import_alias_register_only_proven_fastmcp_instance() {
    let source = r#"from fastmcp import FastMCP as FM
import fastmcp as fm
from unrelated import FastMCP as OtherFastMCP
server = FM()
module_server = fm.FastMCP()
other = OtherFastMCP()
@server.tool
def listed(): return 1
@module_server.tool()
def from_module(): return 3
@other.tool
def unverified(): return 2
FM = OtherFastMCP
spoof = FM()
@spoof.tool()
def after_rebind(): return 4
"#;
    let mut client =
        crate::harness::McpTestHarness::indexed(&[("server.py", source)]).expect("MCP initialize");
    let registrations = client
        .payload(
            "code_map_query",
            serde_json::json!({"operation":"sql","selector":"SELECT name FROM nodes WHERE kind='tool_registration' ORDER BY line"}),
        )
        .expect("MCP tool should succeed");
    let names: Vec<_> = registrations["rows"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["listed", "from_module"]);
    client.finish().expect("server must finish successfully");
}
