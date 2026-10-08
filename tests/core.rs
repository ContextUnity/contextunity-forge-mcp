#[path = "common/mod.rs"]
pub mod common;
#[path = "common/workspace_contract_tests.rs"]
mod workspace_contract_tests;

#[path = "core/adapters.rs"]
mod adapters;
#[path = "core/compact_schema.rs"]
mod compact_schema;
#[path = "core/coverage_diagnostics.rs"]
mod coverage_diagnostics;
#[path = "core/debug_logging.rs"]
mod debug_logging;
#[path = "core/edge_aggregation.rs"]
mod edge_aggregation;
#[path = "core/scanner_guard_limits.rs"]
mod scanner_guard_limits;
