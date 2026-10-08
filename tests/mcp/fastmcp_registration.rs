#![cfg(feature = "lang-python")]

use crate::common::Workspace;
use contextunity_forge_mcp::{core::commitments, db::reader};
use rusqlite::Connection;

fn graph(conn: &Connection) -> Vec<String> {
    conn.prepare("SELECT (SELECT id FROM nodes WHERE node_hash=edge_occurrences.src_hash)||'|'||(SELECT id FROM nodes WHERE node_hash=edge_occurrences.dst_hash)||'|'||kind FROM edge_occurrences WHERE (SELECT path FROM path_dictionary WHERE path_id=edge_occurrences.owner_id)='server.py' ORDER BY (SELECT id FROM nodes WHERE node_hash=edge_occurrences.src_hash),(SELECT id FROM nodes WHERE node_hash=edge_occurrences.dst_hash),kind")
        .unwrap().query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect()
}

fn assert_cold_equivalent(workspace: &Workspace) {
    let delta = workspace.open();
    commitments::verify(&delta).unwrap();
    let cold_path = workspace.path(".forge/cold.sqlite");
    workspace.build_to(&cold_path);
    let cold = reader::open(&cold_path, workspace.root()).unwrap();
    commitments::verify(&cold).unwrap();
    assert_eq!(graph(&delta), graph(&cold));
}

#[test]
fn fastmcp_tool_decorators_link_to_handlers_without_claiming_other_tool_methods() {
    let workspace = Workspace::new();
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
    workspace.write("server.py", source);
    workspace.build();
    let conn = workspace.open();
    let mut stmt = conn
        .prepare("SELECT name,details FROM nodes WHERE kind='tool_registration' ORDER BY line")
        .unwrap();
    let registrations: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        registrations
            .iter()
            .map(|row| row.0.as_str())
            .collect::<Vec<_>>(),
        ["plain", "public_name", "tool@7"]
    );
    assert!(registrations[1]
        .1
        .contains("\"advertised_name\":\"public_name\""));
    assert!(registrations[2].1.contains("\"name_status\":\"dynamic\""));
    let handlers: Vec<String> = conn.prepare("SELECT dst.name FROM edges e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE src.kind='tool_registration' AND e.kind='handles' ORDER BY src.line")
        .unwrap().query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(handlers, ["plain", "internal", "dynamic"]);
    drop(stmt);
    drop(conn);

    workspace.write(
        "server.py",
        &source.replace("@mcp.tool(name=\"public_name\")\n", ""),
    );
    workspace.delta(&["server.py"]);
    let conn = workspace.open();
    let remaining: i64 = conn
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind='tool_registration'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 2);
    drop(conn);
    assert_cold_equivalent(&workspace);
}

#[test]
fn bare_decorator_and_import_alias_register_only_proven_fastmcp_instance() {
    let workspace = Workspace::new();
    workspace.write(
        "server.py",
        r#"from fastmcp import FastMCP as FM
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
"#,
    );
    workspace.build();
    let conn = workspace.open();
    let names: Vec<String> = conn
        .prepare("SELECT name FROM nodes WHERE kind='tool_registration' ORDER BY line")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(names, ["listed", "from_module"]);
}
