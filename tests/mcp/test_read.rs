//! Contract tests for the layered reader.

use forge_core::model::{ReadBounds, ReadMode, ReadResult};
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
