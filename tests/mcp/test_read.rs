//! Contract tests for the layered reader.

use forge_core::model::{ReadBounds, ReadMode, ReadResult};
use crate::common::{mcp_client::StdioClient, Workspace};
use serde_json::json;
use forge_core::subsystems::{
    disabled_read_mode_error, disabled_tool_error, filter_subsystem_tools, SubsystemPreset,
    SubsystemRegistry,
};

#[test]
fn read_models_and_core_tool_are_available_with_minimal_subsystems() {
    let registry = SubsystemRegistry {
        profile: SubsystemPreset::Minimal,
        ..Default::default()
    };
    assert!(disabled_tool_error("forge_read", &registry).is_none());
    assert_eq!(
        filter_subsystem_tools(["forge_read", "get_doc", "code_map_explain"], &registry),
        ["forge_read"]
    );
    let result = ReadResult {
        path: "notes.txt".into(),
        mode: ReadMode::Lines,
        content: "hello".into(),
        truncated: false,
    };
    let value = serde_json::to_value(result).unwrap();
    assert_eq!(value["mode"], "lines");
    assert_eq!(ReadBounds::default().start_line, None);
}

#[test]
fn structural_reader_modes_require_their_subsystems() {
    let mut registry = SubsystemRegistry {
        profile: SubsystemPreset::Minimal,
        ..Default::default()
    };
    assert!(disabled_read_mode_error(ReadMode::Snippet, false, &registry).is_some());
    assert!(disabled_read_mode_error(ReadMode::Outline, false, &registry).is_some());
    assert!(disabled_read_mode_error(ReadMode::Outline, true, &registry).is_some());
    assert!(disabled_read_mode_error(ReadMode::Lines, false, &registry).is_none());
    registry.enabled.insert("docs".into());
    assert!(disabled_read_mode_error(ReadMode::Outline, true, &registry).is_none());
    assert!(disabled_read_mode_error(ReadMode::Outline, false, &registry).is_some());
    registry.enabled.insert("codemap".into());
    assert!(disabled_read_mode_error(ReadMode::Snippet, false, &registry).is_none());
    assert!(disabled_read_mode_error(ReadMode::Outline, false, &registry).is_none());
}

#[test]
fn forge_read_serves_text_in_a_minimal_workspace() {
    let workspace = Workspace::new();
    workspace.write("forge-mcp.yaml", "profile: minimal\n");
    workspace.write("notes.txt", "first line\nsecond line\n");
    let registry = SubsystemRegistry {
        profile: SubsystemPreset::Minimal,
        ..Default::default()
    };
    let enabled_tools = filter_subsystem_tools(["forge_read"], &registry);
    assert_eq!(enabled_tools, ["forge_read"]);
    let mut client = StdioClient::new(&workspace);
    let (_, response) = client.call("forge_read", json!({"path":"notes.txt", "mode":"lines"}));
    assert_ne!(response["result"]["isError"], true, "{response}");
    assert!(response.to_string().contains("first line"), "{response}");
}
