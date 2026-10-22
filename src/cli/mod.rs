/// Implements ast support.
pub mod ast;
pub(crate) mod discovery {
    use crate::{
        core::{
            format::{DiscoveryKind, file_uri_for_path},
            response::QueryOptions,
        },
        db::reader,
        engine::scanner::{self, Adapter},
    };
    use anyhow::Result;
    use rusqlite::Connection;
    use serde_json::{Value, json};
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
        let listing = match crate::engine::milestones::list(root, false, Some("active")) {
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
        let Ok(adapter) = scanner::load_adapter(root, None) else {
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
/// Sends a reload signal to running MCP servers.
pub mod reload;
/// Implements task support.
pub mod task;
use crate::{
    core::format::{self, OperationKind, OutputFormat},
    core::response::{
        CoverageOptions, Detail, MAX_PAGE_SIZE, QueryOptions, ResponsePolicy, SourceOptions,
    },
    db::{reader, symbols, traversal, writer},
};
use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::{Value, json};
use std::io::{self, Write};
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
/// Enumerates the supported command values.
pub enum Command {
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
    Overview,
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
    /// Represents the run case.
    Run {
        /// The operation value.
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
    },
}
#[derive(Subcommand, Debug)]
/// Enumerates the supported docs command values.
pub enum DocsCommand {
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
        Command::Task { command } => {
            let title = match command {
                task::TaskCommand::List { .. } => "task_list",
                task::TaskCommand::Claim { .. } => "task_claim",
                task::TaskCommand::Submit { .. } => "task_submit",
                task::TaskCommand::Blackboard { .. } => "task_blackboard",
                task::TaskCommand::Next { .. }
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
        Command::Milestone { .. } => (OperationKind::Task, "task result", None),
        Command::Guide { .. } => (OperationKind::Guide, "forge_guide", None),
        Command::Checkpoint { .. } => (OperationKind::Checkpoint, "session_checkpoint", None),
        Command::Ast { .. } => (OperationKind::Navigation, "ast_grep_search", None),
        Command::Query {
            command: QueryCommand::Run { operation, .. },
        } => (
            if operation == "sql" {
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
            QueryCommand::Overview => (OperationKind::Navigation, "code_map_overview", None),
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
            QueryCommand::Analyze { .. } | QueryCommand::Run { .. } => unreachable!(),
        },
        Command::Docs { command } => match command {
            DocsCommand::Search { .. } => (OperationKind::Navigation, "search_docs", None),
            DocsCommand::Get { .. } => (OperationKind::Navigation, "get_doc", None),
        },
        Command::Build { .. } => (OperationKind::General, "build result", None),
        Command::Scan { .. } => (OperationKind::General, "scan result", None),
        Command::Delta { .. } => (OperationKind::General, "delta result", None),
        Command::Migrate { .. } => (OperationKind::General, "migrate result", None),
        Command::Reload => (OperationKind::General, "reload result", None),
        Command::Serve => (OperationKind::General, "server", None),
    }
}

fn command_discovery(command: &Command) -> Option<format::DiscoveryKind> {
    match command {
        Command::Ast { .. } => Some(format::DiscoveryKind::Ast),
        Command::Query { command } => match command {
            QueryCommand::Overview => Some(format::DiscoveryKind::Overview),
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
    let value = value.trim_start().to_ascii_lowercase();
    value.starts_with("select ")
        || value.starts_with("select\n")
        || value == "select"
        || value.starts_with("with ")
        || value.starts_with("with\n")
        || value == "with"
}

fn cli_page(limit: usize) -> Result<QueryOptions> {
    QueryOptions::resolve(&ResponsePolicy::default(), Some(limit), 0, None, None)
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

fn open_or_rebuild(db: &Path, root: &Path) -> Result<crate::db::reader::LockedConnection> {
    writer::validate_code_map_target(root, db)?;
    match crate::db::reader::open_locked(db, root) {
        Ok(conn) => Ok(conn),
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
                crate::db::writer::build(root, db, None)?;
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
        let output_format = OutputFormat::from(self.format);
        let root = command_root(self.root.as_deref(), None)?;
        let db = self.db.clone().unwrap_or_else(|| default_db(&root));
        let command = self.command.unwrap_or(Command::Serve);
        let (operation, title, explain) = command_presentation(&command);
        let discovery = command_discovery(&command);
        let checkpoint_action = match &command {
            Command::Checkpoint { action, .. } => Some(action.clone()),
            _ => None,
        };
        let result: Value = match command {
            Command::Migrate { command } => migrate::run(&root, command)?,
            Command::Task { command } => task::run(&root, command)?,
            Command::Milestone { command } => milestone::run(&root, command)?,
            Command::Serve => {
                writer::validate_code_map_target(&root, &db)?;
                crate::mcp::server::serve(root, db).await?;
                return Ok(());
            }
            Command::Reload => reload::run()?,
            Command::Build {
                workspace_root,
                output,
                adapter,
                verbose: _,
            } => {
                let workspace_root = command_root(self.root.as_deref(), workspace_root.as_deref())?;
                let output = output
                    .or(self.db)
                    .unwrap_or_else(|| default_db(&workspace_root));
                writer::validate_code_map_target(&workspace_root, &output)?;
                writer::build(&workspace_root, &output, adapter.as_deref())?
            }
            Command::Scan {
                workspace_root,
                adapter,
                pretty: _,
            } => {
                let workspace_root = command_root(self.root.as_deref(), workspace_root.as_deref())?;
                json!(crate::engine::scanner::scan(
                    &workspace_root,
                    adapter.as_deref()
                )?)
            }
            Command::Delta {
                workspace_root,
                mut modified_files,
                modified,
            } => {
                let workspace_root = command_root(self.root.as_deref(), workspace_root.as_deref())?;
                modified_files.extend(modified);
                if modified_files.is_empty() {
                    bail!("delta requires at least one modified file");
                }
                let db = self.db.unwrap_or_else(|| default_db(&workspace_root));
                writer::delta(&workspace_root, &db, &modified_files)?
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
                let conn = open_or_rebuild(&db, &root)?;
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
                let conn = open_or_rebuild(&db, &root)?;
                match command {
                    QueryCommand::Overview => {
                        let page = cli_page(100)?;
                        let adapter = crate::engine::scanner::load_adapter(&root, None).ok();
                        discovery::overview(&conn, &root, adapter.as_ref(), None, &page)?
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
                    QueryCommand::Run {
                        operation,
                        selector,
                        path,
                        depth,
                        limit,
                    } => traversal::query_with_path_options(
                        &conn,
                        &operation,
                        selector.as_deref(),
                        path.as_deref(),
                        &traversal::GraphQueryOptions {
                            depth,
                            direction: None,
                            coverage: CoverageOptions::default(),
                            page: &cli_page(limit)?,
                        },
                    )?,
                }
            }
            Command::Docs { command } => {
                let conn = open_or_rebuild(&db, &root)?;
                match command {
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
        if output_format == OutputFormat::Markdown {
            if let Ok(adapter) = crate::engine::scanner::load_adapter(&root, None) {
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
            None => {
                format::render_for_operation(&result, output_format, title, discovery, operation)
            }
        };
        if let Ok(adapter) = crate::engine::scanner::load_adapter(&root, None) {
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
        if let Err(e) = writeln!(io::stdout(), "{formatted}") {
            if e.kind() == io::ErrorKind::BrokenPipe {
                return Ok(());
            }
            return Err(e.into());
        }
        Ok(())
    }
}
