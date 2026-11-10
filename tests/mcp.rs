#[path = "common/mod.rs"]
pub mod common;

fn json_array(value: &serde_json::Value) -> &Vec<serde_json::Value> {
    value.as_array().expect("expected JSON array")
}

#[path = "mcp/harness/cache.rs"]
mod cache_fixture;
#[path = "mcp/cache_isolation.rs"]
mod cache_isolation;
pub use common::cli;
#[path = "mcp/cli_contract.rs"]
mod cli_contract;

#[path = "mcp/harness/analysis.rs"]
mod analysis_fixture;
#[path = "mcp/harness.rs"]
mod harness;
#[path = "mcp/harness/navigation.rs"]
mod navigation;
#[path = "mcp/harness/routing.rs"]
mod routing_fixture;
#[path = "mcp/harness/server_failures.rs"]
mod server_failures;
#[path = "mcp/harness/sqlite.rs"]
mod sqlite_fixture;

#[path = "mcp/context.rs"]
mod context;
#[path = "mcp/fastmcp_registration.rs"]
mod fastmcp_registration;
#[path = "mcp/freshness.rs"]
mod freshness;
#[path = "mcp/harness_contract.rs"]
mod harness_contract;
#[cfg(unix)]
#[path = "mcp/installer.rs"]
mod installer;
#[path = "mcp/lint_tools.rs"]
mod lint_tools;
#[path = "mcp/query_ranking.rs"]
mod query_ranking;
#[path = "mcp/skill_sync.rs"]
mod skill_sync;
#[path = "mcp/test_read.rs"]
mod test_read;
#[path = "mcp/tool_evolution.rs"]
mod tool_evolution;

#[path = "mcp/transport.rs"]
mod transport;
#[cfg(unix)]
#[path = "mcp/harness/transport.rs"]
mod transport_fixture;
