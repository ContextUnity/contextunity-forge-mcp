use super::support::*;
use contextunity_forge_mcp::engine::milestones::recovery::db_recover;

const CANONICAL_FORGE_GITIGNORE: &str = concat!(
    "target/\n",
    "!/.forge/\n",
    "!/.forge/acdd/\n",
    "!/.forge/acdd/**\n",
    "!/.forge/frameworks/\n",
    "!/.forge/frameworks/**\n",
    ".forge/*.sqlite*\n",
    ".forge/*.lock\n",
    ".forge/*.log\n",
    ".forge/*.jsonl\n",
    ".forge/tasks/\n",
    ".forge/checkpoints.json\n",
);

mod boundaries;
mod guidance;
pub(super) mod harness;
mod lifecycle;
mod mcp_fixture;
mod retirement;
mod seams;
mod workspaces;
