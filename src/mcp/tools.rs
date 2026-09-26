use super::server::Server;
use crate::{
    cli,
    db::{reader, traversal},
};
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    tool, tool_handler, tool_router, ServerHandler,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
fn result(value: anyhow::Result<Value>) -> CallToolResult {
    match value {
        Ok(v) => CallToolResult::success(vec![ContentBlock::text(v.to_string())]),
        Err(e) => CallToolResult::error(vec![ContentBlock::text(format!("{e:#}"))]),
    }
}
fn yes() -> bool {
    true
}
fn depth() -> u32 {
    2
}
fn limit() -> usize {
    100
}
#[derive(Deserialize, JsonSchema)]
pub struct Selector {
    pub selector: String,
}
#[derive(Deserialize, JsonSchema)]
pub struct Inspect {
    pub selector: String,
    #[serde(default = "yes")]
    pub show_doc: bool,
}
#[derive(Deserialize, JsonSchema)]
pub struct Impact {
    pub selector: String,
    #[serde(default = "depth")]
    pub depth: u32,
}
#[derive(Deserialize, JsonSchema)]
pub struct Query {
    pub operation: String,
    pub selector: Option<String>,
    #[serde(default = "depth")]
    pub depth: u32,
    #[serde(default = "limit")]
    pub limit: usize,
}
#[derive(Deserialize, JsonSchema)]
pub struct Analyze {
    pub target: String,
}
#[derive(Deserialize, JsonSchema)]
pub struct Ast {
    pub pattern: String,
    pub language: String,
    pub path: Option<String>,
    #[serde(default = "limit")]
    pub limit: usize,
}
#[derive(Deserialize, JsonSchema)]
pub struct SearchDocs {
    pub query: String,
    pub doc_type: Option<String>,
    pub component: Option<String>,
    #[serde(default = "limit")]
    pub limit: usize,
}
#[derive(Deserialize, JsonSchema)]
pub struct GetDoc {
    pub path_or_id: String,
    pub section: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub struct Guide {
    pub topic: String,
    #[serde(default)]
    pub force: bool,
}
#[derive(Deserialize, JsonSchema)]
pub struct Checkpoint {
    pub action: String,
    pub name: Option<String>,
    pub content: Option<Value>,
}
#[tool_router]
impl Server {
    #[tool(description = "Overview of indexed components, modules and coverage.")]
    fn code_map_overview(&self) -> CallToolResult {
        result(self.read(reader::overview))
    }
    #[tool(description = "Inspect a symbol and linked architectural invariants.")]
    fn code_map_inspect(&self, Parameters(p): Parameters<Inspect>) -> CallToolResult {
        result(self.read(|c| reader::inspect(c, &p.selector, p.show_doc)))
    }
    #[tool(description = "Trace incoming dependencies to bounded depth.")]
    fn code_map_impact(&self, Parameters(p): Parameters<Impact>) -> CallToolResult {
        result(self.read(|c| traversal::traverse(c, &p.selector, p.depth, true)))
    }
    #[tool(description = "Explain symbol ownership, both edge directions and invariants.")]
    fn code_map_explain(&self, Parameters(p): Parameters<Selector>) -> CallToolResult {
        result(self.read(|c| reader::explain(c, &p.selector)))
    }
    #[tool(description = "Query slice, unwired nodes or documented Cypher subset.")]
    fn code_map_query(&self, Parameters(p): Parameters<Query>) -> CallToolResult {
        if p.limit == 0 || p.limit > 10000 {
            return result(Err(anyhow::anyhow!("limit must be1..10000")));
        }
        result(
            self.read(|c| {
                traversal::query(c, &p.operation, p.selector.as_deref(), p.depth, p.limit)
            }),
        )
    }
    #[tool(description = "Analyze diagnostics and cycles or run read-only SELECT/WITH SQL.")]
    fn code_map_analyze(&self, Parameters(p): Parameters<Analyze>) -> CallToolResult {
        result(self.read(|c| reader::analyze(c, &p.target)))
    }
    #[tool(description = "Check indexed incoming dependencies and coverage for removal.")]
    fn code_map_prove_removal(&self, Parameters(p): Parameters<Selector>) -> CallToolResult {
        result(self.read(|c| traversal::removal(c, &p.selector)))
    }
    #[tool(description = "Search Tree-sitter syntax with $NAME and $$$ARGS captures.")]
    fn ast_grep_search(&self, Parameters(p): Parameters<Ast>) -> CallToolResult {
        result(cli::ast::search(
            &self.root,
            &p.pattern,
            &p.language,
            p.path.as_deref(),
            p.limit,
        ))
    }
    #[tool(description = "Search documentation sections with optional type/component filters.")]
    fn search_docs(&self, Parameters(p): Parameters<SearchDocs>) -> CallToolResult {
        if p.limit == 0 || p.limit > 10000 {
            return result(Err(anyhow::anyhow!("limit must be1..10000")));
        }
        result(self.read(|c| {
            reader::search_docs(
                c,
                &p.query,
                p.doc_type.as_deref(),
                p.component.as_deref(),
                p.limit,
            )
        }))
    }
    #[tool(description = "Read a documentation path/id and optional exact section.")]
    fn get_doc(&self, Parameters(p): Parameters<GetDoc>) -> CallToolResult {
        result(self.read(|c| reader::get_doc(c, &p.path_or_id, p.section.as_deref())))
    }
    #[tool(description = "Initialize or validate adapter; topics init,adapter,docs,validate.")]
    fn forge_guide(&self, Parameters(p): Parameters<Guide>) -> CallToolResult {
        result(cli::guide::run(&self.root, &p.topic, p.force))
    }
    #[tool(description = "Manage local workspace checkpoints: list,get,save,delete.")]
    fn session_checkpoint(&self, Parameters(p): Parameters<Checkpoint>) -> CallToolResult {
        result(cli::guide::checkpoint(
            &self.root,
            &p.action,
            p.name.as_deref(),
            p.content,
        ))
    }
}

#[tool_handler(
    name = "contextunity-forge-mcp",
    version = "0.2.0",
    instructions = "Native workspace code graph. Inspect coverage before absence claims. Use forge_guide for onboarding."
)]
impl ServerHandler for Server {}
