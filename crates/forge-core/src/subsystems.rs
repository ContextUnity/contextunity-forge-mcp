//! Typed registries shared by ACDD profiles and workspace adapters.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Built-in subsystem presets for the MCP tool surface.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SubsystemPreset {
    /// Enable every registered subsystem.
    #[default]
    Full,
    /// Enable code navigation and documentation tools.
    Navigator,
    /// Enable task orchestration, blackboard, and checkpoint tools.
    Orchestrator,
    /// Enable no module-owned tools; `forge_guide` remains available as a core tool.
    Minimal,
}

/// Configurable enablement for the five independent Forge MCP subsystems.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct SubsystemRegistry {
    /// Named built-in profile used when no per-subsystem override is present.
    pub profile: SubsystemPreset,
    /// Explicitly enabled subsystem identifiers.
    pub enabled: BTreeSet<String>,
    /// Explicitly disabled subsystem identifiers.
    pub disabled: BTreeSet<String>,
    /// Subsystems required by the active profile.
    pub required: BTreeSet<String>,
}

impl Default for SubsystemRegistry {
    fn default() -> Self {
        Self {
            profile: SubsystemPreset::Full,
            enabled: BTreeSet::new(),
            disabled: BTreeSet::new(),
            required: BTreeSet::new(),
        }
    }
}

impl SubsystemRegistry {
    /// Validate names and reject contradictory or unsatisfied requirements.
    pub fn validate(&self) -> anyhow::Result<()> {
        for name in self
            .enabled
            .iter()
            .chain(&self.disabled)
            .chain(&self.required)
        {
            anyhow::ensure!(is_known_subsystem(name), "unknown Forge subsystem '{name}'");
        }
        if let Some(name) = self.enabled.intersection(&self.disabled).next() {
            anyhow::bail!("Forge subsystem '{name}' cannot be both enabled and disabled");
        }
        if let Some(name) = self.required.iter().find(|name| !self.is_enabled(name)) {
            anyhow::bail!(
                "FEATURE_REQUIRED_BY_PROFILE: Forge subsystem '{name}' is required but disabled"
            );
        }
        Ok(())
    }

    /// Return whether a subsystem is enabled by the preset and overrides.
    pub fn is_enabled(&self, name: &str) -> bool {
        if self.disabled.contains(name) {
            return false;
        }
        if self.enabled.contains(name) {
            return true;
        }
        match self.profile {
            SubsystemPreset::Full => is_known_subsystem(name),
            SubsystemPreset::Navigator => matches!(name, "codemap" | "docs"),
            SubsystemPreset::Orchestrator => {
                matches!(name, "acdd" | "blackboard" | "checkpoints")
            }
            SubsystemPreset::Minimal => false,
        }
    }

    /// Return the subsystem assigned to an MCP tool, if it is registered here.
    pub fn subsystem_for_tool(tool: &str) -> Option<&'static str> {
        match tool {
            "forge_read" => None,
            "task_blackboard" => Some("blackboard"),
            "session_checkpoint" => Some("checkpoints"),
            "get_doc" | "search_docs" => Some("docs"),
            "task_list" | "task_claim" | "task_submit" | "task_manage" | "milestone_init"
            | "milestone_verify" | "milestone_handoff" => Some("acdd"),
            name if name.starts_with("code_map_") || name == "ast_grep_search" => Some("codemap"),
            _ => None,
        }
    }
}

/// Keep only tools whose owning subsystem is enabled. Unknown tools remain visible
/// so adding a new tool cannot silently hide it before its owner is assigned.
pub fn filter_subsystem_tools<'a>(
    tools: impl IntoIterator<Item = &'a str>,
    registry: &SubsystemRegistry,
) -> Vec<&'a str> {
    tools
        .into_iter()
        .filter(|tool| {
            SubsystemRegistry::subsystem_for_tool(tool)
                .is_none_or(|subsystem| registry.is_enabled(subsystem))
        })
        .collect()
}

/// Return a stable error when a request names a tool in a disabled subsystem.
pub fn disabled_tool_error(tool: &str, registry: &SubsystemRegistry) -> Option<String> {
    let subsystem = SubsystemRegistry::subsystem_for_tool(tool)?;
    (!registry.is_enabled(subsystem)).then(|| {
        format!("FEATURE_DISABLED: MCP tool '{tool}' belongs to disabled subsystem '{subsystem}'")
    })
}

/// Return a disabled-feature error for a layered reader operation.
pub fn disabled_read_mode_error(
    mode: crate::model::ReadMode,
    markdown: bool,
    registry: &SubsystemRegistry,
) -> Option<String> {
    use crate::model::ReadMode;
    let subsystem = match mode {
        ReadMode::Snippet => "codemap",
        ReadMode::Outline if markdown => "docs",
        ReadMode::Outline => "codemap",
        _ => return None,
    };
    (!registry.is_enabled(subsystem)).then(|| {
        format!(
            "FEATURE_DISABLED: reader mode '{mode:?}' requires disabled subsystem '{subsystem}'"
        )
    })
}

fn is_known_subsystem(name: &str) -> bool {
    matches!(
        name,
        "codemap" | "acdd" | "blackboard" | "docs" | "checkpoints"
    )
}
