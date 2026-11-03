//! Layered reader shared by the CLI and MCP transports.

use crate::core::response::{QueryOptions, SourceOptions};
use anyhow::{bail, ensure, Context, Result};
use forge_core::{
    config::{resolve_file_path, Adapter},
    model::{ReadBounds, ReadMode, ReadResult},
    subsystems::{disabled_read_mode_error, filter_subsystem_tools},
};
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};

/// Input shared by both reader transports.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ReadRequest {
    /// Workspace-relative file, including configured linked-workspace paths.
    pub path: Option<String>,
    /// Indexed symbol selector. When supplied, auto mode selects snippet.
    pub selector: Option<String>,
    /// auto, lines, raw, search, delta, snippet, or outline.
    #[serde(default)]
    #[schemars(with = "String")]
    pub mode: ReadMode,
    /// One-based first line.
    pub start_line: Option<usize>,
    /// Maximum lines, default 200 for text modes.
    pub line_count: Option<usize>,
    /// Search pattern, interpreted literally unless regex is true.
    pub pattern: Option<String>,
    /// Interpret the search pattern as a regular expression.
    #[serde(default)]
    pub regex: bool,
    /// Search context lines on each side, default 2.
    pub context: Option<usize>,
    /// Git revision for delta; defaults to the active task baseline or HEAD.
    pub baseline: Option<String>,
    /// Export the returned content to a confined workspace-relative file.
    pub output_file: Option<String>,
}

impl ReadRequest {
    /// Check subsystem admission before either transport opens the code index.
    pub fn requires_index(&self, adapter: &Adapter) -> Result<bool> {
        let mode = self.resolved_mode();
        let markdown = self
            .path
            .as_deref()
            .is_some_and(|p| p.ends_with(".md") || p.ends_with(".markdown"));
        if let Some(error) = disabled_read_mode_error(mode, markdown, &adapter.modules) {
            bail!("{error}");
        }
        Ok(matches!(mode, ReadMode::Snippet | ReadMode::Outline))
    }
    /// Resolve automatic mode without requiring an index.
    pub fn resolved_mode(&self) -> ReadMode {
        match self.mode {
            ReadMode::Auto if self.selector.is_some() => ReadMode::Snippet,
            ReadMode::Auto if self.pattern.is_some() => ReadMode::Search,
            ReadMode::Auto => ReadMode::Lines,
            mode => mode,
        }
    }
}

fn sensitive(adapter: &Adapter, path: &str) -> Result<()> {
    let mut builder = ignore::gitignore::GitignoreBuilder::new("");
    for pattern in &adapter.sensitive {
        builder.add_line(None, pattern)?;
    }
    ensure!(
        !builder
            .build()?
            .matched_path_or_any_parents(path, false)
            .is_ignore(),
        "SENSITIVE_FILE: blocked by sensitive policy"
    );
    Ok(())
}

fn admitted_file(root: &Path, adapter: &Adapter, path: &str) -> Result<std::path::PathBuf> {
    sensitive(adapter, path)?;
    let file = resolve_file_path(root, adapter, path)?;
    ensure!(file.is_file(), "reader path must name a regular file");
    // Check canonical-relative names as well as linked-path aliases.
    let relative = file.strip_prefix(root).unwrap_or(&file).to_string_lossy();
    sensitive(adapter, &relative)?;
    for linked in &adapter.linked_workspaces {
        if let Ok(relative) = file.strip_prefix(&linked.path) {
            sensitive(adapter, &relative.to_string_lossy())?;
        }
    }
    let mut prefix = [0; 8192];
    let count = File::open(&file)?.read(&mut prefix)?;
    ensure!(
        !prefix[..count].contains(&0),
        "BINARY_FILE: NUL byte in first 8 KiB"
    );
    Ok(file)
}

fn active_task(root: &Path, adapter: &Adapter) -> Result<Option<forge_tasks::store::Task>> {
    if !adapter.modules.is_enabled("acdd") {
        return Ok(None);
    }
    let Ok(database) = forge_tasks::engine::database_path(root) else {
        return Ok(None);
    };
    if !std::fs::symlink_metadata(&database).is_ok_and(|m| m.file_type().is_file()) {
        return Ok(None);
    }
    let store = forge_tasks::engine::store(root)?;
    let tasks: Vec<_> = store
        .list(None, "in_progress", None)?
        .into_iter()
        .filter(|t| {
            t.worktree
                .as_deref()
                .and_then(|p| Path::new(p).canonicalize().ok())
                .as_deref()
                == Some(root)
        })
        .collect();
    Ok((tasks.len() == 1)
        .then(|| tasks.into_iter().next())
        .flatten())
}

/// Read a file or symbol using already admitted graph state when available.
pub fn forge_read_tool(
    root: &Path,
    adapter: &Adapter,
    request: &ReadRequest,
    page: &QueryOptions,
    source: &SourceOptions,
    conn: Option<&Connection>,
) -> Result<Value> {
    let mode = request.resolved_mode();
    let markdown = request
        .path
        .as_deref()
        .is_some_and(|p| p.ends_with(".md") || p.ends_with(".markdown"));
    request.requires_index(adapter)?;
    let task = active_task(root, adapter)?;
    let mut value = if mode == ReadMode::Snippet {
        let conn = conn.context("snippet requires the code index")?;
        let selector = request
            .selector
            .as_deref()
            .or(request.path.as_deref())
            .context("snippet requires selector")?;
        // Resolve metadata first; enforce policy before the snippet owner reads source.
        let inspection = crate::db::reader::inspect_paged_with_path(
            conn,
            selector,
            request
                .path
                .as_deref()
                .filter(|_| request.selector.is_some()),
            false,
            page,
        )?;
        let selected = &inspection["node"];
        let path = selected["path"]
            .as_str()
            .context("selected symbol has no path")?;
        admitted_file(root, adapter, path)?;
        let mut value = crate::db::symbols::snippet_paged_with_response_budget(
            conn,
            root,
            selector,
            request
                .path
                .as_deref()
                .filter(|_| request.selector.is_some()),
            source,
            page,
            adapter.response.max_output_bytes,
        )?;
        if let (Some(start), Some(end)) = (
            value["source_preview"]["start_line"].as_u64(),
            value["source_preview"]["end_line"].as_u64(),
        ) {
            value["header"] = json!(format!(
                "{} {} in {path}:{start}-{end}",
                value["node"]["kind"].as_str().unwrap_or("symbol"),
                value["node"]["qualname"]
                    .as_str()
                    .or(value["node"]["name"].as_str())
                    .unwrap_or(selector)
            ));
        }
        value
    } else {
        let path = request
            .path
            .as_deref()
            .context("reader mode requires path")?;
        let file = admitted_file(root, adapter, path)?;
        let bounds = ReadBounds {
            start_line: request.start_line,
            line_count: request.line_count,
        };
        let (content, truncated) = match mode {
            ReadMode::Delta => (
                compute_file_delta(root, path, request.baseline.as_deref(), task.as_ref())?,
                false,
            ),
            ReadMode::Outline => {
                ensure!(
                    file.metadata()?.len() <= adapter.limits.max_file_bytes,
                    "outline file exceeds max_file_bytes"
                );
                let text = std::fs::read_to_string(&file)?;
                if markdown {
                    (crate::engine::docs::render_outline(path, &text)?, false)
                } else {
                    let profile = crate::engine::languages::for_path(&file)
                        .context("no compiled language profile for outline")?;
                    let facts = crate::engine::ast::extract(path, profile.id(), &text)?;
                    (
                        facts
                            .nodes
                            .iter()
                            .filter(|n| n.kind != "module")
                            .map(|n| {
                                format!(
                                    "{}:{} {} {}",
                                    n.line,
                                    n.end_line,
                                    n.kind,
                                    n.details["signature"].as_str().unwrap_or(&n.qualname)
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                        false,
                    )
                }
            }
            _ => extract_file_slice(&file, mode, &bounds, request)?,
        };
        serde_json::to_value(ReadResult {
            path: path.into(),
            mode,
            content,
            truncated,
        })?
    };
    let path = value["node"]["path"]
        .as_str()
        .or(request.path.as_deref())
        .unwrap_or("")
        .to_owned();
    if adapter.modules.is_enabled("codemap") {
        if let Some(conn) = conn {
            value["graph_pulse"] = graph_signals(conn, &path, value.get("node"), adapter)?;
        }
    }
    if let Some(task) = &task {
        value["write_scope"] = json!(if task
            .spec
            .scope
            .iter()
            .chain(&task.spec.extended_scope)
            .any(|p| forge_tasks::tasks::path_matches_scope_entry(&path, p))
        {
            "ALLOWED"
        } else {
            "OUT_OF_SCOPE"
        });
    }
    let hints = render_read_hints(
        root,
        conn,
        adapter,
        &path,
        &value,
        task.as_ref(),
        value["write_scope"] == "OUT_OF_SCOPE",
    )?;
    if !hints.is_empty() {
        value["Hints"] = json!(hints);
    }
    if let Some(output) = &request.output_file {
        sensitive(adapter, output)?;
        let destination = forge_core::fs::checked_child(root, Path::new(output))?;
        ensure!(
            destination != resolve_file_path(root, adapter, &path)?,
            "output_file must differ from input"
        );
        let content = value["content"]
            .as_str()
            .or(value["source"].as_str())
            .context("read has no exportable content")?;
        forge_core::fs::atomic_write(&destination, content.as_bytes(), false)?;
        value["output_file"] = json!(output);
    }
    Ok(value)
}

/// Read bounded text windows or matching lines without loading the complete file.
pub fn extract_file_slice(
    file: &Path,
    mode: ReadMode,
    bounds: &ReadBounds,
    request: &ReadRequest,
) -> Result<(String, bool)> {
    let start = bounds.start_line.unwrap_or(1);
    let count = bounds.line_count.unwrap_or(200);
    ensure!(
        start > 0 && count > 0 && count <= 10000,
        "line bounds require start_line >= 1 and line_count 1..=10000"
    );
    let pattern = if mode == ReadMode::Search {
        Some(
            request
                .pattern
                .as_deref()
                .context("search requires pattern")?,
        )
    } else {
        None
    };
    let matcher = pattern
        .filter(|_| request.regex)
        .map(forge_tasks::regex::Regex::new)
        .transpose()?;
    let context = request.context.unwrap_or(2).min(100);
    let mut prior: std::collections::VecDeque<(usize, String)> = std::collections::VecDeque::new();
    let mut after = 0;
    let mut last = 0;
    let mut output = String::new();
    let mut kept = 0;
    let mut truncated = false;
    let mut reader = BufReader::new(File::open(file)?);
    let mut line = String::new();
    let mut number = 0;
    loop {
        line.clear();
        // Bound even a single physical line before allocating it in memory.
        let bytes = reader.by_ref().take(1024 * 1024 + 1).read_line(&mut line)?;
        if bytes == 0 {
            break;
        }
        ensure!(bytes <= 1024 * 1024, "line exceeds reader byte limit");
        number += 1;
        if number < start {
            continue;
        }
        let matched = pattern.is_some_and(|p| {
            matcher
                .as_ref()
                .map_or_else(|| line.contains(p), |r| r.is_match(&line))
        });
        if matched {
            after = context + 1;
        }
        let include = pattern.is_none() || after > 0;
        if include {
            if matched {
                for (n, text) in &prior {
                    if *n > last && kept < count && output.len() + text.len() + 32 <= 48 * 1024 {
                        output.push_str(&format!("{n}: {text}"));
                        kept += 1;
                        last = *n;
                    }
                }
            }
            if kept >= count || output.len() + line.len() + 32 > 48 * 1024 {
                truncated = true;
                break;
            }
            if mode == ReadMode::Raw {
                output.push_str(&line);
            } else {
                output.push_str(&format!("{number}: {line}"));
            }
            kept += 1;
            last = number;
        }
        after = after.saturating_sub(1);
        if pattern.is_some() {
            prior.push_back((number, line.clone()));
            while prior.len() > context {
                prior.pop_front();
            }
        }
    }
    Ok((output, truncated))
}

/// Compute a path-confined unified diff against HEAD or the task candidate baseline.
pub fn compute_file_delta(
    root: &Path,
    path: &str,
    baseline: Option<&str>,
    task: Option<&forge_tasks::store::Task>,
) -> Result<String> {
    let task_baseline = if baseline.is_none() {
        task.map(|task| {
            forge_tasks::engine::store(root).and_then(|store| {
                forge_tasks::store::task_candidate_baseline(&store.connection, &task.task_id)
            })
        })
        .transpose()?
        .flatten()
    } else {
        None
    };
    let revision = baseline.or(task_baseline.as_deref()).unwrap_or("HEAD");
    ensure!(
        !revision.starts_with('-') && !revision.is_empty(),
        "invalid delta baseline"
    );
    let resolved = forge_core::git::git_command(
        root,
        &["rev-parse", "--verify", &format!("{revision}^{{commit}}")],
    )?;
    ensure!(resolved.status.success(), "delta baseline is not a commit");
    let commit = String::from_utf8(resolved.stdout)?;
    let output = forge_core::git::git_command(
        root,
        &[
            "--no-pager",
            "--literal-pathspecs",
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--unified=3",
            commit.trim(),
            "--",
            path,
        ],
    )?;
    ensure!(output.status.success(), "git delta failed");
    ensure!(
        output.stdout.len() <= 5 * 1024 * 1024,
        "delta exceeds reader byte limit"
    );
    Ok(String::from_utf8(output.stdout)?)
}

fn graph_signals(
    conn: &Connection,
    path: &str,
    node: Option<&Value>,
    adapter: &Adapter,
) -> Result<Value> {
    let id = node.and_then(|n| n["id"].as_str()).unwrap_or("");
    let (edges, callers, implementations, invariants): (u64,u64,u64,u64) = conn.query_row(
        "WITH selected(hash) AS (SELECT n.node_hash FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE (?1!='' AND n.id=?1) OR (?1='' AND p.path=?2)) SELECT count(*),count(DISTINCT CASE WHEN e.kind='calls' AND e.dst_hash IN (SELECT hash FROM selected) THEN e.src_hash END),coalesce(sum(e.kind IN ('implements','inherits') AND e.dst_hash IN (SELECT hash FROM selected)),0),coalesce(sum(e.kind='governs' AND e.dst_hash IN (SELECT hash FROM selected)),0) FROM edges e WHERE e.src_hash IN (SELECT hash FROM selected) OR e.dst_hash IN (SELECT hash FROM selected)",
        params![id,path], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    let page = QueryOptions::resolve(&adapter.response, Some(1), 0, None, None)?;
    let tests = crate::db::symbols::tests_paged_with_path(
        conn,
        if id.is_empty() { path } else { id },
        None,
        "inbound",
        &page,
    )
    .ok()
    .filter(|v| v["discovery_method"] == "graph")
    .and_then(|v| v["nodes"]["total"].as_u64())
    .unwrap_or(0);
    Ok(
        json!({"edges":edges,"callers":callers,"covering_tests":tests,"implementations":implementations,"invariants":invariants}),
    )
}

/// Produce at most three lines of copy-ready calls, only for confirmed signals.
pub fn render_read_hints(
    root: &Path,
    conn: Option<&Connection>,
    adapter: &Adapter,
    path: &str,
    value: &Value,
    task: Option<&forge_tasks::store::Task>,
    out_of_scope: bool,
) -> Result<String> {
    let mut calls: Vec<(&str, String)> = Vec::new();
    if let Some(conn) = conn {
        if adapter.modules.is_enabled("codemap") {
            let selector = value["node"]["id"].as_str().unwrap_or(path);
            let signals = &value["graph_pulse"];
            if signals["edges"].as_u64().unwrap_or(0) > 0 {
                calls.push((
                    "code_map_explain",
                    format!("code_map_explain(selector: {})", json!(selector)),
                ));
                calls.push((
                    "code_map_impact",
                    format!("code_map_impact(selector: {})", json!(selector)),
                ));
            }
            if signals["covering_tests"].as_u64().unwrap_or(0) > 0 {
                calls.push((
                    "code_map_tests",
                    format!("code_map_tests(selector: {})", json!(selector)),
                ));
            }
            if value["node"]["id"].is_string()
                && signals["implementations"].as_u64().unwrap_or(0) > 0
            {
                calls.push((
                    "code_map_query",
                    format!(
                        "code_map_query(operation: \"implementations\", selector: {})",
                        json!(selector)
                    ),
                ));
            }
        }
        if adapter.modules.is_enabled("docs") {
            let id = value["node"]["id"].as_str().unwrap_or("");
            let mut stmt = conn.prepare("SELECT DISTINCT d.path,d.section_title FROM doc_sections d LEFT JOIN nodes dn ON dn.id=d.doc_id LEFT JOIN edges e ON e.src_hash=dn.node_hash AND e.kind IN ('documents','governs') LEFT JOIN nodes target ON target.node_hash=e.dst_hash LEFT JOIN path_dictionary p ON p.path_id=target.path_id WHERE d.path=?1 OR (?2!='' AND target.id=?2) OR (?2='' AND p.path=?1) ORDER BY d.path,d.section_title LIMIT 1")?;
            let section: Option<(String, String)> = stmt
                .query_map(params![path, id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .next()
                .transpose()?;
            if let Some((doc_path, section)) = section {
                calls.push((
                    "get_doc",
                    format!(
                        "get_doc(path: {}, section: {})",
                        json!(doc_path),
                        json!(section)
                    ),
                ));
                calls.push((
                    "search_docs",
                    format!(
                        "search_docs(query: {}, path: {})",
                        json!(section),
                        json!(doc_path)
                    ),
                ));
            }
        }
    }
    if let Some(task) = task {
        if out_of_scope {
            calls.push((
                "task_manage",
                format!(
                    "task_manage(action: \"extend_scope\", task_id: {}, paths: [{}])",
                    json!(task.task_id),
                    json!(path)
                ),
            ));
        }
        let profile = forge_tasks::tasks::profile::load_for_task(
            root,
            task.spec.acdd_profile.as_deref(),
            task.profile_pin.as_deref(),
        )?;
        if profile
            .by_stage(&task.stage)
            .and_then(|gate| gate.command.as_ref())
            .is_some_and(|command| profile.commands.contains_key(command))
        {
            calls.push((
                "task_manage",
                format!(
                    "task_manage(action: \"verify\", task_id: {})",
                    json!(task.task_id)
                ),
            ));
        }
        calls.push((
            "task_blackboard",
            format!(
                "task_blackboard(action: \"read\", scope: \"task\", task_id: {})",
                json!(task.task_id)
            ),
        ));
    }
    let mut groups = [Vec::new(), Vec::new(), Vec::new()];
    for (tool, call) in calls
        .into_iter()
        .filter(|(tool, _)| !filter_subsystem_tools([*tool], &adapter.modules).is_empty())
    {
        let group = if tool.starts_with("code_map_") {
            0
        } else if matches!(tool, "get_doc" | "search_docs") {
            1
        } else {
            2
        };
        let call = format!(
            "{}, worktree: {})",
            call.trim_end_matches(')'),
            json!(root.to_string_lossy())
        );
        groups[group].push(call);
    }
    Ok(groups
        .into_iter()
        .filter(|group| !group.is_empty())
        .map(|group| group.join("; "))
        .collect::<Vec<_>>()
        .join("\n"))
}
