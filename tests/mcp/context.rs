use crate::cli::run_cli;
use crate::common::{assertions, mcp_client::invalidate_mcp_connection_cache, Workspace};
use crate::harness::McpTestHarness;
use crate::sqlite_fixture::TaskClaimsFixture;
use contextunity_forge_mcp::{
    core::{
        format::OutputFormat,
        response::{ResponsePolicy, MAX_OUTPUT_BYTES},
    },
    db::traversal,
    engine::scanner,
    mcp::response,
};
use serde_json::{json, Value};
use std::{fs, time::Duration};

#[cfg(feature = "lang-python")]
#[path = "context/ast_search.rs"]
mod ast_search;
#[path = "context/checkpoints_and_serialization.rs"]
mod checkpoints_and_serialization;
#[path = "context/entrypoints_and_delta.rs"]
mod entrypoints_and_delta;
#[path = "context/guide.rs"]
mod guide;
#[path = "context/navigation_and_search.rs"]
mod navigation_and_search;
#[path = "context/previews_and_repair.rs"]
mod previews_and_repair;
#[path = "context/query_and_policy.rs"]
mod query_and_policy;
#[path = "context/sibling_worktrees.rs"]
mod sibling_worktrees;
#[path = "context/worktree_claims.rs"]
mod worktree_claims;
