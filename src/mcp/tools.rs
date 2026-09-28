use super::{response, server::Server};
use crate::{
    cli,
    core::response::{Detail, QueryOptions, ResponsePolicy, SourceOptions},
    db::{reader, symbols, traversal},
    engine::scanner,
};
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_handler, tool_router,
    ServerHandler,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

fn yes() -> bool {
    true
}
fn depth() -> u32 {
    2
}
fn inbound() -> String {
    "inbound".into()
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct PageInput {
    /// Items per collection, 1..=100; defaults to adapter response.page_size (30).
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: usize,
    /// Compact omits heavy node details; full remains subject to the byte limit.
    pub detail: Option<Detail>,
    /// Use the generation returned by the previous page when offset is nonzero.
    pub generation: Option<String>,
}
impl PageInput {
    fn resolve(&self, policy: &ResponsePolicy) -> anyhow::Result<QueryOptions> {
        QueryOptions::resolve(
            policy,
            self.limit,
            self.offset,
            self.detail,
            self.generation.clone(),
        )
    }
}
#[derive(Deserialize, JsonSchema)]
pub struct Selector {
    pub selector: String,
    #[serde(flatten)]
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
pub struct Inspect {
    pub selector: String,
    #[serde(default = "yes")]
    pub show_doc: bool,
    pub show_source: Option<bool>,
    pub leading_lines: Option<usize>,
    pub max_body_lines: Option<usize>,
    /// Body line offset returned as next_source_offset by a previous preview.
    #[serde(default)]
    pub source_offset: usize,
    #[serde(flatten)]
    pub page: PageInput,
}
impl Inspect {
    fn source(&self, policy: &ResponsePolicy, required: bool) -> anyhow::Result<SourceOptions> {
        anyhow::ensure!(
            self.source_offset == 0 || self.page.generation.is_some(),
            "source continuation requires generation from the previous preview"
        );
        SourceOptions::resolve(
            policy,
            if required {
                Some(true)
            } else {
                self.show_source
            },
            self.leading_lines,
            self.max_body_lines,
            self.source_offset,
        )
    }
}
#[derive(Deserialize, JsonSchema)]
pub struct Explain {
    pub selector: String,
    pub direction: Option<String>,
    #[serde(default = "yes")]
    pub show_doc: bool,
    pub show_source: Option<bool>,
    pub leading_lines: Option<usize>,
    pub max_body_lines: Option<usize>,
    /// Body line offset returned as next_source_offset by a previous preview.
    #[serde(default)]
    pub source_offset: usize,
    #[serde(flatten)]
    pub page: PageInput,
}
impl Explain {
    fn source(&self, policy: &ResponsePolicy, required: bool) -> anyhow::Result<SourceOptions> {
        anyhow::ensure!(
            self.source_offset == 0 || self.page.generation.is_some(),
            "source continuation requires generation from the previous preview"
        );
        SourceOptions::resolve(
            policy,
            if required {
                Some(true)
            } else {
                self.show_source
            },
            self.leading_lines,
            self.max_body_lines,
            self.source_offset,
        )
    }
}
#[derive(Deserialize, JsonSchema)]
pub struct SearchSymbols {
    pub pattern: String,
    pub kind: Option<String>,
    /// Restrict results to a workspace-relative file or directory.
    pub path: Option<String>,
    #[serde(flatten)]
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
pub struct Tests {
    pub selector: String,
    #[serde(default = "inbound")]
    pub direction: String,
    #[serde(flatten)]
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
pub struct Impact {
    pub selector: String,
    #[serde(default = "depth")]
    pub depth: u32,
    #[serde(flatten)]
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
pub struct Query {
    pub operation: String,
    pub selector: Option<String>,
    #[serde(default = "depth")]
    pub depth: u32,
    #[serde(flatten)]
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
pub struct Analyze {
    pub target: String,
    pub include_cycles: Option<bool>,
    /// Read stored syntax diagnostics only; does not run a linter or reparse source.
    #[serde(default)]
    pub lint: bool,
    #[serde(flatten)]
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
pub struct Ast {
    pub pattern: String,
    pub language: String,
    pub path: Option<String>,
    #[serde(flatten)]
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
pub struct SearchDocs {
    pub query: String,
    pub doc_type: Option<String>,
    pub component: Option<String>,
    /// Include a short match-centered content excerpt in each result.
    #[serde(default)]
    pub include_excerpt: bool,
    #[serde(flatten)]
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
pub struct GetDoc {
    pub path_or_id: String,
    pub section: Option<String>,
    #[serde(flatten)]
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
pub struct Guide {
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub force: bool,
}
#[derive(Deserialize, JsonSchema)]
pub struct Checkpoint {
    pub action: String,
    pub name: Option<String>,
    pub content: Option<Value>,
}
impl Server {
    fn responding(
        &self,
        run: impl FnOnce(&ResponsePolicy) -> anyhow::Result<Value>,
    ) -> CallToolResult {
        match scanner::load_adapter(&self.root, None) {
            Ok(adapter) => response::result(run(&adapter.response), &adapter.response),
            Err(error) => response::result(Err(error.into()), &ResponsePolicy::default()),
        }
    }
}

#[tool_router]
impl Server {
    #[tool(
        description = "Workspace overview: components, modules, and coverage. Start here to verify workspace_root matches active worktree and check coverage before making absence claims. Collections independently paginated."
    )]
    fn code_map_overview(&self, Parameters(p): Parameters<PageInput>) -> CallToolResult {
        self.responding(|policy| self.read(|c| reader::overview_paged(c, &p.resolve(policy)?)))
    }
    #[tool(
        description = "Inspect an indexed symbol by selector (e.g. 'function:name', 'class:Name', 'module:path'). Compact output includes location and paged references with whole-symbol resolution counts; detail='full' includes node details and per-reference evidence. Set show_source=true for bounded source or use get_code_snippet."
    )]
    fn code_map_inspect(&self, Parameters(p): Parameters<Inspect>) -> CallToolResult {
        self.responding(|policy| {
            let page = p.page.resolve(policy)?;
            let source = p.source(policy, false)?;
            self.read(|c| {
                symbols::inspect_paged(c, &self.root, &p.selector, p.show_doc, &source, &page)
            })
        })
    }
    #[tool(
        description = "Read source and location for a symbol selector (default 5 leading + 35 body lines). Pass source_offset with previous generation for next lines. Use code_map_inspect for resolution coverage and evidence; use ctx_read for full file edits."
    )]
    fn get_code_snippet(&self, Parameters(p): Parameters<Inspect>) -> CallToolResult {
        self.responding(|policy| {
            let page = p.page.resolve(policy)?;
            let source = p.source(policy, true)?;
            self.read(|c| symbols::snippet_paged(c, &self.root, &p.selector, &source, &page))
        })
    }
    #[tool(
        description = "Search indexed symbols with FTS terms or prefix* pattern (e.g. 'Token*', 'router'). Optional path restricts results to a workspace-relative file or directory. Returns symbol IDs, kinds, and paths. Compact pages default to 30; max 100."
    )]
    fn code_map_search(&self, Parameters(p): Parameters<SearchSymbols>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| {
                symbols::search_paged_in_path(
                    c,
                    &p.pattern,
                    p.kind.as_deref(),
                    p.path.as_deref(),
                    &p.page.resolve(policy)?,
                )
            })
        })
    }
    #[tool(
        description = "Find test relationships: 'inbound' finds tests exercising a target symbol/module; 'outbound' finds production dependencies of a test. Each result labels a direct edge witness or a scope/transitive connection; it does not prove a full path. Narrow symbol or leaf module required; broad scopes are rejected."
    )]
    fn code_map_tests(&self, Parameters(p): Parameters<Tests>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| {
                symbols::tests_paged(c, &p.selector, &p.direction, &p.page.resolve(policy)?)
            })
        })
    }
    #[tool(
        description = "Trace incoming dependencies (blast radius) for a selector. Always start with depth=1 to avoid exponential graph fan-out. Paged results require offset + generation for continuation."
    )]
    fn code_map_impact(&self, Parameters(p): Parameters<Impact>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| {
                traversal::traverse_paged(c, &p.selector, p.depth, true, &p.page.resolve(policy)?)
            })
        })
    }
    #[tool(
        description = "Explain symbol ownership, direct edges, and architectural invariants. direction: both (default), incoming/inbound, or outgoing/outbound. Set show_doc=false to omit linked documents; show_source=true adds bounded source."
    )]
    fn code_map_explain(&self, Parameters(p): Parameters<Explain>) -> CallToolResult {
        self.responding(|policy| {
            let page = p.page.resolve(policy)?;
            let source = p.source(policy, false)?;
            self.read(|c| {
                symbols::explain_paged_with_docs(
                    c,
                    &self.root,
                    &p.selector,
                    p.direction.as_deref(),
                    p.show_doc,
                    &source,
                    &page,
                )
            })
        })
    }
    #[tool(
        description = "Advanced graph query. Operation: 'slice' (subgraph around selector), 'unwired' (nodes without edges), or 'cypher' (e.g. 'MATCH (n:Function) RETURN n'). Specify limit and depth as arguments, not inside Cypher text."
    )]
    fn code_map_query(&self, Parameters(p): Parameters<Query>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| {
                traversal::query_paged(
                    c,
                    &p.operation,
                    p.selector.as_deref(),
                    p.depth,
                    &p.page.resolve(policy)?,
                )
            })
        })
    }
    #[tool(
        description = "Analyze diagnostics, errors, and cycles. target='' for workspace totals or an indexed path for diagnostics; include_cycles=true computes cycles. Also accepts read-only SELECT/WITH SQL. lint=true returns only stored parser syntax diagnostics across compiled profiles, with explicit coverage; no style/type/security checks or external process."
    )]
    fn code_map_analyze(&self, Parameters(p): Parameters<Analyze>) -> CallToolResult {
        self.responding(|policy| {
            anyhow::ensure!(
                !p.lint || p.include_cycles != Some(true),
                "lint=true cannot be combined with include_cycles=true"
            );
            self.read(|c| {
                let options = p.page.resolve(policy)?;
                if p.lint {
                    crate::db::lint::syntax_paged(c, &p.target, &options)
                } else {
                    reader::analyze_paged(c, &p.target, p.include_cycles, &options)
                }
            })
        })
    }
    #[tool(
        description = "Check removal safety using indexed incoming dependencies and unresolved edges before deleting a symbol or module. Complete coverage required before absence claims."
    )]
    fn code_map_prove_removal(&self, Parameters(p): Parameters<Selector>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| traversal::removal_paged(c, &p.selector, &p.page.resolve(policy)?))
        })
    }
    #[tool(
        description = "Syntactic pattern search using Tree-sitter AST ($NAME captures nodes, $$$ARGS captures sequences where valid in the language). Requires a compiled language id, such as python, typescript, rust, html, yaml, or toml, and a valid pattern. Optional path filters scope."
    )]
    fn ast_grep_search(&self, Parameters(p): Parameters<Ast>) -> CallToolResult {
        self.responding(|policy| {
            cli::ast::search_paged(
                &self.root,
                &p.pattern,
                &p.language,
                p.path.as_deref(),
                &p.page.resolve(policy)?,
            )
        })
    }
    #[tool(
        description = "Full-text search indexed documentation and ADRs. Optional doc_type filter ('architecture', 'adr', 'guide', 'api', 'plan') and component filter. Returns section anchors for get_doc; include_excerpt=true adds a bounded match-centered preview."
    )]
    fn search_docs(&self, Parameters(p): Parameters<SearchDocs>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| {
                reader::search_docs_paged_with_excerpt(
                    c,
                    &p.query,
                    p.doc_type.as_deref(),
                    p.component.as_deref(),
                    p.include_excerpt,
                    &p.page.resolve(policy)?,
                )
            })
        })
    }
    #[tool(
        description = "Read documentation content by path or ID (e.g. 'docs/architecture/router.md'). Returns content by default; detail='compact' returns a section outline. Optional section narrows to an exact section title."
    )]
    fn get_doc(&self, Parameters(p): Parameters<GetDoc>) -> CallToolResult {
        self.responding(|policy| {
            let mut page = p.page.resolve(policy)?;
            if p.page.detail.is_none() {
                page.detail = Detail::Full;
            }
            self.read(|c| reader::get_doc_paged(c, &p.path_or_id, p.section.as_deref(), &page))
        })
    }
    #[tool(
        description = "Interactive guide. Topics: 'query' (tool selection, budget recovery, paging rules), 'docs' (documentation frontmatter & invariants), 'adapter' (forge-mcp.yaml config), 'validate' (index status), 'init' (initialize config). Defaults to 'query'."
    )]
    fn forge_guide(&self, Parameters(p): Parameters<Guide>) -> CallToolResult {
        let topic = p.topic.as_deref().unwrap_or("query");
        let topic = if topic.trim().is_empty() {
            "query"
        } else {
            topic.trim()
        };
        if topic == "init" {
            return response::result(
                cli::guide::run(&self.root, topic, p.force),
                &ResponsePolicy::default(),
            );
        }
        self.responding(|_| cli::guide::run(&self.root, topic, p.force))
    }
    #[tool(
        description = "Manage local session checkpoints saved in .forge/checkpoints.json. Actions: 'list', 'save' (requires name + content), 'get' (by name), 'delete' (by name)."
    )]
    fn session_checkpoint(&self, Parameters(p): Parameters<Checkpoint>) -> CallToolResult {
        match scanner::load_adapter(&self.root, None) {
            Ok(adapter) => response::checkpoint_result(
                cli::guide::checkpoint(&self.root, &p.action, p.name.as_deref(), p.content),
                &adapter.response,
            ),
            Err(error) => response::result(Err(error.into()), &ResponsePolicy::default()),
        }
    }
}

#[tool_handler(
    name = "contextunity-forge-mcp",
    version = "0.2.0",
    instructions = "Native workspace code graph (15 tools). Max 64 KiB responses, 2s SQLite budget. Workflow: code_map_overview -> code_map_search (prefix*) -> code_map_inspect -> code_map_explain -> code_map_impact (depth=1) -> code_map_tests -> get_code_snippet -> ctx_read. Paged results include offset and generation; pass both for next page. Use compact detail. Inspect coverage before absence claims."
)]
impl ServerHandler for Server {
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        let call = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        let result = Self::tool_router().call(call).await;
        let policy = scanner::load_adapter(&self.root, None)
            .map(|adapter| adapter.response)
            .unwrap_or_default();
        match result {
            Ok(rmcp::model::CallToolResponse::Complete(result)) => {
                Ok(response::enforce(result, &policy).into())
            }
            Ok(result) => Ok(result),
            Err(error) => {
                Ok(response::result(Err(anyhow::anyhow!(error.to_string())), &policy).into())
            }
        }
    }
}
