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
/// Paging options for collections returning more items than the page limit.
pub struct PageInput {
    /// Maximum items per collection, 1..=100; defaults to 30.
    pub limit: Option<usize>,
    #[serde(default)]
    /// Zero-based collection offset. Use next_offset from previous page.
    pub offset: usize,
    /// compact omits heavy node details. full includes them within the byte budget.
    pub detail: Option<Detail>,
    /// Token from the previous page. Required when offset is greater than 0.
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
/// Input for workspace overview query.
pub struct OverviewInput {
    /// Optional aspects to include: 'counts', 'components', 'languages', 'cycles', 'compiled_profiles', 'metadata'. If omitted, all standard aspects are included.
    pub aspects: Option<Vec<String>>,
    #[serde(flatten)]
    /// Paging options.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Input for symbol removal safety proof.
pub struct Selector {
    /// Indexed symbol id, qualified name, file path, path:symbol, path::symbol, path:line, or path#Lline. Prefixes ./, file:, and file:// are accepted. A bare name can be ambiguous.
    pub selector: String,
    #[serde(flatten)]
    /// Paging options.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Input for detailed code symbol inspection.
pub struct Inspect {
    /// Indexed symbol id, qualified name, file path, path:symbol, path::symbol, path:line, or path#Lline. Prefixes ./, file:, and file:// are accepted. A bare name can be ambiguous.
    pub selector: String,
    #[serde(default = "yes")]
    /// Include linked documentation sections and docstring in output (default true).
    pub show_doc: bool,
    /// Include paged resolution coverage breakdown (resolved vs external vs unresolved references).
    #[serde(default)]
    pub include_coverage: bool,
    /// Include bounded source code preview of symbol definition.
    pub show_source: Option<bool>,
    /// Lines before the symbol. Default 5. Range 0..=20.
    pub leading_lines: Option<usize>,
    /// Body lines to return. Default 35. Range 1..=100.
    pub max_body_lines: Option<usize>,
    /// Line offset within symbol body for continuing preview; requires generation from previous preview.
    #[serde(default)]
    pub source_offset: usize,
    #[serde(flatten)]
    /// Paging options.
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
/// Input for bounded source code snippet retrieval.
pub struct Snippet {
    /// Indexed symbol id, qualified name, file path, path:symbol, path::symbol, path:line, or path#Lline. Prefixes ./, file:, and file:// are accepted. A bare name can be ambiguous.
    pub selector: String,
    /// Ignored. This tool always returns the preview.
    pub show_source: Option<bool>,
    /// Lines before the symbol. Default 5. Range 0..=20.
    pub leading_lines: Option<usize>,
    /// Body lines to return. Default 35. Range 1..=100.
    pub max_body_lines: Option<usize>,
    /// Body line offset. Continue with next_source_offset and the previous generation.
    #[serde(default)]
    pub source_offset: usize,
    #[serde(flatten)]
    /// Paging options.
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
/// Input for structural symbol explanation and architectural invariant inspection.
pub struct Explain {
    /// Indexed symbol id, qualified name, file path, path:symbol, path::symbol, path:line, or path#Lline. Prefixes ./, file:, and file:// are accepted. A bare name can be ambiguous.
    pub selector: String,
    /// both (default), incoming or inbound, outgoing or outbound.
    pub direction: Option<String>,
    #[serde(default = "yes")]
    /// Include linked documentation sections, docstrings, and architectural invariants (default true).
    pub show_doc: bool,
    /// Include paged resolution coverage details for references within this symbol.
    #[serde(default)]
    pub include_coverage: bool,
    /// Include bounded source code preview of symbol definition.
    pub show_source: Option<bool>,
    /// Lines before the symbol. Default 5. Range 0..=20.
    pub leading_lines: Option<usize>,
    /// Body lines to return. Default 35. Range 1..=100.
    pub max_body_lines: Option<usize>,
    /// Body line offset. Continue with next_source_offset and the previous generation.
    #[serde(default)]
    pub source_offset: usize,
    #[serde(flatten)]
    /// Paging options.
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
/// Input for workspace symbol search.
pub struct SearchSymbols {
    /// Search query string. Supports token terms and prefix wildcards (e.g. 'Token*', 'parse_req*'). When exact=true, matches exact symbol or qualified name.
    pub pattern: String,
    /// If true, performs exact case-insensitive symbol name or qualified name matching without FTS.
    #[serde(default)]
    pub exact: bool,
    /// nodes.kind filter. function and method match each other. Omit to match every kind.
    pub kind: Option<String>,
    /// Restrict results to a workspace-relative file or directory (e.g. 'src/engine/' or 'src/main.rs').
    pub path: Option<String>,
    /// Group results by file path in the response.
    #[serde(default)]
    pub group_by_file: bool,
    /// Include Markdown documentation nodes matching pattern in search results.
    #[serde(default)]
    pub include_docs: bool,
    #[serde(flatten)]
    /// Paging options.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Input for finding test relationships for a symbol.
pub struct Tests {
    /// Indexed symbol id, qualified name, file path, path:symbol, path::symbol, path:line, or path#Lline. Prefixes ./, file:, and file:// are accepted. A bare name can be ambiguous.
    pub selector: String,
    #[serde(default = "inbound")]
    /// inbound (default) finds tests that exercise the target. outbound finds production code a test depends on.
    pub direction: String,
    #[serde(flatten)]
    /// Paging options.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Input for dependency graph traversal and change impact analysis.
pub struct Impact {
    /// Indexed symbol id, qualified name, file path, path:symbol, path::symbol, path:line, or path#Lline. Prefixes ./, file:, and file:// are accepted. A bare name can be ambiguous.
    pub selector: String,
    /// inbound (default) lists dependents. outbound lists dependencies.
    #[serde(default = "inbound")]
    pub direction: String,
    #[serde(default = "depth")]
    /// Edge depth. Default 2. Maximum 16. Depth above 1 is rejected when the first frontier exceeds 1000 links.
    pub depth: u32,
    /// Omit for the default walk: calls, inherits, implements, overrides, extends, includes, mutates, handles, references, calls_endpoint, imports, contains, documents, plus reverse decorates. data-flow or dataflow follows mutates, references, and handles. all matches the default walk.
    pub mode: Option<String>,
    /// Edge kind names. A non-empty list replaces mode. decorates is walked in reverse; every other kind is walked forward.
    pub edge_types: Option<Vec<String>>,
    #[serde(flatten)]
    /// Paging options.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Input for specialized graph operations or read-only SQL queries.
pub struct Query {
    /// overview, impact, slice, unwired, or sql. inspect and explain match the dedicated tools. Aliases: doctor for overview; search, discover, and find for symbol search.
    pub operation: String,
    /// Selector for impact, slice, inspect, or explain. For sql, exactly one read-only SELECT or WITH. Ignored for overview and unwired.
    pub selector: Option<String>,
    /// inbound or outbound for impact and slice. Default inbound.
    pub direction: Option<String>,
    /// Include resolution coverage breakdown in query results.
    #[serde(default)]
    pub include_coverage: bool,
    #[serde(default = "depth")]
    /// Edge depth for impact and slice. Default 2. Maximum 16.
    pub depth: u32,
    #[serde(flatten)]
    /// Paging options.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Input for workspace diagnostics, parse errors, and dependency cycle analysis.
pub struct Analyze {
    /// Empty string for workspace totals, an indexed file or directory, or exactly one read-only SELECT or WITH. lint=true reads stored syntax diagnostics for that path and does not run SQL.
    pub target: String,
    /// Compute static dependency cycles. Cannot combine with lint=true. Omitted cycles stay off.
    pub include_cycles: Option<bool>,
    /// Set true to return stored syntax parse diagnostics across indexed files. Cannot be combined with include_cycles=true.
    #[serde(default)]
    pub lint: bool,
    #[serde(flatten)]
    /// Paging options.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Input for structural AST pattern search using Tree-sitter.
pub struct Ast {
    /// Structural syntax pattern with Tree-sitter metavariables (e.g. 'fn $NAME($$$ARGS) -> $RET', '$VAR = await $CALL($$$ARGS)').
    pub pattern: String,
    /// Compiled language profile id, such as rust, python, typescript, javascript, go, or java. An unknown id fails with the compiled list.
    pub language: String,
    /// Optional workspace-relative file or directory to constrain search scope (e.g. 'src/engine/').
    pub path: Option<String>,
    #[serde(flatten)]
    /// Paging options.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Input for full-text search across indexed documentation and ADRs.
pub struct SearchDocs {
    /// Documentation search query or keywords (e.g. 'Merkle tree commitments', 'LanguageLinker trait', 'ACDD gates').
    pub query: String,
    /// Exact doc_type frontmatter value, such as architecture, adr, guide, api, or plan.
    pub doc_type: Option<String>,
    /// Component name or document path prefix.
    pub component: Option<String>,
    /// Include a match-centered text preview for each matched section (default false).
    #[serde(default)]
    pub include_excerpt: bool,
    #[serde(flatten)]
    /// Paging options.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Input for reading indexed markdown documentation.
pub struct GetDoc {
    /// Document path relative to workspace or document ID (e.g. 'docs/architecture/indexing.md', 'docs/adr/0001-record.md').
    pub path_or_id: String,
    /// Optional heading section title to read only that specific section instead of entire document.
    pub section: Option<String>,
    #[serde(flatten)]
    /// Paging options.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Input for interactive reference and operational guidance.
pub struct Guide {
    #[serde(default)]
    /// query (default), acdd, docs, adapter, validate, or init. An unknown topic fails.
    pub topic: Option<String>,
    #[serde(default)]
    /// For topic=init, overwrite an existing forge-mcp.yaml.
    pub force: bool,
}
#[derive(Deserialize, JsonSchema)]
/// Input for session checkpoint operations.
pub struct Checkpoint {
    /// Checkpoint operation: 'list' (show all), 'save' (persist state), 'get' (retrieve by name), or 'delete' (remove by name).
    pub action: String,
    /// Required for save, get, and delete. Length 1..=200. Must not contain .., /, \, or NUL.
    pub name: Option<String>,
    #[schemars(schema_with = "checkpoint_content_schema")]
    /// JSON value required for save. At most 1 MiB.
    pub content: Option<Value>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
/// Operation for task blackboard.
pub enum BlackboardAction {
    /// Post a new coordination message to the task blackboard.
    Post,
    /// Read chronological coordination messages from the task blackboard.
    Read,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
/// Parameters for task blackboard collaboration in .forge/tasks.sqlite.
pub struct Blackboard {
    /// Action to perform: 'post' to write a message, 'read' to read messages.
    pub action: BlackboardAction,
    /// Task identifier (e.g. 'repository/project/milestone:task_ref').
    pub task_id: String,
    /// Message author for 'post'. Defaults to the active claim worker or 'mcp'.
    pub author: Option<String>,
    /// Free-form topic. Conventional topics: hypothesis, architectural_seam, measured_delta, blockers, contract_draft, build_proof, architectural_notes. architectural_notes is kept in the delivery receipt; other topics are cleared at deliver.
    pub topic: Option<String>,
    /// Required for post. Rejected for read.
    pub payload: Option<String>,
    /// Maximum messages for read. Rejected for post. Omit to read every match.
    pub limit: Option<usize>,
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
        description = "List tasks. The default is ready, unclaimed tasks with satisfied dependencies in the primary workspace."
    )]
    fn task_list(&self, Parameters(p): Parameters<super::tasks::List>) -> CallToolResult {
        self.responding(|_| super::tasks::list(&self.root, p))
    }
    #[tool(
        description = "Atomically claim one gate in a worktree. bundle=true returns the specification, guidance, and recent blackboard messages."
    )]
    fn task_claim(&self, Parameters(p): Parameters<super::tasks::Claim>) -> CallToolResult {
        self.responding(|_| super::tasks::claim(&self.root, p))
    }
    #[tool(
        description = "Pass or reject the claimed gate. Passing deliver/v1 writes the milestone receipt and clears the blackboard. The evidence field describes the required object."
    )]
    fn task_submit(&self, Parameters(p): Parameters<super::tasks::Submit>) -> CallToolResult {
        self.responding(|_| super::tasks::submit(&self.root, p))
    }
    #[tool(
        description = "Run one task-store action: create, sync, inspect, delete, extend_scope, subtask_add, subtask_update, subtask_list, reset, reopen, or context. sync without milestone_ref imports the workspace milestone directory."
    )]
    fn task_manage(&self, Parameters(p): Parameters<super::tasks::Manage>) -> CallToolResult {
        self.responding(|_| super::tasks::manage(&self.root, p))
    }
    #[tool(
        description = "Post or read task messages. post requires topic and payload and rejects limit. read rejects author and payload. An omitted author is the claim worker, or mcp when unclaimed."
    )]
    fn task_blackboard(&self, Parameters(p): Parameters<Blackboard>) -> CallToolResult {
        self.responding(|_| {
            let store = crate::engine::tasks::store_for_task(&self.root, &p.task_id)?;
            match p.action {
                BlackboardAction::Post => {
                    if p.limit.is_some() {
                        anyhow::bail!("TASK_BLACKBOARD_INVALID: limit is only valid for read");
                    }
                    let topic = p.topic.as_deref().ok_or_else(|| {
                        anyhow::anyhow!("TASK_BLACKBOARD_INVALID: post requires topic")
                    })?;
                    let payload = p.payload.as_deref().ok_or_else(|| {
                        anyhow::anyhow!("TASK_BLACKBOARD_INVALID: post requires payload")
                    })?;
                    let author = match p.author {
                        Some(author) => author,
                        None => store
                            .inspect(&p.task_id)?
                            .worker_id
                            .unwrap_or_else(|| "mcp".into()),
                    };
                    Ok(serde_json::json!({
                        "id": store.blackboard_post(&p.task_id, &author, topic, payload)?
                    }))
                }
                BlackboardAction::Read => {
                    if p.author.is_some() || p.payload.is_some() {
                        anyhow::bail!(
                            "TASK_BLACKBOARD_INVALID: author and payload are only valid for post"
                        );
                    }
                    Ok(serde_json::json!({
                        "messages": store.blackboard_read(&p.task_id, p.topic.as_deref(), p.limit)?
                    }))
                }
            }
        })
    }
    #[tool(
        description = "Workspace root, counts, languages, and resolution coverage. Call this first and confirm workspace_root is the active worktree before an absence claim."
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
        description = "Compact symbol record: signature, docstring, container, call counts, and at most five direct call previews per direction. include_coverage adds paged resolution evidence. Source stays off unless show_source is true."
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
        description = "Bounded source preview for one selector. Defaults are 5 leading lines and 35 body lines. The on-disk file must match the indexed digest. Continue with source_offset and generation."
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
        description = "Search names, qualified names, and tokens. A trailing * is a prefix. exact=true matches an indexed name or qualname without full text. Results include selectors for inspect."
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
        description = "Test relationships for a narrow selector. inbound finds tests that exercise the target; outbound finds production code a test depends on. A hit is a direct edge or scope_or_transitive, not a full path. Broad scopes are rejected."
    )]
    fn code_map_tests(&self, Parameters(p): Parameters<Tests>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| {
                symbols::tests_paged(c, &p.selector, &p.direction, &p.page.resolve(policy)?)
            })
        })
    }
    #[tool(
        description = "Transitive reachability from one selector. inbound (default) lists dependents; outbound lists dependencies. Start at depth=1. Each node includes one shortest predecessor edge. Depth above 1 fails when the first frontier exceeds 1000 links."
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
        description = "Symbol summary plus paged incoming and outgoing edges of every kind. The embedded call summary shows counts and at most five direct call previews per direction. direction defaults to both."
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
        description = "Bounded graph operation or one read-only SELECT/WITH. Prefer code_map_inspect, code_map_explain, and code_map_impact for those jobs. unwired lists functions and methods with no indexed static caller and is not a dead-code proof."
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
        description = "Diagnostics for the workspace or one path, one read-only SELECT or WITH, stored syntax diagnostics, or static cycles. lint=true cannot combine with include_cycles=true and does not run SQL."
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
        description = "Indexed removal check for one symbol or file. Returns incoming dependency edges, candidate-scoped unresolved references and parse errors, and safe_to_remove. That verdict excludes workspace-wide diagnostics and is not a runtime-safety proof. More than 10000 nodes is rejected."
    )]
    fn code_map_prove_removal(&self, Parameters(p): Parameters<Selector>) -> CallToolResult {
        self.responding(|policy| {
            self.read(|c| traversal::removal_paged(c, &p.selector, &p.page.resolve(policy)?))
        })
    }
    #[tool(
        description = "Tree-sitter structural search over admitted source. $NAME captures one node and $$$ARGS captures a sequence. language must be a compiled profile id."
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
        description = "Full-text search of indexed Markdown sections. Returns paths and section anchors for get_doc. include_excerpt adds a short match-centered preview."
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
        description = "Read one indexed Markdown document. The default detail is full text. detail=compact returns a heading outline. A missing path or section is an error."
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
    #[tool(description = "Built-in guide. The default topic is query.")]
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
        description = "List, save, get, or delete a JSON checkpoint in .forge/checkpoints.json."
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
    instructions = "Workspace code graph and task coordination, 20 tools. Responses cap at 64 KiB. SQLite budget is 2 seconds. Start with code_map_overview and check coverage before an absence claim. Continue a page with offset and generation. Prefer detail=compact. Selector rules and gate proofs: forge_guide topic=query or acdd."
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
