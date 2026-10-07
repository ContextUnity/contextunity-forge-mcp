use super::{response, server::Server};
use crate::{
    cli,
    core::response::{CoverageOptions, Detail, ResponsePolicy},
    db::{reader, symbols, traversal},
    engine::scanner,
};
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_handler, tool_router,
    ServerHandler,
};
use serde_json::Value;

#[path = "tools/inputs.rs"]
mod inputs;
pub use inputs::{
    Analyze, Ast, Checkpoint, Explain, GetDoc, Guide, Impact, Inspect, OverviewInput, PageInput,
    Query, SearchDocs, SearchSymbols, Selector, Snippet, Tests,
};

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

impl Server {
    fn responding(
        &self,
        run: impl FnOnce(&scanner::Adapter, &ResponsePolicy) -> anyhow::Result<Value>,
    ) -> CallToolResult {
        self.responding_with_symbol(run, None)
    }

    fn responding_with_symbol(
        &self,
        run: impl FnOnce(&scanner::Adapter, &ResponsePolicy) -> anyhow::Result<Value>,
        explain: Option<bool>,
    ) -> CallToolResult {
        match self.adapter_snapshot() {
            Ok(adapter) => response::result_with_symbol(
                run(&adapter, &adapter.response).map_err(navigation_error),
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
        description = "List tasks. The default is ready, unclaimed tasks with satisfied dependencies in the primary workspace. milestone_status defaults to active and accepts active, planned, completed, or all; a targeted milestone_ref defaults to all statuses. Subtask listings are compact by default; set detail=full to include titles and verification evidence."
    )]
    fn task_list(&self, Parameters(p): Parameters<super::tasks::List>) -> CallToolResult {
        self.responding(|_, _| super::tasks::list(&self.root, p))
    }
    #[tool(
        description = "Atomically claim one gate in a worktree. Returns the zero-shot context bundle (contract, guidance, scoped ADRs, covering tests, blackboard history) by default (bundle=false returns minimal details)."
    )]
    fn task_claim(&self, Parameters(p): Parameters<super::tasks::Claim>) -> CallToolResult {
        self.responding(|_, _| super::tasks::claim(&self.root, p))
    }
    #[tool(
        description = "Pass or reject the claimed gate. Passing deliver/v1 writes the milestone receipt and clears the blackboard. The evidence field describes the required object."
    )]
    fn task_submit(&self, Parameters(p): Parameters<super::tasks::Submit>) -> CallToolResult {
        self.responding(|_, _| super::tasks::submit(&self.root, p))
    }
    #[tool(
        description = "Run one task-store action: create, sync, inspect, delete, extend_scope, subtask_add, subtask_update, subtask_list, reset, reopen, or context. sync without milestone_ref imports the workspace milestone directory."
    )]
    fn task_manage(&self, Parameters(p): Parameters<super::tasks::Manage>) -> CallToolResult {
        self.responding(|_, _| super::tasks::manage(&self.root, p))
    }
    #[tool(
        description = "Post, read, or inspect milestone-, task-, and subtask-scoped messages. Omitted scope and keys resolve only from a unique active context. Reads default to 10 payload-free summaries, capped at 50; inspect by message_id returns the payload."
    )]
    fn task_blackboard(
        &self,
        Parameters(p): Parameters<crate::engine::tasks::BlackboardRequest>,
    ) -> CallToolResult {
        self.responding(|_, _| crate::engine::tasks::blackboard(&self.root, p, "mcp"))
    }
    #[tool(
        description = "Workspace root, counts, languages, and resolution coverage. Call this first and confirm workspace_root is the active worktree before an absence claim."
    )]
    fn code_map_overview(&self, Parameters(p): Parameters<OverviewInput>) -> CallToolResult {
        self.responding(|adapter, policy| {
            let page = p.page.resolve(policy)?;
            self.read_with_adapter(adapter, |c| {
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
            |adapter, policy| {
                let page = p.page.resolve(policy)?;
                let source = p.source(policy, false)?;
                let coverage = CoverageOptions {
                    include_coverage: p.include_coverage,
                };
                self.read_with_adapter(adapter, |c| {
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
        self.responding(|adapter, policy| {
            let page = p.page.resolve(policy)?;
            let source = p.source(policy)?;
            self.read_with_adapter(adapter, |c| {
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
        self.responding(|adapter, policy| {
            self.read_with_adapter(adapter, |c| {
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
        self.responding(|adapter, policy| {
            self.read_with_adapter(adapter, |c| {
                symbols::tests_paged(c, &p.selector, &p.direction, &p.page.resolve(policy)?)
            })
        })
    }
    #[tool(
        description = "Transitive reachability from one selector. inbound (default) lists dependents; outbound lists dependencies. Start at depth=1. Each node includes one shortest predecessor edge. Depth above 1 fails when the first frontier exceeds 1000 links."
    )]
    fn code_map_impact(&self, Parameters(p): Parameters<Impact>) -> CallToolResult {
        self.responding(|adapter, policy| {
            let page = p.page.resolve(policy)?;
            self.read_with_adapter(adapter, |c| {
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
            |adapter, policy| {
                let page = p.page.resolve(policy)?;
                let source = p.source(policy, false)?;
                let coverage = CoverageOptions {
                    include_coverage: p.include_coverage,
                };
                self.read_with_adapter(adapter, |c| {
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
        self.responding(|adapter, policy| {
            self.read_with_adapter(adapter, |c| {
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
        self.responding(|adapter, policy| {
            anyhow::ensure!(
                !p.lint || p.include_cycles != Some(true),
                "lint=true cannot be combined with include_cycles=true"
            );
            self.read_with_adapter(adapter, |c| {
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
        self.responding(|adapter, policy| {
            self.read_with_adapter(adapter, |c| {
                traversal::removal_paged(c, &p.selector, &p.page.resolve(policy)?)
            })
        })
    }
    #[tool(
        description = "Tree-sitter structural search over admitted source. $NAME captures one node and $$$ARGS captures a sequence. language must be a compiled profile id."
    )]
    fn ast_grep_search(&self, Parameters(p): Parameters<Ast>) -> CallToolResult {
        self.responding(|adapter, policy| {
            self.read_with_adapter(adapter, |conn| {
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
        self.responding(|adapter, policy| {
            self.read_with_adapter(adapter, |c| {
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
        self.responding(|adapter, policy| {
            let mut page = p.page.resolve(policy)?;
            if p.page.detail.is_none() {
                page.detail = Detail::Full;
            }
            self.read_with_adapter(adapter, |c| {
                reader::get_doc_paged(c, &p.path_or_id, p.section.as_deref(), &page)
            })
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
        self.responding(|_, _| cli::guide::run(&self.root, topic, p.force))
    }
    #[tool(
        description = "List, save, get, or delete a JSON checkpoint in .forge/checkpoints.json."
    )]
    fn session_checkpoint(&self, Parameters(p): Parameters<Checkpoint>) -> CallToolResult {
        match self.adapter_snapshot() {
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
        let adapter = match scanner::load_adapter(&self.root, None) {
            Ok(adapter) => super::server::RequestAdapter::Loaded(std::sync::Arc::new(adapter)),
            Err(error) => super::server::RequestAdapter::Failed {
                kind: error.kind(),
                message: error.to_string(),
            },
        };
        let request_server = self.with_request_adapter(adapter.clone());
        let call =
            rmcp::handler::server::tool::ToolCallContext::new(&request_server, request, context);
        let result = Self::tool_router().call(call).await;
        let policy = match &adapter {
            super::server::RequestAdapter::Loaded(adapter) => adapter.response.clone(),
            super::server::RequestAdapter::Failed { .. } => Default::default(),
        };
        let final_response: Result<rmcp::model::CallToolResponse, rmcp::ErrorData> = match result {
            Ok(rmcp::model::CallToolResponse::Complete(result)) => {
                Ok(response::enforce(result, &policy).into())
            }
            Ok(result) => Ok(result),
            Err(error) => {
                Ok(response::result(Err(anyhow::anyhow!(error.to_string())), &policy).into())
            }
        };
        if matches!(&adapter, super::server::RequestAdapter::Loaded(adapter) if adapter.debug) {
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
