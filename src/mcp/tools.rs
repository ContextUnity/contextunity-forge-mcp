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
        description = "Overview of indexed components, modules and coverage. Collections are independently paginated."
    )]
    fn code_map_overview(&self, Parameters(p): Parameters<PageInput>) -> CallToolResult {
        self.responding(|policy| self.read(|c| reader::overview_paged(c, &p.resolve(policy)?)))
    }
    #[tool(
        description = "Inspect a symbol, paged relations and invariants. Source defaults off; request show_source for a bounded AST preview."
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
        description = "Read a bounded AST source preview (5 leading/35 body lines by default). Continue with source_offset and generation; oversized lines identify the local file to read."
    )]
    fn get_code_snippet(&self, Parameters(p): Parameters<Inspect>) -> CallToolResult {
        self.responding(|policy| {
            let page = p.page.resolve(policy)?;
            let source = p.source(policy, true)?;
            self.read(|c| symbols::inspect_paged(c, &self.root, &p.selector, false, &source, &page))
        })
    }
    #[tool(
        description = "Search symbols with FTS terms or '*' patterns. Prefer prefix*; a leading wildcard can scan the full index. Compact pages default to 30; max 100."
    )]
    fn code_map_search(&self, Parameters(p): Parameters<SearchSymbols>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| {
                symbols::search_paged(c, &p.pattern, p.kind.as_deref(), &p.page.resolve(policy)?)
            })
        })
    }
    #[tool(
        description = "Find paged transitive test dependents (inbound) or production dependencies of a test (outbound)."
    )]
    fn code_map_tests(&self, Parameters(p): Parameters<Tests>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| {
                symbols::tests_paged(c, &p.selector, &p.direction, &p.page.resolve(policy)?)
            })
        })
    }
    #[tool(description = "Trace incoming dependencies to bounded depth with paged results.")]
    fn code_map_impact(&self, Parameters(p): Parameters<Impact>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| {
                traversal::traverse_paged(c, &p.selector, p.depth, true, &p.page.resolve(policy)?)
            })
        })
    }
    #[tool(
        description = "Explain symbol ownership, independently paged edge directions and invariants. Set show_source for a bounded source preview."
    )]
    fn code_map_explain(&self, Parameters(p): Parameters<Explain>) -> CallToolResult {
        self.responding(|policy| {
            let page = p.page.resolve(policy)?;
            let source = p.source(policy, false)?;
            self.read(|c| {
                symbols::explain_paged(
                    c,
                    &self.root,
                    &p.selector,
                    p.direction.as_deref(),
                    &source,
                    &page,
                )
            })
        })
    }
    #[tool(
        description = "Query a paged slice, unwired nodes or the documented Cypher subset. Pass limit as a separate argument, not inside Cypher."
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
        description = "Summarize workspace diagnostics with target='', or an indexed file/directory path. Cycles are omitted by default; set include_cycles=true to compute them. Page exact-file diagnostics or run bounded read-only SELECT/WITH SQL."
    )]
    fn code_map_analyze(&self, Parameters(p): Parameters<Analyze>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| {
                reader::analyze_paged(c, &p.target, p.include_cycles, &p.page.resolve(policy)?)
            })
        })
    }
    #[tool(
        description = "Check removal safety using complete indexed evidence. Displayed dependency and coverage collections are paginated."
    )]
    fn code_map_prove_removal(&self, Parameters(p): Parameters<Selector>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| traversal::removal_paged(c, &p.selector, &p.page.resolve(policy)?))
        })
    }
    #[tool(
        description = "Search Tree-sitter syntax with $NAME and $$$ARGS captures. Bounded pages include continuation and computation limits."
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
        description = "Search paged documentation sections with optional type/component filters."
    )]
    fn search_docs(&self, Parameters(p): Parameters<SearchDocs>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| {
                reader::search_docs_paged(
                    c,
                    &p.query,
                    p.doc_type.as_deref(),
                    p.component.as_deref(),
                    &p.page.resolve(policy)?,
                )
            })
        })
    }
    #[tool(
        description = "Read paged documentation sections by path/id, optionally selecting an exact section. Oversized sections report an actionable byte limit."
    )]
    fn get_doc(&self, Parameters(p): Parameters<GetDoc>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| {
                reader::get_doc_paged(
                    c,
                    &p.path_or_id,
                    p.section.as_deref(),
                    &p.page.resolve(policy)?,
                )
            })
        })
    }
    #[tool(
        description = "Guide topics init,adapter,docs,query,validate. The query topic selects narrow graph tools and explains page continuation and budget recovery."
    )]
    fn forge_guide(&self, Parameters(p): Parameters<Guide>) -> CallToolResult {
        if p.topic == "init" {
            return response::result(
                cli::guide::run(&self.root, &p.topic, p.force),
                &ResponsePolicy::default(),
            );
        }
        self.responding(|_| cli::guide::run(&self.root, &p.topic, p.force))
    }
    #[tool(
        description = "Manage local workspace checkpoints: list,get,save,delete. Large checkpoint reads return a bounded error; use the local .forge/checkpoints.json file."
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
    instructions = "Native workspace code graph. Responses are at most 64 KiB. Compact pages default to 30 (maximum 100). Pass generation with continuation offsets. Inspect coverage before absence claims. Use forge_guide topic=query for tool selection and budget recovery."
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
