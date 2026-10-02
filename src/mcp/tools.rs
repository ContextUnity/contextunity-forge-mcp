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

fn navigation_error(error: anyhow::Error) -> anyhow::Error {
    if let Some(reader::SelectorError::Ambiguous { candidates, .. }) =
        error.downcast_ref::<reader::SelectorError>()
    {
        let calls = candidates
            .iter()
            .map(|id| format!("code_map_inspect({})", serde_json::json!({"selector":id})))
            .collect::<Vec<_>>()
            .join("\n");
        return anyhow::anyhow!("{error}\n{calls}");
    }
    error
}

#[derive(Default, Deserialize, JsonSchema)]
/// Represents page input data.
pub struct PageInput {
    /// Items per collection, 1..=100; defaults to adapter response.page_size (30).
    pub limit: Option<usize>,
    #[serde(default)]
    /// The offset value.
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
/// Represents overview input data.
pub struct OverviewInput {
    /// Optional aspects to include: 'counts', 'components', 'languages', 'cycles', 'compiled_profiles', 'metadata'. If omitted, all standard aspects are included.
    pub aspects: Option<Vec<String>>,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents selector data.
pub struct Selector {
    /// The selector value.
    pub selector: String,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents inspect data.
pub struct Inspect {
    /// The selector value.
    pub selector: String,
    #[serde(default = "yes")]
    /// Whether show doc applies.
    pub show_doc: bool,
    /// Include paged resolution coverage in addition to the compact summary.
    #[serde(default)]
    pub include_coverage: bool,
    /// Optional show source value.
    pub show_source: Option<bool>,
    /// Optional leading lines value.
    pub leading_lines: Option<usize>,
    /// Optional max body lines value.
    pub max_body_lines: Option<usize>,
    /// Body line offset returned as next_source_offset by a previous preview.
    #[serde(default)]
    pub source_offset: usize,
    #[serde(flatten)]
    /// The page value.
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
/// Represents snippet data.
pub struct Snippet {
    /// The selector value.
    pub selector: String,
    /// Optional show source value.
    pub show_source: Option<bool>,
    /// Optional leading lines value.
    pub leading_lines: Option<usize>,
    /// Optional max body lines value.
    pub max_body_lines: Option<usize>,
    /// Body line offset returned as next_source_offset by a previous preview.
    #[serde(default)]
    pub source_offset: usize,
    #[serde(flatten)]
    /// The page value.
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
/// Represents explain data.
pub struct Explain {
    /// The selector value.
    pub selector: String,
    /// Optional direction value.
    pub direction: Option<String>,
    #[serde(default = "yes")]
    /// Whether show doc applies.
    pub show_doc: bool,
    /// Include paged resolution coverage in addition to the compact summary.
    #[serde(default)]
    pub include_coverage: bool,
    /// Optional show source value.
    pub show_source: Option<bool>,
    /// Optional leading lines value.
    pub leading_lines: Option<usize>,
    /// Optional max body lines value.
    pub max_body_lines: Option<usize>,
    /// Body line offset returned as next_source_offset by a previous preview.
    #[serde(default)]
    pub source_offset: usize,
    #[serde(flatten)]
    /// The page value.
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
/// Represents search symbols data.
pub struct SearchSymbols {
    /// The pattern value.
    pub pattern: String,
    /// Match only the complete symbol name or qualified name, without full-text search.
    #[serde(default)]
    pub exact: bool,
    /// Optional kind value.
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
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents tests data.
pub struct Tests {
    /// The selector value.
    pub selector: String,
    #[serde(default = "inbound")]
    /// The direction value.
    pub direction: String,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents impact data.
pub struct Impact {
    /// The selector value.
    pub selector: String,
    /// Impact direction: 'inbound' (default) or 'outbound'.
    #[serde(default = "inbound")]
    pub direction: String,
    #[serde(default = "depth")]
    /// The depth value.
    pub depth: u32,
    /// Traversal mode: 'calls' (default, invocation/call graph), 'data-flow' (parameter/assignment flow), 'all' (all dependency edges).
    pub mode: Option<String>,
    /// Optional specific edge kinds to follow (e.g. ['calls', 'mutates', 'inherits', 'implements', 'imports']).
    pub edge_types: Option<Vec<String>>,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents query data.
pub struct Query {
    /// The operation value.
    pub operation: String,
    /// Optional selector value.
    pub selector: Option<String>,
    /// Impact direction: 'inbound' (default) or 'outbound'.
    pub direction: Option<String>,
    /// Include paged resolution coverage for inspect and explain operations.
    #[serde(default)]
    pub include_coverage: bool,
    #[serde(default = "depth")]
    /// The depth value.
    pub depth: u32,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents analyze data.
pub struct Analyze {
    /// The target value.
    pub target: String,
    /// Whether include cycles applies.
    pub include_cycles: Option<bool>,
    /// Read stored syntax diagnostics only; does not run a linter or reparse source.
    #[serde(default)]
    pub lint: bool,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents ast data.
pub struct Ast {
    /// The pattern value.
    pub pattern: String,
    /// The language value.
    pub language: String,
    /// Optional path value.
    pub path: Option<String>,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents search docs data.
pub struct SearchDocs {
    /// The query value.
    pub query: String,
    /// Optional doc type value.
    pub doc_type: Option<String>,
    /// Optional component value.
    pub component: Option<String>,
    /// Include a short match-centered content excerpt in each result.
    #[serde(default)]
    pub include_excerpt: bool,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents get doc data.
pub struct GetDoc {
    /// The path or id value.
    pub path_or_id: String,
    /// Optional section value.
    pub section: Option<String>,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents guide data.
pub struct Guide {
    #[serde(default)]
    /// Optional topic value.
    pub topic: Option<String>,
    #[serde(default)]
    /// Whether force applies.
    pub force: bool,
}
#[derive(Deserialize, JsonSchema)]
/// Represents checkpoint data.
pub struct Checkpoint {
    /// The action value.
    pub action: String,
    /// Optional name value.
    pub name: Option<String>,
    #[schemars(schema_with = "checkpoint_content_schema")]
    /// Optional content value.
    pub content: Option<Value>,
}
impl Server {
    fn responding(
        &self,
        run: impl FnOnce(&ResponsePolicy) -> anyhow::Result<Value>,
    ) -> CallToolResult {
        self.responding_with_symbol(run, None)
    }

    fn responding_with_symbol(
        &self,
        run: impl FnOnce(&ResponsePolicy) -> anyhow::Result<Value>,
        explain: Option<bool>,
    ) -> CallToolResult {
        match scanner::load_adapter(&self.root, None) {
            Ok(adapter) => response::result_with_symbol(
                run(&adapter.response).map_err(navigation_error),
                &adapter.response,
                explain,
            ),
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
            self.read(|c| {
                reader::overview_with_options(
                    c,
                    &reader::OverviewOptions {
                        aspects: p.aspects.as_deref(),
                        page: &page,
                    },
                )
            })
        })
    }
    #[tool(
        description = "Inspect an indexed symbol by selector (e.g. 'function:name', 'class:Name', 'module:path'). Compact output includes exact signature and docstring, receiver-aware container, inbound/outbound call counts, and up to five direct callers/callees. Set include_coverage=true for paged resolution coverage; detail='full' includes node details and per-reference evidence. Set show_source=true for bounded source or use get_code_snippet."
    )]
    fn code_map_inspect(&self, Parameters(p): Parameters<Inspect>) -> CallToolResult {
        self.responding_with_symbol(
            |policy| {
                let page = p.page.resolve(policy)?;
                let source = p.source(policy, false)?;
                let coverage = CoverageOptions {
                    include_coverage: p.include_coverage,
                };
                self.read(|c| {
                    symbols::inspect_with_options(
                        c,
                        &self.root,
                        &p.selector,
                        &symbols::InspectOptions {
                            show_doc: p.show_doc,
                            source: &source,
                            coverage,
                            page: &page,
                        },
                    )
                })
            },
            Some(false),
        )
    }
    #[tool(
        description = "Read source and location for a symbol selector (default 5 leading + 35 body lines). Pass source_offset with previous generation for next lines. Use code_map_inspect for resolution coverage and evidence; use ctx_read for full file edits."
    )]
    fn get_code_snippet(&self, Parameters(p): Parameters<Snippet>) -> CallToolResult {
        self.responding(|policy| {
            let page = p.page.resolve(policy)?;
            let source = p.source(policy)?;
            self.read(|c| {
                let mut value = symbols::snippet_paged(c, &self.root, &p.selector, &source, &page)?;
                if let (Some(path), Some(start), Some(end)) = (
                    value["node"]["path"].as_str(),
                    value["source_preview"]["start_line"].as_u64(),
                    value["source_preview"]["end_line"].as_u64(),
                ) {
                    value["header"] = serde_json::json!(format!(
                        "{} {} in {path}:{start}-{end}",
                        value["node"]["kind"].as_str().unwrap_or("symbol"),
                        value["node"]["qualname"]
                            .as_str()
                            .or_else(|| value["node"]["name"].as_str())
                            .unwrap_or(&p.selector)
                    ));
                }
                Ok(value)
            })
        })
    }
    #[tool(
        description = "Search indexed symbols with FTS terms or prefix* pattern (e.g. 'Token*', 'router'). Set exact=true for indexed, case-insensitive name or qualified-name equality without FTS. Optional path restricts results to a workspace-relative file or directory. Returns symbol IDs, kinds, and paths. Compact pages default to 30; max 100."
    )]
    fn code_map_search(&self, Parameters(p): Parameters<SearchSymbols>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| {
                let page = p.page.resolve(policy)?;
                let mut res = symbols::search_with_options(
                    c,
                    &p.pattern,
                    &symbols::SearchOptions {
                        kind: p.kind.as_deref(),
                        path: p.path.as_deref(),
                        include_docs: p.include_docs,
                        exact: p.exact,
                        page: &page,
                    },
                )?;
                if let Some(items) = res["nodes"]["items"].as_array_mut() {
                    for item in items {
                        if let Some(id) = item["id"].as_str().map(str::to_owned) {
                            item["inspect_selector"] = Value::String(id);
                        }
                    }
                }
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
                traversal::traverse_with_options(
                    c,
                    &p.selector,
                    &traversal::TraversalOptions {
                        depth: p.depth,
                        inbound: traversal::impact_inbound(Some(p.direction.as_str()))?,
                        mode: p.mode.as_deref(),
                        edge_types: p.edge_types.as_deref(),
                        page: &page,
                    },
                )
            })
        })
    }
    #[tool(
        description = "Explain symbol ownership, direct edges, and architectural invariants. Compact results include exact signature and docstring, receiver-aware container, inbound/outbound call counts, and up to five direct callers/callees. direction: both (default), incoming/inbound, or outgoing/outbound. Set include_coverage=true for paged resolution coverage, show_doc=false to omit linked documents, or show_source=true for bounded source."
    )]
    fn code_map_explain(&self, Parameters(p): Parameters<Explain>) -> CallToolResult {
        self.responding_with_symbol(
            |policy| {
                let page = p.page.resolve(policy)?;
                let source = p.source(policy, false)?;
                let coverage = CoverageOptions {
                    include_coverage: p.include_coverage,
                };
                self.read(|c| {
                    symbols::explain_with_options(
                        c,
                        &self.root,
                        &p.selector,
                        &symbols::ExplainOptions {
                            direction: p.direction.as_deref(),
                            show_doc: p.show_doc,
                            source: &source,
                            coverage,
                            page: &page,
                        },
                    )
                })
            },
            Some(true),
        )
    }
    #[tool(
        description = "Legacy bounded graph operation: overview, impact, slice, unwired, or sql. The inspect and explain operations are deprecated; use code_map_inspect and code_map_explain. Use code_map_analyze for diagnostics. For sql, pass one read-only SELECT/WITH statement in selector; limit, offset, generation, and depth are separate arguments."
    )]
    fn code_map_query(&self, Parameters(p): Parameters<Query>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| {
                traversal::query_with_options(
                    c,
                    &p.operation,
                    p.selector.as_deref(),
                    &traversal::GraphQueryOptions {
                        depth: p.depth,
                        direction: p.direction.as_deref(),
                        coverage: CoverageOptions {
                            include_coverage: p.include_coverage,
                        },
                        page: &p.page.resolve(policy)?,
                    },
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
                reader::search_docs_with_options(
                    c,
                    &p.query,
                    &reader::DocSearchOptions {
                        doc_type: p.doc_type.as_deref(),
                        component: p.component.as_deref(),
                        include_excerpt: p.include_excerpt,
                        page: &p.page.resolve(policy)?,
                    },
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
