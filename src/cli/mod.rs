/// Implements ast support.
pub mod ast;
mod docs_check;
pub(crate) mod discovery {
    use crate::{
        core::{
            format::{file_uri_for_path, DiscoveryKind},
            response::QueryOptions,
        },
        db::reader,
        engine::scanner::{self, Adapter},
    };
    use anyhow::Result;
    use rusqlite::Connection;
    use serde_json::{json, Value};
    use std::path::Path;

    /// Composes indexed, scanner-owned, and canonical task-owned workspace evidence.
    ///
    /// This is an adapter-level composition point: the database reader stays read-only
    /// and never invokes scanners or task lifecycle services.
    pub(crate) fn overview(
        conn: &Connection,
        root: &Path,
        adapter: Option<&Adapter>,
        aspects: Option<&[String]>,
        page: &QueryOptions,
    ) -> Result<Value> {
        let mut result =
            reader::overview_with_options(conn, &reader::OverviewOptions { aspects, page })?;
        if !includes_discovery(aspects) {
            return Ok(result);
        }

        let generation = result["generation"].as_str().unwrap_or_default().to_owned();
        let mut discovery = serde_json::Map::new();
        discovery.insert("workspace_root".into(), json!(root.display().to_string()));

        let manifests = match adapter {
            Some(adapter) => {
                scanner::discovery::manifest_components(root, adapter, page, &generation)
                    .unwrap_or_else(|error| unavailable("manifest_components", &error.to_string()))
            }
            None => unavailable("manifest_components", "workspace adapter is unavailable"),
        };
        discovery.insert("manifest_components".into(), manifests);

        let hubs = if adapter.is_some() {
            reader::documentation_hubs(conn, page)?
        } else {
            unavailable("documentation_hubs", "workspace adapter is unavailable")
        };
        discovery.insert("documentation_hubs".into(), hubs);
        discovery.insert("active_milestones".into(), active_milestones(root, page));

        result["workspace_root"] = json!(root.display().to_string());
        result["discovery"] = Value::Object(discovery);
        Ok(result)
    }

    fn includes_discovery(aspects: Option<&[String]>) -> bool {
        match aspects {
            None => true,
            Some([]) => true,
            Some(aspects) => aspects.iter().any(|aspect| {
                matches!(
                    aspect.to_ascii_lowercase().as_str(),
                    "components"
                        | "discovery"
                        | "manifest_components"
                        | "documentation_hubs"
                        | "milestones"
                )
            }),
        }
    }

    fn unavailable(field: &str, cause: &str) -> Value {
        json!({"availability":"unavailable","cause":cause,"items":[],"field":field})
    }

    fn active_milestones(root: &Path, page: &QueryOptions) -> Value {
        let listing = match crate::engine::milestones::list(root, false, Some("active"), 10) {
            Ok(listing) => listing,
            Err(_) => {
                return json!({
                    "availability":"unavailable",
                    "cause":"canonical active milestone inventory could not be read",
                    "items":[]
                });
            }
        };
        let all = listing["milestones"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let total = all.len();
        let start = page.offset.min(total);
        let end = start.saturating_add(page.limit).min(total);
        let items = all[start..end].to_vec();
        json!({
            "availability":"available",
            "total":total,
            "offset":start,
            "limit":page.limit,
            "items":items,
            "omitted":total.saturating_sub(end),
            "continuation_available":false,
            "continuation_cause":"the canonical milestone listing has no continuation token"
        })
    }

    fn attach_manifest_uris(root: &Path, adapter: &Adapter, page: &mut Value) {
        let Some(items) = page.get_mut("items").and_then(Value::as_array_mut) else {
            return;
        };
        for item in items {
            let workspace = item["workspace"].as_str().unwrap_or("primary").to_owned();
            if let Some(relative) = item["root"].as_str() {
                if let Some(absolute) = resolve_component_path(root, adapter, &workspace, relative)
                {
                    if let Some(uri) = file_uri_for_path(&absolute, None) {
                        item["root_uri"] = json!(uri);
                    }
                }
            }
            if let Some(relative) = item["manifest_path"].as_str() {
                if let Some(absolute) = resolve_component_path(root, adapter, &workspace, relative)
                {
                    if let Some(uri) = file_uri_for_path(&absolute, None) {
                        item["manifest_uri"] = json!(uri);
                    }
                }
            }
        }
    }

    fn resolve_component_path(
        root: &Path,
        adapter: &Adapter,
        workspace: &str,
        relative: &str,
    ) -> Option<std::path::PathBuf> {
        if relative.is_empty() || relative == "." {
            if workspace == "primary" {
                return scanner::canonical_root(root).ok();
            }
            return adapter
                .linked_workspaces
                .iter()
                .find(|linked| linked.name == workspace)
                .and_then(|linked| scanner::canonical_root(&linked.path).ok());
        }
        let selector = if workspace == "primary" {
            relative.to_owned()
        } else {
            format!("[{workspace}]/{relative}")
        };
        scanner::resolve_file_path(root, adapter, &selector).ok()
    }

    fn attach_document_uris(root: &Path, adapter: &Adapter, page: &mut Value) {
        let Some(items) = page.get_mut("items").and_then(Value::as_array_mut) else {
            return;
        };
        for item in items {
            let Some(path) = item.get("path").and_then(Value::as_str) else {
                continue;
            };
            let Ok(absolute) = scanner::resolve_file_path(root, adapter, path) else {
                continue;
            };
            let line = item
                .get("line")
                .and_then(Value::as_i64)
                .and_then(|line| usize::try_from(line).ok());
            if let Some(uri) = file_uri_for_path(&absolute, line) {
                item["uri"] = json!(uri);
            }
        }
    }

    /// Adds workspace-resolved URIs to documentation coordinates before Markdown rendering.
    pub(crate) fn attach_document_presentation_uris(
        root: &Path,
        adapter: &Adapter,
        value: &mut Value,
        title: &str,
    ) {
        match title {
            "search_docs" => attach_page_uris(root, adapter, value.pointer_mut("/sections")),
            "get_doc" => {
                for field in ["sections", "referenced_symbols"] {
                    attach_page_uris(root, adapter, value.get_mut(field));
                }
                for field in ["cites_doc", "cited_by_doc"] {
                    attach_page_uris_without_line(root, adapter, value.get_mut(field));
                }
                if let Some(items) = value
                    .pointer_mut("/referenced_symbol_candidates/items")
                    .and_then(Value::as_array_mut)
                {
                    for candidate in items {
                        for entity in ["source", "target"] {
                            if let Some(entity) = candidate.get_mut(entity) {
                                attach_uri_from_fields(root, adapter, entity, "path", "line");
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Adds guarded scope links for task Markdown without opening the code map.
    pub(crate) fn attach_task_scope_uris(root: &Path, value: &mut Value) {
        let root = value
            .get("workspace_root")
            .and_then(Value::as_str)
            .map(Path::new)
            .unwrap_or(root);
        let settings = forge_tasks::tasks::configuration::current(root);
        let config = settings
            .as_ref()
            .and_then(|settings| settings.config.as_deref());
        let Ok(adapter) = scanner::load_adapter(root, config) else {
            return;
        };
        let Some(scope) = value
            .pointer("/spec/scope")
            .and_then(Value::as_array)
            .or_else(|| {
                value
                    .pointer("/authority_snapshot/task_scope")
                    .and_then(Value::as_array)
            })
        else {
            return;
        };
        let mut uris = serde_json::Map::new();
        for path in scope.iter().filter_map(Value::as_str) {
            if let Ok(absolute) = scanner::resolve_file_path(root, &adapter, path) {
                if absolute.exists() {
                    if let Ok(absolute) = absolute.canonicalize() {
                        if let Some(uri) = file_uri_for_path(&absolute, None) {
                            uris.insert(path.to_owned(), json!(uri));
                        }
                    }
                }
            }
        }
        if !uris.is_empty() {
            value["_presentation_scope_uris"] = json!(uris);
        }
    }

    fn attach_page_uris(root: &Path, adapter: &Adapter, page: Option<&mut Value>) {
        let Some(items) = page
            .and_then(|page| page.get_mut("items"))
            .and_then(Value::as_array_mut)
        else {
            return;
        };
        for item in items {
            attach_uri_from_fields(root, adapter, item, "path", "line");
        }
    }

    fn attach_page_uris_without_line(root: &Path, adapter: &Adapter, page: Option<&mut Value>) {
        let Some(items) = page
            .and_then(|page| page.get_mut("items"))
            .and_then(Value::as_array_mut)
        else {
            return;
        };
        for item in items {
            let Some(path) = item.get("path").and_then(Value::as_str).map(str::to_owned) else {
                continue;
            };
            let Ok(absolute) = scanner::resolve_file_path(root, adapter, &path) else {
                continue;
            };
            if let Some(uri) = file_uri_for_path(&absolute, None) {
                item["uri"] = json!(uri);
            }
        }
    }

    /// Adds absolute coordinates for Markdown presentation after canonical path resolution.
    pub(crate) fn attach_presentation_uris(
        root: &Path,
        adapter: &Adapter,
        value: &mut Value,
        kind: DiscoveryKind,
    ) {
        match kind {
            DiscoveryKind::Overview => {
                if !value.get("workspace_root").is_some_and(Value::is_string) {
                    value["workspace_root"] = json!(root.display().to_string());
                }
                if let Some(manifests) = value.pointer_mut("/discovery/manifest_components") {
                    attach_manifest_uris(root, adapter, manifests);
                }
                if let Some(hubs) = value.pointer_mut("/discovery/documentation_hubs") {
                    attach_document_uris(root, adapter, hubs);
                }
                if let Some(items) = value
                    .pointer_mut("/discovery/active_milestones/items")
                    .and_then(Value::as_array_mut)
                {
                    for item in items {
                        if let Some(path) =
                            item.get("path").and_then(Value::as_str).map(str::to_owned)
                        {
                            attach_uri(root, adapter, item, &path, "line");
                        }
                    }
                }
            }
            DiscoveryKind::Inspect => attach_inspect_uris(root, adapter, value),
            DiscoveryKind::Search => attach_search_uris(root, adapter, value),
            DiscoveryKind::Ast => attach_ast_uris(root, adapter, value),
        }
    }

    /// Adds guarded file links for supplied traversal coordinates in Markdown output.
    pub(crate) fn attach_traversal_presentation_uris(
        root: &Path,
        adapter: &Adapter,
        value: &mut Value,
    ) {
        if let Some(selector) = value.get_mut("selector") {
            attach_uri_from_fields(root, adapter, selector, "path", "line");
        }
        if let Some(node) = value.get_mut("node") {
            attach_uri_from_fields(root, adapter, node, "path", "line");
        }

        for field in ["nodes", "changed_symbols", "tests", "unmapped_changes"] {
            if let Some(page) = value.get_mut(field) {
                attach_traversal_page(root, adapter, page, "line");
            }
        }
        for field in ["incoming", "outgoing"] {
            if let Some(page) = value.get_mut(field) {
                if let Some(items) = page.get_mut("items").and_then(Value::as_array_mut) {
                    for item in items {
                        attach_uri_from_fields(root, adapter, item, "path", "line");
                    }
                }
            }
        }
        if let Some(page) = value.get_mut("affected_docs") {
            if let Some(items) = page.get_mut("items").and_then(Value::as_array_mut) {
                for item in items {
                    attach_uri_from_fields(root, adapter, item, "path", "__no_line");
                }
            }
        }
        if let Some(page) = value.get_mut("documents") {
            if let Some(items) = page.get_mut("items").and_then(Value::as_array_mut) {
                for item in items {
                    attach_uri_from_fields(root, adapter, item, "path", "doc_line");
                }
            }
        }
        for field in [
            "analysis_gaps",
            "untested_public_surface",
            "errors",
            "resolution",
            "external_imports",
            "doc_diagnostics",
            "diagnostics",
            "top_files",
            "resolution_statuses",
            "resolution_causes",
            "parse_error_languages",
        ] {
            if let Some(page) = value.get_mut(field) {
                attach_traversal_page(root, adapter, page, "line");
            }
        }
        if let Some(invariants) = value.get_mut("governing_invariants") {
            if let Some(rules) = invariants.get_mut("direct").and_then(Value::as_array_mut) {
                for rule in rules {
                    if let Some(source) = rule.get_mut("source") {
                        attach_uri_from_fields(root, adapter, source, "path", "line");
                    }
                }
            }
            if let Some(entries) = invariants
                .get_mut("omitted_sources")
                .and_then(Value::as_array_mut)
            {
                for entry in entries {
                    if let Some(source) = entry.get_mut("source") {
                        attach_uri_from_fields(root, adapter, source, "path", "line");
                    }
                }
            }
        }
        if let Some(changes) = value.get_mut("changes") {
            if let Some(files) = changes.get_mut("files").and_then(Value::as_array_mut) {
                for file in files {
                    attach_uri_from_fields(root, adapter, file, "path", "__no_line");
                }
            }
        }
        if let Some(summary) = value.get_mut("summary") {
            if let Some(calls) = summary.get_mut("calls") {
                for direction in ["inbound", "outbound"] {
                    if let Some(preview) = calls
                        .get_mut(direction)
                        .and_then(|direction| direction.get_mut("preview"))
                        .and_then(Value::as_array_mut)
                    {
                        for item in preview {
                            attach_uri_from_fields(root, adapter, item, "path", "line");
                        }
                    }
                }
            }
        }
    }

    fn attach_traversal_page(root: &Path, adapter: &Adapter, page: &mut Value, line: &str) {
        if let Some(items) = page.get_mut("items").and_then(Value::as_array_mut) {
            for item in items {
                attach_uri_from_fields(root, adapter, item, "path", line);
                if let Some(edge) = item.get_mut("edge") {
                    attach_uri_from_fields(root, adapter, edge, "path", "line");
                }
                if let Some(path_nodes) = item
                    .pointer_mut("/connection/verified_call_path_nodes")
                    .and_then(Value::as_array_mut)
                {
                    for node in path_nodes {
                        attach_uri_from_fields(root, adapter, node, "path", "line");
                    }
                }
            }
        }
    }

    fn attach_inspect_uris(root: &Path, adapter: &Adapter, value: &mut Value) {
        attach_uri_from_fields(root, adapter, &mut value["node"], "path", "line");
        if let Some(summary) = value.get_mut("summary") {
            attach_uri_from_fields(root, adapter, &mut summary["container"], "path", "line");
            for direction in ["inbound", "outbound"] {
                if let Some(items) = summary["calls"][direction]["preview"].as_array_mut() {
                    for item in items {
                        attach_uri_from_fields(root, adapter, item, "path", "line");
                    }
                }
            }
        }
        for rule in value["governing_invariants"]["direct"]
            .as_array_mut()
            .into_iter()
            .flatten()
        {
            attach_uri_from_fields(root, adapter, &mut rule["source"], "path", "line");
        }
        for source in value["governing_invariants"]["omitted_sources"]
            .as_array_mut()
            .into_iter()
            .flatten()
        {
            attach_uri_from_fields(root, adapter, &mut source["source"], "path", "line");
        }
        for (field, line) in [
            ("documented_in", "doc_line"),
            ("analysis_gaps", "line"),
            ("documents", "doc_line"),
        ] {
            if let Some(items) = value[field]["items"].as_array_mut() {
                for item in items {
                    attach_uri_from_fields(root, adapter, item, "path", line);
                }
            }
        }
    }

    fn attach_search_uris(root: &Path, adapter: &Adapter, value: &mut Value) {
        if let Some(items) = value["nodes"]["items"].as_array_mut() {
            for item in items {
                attach_uri_from_fields(root, adapter, item, "path", "line");
                attach_search_pulse_uris(root, adapter, item);
            }
        }
        if let Some(groups) = value["nodes"]["grouped_by_file"].as_object_mut() {
            for (path, items) in groups {
                if let Some(items) = items.as_array_mut() {
                    for item in items {
                        attach_uri(root, adapter, item, path, "line");
                        attach_search_pulse_uris(root, adapter, item);
                    }
                }
            }
        }
    }

    fn attach_search_pulse_uris(root: &Path, adapter: &Adapter, item: &mut Value) {
        if let Some(callers) = item["graph_pulse"]["callers"]["preview"].as_array_mut() {
            for caller in callers {
                attach_uri_from_fields(root, adapter, caller, "path", "line");
            }
        }
        if let Some(tests) = item["graph_pulse"]["covering_tests"]["items"].as_array_mut() {
            for test in tests {
                attach_uri_from_fields(root, adapter, test, "path", "line");
            }
        }
    }

    fn attach_uri_from_fields(
        root: &Path,
        adapter: &Adapter,
        item: &mut Value,
        path_field: &str,
        line_field: &str,
    ) {
        let Some(path) = item
            .get(path_field)
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return;
        };
        attach_uri(root, adapter, item, &path, line_field);
    }

    fn attach_uri(root: &Path, adapter: &Adapter, item: &mut Value, path: &str, line_field: &str) {
        let Ok(absolute) = scanner::resolve_file_path(root, adapter, path) else {
            return;
        };
        let line = item
            .get(line_field)
            .and_then(|line| {
                line.as_i64()
                    .or_else(|| line.as_u64().and_then(|line| i64::try_from(line).ok()))
            })
            .and_then(|line| usize::try_from(line).ok());
        if let Some(uri) = file_uri_for_path(&absolute, line) {
            item["uri"] = json!(uri);
        }
    }

    pub(crate) fn attach_ast_uris(root: &Path, adapter: &Adapter, result: &mut Value) {
        let Some(matches) = result.get_mut("matches") else {
            return;
        };
        let items = match matches {
            Value::Array(items) => Some(items),
            Value::Object(page) => page.get_mut("items").and_then(Value::as_array_mut),
            _ => None,
        };
        let Some(items) = items else { return };
        for item in items {
            attach_uri_from_fields(root, adapter, item, "path", "line");
        }
    }
}
/// Implements guide support.
pub mod guide;
/// Implements migrate support.
pub mod migrate;
/// Implements milestone commands.
pub mod milestone;
mod paths;
/// Structured documentation plan execution.
pub mod plan;
/// Sends a reload signal to running MCP servers.
pub mod reload;
/// Implements task support.
pub mod task;
/// Fail-fast ACDD profile and milestone manifest validation.
pub mod validate;
/// Standalone workflow discovery and execution.
pub mod workflow;
use crate::{
    core::format::{self, OperationKind, OutputFormat},
    core::response::{
        CoverageOptions, Detail, QueryOptions, ResponsePolicy, SourceOptions, MAX_PAGE_SIZE,
    },
    db::{reader, symbols, traversal, writer},
};
use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::{json, Value};
use std::io;
use std::path::{Path, PathBuf};
#[derive(Parser, Debug)]
#[command(
    name = "contextunity-forge-mcp",
    version,
    about = "Native code graph, documentation engine, MCP server and CLI"
)]
/// Represents cli data.
pub struct Cli {
    #[arg(long, global = true, env = "FORGE_WORKSPACE_ROOT")]
    /// Optional root value.
    pub root: Option<PathBuf>,
    #[arg(long, global = true, env = "FORGE_DB")]
    /// Optional db value.
    pub db: Option<PathBuf>,
    #[arg(long, global = true, env = "FORGE_CONFIG")]
    /// Explicit root-confined Forge configuration file.
    pub config: Option<PathBuf>,
    #[arg(long, global = true, env = "FORGE_PROFILE")]
    /// Explicit root-confined gate profile file.
    pub profile: Option<PathBuf>,
    #[arg(long, global = true, value_enum, default_value = "markdown")]
    /// Output format: markdown (default) or json.
    pub format: CliOutputFormat,
    #[command(subcommand)]
    /// Optional command value.
    pub command: Option<Command>,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
/// Output representation selected for command results.
pub enum CliOutputFormat {
    #[default]
    /// Human-readable Markdown, selected when `--format` is omitted.
    Markdown,
    /// JSON for callers that parse stdout.
    Json,
}

impl From<CliOutputFormat> for OutputFormat {
    fn from(value: CliOutputFormat) -> Self {
        match value {
            CliOutputFormat::Markdown => Self::Markdown,
            CliOutputFormat::Json => Self::Json,
        }
    }
}
#[derive(Subcommand, Debug)]
/// Unified clear operations across milestones, plans, and workflows.
pub enum ClearCommand {
    /// Clear milestone tasks, blackboard messages, gate execution traces, metadata, and Git state refs.
    Milestone {
        /// Milestone identifier or selector.
        id: String,
    },
    /// Clear working snapshots and traces for a plan without moving its manifest.
    Plan {
        /// Plan name or discovered root-relative path.
        selector: String,
        /// Relocate the plan manifest to docs/plans/archive/ instead of clearing in place.
        #[arg(long)]
        archive: bool,
    },
    /// Clear standalone workflow execution run logs in .forge/runs/ and traces.
    Workflow {
        /// Workflow name or discovered root-relative path.
        selector: String,
    },
}

#[derive(Subcommand, Debug)]
/// Enumerates the supported command values.
pub enum Command {
    /// Purge runtime artifacts, snapshots, logs, or state for milestones, plans, or workflows.
    Clear {
        #[command(subcommand)]
        /// The target subject to clear.
        command: ClearCommand,
    },
    /// Discover or execute a structured documentation plan.
    Plan {
        #[command(subcommand)]
        /// The selected plan operation.
        command: plan::PlanCommand,
    },
    /// Discover or execute a standalone root-gate workflow.
    Workflow {
        #[command(subcommand)]
        /// The selected workflow operation.
        command: workflow::WorkflowCommand,
    },
    /// Read text, symbols, outlines, or Git changes through the layered reader.
    Read {
        /// Workspace-relative file, or symbol selector with --mode snippet.
        path: String,
        /// auto, lines, raw, search, delta, snippet, or outline.
        #[arg(long, default_value = "auto")]
        mode: String,
        /// Indexed symbol selector, optionally confined by path.
        #[arg(long)]
        selector: Option<String>,
        /// One-based first line.
        #[arg(long)]
        start_line: Option<usize>,
        /// Maximum number of text lines.
        #[arg(long)]
        line_count: Option<usize>,
        /// Search pattern.
        #[arg(long)]
        pattern: Option<String>,
        /// Interpret pattern as a regular expression.
        #[arg(long)]
        regex: bool,
        /// Search context lines on each side.
        #[arg(long)]
        context: Option<usize>,
        /// Git baseline revision.
        #[arg(long)]
        baseline: Option<String>,
        /// Workspace-relative export destination.
        #[arg(long)]
        output_file: Option<String>,
        /// Symbol source page offset.
        #[arg(long, default_value_t = 0)]
        source_offset: usize,
        /// Previous source page generation.
        #[arg(long)]
        generation: Option<String>,
        /// Smaller symbol body limit.
        #[arg(long)]
        max_body_lines: Option<usize>,
        /// Symbol leading context lines.
        #[arg(long)]
        leading_lines: Option<usize>,
    },
    /// Validates one ACDD profile or milestone manifest.
    Validate {
        /// Optional workspace-relative file; without it the active profile and open milestones are scanned.
        path: Option<PathBuf>,
    },
    /// Represents the migrate case.
    Migrate {
        #[command(subcommand)]
        /// The command value.
        command: migrate::MigrateCommand,
    },
    /// Represents the task case.
    Task {
        #[command(subcommand)]
        /// The command value.
        command: task::TaskCommand,
    },
    /// Runs milestone lifecycle commands.
    Milestone {
        #[command(subcommand)]
        /// The selected milestone operation.
        command: milestone::MilestoneCommand,
    },
    /// Represents the serve case.
    Serve,
    /// Reloads running MCP servers owned by this user.
    Reload,
    /// Represents the build case.
    Build {
        /// Optional workspace root value.
        workspace_root: Option<PathBuf>,
        #[arg(long)]
        /// Optional output value.
        output: Option<PathBuf>,
        #[arg(long)]
        /// Optional adapter value.
        adapter: Option<PathBuf>,
        #[arg(long)]
        /// Whether verbose applies.
        verbose: bool,
    },
    /// Represents the scan case.
    Scan {
        /// Optional workspace root value.
        workspace_root: Option<PathBuf>,
        #[arg(long)]
        /// Optional adapter value.
        adapter: Option<PathBuf>,
        #[arg(long)]
        /// Whether pretty applies.
        pretty: bool,
    },
    /// Represents the delta case.
    Delta {
        /// Optional workspace root value.
        workspace_root: Option<PathBuf>,
        /// The modified files value.
        modified_files: Vec<PathBuf>,
        #[arg(long = "modified", value_name = "FILE")]
        /// The modified value.
        modified: Vec<PathBuf>,
    },
    /// Represents the query case.
    Query {
        #[command(subcommand)]
        /// The command value.
        command: QueryCommand,
    },
    /// Represents the docs case.
    Docs {
        #[command(subcommand)]
        /// The command value.
        command: DocsCommand,
    },
    /// Represents the ast case.
    Ast {
        #[command(subcommand)]
        /// The command value.
        command: AstCommand,
    },
    /// Represents the guide case.
    Guide {
        /// The topic value.
        topic: String,
        #[arg(long)]
        /// Whether force applies.
        force: bool,
    },
    /// Represents the checkpoint case.
    Checkpoint {
        /// The action value.
        action: String,
        #[arg(long)]
        /// Optional name value.
        name: Option<String>,
        #[arg(long)]
        /// Optional content value.
        content: Option<String>,
    },
}
#[derive(Subcommand, Debug)]
/// Enumerates the supported query command values.
pub enum QueryCommand {
    /// Represents the overview case.
    Overview {
        /// Comma-separated overview aspects.
        #[arg(long, value_delimiter = ',')]
        aspects: Option<Vec<String>>,
        /// Maximum items per aspect.
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// Number of items to skip.
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Generation from the previous page.
        #[arg(long)]
        generation: Option<String>,
    },
    /// Represents the inspect case.
    Inspect {
        /// The selector value.
        selector: String,
        /// Restrict selector resolution to a workspace-relative file or directory.
        #[arg(long)]
        path: Option<String>,
        #[arg(long,default_value_t=true,action=clap::ArgAction::Set,num_args=0..=1,default_missing_value="true")]
        /// Whether show doc applies.
        show_doc: bool,
        #[arg(long)]
        /// Whether show source applies.
        show_source: bool,
        #[arg(long, default_value_t = 0)]
        /// Source body offset returned by the prior preview.
        source_offset: usize,
        #[arg(long)]
        /// Generation returned by the prior source preview.
        generation: Option<String>,
    },
    /// Represents the search case.
    Search {
        /// The pattern value.
        pattern: String,
        #[arg(long)]
        /// Optional kind value.
        kind: Option<String>,
        #[arg(long, default_value_t = 100)]
        /// The limit value.
        limit: usize,
    },
    /// Represents the tests case.
    Tests {
        /// The selector value.
        selector: String,
        /// Restrict selector resolution to a workspace-relative file or directory.
        #[arg(long)]
        path: Option<String>,
        #[arg(long, default_value = "inbound")]
        /// The direction value.
        direction: String,
        #[arg(long, default_value_t = 100)]
        /// The limit value.
        limit: usize,
    },
    /// Represents the impact case.
    Impact {
        /// Optional exact symbol selector; provide this or --changes.
        selector: Option<String>,
        #[arg(long)]
        /// Git change view: staged, unstaged, or head.
        changes: Option<String>,
        /// Restrict selector resolution to a workspace-relative file or directory.
        #[arg(long)]
        path: Option<String>,
        #[arg(long, default_value_t = 1)]
        /// Maximum impact depth; defaults to one edge.
        depth: u32,
        #[arg(long, default_value_t = 100)]
        /// Maximum result page size (1..=100).
        limit: usize,
        #[arg(long, default_value_t = 0)]
        /// Result page offset.
        offset: usize,
        #[arg(long)]
        /// Continuation generation from the preceding result page.
        generation: Option<String>,
    },
    /// Represents the explain case.
    Explain {
        /// The selector value.
        selector: String,
        /// Restrict selector resolution to a workspace-relative file or directory.
        #[arg(long)]
        path: Option<String>,
    },
    /// Represents the remove case.
    Remove {
        /// The selector value.
        selector: String,
        /// Restrict selector resolution to a workspace-relative file or directory.
        #[arg(long)]
        path: Option<String>,
    },
    /// Represents the analyze case.
    Analyze {
        #[arg(default_value = "")]
        /// The target value.
        target: String,
    },
    /// Execute a read-only SELECT/WITH SQL statement against the code graph.
    Sql {
        /// Read-only SELECT or WITH SQL statement.
        sql: String,
        #[arg(long, default_value_t = 100)]
        /// The limit value.
        limit: usize,
    },
    /// Represents the run case.
    Run {
        /// The operation value ('sql', 'untested', 'implementations', 'slice', 'unwired', 'duplicates', or a direct SELECT/WITH statement).
        operation: String,
        /// Optional selector value.
        selector: Option<String>,
        /// Restrict selector resolution to a workspace-relative file or directory.
        #[arg(long)]
        path: Option<String>,
        #[arg(long, default_value_t = 2)]
        /// Maximum graph depth for generic query operations; impact defaults to two.
        depth: u32,
        #[arg(long, default_value_t = 100)]
        /// The limit value.
        limit: usize,
        #[arg(long, default_value_t = 0)]
        /// Result page offset.
        offset: usize,
        #[arg(long)]
        /// Continuation generation from the preceding result page.
        generation: Option<String>,
        #[arg(long)]
        /// Deduplication level: 'symbol' (functions/methods) or 'file' (identical files).
        level: Option<String>,
        #[arg(long, default_value_t = 10)]
        /// Minimum line count for duplicates detection (default 10).
        min_lines: usize,
    },
}
#[derive(Subcommand, Debug)]
/// Enumerates the supported docs command values.
pub enum DocsCommand {
    /// Check Markdown links and anchors in the workspace.
    Check,
    /// Represents the search case.
    Search {
        /// The query value.
        query: String,
        #[arg(long)]
        /// Optional doc type value.
        doc_type: Option<String>,
        #[arg(long)]
        /// Optional component value.
        component: Option<String>,
        #[arg(long, default_value_t = 100)]
        /// The limit value.
        limit: usize,
    },
    /// Represents the get case.
    Get {
        /// The path or id value.
        path_or_id: String,
        #[arg(long)]
        /// Optional section value.
        section: Option<String>,
    },
}
#[derive(Subcommand, Debug)]
/// Enumerates the supported ast command values.
pub enum AstCommand {
    /// Represents the grep case.
    Grep {
        /// The pattern value.
        pattern: String,
        #[arg(long = "lang", alias = "language")]
        /// The language value.
        language: String,
        #[arg(long)]
        /// Optional path value.
        path: Option<String>,
        #[arg(long, default_value_t = 100)]
        /// The limit value.
        limit: usize,
    },
}
/// Performs default db.
pub fn default_db(root: &std::path::Path) -> PathBuf {
    root.join(".forge/code-map.sqlite")
}

fn command_presentation(command: &Command) -> (OperationKind, &'static str, Option<bool>) {
    match command {
        Command::Read { .. } => (OperationKind::Navigation, "forge_read", None),
        Command::Task { command } => {
            let title = match command {
                task::TaskCommand::Run { .. } => "task_manage",
                task::TaskCommand::List { .. } => "task_list",
                task::TaskCommand::Claim { .. } => "task_claim",
                task::TaskCommand::Submit { .. } => "task_submit",
                task::TaskCommand::Blackboard { .. } => "task_blackboard",
                task::TaskCommand::Next { .. }
                | task::TaskCommand::Verify { .. }
                | task::TaskCommand::Subtask { .. }
                | task::TaskCommand::Reset { .. }
                | task::TaskCommand::Reopen { .. }
                | task::TaskCommand::Context { .. }
                | task::TaskCommand::Inspect { .. }
                | task::TaskCommand::Create { .. }
                | task::TaskCommand::Sync { .. }
                | task::TaskCommand::ExtendScope { .. }
                | task::TaskCommand::Delete { .. } => "task_manage",
                task::TaskCommand::Cleanup => "task result",
            };
            (OperationKind::Task, title, None)
        }
        Command::Milestone {
            command: milestone::MilestoneCommand::List { .. },
        } => (OperationKind::Task, "milestone_list", None),
        Command::Milestone { .. } => (OperationKind::Task, "task result", None),
        Command::Workflow { .. } => (OperationKind::Task, "workflow result", None),
        Command::Plan { .. } => (OperationKind::Task, "plan result", None),
        Command::Clear { .. } => (OperationKind::Task, "clear result", None),
        Command::Guide { .. } => (OperationKind::Guide, "forge_guide", None),
        Command::Checkpoint { .. } => (OperationKind::Checkpoint, "session_checkpoint", None),
        Command::Ast { .. } => (OperationKind::Navigation, "ast_grep_search", None),
        Command::Query {
            command: QueryCommand::Sql { .. },
        } => (OperationKind::Sql, "code_map_query", None),
        Command::Query {
            command: QueryCommand::Run { operation, .. },
        } => (
            if operation == "sql" || looks_like_sql(operation) {
                OperationKind::Sql
            } else {
                OperationKind::Navigation
            },
            "code_map_query",
            None,
        ),
        Command::Query {
            command: QueryCommand::Analyze { target },
        } => (
            if looks_like_sql(target) {
                OperationKind::Sql
            } else {
                OperationKind::Navigation
            },
            "code_map_analyze",
            None,
        ),
        Command::Query { command } => match command {
            QueryCommand::Overview { .. } => (OperationKind::Navigation, "code_map_overview", None),
            QueryCommand::Inspect { .. } => {
                (OperationKind::Navigation, "code_map_inspect", Some(false))
            }
            QueryCommand::Search { .. } => (OperationKind::Navigation, "code_map_search", None),
            QueryCommand::Tests { .. } => (OperationKind::Navigation, "code_map_tests", None),
            QueryCommand::Impact { .. } => (OperationKind::Navigation, "code_map_impact", None),
            QueryCommand::Explain { .. } => {
                (OperationKind::Navigation, "code_map_explain", Some(true))
            }
            QueryCommand::Remove { .. } => {
                (OperationKind::Navigation, "code_map_prove_removal", None)
            }
            QueryCommand::Analyze { .. } | QueryCommand::Run { .. } | QueryCommand::Sql { .. } => {
                unreachable!()
            }
        },
        Command::Docs { command } => match command {
            DocsCommand::Check => (OperationKind::General, "docs check", None),
            DocsCommand::Search { .. } => (OperationKind::Navigation, "search_docs", None),
            DocsCommand::Get { .. } => (OperationKind::Navigation, "get_doc", None),
        },
        Command::Build { .. } => (OperationKind::General, "build result", None),
        Command::Scan { .. } => (OperationKind::General, "scan result", None),
        Command::Delta { .. } => (OperationKind::General, "delta result", None),
        Command::Migrate { .. } => (OperationKind::General, "migrate result", None),
        Command::Validate { .. } => (OperationKind::Task, "validate result", None),
        Command::Reload => (OperationKind::General, "reload result", None),
        Command::Serve => (OperationKind::General, "server", None),
    }
}

fn command_discovery(command: &Command) -> Option<format::DiscoveryKind> {
    match command {
        Command::Ast { .. } => Some(format::DiscoveryKind::Ast),
        Command::Query { command } => match command {
            QueryCommand::Overview { .. } => Some(format::DiscoveryKind::Overview),
            QueryCommand::Inspect { .. } => Some(format::DiscoveryKind::Inspect),
            QueryCommand::Run { operation, .. } if operation.trim() == "inspect" => {
                Some(format::DiscoveryKind::Inspect)
            }
            QueryCommand::Search { .. } => Some(format::DiscoveryKind::Search),
            _ => None,
        },
        _ => None,
    }
}

fn looks_like_sql(value: &str) -> bool {
    value.split_whitespace().next().is_some_and(|keyword| {
        keyword.eq_ignore_ascii_case("select") || keyword.eq_ignore_ascii_case("with")
    })
}

fn cli_page(limit: usize) -> Result<QueryOptions> {
    QueryOptions::resolve(&ResponsePolicy::default(), Some(limit), 0, None, None)
}

fn cli_page_with_offset(
    limit: usize,
    offset: usize,
    generation: Option<String>,
) -> Result<QueryOptions> {
    QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(limit),
        offset,
        None,
        generation,
    )
}

fn cli_source(enabled: bool) -> Result<SourceOptions> {
    SourceOptions::resolve(&ResponsePolicy::default(), Some(enabled), None, None, 0)
}
fn command_root(global: Option<&Path>, positional: Option<&Path>) -> Result<PathBuf> {
    let global = global.map(Path::canonicalize).transpose()?;
    let positional = positional.map(Path::canonicalize).transpose()?;
    if let (Some(global), Some(positional)) = (&global, &positional) {
        if global != positional {
            bail!(
                "conflicting workspace roots: --root {} and positional {}",
                global.display(),
                positional.display()
            );
        }
    }
    Ok(positional
        .or(global)
        .unwrap_or(std::env::current_dir()?.canonicalize()?))
}

fn open_or_rebuild(
    db: &Path,
    root: &Path,
    cache_dir: Option<&Path>,
) -> Result<crate::db::reader::LockedConnection> {
    let selected = forge_tasks::tasks::configuration::current(root);
    let adapter = selected
        .as_ref()
        .and_then(|settings| settings.config.as_deref());
    writer::validate_code_map_target(root, db)?;
    match crate::db::reader::open_locked(db, root) {
        Ok(conn) => {
            if let Some(path) = adapter {
                let selected = crate::engine::scanner::load_adapter(root, Some(path))?;
                let persisted = crate::core::config::read_policy(root, &conn)?;
                if selected.digest != persisted.digest
                    || selected.adapter_path != persisted.adapter_path
                    || selected.linked_workspaces != persisted.linked_workspaces
                {
                    drop(conn);
                    match cache_dir {
                        Some(cache_dir) => crate::engine::pipeline::build_with_cache_dir(
                            root, db, adapter, cache_dir,
                        )?,
                        None => writer::build(root, db, adapter)?,
                    };
                    return crate::db::reader::open_locked(db, root);
                }
            }
            Ok(conn)
        }
        Err(err) => {
            let msg = err.to_string();
            let requires_rebuild = err
                .downcast_ref::<crate::db::reader::IndexRebuildRequired>()
                .is_some();
            if requires_rebuild || !db.exists() {
                eprintln!(
                    "Forge index incompatible or missing ({msg}); auto-rebuilding for {}...",
                    root.display()
                );
                match cache_dir {
                    Some(cache_dir) => {
                        crate::engine::pipeline::build_with_cache_dir(root, db, adapter, cache_dir)?
                    }
                    None => crate::db::writer::build(root, db, adapter)?,
                };
                crate::db::reader::open_locked(db, root)
            } else {
                Err(err)
            }
        }
    }
}
impl Cli {
    /// Performs run.
    pub async fn run(self) -> Result<()> {
        self.run_with_writer(&mut io::stdout()).await
    }

    /// Runs CLI dispatch and formatting with a caller-provided output writer.
    pub async fn run_with_writer<W: io::Write>(self, output: &mut W) -> Result<()> {
        self.run_inner(output, None).await
    }

    /// Runs CLI dispatch with explicit output and extraction-cache locations.
    pub async fn run_with_writer_and_cache_dir<W: io::Write>(
        self,
        output: &mut W,
        cache_dir: &Path,
    ) -> Result<()> {
        self.run_inner(output, Some(cache_dir)).await
    }

    async fn run_inner<W: io::Write>(self, output: &mut W, cache_dir: Option<&Path>) -> Result<()> {
        let positional_root = match self.command.as_ref() {
            Some(
                Command::Build { workspace_root, .. }
                | Command::Scan { workspace_root, .. }
                | Command::Delta { workspace_root, .. },
            ) => workspace_root.as_deref(),
            _ => None,
        };
        let root = match command_root(self.root.as_deref(), positional_root) {
            Ok(root) => root,
            Err(error) => {
                if matches!(
                    self.command.as_ref(),
                    Some(Command::Docs {
                        command: DocsCommand::Check
                    })
                ) {
                    return docs_check::check(
                        self.root.as_deref().unwrap_or_else(|| Path::new(".")),
                        OutputFormat::from(self.format),
                        output,
                    );
                }
                return Err(error);
            }
        };
        // Initialization must work even when the adapter it repairs cannot be parsed.
        if let Some(Command::Guide { topic, force }) = self.command.as_ref() {
            if topic.trim() == "init" {
                let result = guide::run(&root, topic, *force)?;
                let formatted = format::render_for_operation(
                    &result,
                    OutputFormat::from(self.format),
                    "forge_guide",
                    None,
                    OperationKind::Guide,
                );
                writeln!(output, "{formatted}")?;
                return Ok(());
            }
        }
        let settings =
            paths::ResolvedPaths::resolve(&root, self.config.as_deref(), self.profile.as_deref())
                .map_err(|error| match self.command.as_ref() {
                Some(Command::Task { command }) => task::preflight_error(command, error),
                _ => error,
            })?;
        let database = self
            .db
            .as_ref()
            .map(|path| {
                if path.is_absolute() {
                    path.clone()
                } else {
                    root.join(path)
                }
            })
            .unwrap_or_else(|| default_db(&root));
        let profile_argument = self.profile.as_ref().map(|path| {
            if path.is_absolute() {
                path.clone()
            } else {
                root.join(path)
            }
        });
        let selection = forge_tasks::tasks::configuration::InvocationSettings::new(
            root.clone(),
            settings.config.clone(),
            settings.profile.clone(),
            profile_argument,
            database.clone(),
        );
        selection
            .scope(self.run_selected(output, cache_dir, root, settings, database))
            .await
    }

    async fn run_selected<W: io::Write>(
        self,
        output: &mut W,
        cache_dir: Option<&Path>,
        root: PathBuf,
        settings: paths::ResolvedPaths,
        db: PathBuf,
    ) -> Result<()> {
        let output_format = OutputFormat::from(self.format);
        let command = self.command.unwrap_or(Command::Serve);
        if matches!(
            &command,
            Command::Docs {
                command: DocsCommand::Check
            }
        ) {
            return docs_check::check(&root, output_format, output);
        }
        let validation_command = matches!(&command, Command::Validate { .. });
        let (operation, title, explain) = command_presentation(&command);
        let discovery = command_discovery(&command);
        let checkpoint_action = match &command {
            Command::Checkpoint { action, .. } => Some(action.clone()),
            _ => None,
        };
        let result: Value = match command {
            Command::Read {
                path,
                mode,
                selector,
                start_line,
                line_count,
                pattern,
                regex,
                context,
                baseline,
                output_file,
                source_offset,
                generation,
                max_body_lines,
                leading_lines,
            } => {
                let adapter =
                    crate::engine::scanner::load_adapter(&root, settings.config.as_deref())?;
                let mode = serde_json::from_value::<forge_core::model::ReadMode>(json!(mode))?;
                let request = crate::engine::reader::ReadRequest {
                    path: Some(path),
                    paths: None,
                    mode,
                    selector,
                    start_line,
                    line_count,
                    pattern,
                    regex,
                    context,
                    baseline,
                    output_file,
                };
                anyhow::ensure!(
                    source_offset == 0 || generation.is_some(),
                    "source continuation requires generation"
                );
                let source = SourceOptions::resolve_full_symbol(
                    &adapter.response,
                    leading_lines,
                    max_body_lines,
                    source_offset,
                )?;
                let mut page = QueryOptions::resolve(&adapter.response, None, 0, None, generation)?;
                page.detail = Detail::Full;
                let structural = request.requires_index(&adapter)?;
                if structural {
                    let conn = open_or_rebuild(&db, &root, cache_dir)?;
                    crate::engine::reader::forge_read_tool(
                        &root,
                        &adapter,
                        &request,
                        &page,
                        &source,
                        Some(&conn),
                    )?
                } else if db.is_file()
                    && (adapter.modules.is_enabled("codemap") || adapter.modules.is_enabled("docs"))
                {
                    reader::open_locked(&db, &root)
                        .and_then(|conn| {
                            crate::engine::reader::forge_read_tool(
                                &root,
                                &adapter,
                                &request,
                                &page,
                                &source,
                                Some(&conn),
                            )
                        })
                        .or_else(|_| {
                            crate::engine::reader::forge_read_tool(
                                &root, &adapter, &request, &page, &source, None,
                            )
                        })?
                } else {
                    crate::engine::reader::forge_read_tool(
                        &root, &adapter, &request, &page, &source, None,
                    )?
                }
            }
            Command::Validate { path } => validate::run(&root, path.as_deref())?,
            Command::Migrate { command } => migrate::run(&root, command)?,
            Command::Task { command } => task::run(&root, command)?,
            Command::Milestone { command } => milestone::run(&root, command)?,
            Command::Workflow { command } => workflow::run(&root, command)?,
            Command::Plan { command } => plan::run(&root, command, &settings)?,
            Command::Clear { command } => match command {
                ClearCommand::Milestone { id } => {
                    milestone::run(&root, milestone::MilestoneCommand::Clear { id })?
                }
                ClearCommand::Plan { selector, archive } => {
                    if archive {
                        plan::run(&root, plan::PlanCommand::Archive { selector }, &settings)?
                    } else {
                        plan::run(&root, plan::PlanCommand::Clear { selector }, &settings)?
                    }
                }
                ClearCommand::Workflow { selector } => {
                    workflow::run(&root, workflow::WorkflowCommand::Clear { selector })?
                }
            },
            Command::Serve => {
                writer::validate_code_map_target(&root, &db)?;
                let invocation = forge_tasks::tasks::configuration::current(&root)
                    .map(|settings| (*settings).clone());
                crate::mcp::server::serve_with_settings(
                    root,
                    db,
                    cache_dir.map(Path::to_path_buf),
                    invocation,
                )
                .await?;
                return Ok(());
            }
            Command::Reload => reload::run()?,
            Command::Build {
                workspace_root: _,
                output,
                adapter,
                verbose: _,
            } => {
                let workspace_root = root.clone();
                let adapter = match adapter {
                    Some(adapter) => {
                        let adapter = paths::confined_file(&root, &adapter)?;
                        if self.config.is_some() {
                            anyhow::ensure!(settings.config.as_ref() == Some(&adapter), "CLI_CONFIG_CONFLICT: --adapter and --config select different files");
                        }
                        Some(adapter)
                    }
                    None => settings.config.clone(),
                };
                let output = output
                    .map(|path| {
                        if path.is_absolute() {
                            path
                        } else {
                            root.join(path)
                        }
                    })
                    .unwrap_or_else(|| db.clone());
                writer::validate_code_map_target(&workspace_root, &output)?;
                match cache_dir {
                    Some(cache_dir) => crate::engine::pipeline::build_with_cache_dir(
                        &workspace_root,
                        &output,
                        adapter.as_deref(),
                        cache_dir,
                    )?,
                    None => writer::build(&workspace_root, &output, adapter.as_deref())?,
                }
            }
            Command::Scan {
                workspace_root: _,
                adapter,
                pretty: _,
            } => {
                let workspace_root = root.clone();
                let adapter = match adapter {
                    Some(adapter) => {
                        let adapter = paths::confined_file(&root, &adapter)?;
                        if self.config.is_some() {
                            anyhow::ensure!(settings.config.as_ref() == Some(&adapter), "CLI_CONFIG_CONFLICT: --adapter and --config select different files");
                        }
                        Some(adapter)
                    }
                    None => settings.config.clone(),
                };
                json!(crate::engine::scanner::scan(
                    &workspace_root,
                    adapter.as_deref()
                )?)
            }
            Command::Delta {
                workspace_root: _,
                mut modified_files,
                modified,
            } => {
                let workspace_root = root.clone();
                modified_files.extend(modified);
                if modified_files.is_empty() {
                    bail!("delta requires at least one modified file");
                }
                crate::engine::pipeline::delta_with_selection(
                    &workspace_root,
                    &db,
                    &modified_files,
                    self.config.as_ref().and(settings.config.as_deref()),
                    cache_dir,
                )?
            }
            Command::Guide { topic, force } => guide::run(&root, &topic, force)?,
            Command::Checkpoint {
                action,
                name,
                content,
            } => guide::checkpoint(
                &root,
                &action,
                name.as_deref(),
                content.map(|s| serde_json::from_str(&s)).transpose()?,
            )?,
            Command::Ast {
                command:
                    AstCommand::Grep {
                        pattern,
                        language,
                        path,
                        limit,
                    },
            } => {
                let conn = open_or_rebuild(&db, &root, cache_dir)?;
                // MCP accepts one AST page; share that producer throughout its limit range.
                let mut result = if (1..=MAX_PAGE_SIZE).contains(&limit) {
                    ast::search_paged(
                        &conn,
                        &root,
                        &pattern,
                        &language,
                        path.as_deref(),
                        &cli_page(limit)?,
                    )?
                } else {
                    // Preserve the CLI's larger aggregate limit and its existing validation.
                    ast::search(&conn, &root, &pattern, &language, path.as_deref(), limit)?
                };
                result["pattern"] = json!(pattern);
                result["language"] = json!(language);
                if output_format == OutputFormat::Markdown {
                    result["request_scope"] = json!({
                        "workspace_root": root.display().to_string(),
                        "pattern": pattern,
                        "language": language,
                        "path": path,
                    });
                }
                result
            }
            Command::Query { command } => {
                let conn = open_or_rebuild(&db, &root, cache_dir)?;
                match command {
                    QueryCommand::Overview {
                        aspects,
                        limit,
                        offset,
                        generation,
                    } => {
                        let page = QueryOptions::resolve(
                            &ResponsePolicy::default(),
                            Some(limit),
                            offset,
                            None,
                            generation,
                        )?;
                        let adapter =
                            crate::engine::scanner::load_adapter(&root, settings.config.as_deref())
                                .ok();
                        discovery::overview(
                            &conn,
                            &root,
                            adapter.as_ref(),
                            aspects.as_deref(),
                            &page,
                        )?
                    }
                    QueryCommand::Inspect {
                        selector,
                        path,
                        show_doc,
                        show_source,
                        source_offset,
                        generation,
                    } => {
                        anyhow::ensure!(source_offset == 0 || generation.is_some(), "source continuation requires restarting at source_offset=0 or providing the previous page's generation");
                        let source = SourceOptions::resolve(
                            &ResponsePolicy::default(),
                            Some(show_source),
                            None,
                            None,
                            source_offset,
                        )?;
                        let page = QueryOptions::resolve(
                            &ResponsePolicy::default(),
                            Some(100),
                            0,
                            None,
                            generation,
                        )?;
                        let mut result = symbols::inspect_with_path_options(
                            &conn,
                            &root,
                            &selector,
                            path.as_deref(),
                            &symbols::InspectOptions {
                                show_doc,
                                source: &source,
                                coverage: CoverageOptions {
                                    include_coverage: true,
                                },
                                page: &page,
                            },
                        )?;
                        if output_format == OutputFormat::Markdown {
                            result["request_scope"] = json!({
                                "workspace_root": root.display().to_string(),
                                "selector": selector,
                                "path": path,
                            });
                        }
                        result
                    }
                    QueryCommand::Search {
                        pattern,
                        kind,
                        limit,
                    } => {
                        let page = cli_page(limit)?;
                        let exact = false;
                        let include_docs = false;
                        let result = symbols::search_with_options(
                            &conn,
                            &pattern,
                            &symbols::SearchOptions {
                                kind: kind.as_deref(),
                                path: None,
                                include_docs,
                                exact,
                                page: &page,
                            },
                        )?;
                        let mut result =
                            symbols::search_results_with_navigation(&conn, &root, result, false)?;
                        if output_format == OutputFormat::Markdown {
                            result["request_scope"] = json!({
                                "workspace_root": root.display().to_string(),
                                "pattern": pattern,
                                "kind": kind,
                                "exact": exact,
                                "include_docs": include_docs,
                            });
                        }
                        result
                    }
                    QueryCommand::Tests {
                        selector,
                        path,
                        direction,
                        limit,
                    } => symbols::tests_paged_with_path(
                        &conn,
                        &selector,
                        path.as_deref(),
                        &direction,
                        &cli_page(limit)?,
                    )?,
                    QueryCommand::Impact {
                        selector,
                        changes,
                        path,
                        depth,
                        limit,
                        offset,
                        generation,
                    } => traversal::impact_with_path_options(
                        &conn,
                        &root,
                        selector.as_deref(),
                        changes.as_deref(),
                        path.as_deref(),
                        &traversal::TraversalOptions {
                            depth,
                            inbound: true,
                            mode: None,
                            edge_types: None,
                            page: &QueryOptions::resolve(
                                &ResponsePolicy::default(),
                                Some(limit),
                                offset,
                                None,
                                generation,
                            )?,
                        },
                    )?,
                    QueryCommand::Explain { selector, path } => symbols::explain_with_path_options(
                        &conn,
                        &root,
                        &selector,
                        path.as_deref(),
                        &symbols::ExplainOptions {
                            direction: None,
                            show_doc: true,
                            source: &cli_source(false)?,
                            coverage: CoverageOptions {
                                include_coverage: true,
                            },
                            page: &cli_page(100)?,
                        },
                    )?,
                    QueryCommand::Remove { selector, path } => traversal::removal_paged_with_path(
                        &conn,
                        &selector,
                        path.as_deref(),
                        &cli_page(100)?,
                    )?,
                    QueryCommand::Analyze { target } => {
                        reader::analyze_paged(&conn, &target, None, &cli_page(100)?)?
                    }
                    QueryCommand::Sql { sql, limit } => traversal::query_with_path_options(
                        &conn,
                        "sql",
                        Some(sql.as_str()),
                        None,
                        &traversal::GraphQueryOptions {
                            depth: 2,
                            direction: None,
                            coverage: CoverageOptions::default(),
                            page: &cli_page(limit)?,
                        },
                    )?,
                    QueryCommand::Run {
                        operation,
                        selector,
                        path,
                        depth,
                        limit,
                        offset,
                        generation,
                        level,
                        min_lines,
                    } => {
                        let (effective_op, effective_selector) =
                            if selector.is_none() && looks_like_sql(&operation) {
                                ("sql", Some(operation.as_str()))
                            } else {
                                (operation.as_str(), selector.as_deref())
                            };
                        let page = cli_page_with_offset(limit, offset, generation)?;
                        if effective_op.trim() == "duplicates" {
                            traversal::duplicates_paged(
                                &conn,
                                effective_selector,
                                path.as_deref(),
                                level.as_deref(),
                                Some(min_lines),
                                &page,
                            )?
                        } else {
                            traversal::query_with_path_options(
                                &conn,
                                effective_op,
                                effective_selector,
                                path.as_deref(),
                                &traversal::GraphQueryOptions {
                                    depth,
                                    direction: None,
                                    coverage: CoverageOptions::default(),
                                    page: &page,
                                },
                            )?
                        }
                    }
                }
            }
            Command::Docs { command } => {
                let conn = open_or_rebuild(&db, &root, cache_dir)?;
                match command {
                    DocsCommand::Check => unreachable!(),
                    DocsCommand::Search {
                        query,
                        doc_type,
                        component,
                        limit,
                    } => reader::search_docs_with_options(
                        &conn,
                        &query,
                        &reader::DocSearchOptions {
                            doc_type: doc_type.as_deref(),
                            component: component.as_deref(),
                            include_excerpt: false,
                            page: &cli_page(limit)?,
                        },
                    )?,
                    DocsCommand::Get {
                        path_or_id,
                        section,
                    } => {
                        let mut options = cli_page(100)?;
                        options.detail = Detail::Full;
                        reader::get_doc_paged(&conn, &path_or_id, section.as_deref(), &options)?
                    }
                }
            }
        };
        let mut result = result;
        let validation_failed =
            validation_command && result.get("valid").and_then(Value::as_bool) == Some(false);
        if output_format == OutputFormat::Markdown {
            if let Ok(adapter) =
                crate::engine::scanner::load_adapter(&root, settings.config.as_deref())
            {
                if let Some(kind) = discovery {
                    discovery::attach_presentation_uris(&root, &adapter, &mut result, kind);
                }
                if matches!(title, "search_docs" | "get_doc") {
                    discovery::attach_document_presentation_uris(
                        &root,
                        &adapter,
                        &mut result,
                        title,
                    );
                }
                if title == "task_manage" {
                    discovery::attach_task_scope_uris(&root, &mut result);
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
                    discovery::attach_traversal_presentation_uris(&root, &adapter, &mut result);
                }
            }
        }
        format::compact(&mut result, operation, explain);
        let formatted = match checkpoint_action.as_deref() {
            Some(action) => format::render_checkpoint(&result, output_format, title, action),
            None if matches!(title, "task_list" | "forge_read") => {
                let policy =
                    crate::engine::scanner::load_adapter(&root, settings.config.as_deref())
                        .map(|adapter| adapter.response)
                        .unwrap_or_default();
                let bounded = crate::mcp::response::result_for_presented(
                    Ok(result.clone()),
                    &policy,
                    output_format,
                    operation,
                    title,
                    explain,
                    discovery,
                );
                let text = bounded
                    .content
                    .first()
                    .and_then(rmcp::model::ContentBlock::as_text)
                    .map(|block| block.text.as_str())
                    .ok_or_else(|| anyhow::anyhow!("task list response has no text content"))?;
                if bounded.is_error == Some(true) {
                    bail!("{text}");
                }
                text.to_owned()
            }
            None => {
                format::render_for_operation(&result, output_format, title, discovery, operation)
            }
        };
        if let Ok(adapter) = crate::engine::scanner::load_adapter(&root, settings.config.as_deref())
        {
            if adapter.debug {
                let cli_cmd = {
                    let args: Vec<String> = std::env::args().skip(1).collect();
                    if args.is_empty() {
                        "serve".to_string()
                    } else {
                        args.join(" ")
                    }
                };
                crate::core::debug_log::log_cli(&root, &cli_cmd, None, &result);
            }
        }
        if let Err(e) = writeln!(output, "{formatted}") {
            if e.kind() == io::ErrorKind::BrokenPipe {
                return Ok(());
            }
            return Err(e.into());
        }
        if validation_failed {
            bail!("VALIDATION_FAILED: one or more documents failed validation");
        }
        Ok(())
    }
}
