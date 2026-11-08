//! Test module reserved for the AC-001 gate AST contract.

use super::{snapshot::FixtureSnapshot, support, tasks::harness::TaskHarness};
use contextunity_forge_mcp::core::tasks::{
    catalog::{filter_subsystem_tools, SubsystemPreset, SubsystemRegistry},
    profile::{parse_profile, validate_profile_document, GateDriver},
    Milestone,
};

#[path = "gates_ast/context_and_snapshots.rs"]
mod context_and_snapshots;
#[path = "gates_ast/gate_cases.rs"]
mod gate_cases;
#[path = "gates_ast/profile_parser.rs"]
mod profile_parser;
