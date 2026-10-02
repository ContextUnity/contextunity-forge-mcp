use super::{response, server::Server};
use crate::{
    cli,
    core::response::{CoverageOptions, Detail, QueryOptions, ResponsePolicy, SourceOptions},
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

// An empty schema object accepts any JSON value and stays valid for MCP clients.
fn checkpoint_content_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    serde_json::Map::new().into()
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
#[derive(Default, Deserialize, JsonSchema)]
pub struct OverviewInput {
    /// Optional aspects to include: 'counts', 'components', 'languages', 'cycles', 'compiled_profiles', 'metadata'. If omitted, all standard aspects are included.
    pub aspects: Option<Vec<String>>,
    #[serde(flatten)]
    pub page: PageInput,
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
    /// Include paged resolution coverage in addition to the compact summary.
    #[serde(default)]
    pub include_coverage: bool,
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
        source_options(
            policy,
            self.show_source,
            self.leading_lines,
            self.max_body_lines,
            self.source_offset,
            self.page.generation.as_deref(),
            required,
        )
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct Snippet {
    pub selector: String,
    pub show_source: Option<bool>,
    pub leading_lines: Option<usize>,
    pub max_body_lines: Option<usize>,
    /// Body line offset returned as next_source_offset by a previous preview.
    #[serde(default)]
    pub source_offset: usize,
    #[serde(flatten)]
    pub page: PageInput,
}
impl Snippet {
    fn source(&self, policy: &ResponsePolicy) -> anyhow::Result<SourceOptions> {
        source_options(
            policy,
            self.show_source,
            self.leading_lines,
            self.max_body_lines,
            self.source_offset,
            self.page.generation.as_deref(),
            true,
        )
    }
}

fn source_options(
    policy: &ResponsePolicy,
    show_source: Option<bool>,
    leading_lines: Option<usize>,
    max_body_lines: Option<usize>,
    source_offset: usize,
    generation: Option<&str>,
    required: bool,
) -> anyhow::Result<SourceOptions> {
    anyhow::ensure!(
        source_offset == 0 || generation.is_some(),
        "source continuation requires generation from the previous preview"
    );
    SourceOptions::resolve(
        policy,
        if required { Some(true) } else { show_source },
        leading_lines,
        max_body_lines,
        source_offset,
    )
}
#[derive(Deserialize, JsonSchema)]
pub struct Explain {
    pub selector: String,
    pub direction: Option<String>,
    #[serde(default = "yes")]
    pub show_doc: bool,
    /// Include paged resolution coverage in addition to the compact summary.
    #[serde(default)]
    pub include_coverage: bool,
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
    /// Match only the complete symbol name or qualified name, without full-text search.
    #[serde(default)]
    pub exact: bool,
    pub kind: Option<String>,
    /// Restrict results to a workspace-relative file or directory.
    pub path: Option<String>,
    /// Group results by file path.
    #[serde(default)]
    pub group_by_file: bool,
    /// Include Markdown documentation nodes in the code symbol search.
    #[serde(default)]
    pub include_docs: bool,
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
    /// Impact direction: 'inbound' (default) or 'outbound'.
    #[serde(default = "inbound")]
    pub direction: String,
    #[serde(default = "depth")]
    pub depth: u32,
    /// Traversal mode: 'calls' (default, invocation/call graph), 'data-flow' (parameter/assignment flow), 'all' (all dependency edges).
    pub mode: Option<String>,
    /// Optional specific edge kinds to follow (e.g. ['calls', 'mutates', 'inherits', 'implements', 'imports']).
    pub edge_types: Option<Vec<String>>,
    #[serde(flatten)]
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
pub struct Query {
    pub operation: String,
    pub selector: Option<String>,
    /// Impact direction: 'inbound' (default) or 'outbound'.
    pub direction: Option<String>,
    /// Include paged resolution coverage for inspect and explain operations.
    #[serde(default)]
    pub include_coverage: bool,
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
    #[schemars(schema_with = "checkpoint_content_schema")]
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
        description = "List ready, unclaimed tasks with satisfied dependencies in the primary repository by default. Select repository=all or an enabled linked workspace name to include linked tasks."
    )]
    fn task_list(&self, Parameters(p): Parameters<super::tasks::List>) -> CallToolResult {
        self.responding(|_| super::tasks::list(&self.root, p))
    }
    #[tool(description = "Atomically claim the current lifecycle gate in an isolated worktree.")]
    fn task_claim(&self, Parameters(p): Parameters<super::tasks::Claim>) -> CallToolResult {
        self.responding(|_| super::tasks::claim(&self.root, p))
    }
    #[tool(
        description = "Submit revision-bound evidence. Handoff synchronously validates the Git milestone receipt."
    )]
    fn task_submit(&self, Parameters(p): Parameters<super::tasks::Submit>) -> CallToolResult {
        self.responding(|_| super::tasks::submit(&self.root, p))
    }
    #[tool(
        description = "Create, sync, inspect, delete or extend task scope. Select workspace for creation, synchronization or milestone deletion; sync with only workspace imports its milestone directory."
    )]
    fn task_manage(&self, Parameters(p): Parameters<super::tasks::Manage>) -> CallToolResult {
        self.responding(|_| super::tasks::manage(&self.root, p))
    }
    #[tool(
        description = "Workspace overview: components, modules, counts, and coverage. Optional aspects: 'counts', 'components', 'languages', 'cycles', 'compiled_profiles', 'metadata'. Start here to verify workspace_root matches active worktree and check coverage before making absence claims. Collections independently paginated."
    )]
    fn code_map_overview(&self, Parameters(p): Parameters<OverviewInput>) -> CallToolResult {
        self.responding(|policy| {
            let page = p.page.resolve(policy)?;
            self.read(|c| reader::overview_paged_with_aspects(c, p.aspects.as_deref(), &page))
        })
    }
    #[tool(
        description = "Inspect an indexed symbol by selector (e.g. 'function:name', 'class:Name', 'module:path'). Compact output includes exact signature and docstring, receiver-aware container, inbound/outbound call counts, and up to five direct callers/callees. Set include_coverage=true for paged resolution coverage; detail='full' includes node details and per-reference evidence. Set show_source=true for bounded source or use get_code_snippet."
    )]
    fn code_map_inspect(&self, Parameters(p): Parameters<Inspect>) -> CallToolResult {
        self.responding(|policy| {
            let page = p.page.resolve(policy)?;
            let source = p.source(policy, false)?;
            let coverage = CoverageOptions {
                include_coverage: p.include_coverage,
            };
            self.read(|c| {
                symbols::inspect_paged_with_coverage(
                    c,
                    &self.root,
                    &p.selector,
                    p.show_doc,
                    &source,
                    &page,
                    coverage,
                )
            })
        })
    }
    #[tool(
        description = "Read source and location for a symbol selector (default 5 leading + 35 body lines). Pass source_offset with previous generation for next lines. Use code_map_inspect for resolution coverage and evidence; use ctx_read for full file edits."
    )]
    fn get_code_snippet(&self, Parameters(p): Parameters<Snippet>) -> CallToolResult {
        self.responding(|policy| {
            let page = p.page.resolve(policy)?;
            let source = p.source(policy)?;
            self.read(|c| symbols::snippet_paged(c, &self.root, &p.selector, &source, &page))
        })
    }
    #[tool(
        description = "Search indexed symbols with FTS terms or prefix* pattern (e.g. 'Token*', 'router'). Set exact=true for indexed, case-insensitive name or qualified-name equality without FTS. Optional path restricts results to a workspace-relative file or directory. Returns symbol IDs, kinds, and paths. Compact pages default to 30; max 100."
    )]
    fn code_map_search(&self, Parameters(p): Parameters<SearchSymbols>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| {
                let mut res = symbols::search_paged_in_path_with_options(
                    c,
                    &p.pattern,
                    p.kind.as_deref(),
                    p.path.as_deref(),
                    p.include_docs,
                    p.exact,
                    &p.page.resolve(policy)?,
                )?;
                if p.group_by_file {
                    if let Some(items) = res
                        .get_mut("nodes")
                        .and_then(|nodes| nodes.get_mut("items"))
                        .and_then(Value::as_array_mut)
                    {
                        let mut grouped: std::collections::BTreeMap<String, Vec<Value>> =
                            std::collections::BTreeMap::new();
                        for mut item in items.drain(..) {
                            let path = item["path"].as_str().unwrap_or("").to_string();
                            if let Some(object) = item.as_object_mut() {
                                object.remove("path");
                            }
                            grouped.entry(path).or_default().push(item);
                        }
                        res["nodes"]["grouped_by_file"] =
                            serde_json::to_value(grouped).unwrap_or_default();
                    }
                }
                Ok(res)
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
        description = "Trace dependencies for a selector. direction: 'inbound' (default) or 'outbound'. Optional mode: 'calls' (default, invocation/call graph), 'data-flow' (parameter/assignment flow), 'all' (all dependency edges). Optional edge_types: ['calls', 'mutates', 'inherits', 'implements', 'imports']. Always start with depth=1 to avoid exponential graph fan-out. Paged results require offset + generation for continuation."
    )]
    fn code_map_impact(&self, Parameters(p): Parameters<Impact>) -> CallToolResult {
        self.responding(|policy| {
            let page = p.page.resolve(policy)?;
            self.read(|c| {
                traversal::traverse_paged_with_filter(
                    c,
                    &p.selector,
                    p.depth,
                    traversal::impact_inbound(Some(p.direction.as_str()))?,
                    p.mode.as_deref(),
                    p.edge_types.as_deref(),
                    &page,
                )
            })
        })
    }
    #[tool(
        description = "Explain symbol ownership, direct edges, and architectural invariants. Compact results include exact signature and docstring, receiver-aware container, inbound/outbound call counts, and up to five direct callers/callees. direction: both (default), incoming/inbound, or outgoing/outbound. Set include_coverage=true for paged resolution coverage, show_doc=false to omit linked documents, or show_source=true for bounded source."
    )]
    fn code_map_explain(&self, Parameters(p): Parameters<Explain>) -> CallToolResult {
        self.responding(|policy| {
            let page = p.page.resolve(policy)?;
            let source = p.source(policy, false)?;
            let coverage = CoverageOptions {
                include_coverage: p.include_coverage,
            };
            self.read(|c| {
                symbols::explain_paged_with_docs_and_coverage(
                    c,
                    &self.root,
                    &p.selector,
                    p.direction.as_deref(),
                    p.show_doc,
                    &source,
                    &page,
                    coverage,
                )
            })
        })
    }
    #[tool(
        description = "Run a bounded graph operation: overview, inspect, explain, impact, slice, unwired, or sql. For sql, pass exactly one read-only SELECT/WITH statement in selector; limit, offset, generation, and depth are separate arguments. Impact direction is inbound (default) or outbound. Set include_coverage=true for inspect or explain coverage."
    )]
    fn code_map_query(&self, Parameters(p): Parameters<Query>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| {
                traversal::query_paged_with_options(
                    c,
                    &p.operation,
                    p.selector.as_deref(),
                    p.depth,
                    p.direction.as_deref(),
                    CoverageOptions {
                        include_coverage: p.include_coverage,
                    },
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
        description = "Check one candidate's removal safety using indexed incoming dependencies and candidate-scoped unresolved references. Workspace unresolved and parse-error diagnostics are reported separately."
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
            self.read(|conn| {
                cli::ast::search_paged(
                    conn,
                    &self.root,
                    &p.pattern,
                    &p.language,
                    p.path.as_deref(),
                    &p.page.resolve(policy)?,
                )
            })
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
        let tool_name = request.name.clone();
        let tool_args = serde_json::to_string_pretty(&request.arguments).unwrap_or_default();
        let call = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        let result = Self::tool_router().call(call).await;
        let adapter = scanner::load_adapter(&self.root, None).ok();
        let policy = adapter
            .as_ref()
            .map(|a| a.response.clone())
            .unwrap_or_default();
        let final_response: Result<rmcp::model::CallToolResponse, rmcp::ErrorData> = match result {
            Ok(rmcp::model::CallToolResponse::Complete(result)) => {
                Ok(response::enforce(result, &policy).into())
            }
            Ok(result) => Ok(result),
            Err(error) => {
                Ok(response::result(Err(anyhow::anyhow!(error.to_string())), &policy).into())
            }
        };
        if adapter.as_ref().is_some_and(|a| a.debug) {
            let output_val = match &final_response {
                Ok(rmcp::model::CallToolResponse::Complete(res)) => {
                    let mut text_val = None;
                    for item in &res.content {
                        if let Some(t) = item.as_text() {
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&t.text) {
                                text_val = Some(val);
                                break;
                            }
                        }
                    }
                    text_val.unwrap_or_else(|| {
                        serde_json::to_value(res)
                            .unwrap_or(serde_json::json!({"status": "complete"}))
                    })
                }
                Ok(resp) => serde_json::json!({ "raw": format!("{resp:?}") }),
                Err(err) => serde_json::json!({ "error": err.to_string() }),
            };
            crate::core::debug_log::log_mcp(&self.root, &tool_name, &tool_args, &output_val);
        }
        final_response
    }
}
