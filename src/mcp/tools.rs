use super::{response, server::Server};
use crate::{
    cli,
    core::{
        format::{DiscoveryKind, OperationKind, OutputFormat},
        response::{CoverageOptions, Detail, ResponsePolicy},
    },
    db::{reader, symbols, traversal},
    engine::scanner,
};
use rmcp::{
    ServerHandler, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_handler,
    tool_router,
};
use serde_json::Value;

#[path = "tools/inputs.rs"]
mod inputs;
pub use inputs::{
    Analyze, Ast, Checkpoint, Explain, GetDoc, Guide, Impact, Inspect, OverviewInput, PageInput,
    Query, SearchDocs, SearchSymbols, Selector, Snippet, TaskBlackboardInput, TaskClaimInput,
    TaskListInput, TaskManageInput, TaskSubmitInput, Tests,
};

fn rejected_selector_fields(
    tool_name: &str,
    arguments: Option<&serde_json::Map<String, Value>>,
) -> Vec<String> {
    let Some(arguments) = arguments else {
        return Vec::new();
    };
    let allowed: &[&str] = match tool_name {
        "code_map_inspect" => &[
            "selector",
            "path",
            "show_doc",
            "include_coverage",
            "show_source",
            "leading_lines",
            "max_body_lines",
            "source_offset",
            "limit",
            "offset",
            "detail",
            "generation",
            "worktree",
        ],
        "get_code_snippet" => &[
            "selector",
            "path",
            "show_source",
            "leading_lines",
            "max_body_lines",
            "source_offset",
            "limit",
            "offset",
            "detail",
            "generation",
            "worktree",
        ],
        "code_map_tests" => &[
            "selector",
            "path",
            "direction",
            "limit",
            "offset",
            "detail",
            "generation",
            "worktree",
        ],
        "code_map_impact" => &[
            "selector",
            "changes",
            "path",
            "direction",
            "depth",
            "mode",
            "edge_types",
            "limit",
            "offset",
            "detail",
            "generation",
            "worktree",
        ],
        "code_map_explain" => &[
            "selector",
            "path",
            "direction",
            "show_doc",
            "include_coverage",
            "show_source",
            "leading_lines",
            "max_body_lines",
            "source_offset",
            "limit",
            "offset",
            "detail",
            "generation",
            "worktree",
        ],
        "code_map_query" => &[
            "operation",
            "selector",
            "path",
            "direction",
            "include_coverage",
            "depth",
            "limit",
            "offset",
            "detail",
            "generation",
            "worktree",
        ],
        "code_map_prove_removal" => &[
            "selector",
            "path",
            "limit",
            "offset",
            "detail",
            "generation",
            "worktree",
        ],
        "code_map_search" => &[
            "pattern",
            "query",
            "exact",
            "kind",
            "path",
            "group_by_file",
            "include_docs",
            "include_preview",
            "limit",
            "offset",
            "detail",
            "generation",
            "worktree",
        ],
        "ast_grep_search" => &[
            "pattern",
            "language",
            "path",
            "limit",
            "offset",
            "detail",
            "generation",
            "worktree",
        ],
        _ => return Vec::new(),
    };
    arguments
        .keys()
        .filter(|field| field.as_str() != "format" && !allowed.contains(&field.as_str()))
        .take(16)
        .cloned()
        .collect()
}

impl Server {
    fn responding(
        &self,
        output_format: OutputFormat,
        operation: OperationKind,
        title: &'static str,
        run: impl FnOnce(&scanner::Adapter, &ResponsePolicy) -> anyhow::Result<Value>,
    ) -> CallToolResult {
        self.responding_with_symbol(output_format, operation, title, run, None)
    }

    fn responding_with_symbol(
        &self,
        output_format: OutputFormat,
        operation: OperationKind,
        title: &'static str,
        run: impl FnOnce(&scanner::Adapter, &ResponsePolicy) -> anyhow::Result<Value>,
        explain: Option<bool>,
    ) -> CallToolResult {
        self.responding_with_presentation(output_format, operation, title, run, explain, None)
    }

    fn responding_with_presentation(
        &self,
        output_format: OutputFormat,
        operation: OperationKind,
        title: &'static str,
        run: impl FnOnce(&scanner::Adapter, &ResponsePolicy) -> anyhow::Result<Value>,
        explain: Option<bool>,
        discovery: Option<DiscoveryKind>,
    ) -> CallToolResult {
        match self.adapter_snapshot() {
            Ok(adapter) => {
                let mut value = run(&adapter, &adapter.response);
                if output_format == OutputFormat::Markdown {
                    if let (Some(kind), Ok(value)) = (discovery, &mut value) {
                        cli::discovery::attach_presentation_uris(&self.root, &adapter, value, kind);
                    } else if matches!(title, "search_docs" | "get_doc") {
                        if let Ok(value) = &mut value {
                            cli::discovery::attach_document_presentation_uris(
                                &self.root, &adapter, value, title,
                            );
                        }
                    }
                    if operation == OperationKind::Navigation
                        && discovery.is_none()
                        && matches!(
                            title,
                            "code_map_impact"
                                | "code_map_explain"
                                | "code_map_tests"
                                | "code_map_analyze"
                                | "code_map_query"
                        )
                    {
                        if let Ok(value) = &mut value {
                            cli::discovery::attach_traversal_presentation_uris(
                                &self.root, &adapter, value,
                            );
                        }
                    }
                }
                response::result_for_presented(
                    value,
                    &adapter.response,
                    output_format,
                    operation,
                    title,
                    explain,
                    discovery,
                )
            }
            Err(error) => response::result_for_presented(
                Err(error.into()),
                &ResponsePolicy::default(),
                output_format,
                operation,
                title,
                explain,
                discovery,
            ),
        }
    }
}

#[tool_router]
impl Server {
    #[tool(
        description = "List tasks. The default is ready, unclaimed tasks with satisfied dependencies in the primary workspace. status accepts ready, in_progress, blocked, parked, completed, or all. milestone_status defaults to active and accepts active, planned, completed, or all; a targeted milestone_ref defaults to all statuses. Subtask listings are compact by default; set detail=full to include titles and verification evidence."
    )]
    fn task_list(&self, Parameters(p): Parameters<TaskListInput>) -> CallToolResult {
        let format = p.format.unwrap_or_default();
        self.responding(format, OperationKind::Task, "task_list", |_, _| {
            super::tasks::list(&self.root, p.request)
        })
    }
    #[tool(
        description = "Atomically claim one gate in a worktree. Returns context tailored to the active gate by default; bundle=false returns minimal details."
    )]
    fn task_claim(&self, Parameters(p): Parameters<TaskClaimInput>) -> CallToolResult {
        let format = p.format.unwrap_or_default();
        self.responding(format, OperationKind::Task, "task_claim", |_, _| {
            super::tasks::claim(&self.root, p.request)
        })
    }
    #[tool(
        description = "Pass or reject the claimed gate. Passing deliver writes the milestone receipt and clears the blackboard. The evidence field describes the required object."
    )]
    fn task_submit(&self, Parameters(p): Parameters<TaskSubmitInput>) -> CallToolResult {
        let format = p.format.unwrap_or_default();
        self.responding(format, OperationKind::Task, "task_submit", |_, _| {
            super::tasks::submit(&self.root, p.request)
        })
    }
    #[tool(
        description = "Run one task-store action: next, create, sync, inspect, delete, extend_scope, subtask_add, subtask_update, subtask_list, reset, reopen, or context. next atomically claims eligible work and returns its context bundle; sync without milestone_ref imports the workspace milestone directory."
    )]
    fn task_manage(&self, Parameters(p): Parameters<TaskManageInput>) -> CallToolResult {
        let format = p.format.unwrap_or_default();
        self.responding(format, OperationKind::Task, "task_manage", |_, _| {
            super::tasks::manage(&self.root, p.request)
        })
    }
    #[tool(
        description = "Post, read, or inspect milestone-, task-, and subtask-scoped messages. Omitted scope and keys resolve only from a unique active context. Reads default to 10 payload-free summaries, capped at 50; inspect by message_id returns the payload."
    )]
    fn task_blackboard(&self, Parameters(p): Parameters<TaskBlackboardInput>) -> CallToolResult {
        let format = p.format.unwrap_or_default();
        self.responding(format, OperationKind::Task, "task_blackboard", |_, _| {
            crate::engine::tasks::blackboard(&self.root, p.request, "mcp")
        })
    }
    #[tool(
        description = "Workspace root, counts, languages, and resolution coverage, including paged coverage_by_workspace counts from the indexed primary and linked workspace snapshot. Uses server worktree routing; pass worktree for explicit selection."
    )]
    fn code_map_overview(&self, Parameters(p): Parameters<OverviewInput>) -> CallToolResult {
        let format = p.page.output_format();
        self.responding_with_presentation(
            format,
            OperationKind::Navigation,
            "code_map_overview",
            |adapter, policy| {
                let page = p.page.resolve(policy)?;
                let mut result = self.read_with_adapter(adapter, |c| {
                    cli::discovery::overview(
                        c,
                        &self.root,
                        Some(adapter),
                        p.aspects.as_deref(),
                        &page,
                    )
                })?;
                if format == OutputFormat::Markdown {
                    result["request_scope"] = serde_json::json!({
                        "workspace_root": self.root.display().to_string(),
                        "aspects": p.aspects,
                    });
                }
                Ok(result)
            },
            None,
            Some(DiscoveryKind::Overview),
        )
    }
    #[tool(
        description = "Compact symbol record: signature, docstring, container, call counts, and at most five direct call previews per direction. include_coverage adds paged resolution evidence. Source stays off unless show_source is true."
    )]
    fn code_map_inspect(&self, Parameters(p): Parameters<Inspect>) -> CallToolResult {
        let format = p.page.output_format();
        self.responding_with_presentation(
            format,
            OperationKind::Navigation,
            "code_map_inspect",
            |adapter, policy| {
                let page = p.page.resolve(policy)?;
                let source = p.source(policy, false)?;
                let coverage = CoverageOptions {
                    include_coverage: p.include_coverage,
                };
                let mut result = self.read_with_adapter(adapter, |c| {
                    symbols::inspect_with_path_options(
                        c,
                        &self.root,
                        &p.selector,
                        p.path.as_deref(),
                        &symbols::InspectOptions {
                            show_doc: p.show_doc,
                            source: &source,
                            coverage,
                            page: &page,
                        },
                    )
                })?;
                if format == OutputFormat::Markdown {
                    result["request_scope"] = serde_json::json!({
                        "workspace_root": self.root.display().to_string(),
                        "selector": p.selector,
                        "path": p.path,
                    });
                }
                Ok(result)
            },
            Some(false),
            Some(DiscoveryKind::Inspect),
        )
    }
    #[tool(
        description = "Bounded source preview for one selector. Defaults are 5 leading lines and 200 body lines. Requested body lines clamp to 1–500. The on-disk file must match the indexed digest. Continue with source_offset and generation."
    )]
    fn get_code_snippet(&self, Parameters(p): Parameters<Snippet>) -> CallToolResult {
        let format = p.page.output_format();
        self.responding(
            format,
            OperationKind::Navigation,
            "get_code_snippet",
            |adapter, policy| {
                let page = p.page.resolve(policy)?;
                let source = p.source(policy)?;
                self.read_with_adapter(adapter, |c| {
                    let mut value = symbols::snippet_paged_with_path(
                        c,
                        &self.root,
                        &p.selector,
                        p.path.as_deref(),
                        &source,
                        &page,
                    )?;
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
            },
        )
    }
    #[tool(
        description = "Search names, qualified names, and tokens. A trailing * is a prefix. exact=true matches an indexed name or qualname without full text. Hits include indexed signatures and inspect_selector arguments for code_map_inspect; previews appear automatically for at most three total hits or on request, capped at three hits, 20 lines, and 12 KiB combined."
    )]
    fn code_map_search(&self, Parameters(p): Parameters<SearchSymbols>) -> CallToolResult {
        let format = p.page.output_format();
        self.responding_with_presentation(
            format,
            OperationKind::Navigation,
            "code_map_search",
            |adapter, policy| {
                self.read_with_adapter(adapter, |c| {
                    let page = p.page.resolve(policy)?;
                    let res = symbols::search_with_options(
                        c,
                        &p.pattern,
                        &symbols::SearchOptions {
                            kind: p.kind.as_deref(),
                            path: p.path.as_deref().filter(|path| !path.is_empty()),
                            include_docs: p.include_docs,
                            exact: p.exact,
                            page: &page,
                        },
                    )?;
                    let mut res = symbols::search_results_with_navigation(
                        c,
                        &self.root,
                        res,
                        p.include_preview,
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
                    if format == OutputFormat::Markdown {
                        res["request_scope"] = serde_json::json!({
                            "workspace_root": self.root.display().to_string(),
                            "pattern": p.pattern,
                            "kind": p.kind,
                            "path": p.path,
                            "exact": p.exact,
                            "include_docs": p.include_docs,
                        });
                    }
                    Ok(res)
                })
            },
            None,
            Some(DiscoveryKind::Search),
        )
    }
    #[tool(
        description = "Test relationships for a narrow selector. Inbound returns verified test cases connected by exact calls paths and keeps the bounded lexical fallback when no call path exists; outbound returns production dependencies reached through exact calls. Results include test roles and shortest call paths where graph evidence exists. Possible calls do not establish coverage. Broad scopes are rejected."
    )]
    fn code_map_tests(&self, Parameters(p): Parameters<Tests>) -> CallToolResult {
        let format = p.page.output_format();
        self.responding(
            format,
            OperationKind::Navigation,
            "code_map_tests",
            |adapter, policy| {
                self.read_with_adapter(adapter, |c| {
                    symbols::tests_paged_with_path(
                        c,
                        &p.selector,
                        p.path.as_deref(),
                        &p.direction,
                        &p.page.resolve(policy)?,
                    )
                })
            },
        )
    }
    #[tool(
        description = "Transitive reachability from one selector or a Git change view (staged, unstaged, or head). Provide exactly one of selector or changes. inbound (default) lists dependents; outbound lists dependencies. Each reached depth-one node includes the persisted edge endpoints, kind, path, line, direction, confidence, and occurrence count; full detail adds edge evidence. Change views bind every page to the graph, selected diff, and verified postimages, and report unmapped changes explicitly."
    )]
    fn code_map_impact(&self, Parameters(p): Parameters<Impact>) -> CallToolResult {
        let format = p.page.output_format();
        self.responding(
            format,
            OperationKind::Navigation,
            "code_map_impact",
            |adapter, policy| {
                let page = p.page.resolve(policy)?;
                self.read_with_adapter(adapter, |c| {
                    traversal::impact_with_path_options(
                        c,
                        &self.root,
                        p.selector.as_deref(),
                        p.changes.as_deref(),
                        p.path.as_deref(),
                        &traversal::TraversalOptions {
                            depth: p.depth,
                            inbound: traversal::impact_inbound(Some(p.direction.as_str()))?,
                            mode: p.mode.as_deref(),
                            edge_types: p.edge_types.as_deref(),
                            page: &page,
                        },
                    )
                })
            },
        )
    }
    #[tool(
        description = "Symbol summary plus paged incoming and outgoing edges of every kind. The embedded call summary shows counts and at most five direct call previews per direction. direction defaults to both."
    )]
    fn code_map_explain(&self, Parameters(p): Parameters<Explain>) -> CallToolResult {
        let format = p.page.output_format();
        self.responding_with_symbol(
            format,
            OperationKind::Navigation,
            "code_map_explain",
            |adapter, policy| {
                let page = p.page.resolve(policy)?;
                let source = p.source(policy, false)?;
                let coverage = CoverageOptions {
                    include_coverage: p.include_coverage,
                };
                self.read_with_adapter(adapter, |c| {
                    symbols::explain_with_path_options(
                        c,
                        &self.root,
                        &p.selector,
                        p.path.as_deref(),
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
        let format = p.page.output_format();
        let operation = if p.operation == "sql" {
            OperationKind::Sql
        } else {
            OperationKind::Navigation
        };
        self.responding(format, operation, "code_map_query", |adapter, policy| {
            self.read_with_adapter(adapter, |c| {
                traversal::query_with_path_options(
                    c,
                    &p.operation,
                    p.selector.as_deref(),
                    p.path.as_deref(),
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
        description = "Diagnostics for the workspace or one path, declared-public production symbols not reached by an indexed test case, stored syntax diagnostics, or static cycles. Exact-file resolution and external-import rows include an evidence-based reason when known and a verified source-line snippet; full detail retains resolver evidence. lint=true cannot combine with include_cycles=true."
    )]
    fn code_map_analyze(&self, Parameters(p): Parameters<Analyze>) -> CallToolResult {
        let format = p.page.output_format();
        let operation = if looks_like_sql(&p.target) {
            OperationKind::Sql
        } else {
            OperationKind::Navigation
        };
        self.responding(format, operation, "code_map_analyze", |adapter, policy| {
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
        let format = p.page.output_format();
        self.responding(
            format,
            OperationKind::Navigation,
            "code_map_prove_removal",
            |adapter, policy| {
                self.read_with_adapter(adapter, |c| {
                    traversal::removal_paged_with_path(
                        c,
                        &p.selector,
                        p.path.as_deref(),
                        &p.page.resolve(policy)?,
                    )
                })
            },
        )
    }
    #[tool(
        description = "Tree-sitter structural search over admitted source. Language profiles declare complete_syntax, declaration_without_body, fragment_probe, and attribute capabilities. $NAME captures one syntax node; $$$SEQ captures a syntax-node sequence. language must be a compiled profile id (e.g. 'python', 'rust', 'typescript') or omitted if path has a recognized file extension. Call forge_guide with topic=ast for the profile capability matrix and examples."
    )]
    fn ast_grep_search(&self, Parameters(p): Parameters<Ast>) -> CallToolResult {
        let format = p.page.output_format();
        self.responding_with_presentation(format, OperationKind::Navigation, "ast_grep_search", |adapter, policy| {
            self.read_with_adapter(adapter, |conn| {
                let language = match p.language.as_deref().filter(|l| !l.is_empty()) {
                    Some(lang) => lang.to_owned(),
                    None => {
                        let inferred = p
                            .path
                            .as_deref()
                            .and_then(|path| {
                                crate::engine::languages::for_path(std::path::Path::new(path))
                            })
                            .map(|profile| profile.id());
                        match inferred {
                            Some(lang) => lang.to_owned(),
                            None => {
                                let compiled = crate::engine::languages::profiles()
                                    .map(|p| p.id())
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                let compiled = if compiled.is_empty() {
                                    "none (document-only build)"
                                } else {
                                    &compiled
                                };
                                anyhow::bail!(
                                    "Cannot infer AST language. Specify 'language' or provide a file 'path' with a recognized extension.\nCompiled languages: {compiled}.\nExamples: language='python' with pattern='class Widget', or path='src/example.py' with pattern='class Widget'.\nCall forge_guide(topic='ast') for profile capabilities and examples."
                                );
                            }
                        }
                    }
                };
                let mut result = cli::ast::search_paged(
                    conn,
                    &self.root,
                    &p.pattern,
                    &language,
                    p.path.as_deref(),
                    &p.page.resolve(policy)?,
                )?;
                result["pattern"] = serde_json::json!(p.pattern);
                result["language"] = serde_json::json!(language);
                if format == OutputFormat::Markdown {
                    result["request_scope"] = serde_json::json!({
                        "workspace_root": self.root.display().to_string(),
                        "pattern": p.pattern,
                        "language": language,
                        "path": p.path,
                    });
                }
                Ok(result)
            })
        }, None, Some(DiscoveryKind::Ast))
    }
    #[tool(
        description = "Search indexed Markdown sections with natural-language terms: common English stop words are ignored, any remaining term can match, and section-title matches receive three times the BM25 weight of body text. Returns paths and section anchors for get_doc. include_excerpt adds a short match-centered preview."
    )]
    fn search_docs(&self, Parameters(p): Parameters<SearchDocs>) -> CallToolResult {
        let format = p.page.output_format();
        self.responding(
            format,
            OperationKind::Navigation,
            "search_docs",
            |adapter, policy| {
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
            },
        )
    }
    #[tool(
        description = "Read one indexed Markdown document. The default detail is full text. detail=compact returns a heading outline. A missing path or section is an error."
    )]
    fn get_doc(&self, Parameters(p): Parameters<GetDoc>) -> CallToolResult {
        let format = p.page.output_format();
        self.responding(
            format,
            OperationKind::Navigation,
            "get_doc",
            |adapter, policy| {
                let mut page = p.page.resolve(policy)?;
                if p.page.detail.is_none() {
                    page.detail = Detail::Full;
                }
                self.read_with_adapter(adapter, |c| {
                    reader::get_doc_paged(c, &p.path_or_id, p.section.as_deref(), &page)
                })
            },
        )
    }
    #[tool(description = "Built-in guide. The default topic is query.")]
    fn forge_guide(&self, Parameters(p): Parameters<Guide>) -> CallToolResult {
        let format = p.format.unwrap_or_default();
        let topic = p.topic.as_deref().unwrap_or("query");
        let topic = if topic.trim().is_empty() {
            "query"
        } else {
            topic.trim()
        };
        if topic == "init" {
            return response::result_for(
                cli::guide::run(&self.root, topic, p.force),
                &ResponsePolicy::default(),
                format,
                OperationKind::Guide,
                "forge_guide",
                None,
            );
        }
        self.responding(format, OperationKind::Guide, "forge_guide", |_, _| {
            cli::guide::run(&self.root, topic, p.force)
        })
    }
    #[tool(
        description = "List, save, get, or delete a JSON checkpoint in .forge/checkpoints.json."
    )]
    fn session_checkpoint(&self, Parameters(p): Parameters<Checkpoint>) -> CallToolResult {
        let format = p.format.unwrap_or_default();
        match self.adapter_snapshot() {
            Ok(adapter) => response::checkpoint_result_for(
                cli::guide::checkpoint(&self.root, &p.action, p.name.as_deref(), p.content),
                &adapter.response,
                format,
                "session_checkpoint",
                &p.action,
            ),
            Err(error) => response::result_for(
                Err(error.into()),
                &ResponsePolicy::default(),
                format,
                OperationKind::Checkpoint,
                "session_checkpoint",
                None,
            ),
        }
    }
}

impl Server {
    fn invalid_format_policy(
        &self,
        request: &rmcp::model::CallToolRequestParams,
    ) -> ResponsePolicy {
        let primary_adapter = scanner::load_adapter(&self.root, None);
        let fallback_policy = primary_adapter
            .as_ref()
            .map(|adapter| adapter.response.clone())
            .unwrap_or_default();
        if is_task_tool(&request.name) {
            return fallback_policy;
        }

        let requested_worktree = request
            .arguments
            .as_ref()
            .and_then(|arguments| arguments.get("worktree"))
            .and_then(Value::as_str);
        let Ok((target_root, _)) =
            self.resolve_target_root_and_db(requested_worktree, &primary_adapter)
        else {
            return fallback_policy;
        };
        if target_root == self.root {
            return fallback_policy;
        }

        scanner::load_adapter(&target_root, None)
            .map(|adapter| adapter.response)
            .unwrap_or(fallback_policy)
    }
}

fn is_task_tool(name: &str) -> bool {
    matches!(
        name,
        "task_list"
            | "task_claim"
            | "task_submit"
            | "task_manage"
            | "task_blackboard"
            | "forge_guide"
            | "session_checkpoint"
    )
}

fn operation_kind(name: &str, arguments: Option<&serde_json::Map<String, Value>>) -> OperationKind {
    match name {
        "task_list" | "task_claim" | "task_submit" | "task_manage" | "task_blackboard" => {
            OperationKind::Task
        }
        "forge_guide" => OperationKind::Guide,
        "session_checkpoint" => OperationKind::Checkpoint,
        "code_map_query"
            if arguments
                .and_then(|arguments| arguments.get("operation"))
                .and_then(Value::as_str)
                == Some("sql") =>
        {
            OperationKind::Sql
        }
        "code_map_analyze"
            if arguments
                .and_then(|arguments| arguments.get("target"))
                .and_then(Value::as_str)
                .is_some_and(looks_like_sql) =>
        {
            OperationKind::Sql
        }
        _ => OperationKind::Navigation,
    }
}

fn looks_like_sql(value: &str) -> bool {
    let value = value.trim_start().to_ascii_lowercase();
    value.starts_with("select ")
        || value.starts_with("select\n")
        || value == "select"
        || value.starts_with("with ")
        || value.starts_with("with\n")
        || value == "with"
}

#[tool_handler(
    name = "contextunity-forge-mcp",
    version = "0.3.0",
    instructions = "Workspace code graph and task coordination, 20 tools. Responses cap at 64 KiB. SQLite budget is 2 seconds. Start with code_map_overview and check coverage before an absence claim. Continue a page with offset and generation. Prefer detail=compact. Selector rules and gate proofs: forge_guide topic=query or acdd."
)]
impl ServerHandler for Server {
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        let format_value = request
            .arguments
            .as_ref()
            .and_then(|arguments| arguments.get("format"));
        let output_format = match format_value {
            None => OutputFormat::default(),
            Some(value) => match value
                .as_str()
                .and_then(|requested| OutputFormat::parse(requested).ok())
            {
                Some(format) => format,
                None => {
                    let policy = self.invalid_format_policy(&request);
                    return Ok(response::invalid_format(value, &policy).into());
                }
            },
        };
        let tool_name = request.name.clone();
        let operation = operation_kind(&tool_name, request.arguments.as_ref());
        let rejected = rejected_selector_fields(&request.name, request.arguments.as_ref());
        if !rejected.is_empty() {
            return Ok(response::rejected_arguments(&rejected).into());
        }
        let primary_adapter = scanner::load_adapter(&self.root, None);
        let requested_worktree = request
            .arguments
            .as_ref()
            .and_then(|args| args.get("worktree"))
            .and_then(|v| v.as_str());
        let (target_root, target_db) = if is_task_tool(&request.name) {
            (self.root.clone(), self.db.clone())
        } else {
            match self.resolve_target_root_and_db(requested_worktree, &primary_adapter) {
                Ok(pair) => pair,
                Err(error) => {
                    return Ok(response::result_for(
                        Err(error),
                        &ResponsePolicy::default(),
                        output_format,
                        operation,
                        &tool_name,
                        None,
                    )
                    .into());
                }
            }
        };
        let adapter_result = if target_root == self.root {
            primary_adapter
        } else {
            scanner::load_adapter(&target_root, None)
        };
        let adapter = match adapter_result {
            Ok(adapter) => super::server::RequestAdapter::Loaded(std::sync::Arc::new(adapter)),
            Err(error) => super::server::RequestAdapter::Failed {
                kind: error.kind(),
                message: error.to_string(),
            },
        };
        let debug_request =
            matches!(&adapter, super::server::RequestAdapter::Loaded(adapter) if adapter.debug)
                .then(|| {
                    (
                        request.name.clone(),
                        serde_json::to_string_pretty(&request.arguments).unwrap_or_default(),
                    )
                });
        let request_server = self.with_root_and_db(target_root, target_db, adapter.clone());
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
            Err(error) => Ok(response::result_for(
                Err(anyhow::anyhow!(error.to_string())),
                &policy,
                output_format,
                operation,
                &tool_name,
                None,
            )
            .into()),
        };
        if let Some((tool_name, tool_args)) = debug_request {
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
