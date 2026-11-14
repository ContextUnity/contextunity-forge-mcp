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
    /// Optional batch file paths when reading multiple files.
    #[serde(default)]
    pub paths: Option<Vec<String>>,
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

/// Strips workspace root prefix from absolute paths, resolving to workspace-relative path.
pub fn strip_root_path(path: &str, root: &Path) -> Option<String> {
    let p = Path::new(path);
    if !p.is_absolute()
        || p.components()
            .any(|part| part == std::path::Component::ParentDir)
    {
        return None;
    }
    if p.ancestors().any(|component| {
        !forge_core::fs::is_system_root_symlink(component)
            && std::fs::symlink_metadata(component)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
    }) {
        return None;
    }
    if let Ok(rel) = p.strip_prefix(root) {
        return Some(rel.to_string_lossy().into_owned());
    }
    if let (Ok(can_p), Ok(can_root)) = (p.canonicalize(), root.canonicalize()) {
        if let Ok(rel) = can_p.strip_prefix(&can_root) {
            return Some(rel.to_string_lossy().into_owned());
        }
    }
    let root_str = root.to_string_lossy();
    let root_trimmed = root_str.trim_end_matches('/');
    if let Some(rest) = path.strip_prefix(root_trimmed) {
        if rest.is_empty() {
            return Some(String::new());
        }
        if rest.starts_with([std::path::MAIN_SEPARATOR, '/']) {
            return Some(rest[1..].to_owned());
        }
    }
    None
}

impl ReadRequest {
    /// Check subsystem admission before either transport opens the code index.
    pub fn requires_index(&self, adapter: &Adapter) -> Result<bool> {
        let mode = self.resolved_mode();
        let is_md_path = forge_core::is_doc_path;
        let mut target_paths = Vec::new();
        if let Some(p) = self.path.as_deref() {
            target_paths.push(p);
        }
        if let Some(paths) = &self.paths {
            for p in paths {
                target_paths.push(p.as_str());
            }
        }
        if target_paths.is_empty() {
            if let Some(error) = disabled_read_mode_error(mode, false, &adapter.modules) {
                bail!("{error}");
            }
        } else {
            for p in &target_paths {
                let markdown = is_md_path(p);
                if let Some(error) = disabled_read_mode_error(mode, markdown, &adapter.modules) {
                    bail!("{error}");
                }
            }
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

fn sensitive_with_case(adapter: &Adapter, path: &str, case_insensitive: bool) -> Result<()> {
    let mut builder = ignore::gitignore::GitignoreBuilder::new("");
    builder.case_insensitive(case_insensitive)?;
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

fn sensitive(adapter: &Adapter, root: &Path, path: &str) -> Result<()> {
    sensitive_with_case(
        adapter,
        path,
        forge_core::fs::is_case_insensitive_volume(root),
    )
}

fn admitted_file(root: &Path, adapter: &Adapter, path: &str) -> Result<std::path::PathBuf> {
    let stripped = strip_root_path(path, root);
    let path = stripped.as_deref().unwrap_or(path);
    sensitive(adapter, root, path)?;
    let file = resolve_file_path(root, adapter, path)?;
    ensure!(
        file.exists(),
        "FILE_NOT_FOUND: file '{path}' does not exist"
    );
    ensure!(file.is_file(), "reader path must name a regular file");
    let file_str = file.to_string_lossy();
    let file_ci = forge_core::fs::is_case_insensitive_volume(&file);
    let relative = strip_root_path(&file_str, root).unwrap_or_else(|| {
        file.strip_prefix(root)
            .unwrap_or(&file)
            .to_string_lossy()
            .into_owned()
    });
    sensitive_with_case(adapter, &relative, file_ci)?;
    for linked in &adapter.linked_workspaces {
        if let Some(relative) = strip_root_path(&file_str, &linked.path).or_else(|| {
            file.strip_prefix(&linked.path)
                .ok()
                .map(|rel| rel.to_string_lossy().into_owned())
        }) {
            let linked_ci = forge_core::fs::is_case_insensitive_volume(&linked.path) || file_ci;
            sensitive_with_case(adapter, &relative, linked_ci)?;
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
    if request.resolved_mode() == ReadMode::Lines {
        line_coordinates(request.start_line.unwrap_or(1), request.line_count)?;
    }
    let mut normalized_req;
    let request = if let Some(stripped) = request
        .path
        .as_deref()
        .and_then(|p| strip_root_path(p, root))
    {
        normalized_req = request.clone();
        normalized_req.path = Some(stripped);
        &normalized_req
    } else {
        request
    };
    if let Some(paths) = &request.paths {
        if !paths.is_empty() {
            let normalized_paths: Vec<String> = paths
                .iter()
                .map(|p| strip_root_path(p, root).unwrap_or_else(|| p.clone()))
                .collect();
            let paths = &normalized_paths;
            if let Some(output) = &request.output_file {
                sensitive(adapter, root, output)?;
                let destination = forge_core::fs::checked_child(root, Path::new(output))?;
                for p in paths {
                    if let Ok(input_res) = resolve_file_path(root, adapter, p) {
                        ensure!(
                            !forge_core::fs::same_filesystem_path(&destination, &input_res),
                            "output_file must differ from any input file in paths"
                        );
                    }
                }
            }
            let mut results = Vec::new();
            let mut markdown_sections = Vec::new();
            for p in paths {
                let mut single_req = request.clone();
                single_req.path = Some(p.clone());
                single_req.paths = None;
                single_req.output_file = None;
                match forge_read_single(root, adapter, &single_req, page, source, conn) {
                    Ok(mut val) => {
                        let content = exportable_content(&val).unwrap_or("");
                        markdown_sections.push(format!("### {p}\n\n{content}"));
                        if let Some(obj) = val.as_object_mut() {
                            obj.remove("content");
                        }
                        results.push(val);
                    }
                    Err(e) => {
                        markdown_sections.push(format!("### {p}\n\n*Error reading file: {e}*"));
                        results.push(json!({
                            "path": p,
                            "error": e.to_string(),
                        }));
                    }
                }
            }
            let aggregate_content = markdown_sections.join("\n\n---\n\n");
            let mut response = json!({
                "mode": "batch",
                "paths": paths,
                "batch": results,
                "content": aggregate_content,
            });
            if let Some(output) = &request.output_file {
                let destination = forge_core::fs::checked_child(root, Path::new(output))?;
                forge_core::fs::atomic_write(&destination, aggregate_content.as_bytes(), false)?;
                response["output_file"] = json!(output);
            }
            return Ok(response);
        }
    }
    forge_read_single(root, adapter, request, page, source, conn)
}

fn line_coordinates(start: usize, count: Option<usize>) -> Result<(i64, Option<i64>)> {
    let first = i64::try_from(start)
        .context("invalid line bounds: start_line exceeds SQLite coordinates")?;
    let last = count
        .map(|count| {
            count
                .checked_sub(1)
                .and_then(|offset| start.checked_add(offset))
                .and_then(|end| i64::try_from(end).ok())
                .context("invalid line bounds: range exceeds SQLite coordinates")
        })
        .transpose()?;
    Ok((first, last))
}

fn exportable_content(value: &Value) -> Option<&str> {
    value["source"].as_str().or(value["content"].as_str())
}

fn find_enclosing_symbol(
    conn: &Connection,
    path: &str,
    start_line: i64,
    end_line: Option<i64>,
) -> Result<Option<(String, String)>> {
    let clean_path = path.strip_prefix("./").unwrap_or(path);
    let check_end = end_line.unwrap_or(start_line);
    let mut stmt = conn.prepare(
        "SELECT n.kind, CASE WHEN (SELECT count(*) FROM nodes duplicate
             WHERE duplicate.path_id = n.path_id AND duplicate.name = n.name) > 1
             THEN CASE WHEN (SELECT count(*) FROM nodes qualified
                 WHERE qualified.path_id = n.path_id AND qualified.qualname = n.qualname) = 1
                 THEN n.qualname ELSE n.id END ELSE n.name END
         FROM path_dictionary p
         JOIN nodes n ON n.path_id = p.path_id
         WHERE (p.path = ?1 OR (substr(p.path, -length(?1) - 1) = '/' || ?1
                AND NOT EXISTS (SELECT 1 FROM path_dictionary exact WHERE exact.path = ?1)))
           AND n.line <= ?2
           AND n.end_line >= ?3
           AND n.kind IN ('function', 'method', 'struct', 'class', 'trait', 'interface', 'enum')
         ORDER BY (p.path = ?1) DESC, (n.end_line - n.line) ASC, n.id
         LIMIT 1",
    )?;
    let mut rows = stmt.query(params![clean_path, start_line, check_end])?;
    if let Some(row) = rows.next()? {
        let kind: String = row.get(0)?;
        let name: String = row.get(1)?;
        return Ok(Some((kind, name)));
    }
    let mut stmt = conn.prepare(
        "SELECT n.kind, CASE WHEN (SELECT count(*) FROM nodes duplicate
             WHERE duplicate.path_id = n.path_id AND duplicate.name = n.name) > 1
             THEN CASE WHEN (SELECT count(*) FROM nodes qualified
                 WHERE qualified.path_id = n.path_id AND qualified.qualname = n.qualname) = 1
                 THEN n.qualname ELSE n.id END ELSE n.name END
         FROM path_dictionary p
         JOIN nodes n ON n.path_id = p.path_id
         WHERE (p.path = ?1 OR (substr(p.path, -length(?1) - 1) = '/' || ?1
                AND NOT EXISTS (SELECT 1 FROM path_dictionary exact WHERE exact.path = ?1)))
           AND n.line <= ?2
           AND n.end_line >= ?2
           AND n.kind IN ('function', 'method', 'struct', 'class', 'trait', 'interface', 'enum')
         ORDER BY (p.path = ?1) DESC, (n.end_line - n.line) ASC, n.id
         LIMIT 1",
    )?;
    let mut rows = stmt.query(params![clean_path, start_line])?;
    if let Some(row) = rows.next()? {
        let kind: String = row.get(0)?;
        let name: String = row.get(1)?;
        Ok(Some((kind, name)))
    } else {
        Ok(None)
    }
}

fn forge_read_single(
    root: &Path,
    adapter: &Adapter,
    request: &ReadRequest,
    page: &QueryOptions,
    source: &SourceOptions,
    conn: Option<&Connection>,
) -> Result<Value> {
    let mut normalized_req;
    let request = if let Some(stripped) = request
        .path
        .as_deref()
        .and_then(|p| strip_root_path(p, root))
    {
        normalized_req = request.clone();
        normalized_req.path = Some(stripped);
        &normalized_req
    } else {
        request
    };
    let mode = request.resolved_mode();
    let markdown = request
        .path
        .as_deref()
        .is_some_and(|p| p.ends_with(".md") || p.ends_with(".markdown"));
    request.requires_index(adapter)?;
    let task = active_task(root, adapter).ok().flatten();
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
                compute_file_delta(root, &file, request.baseline.as_deref(), task.as_ref())?,
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
    let footer_hint = if mode == ReadMode::Lines {
        if let (Some(conn), Some(start_line)) = (conn, request.start_line) {
            let (start, end_line) = line_coordinates(start_line, request.line_count)?;
            find_enclosing_symbol(conn, &path, start, end_line)?.map(|(kind, name)| {
                let kind_label = match kind.as_str() {
                    "function" | "method" => "fn",
                    "struct" => "struct",
                    "class" => "class",
                    "trait" => "trait",
                    "enum" => "enum",
                    "interface" => "interface",
                    other => other,
                };
                format!(
                    "💡 Enclosing symbol: '{kind_label} {name}'. To read this entire symbol safely with verified digest, use forge_read(path=\"{path}\", selector=\"{name}\")"
                )
            })
        } else {
            None
        }
    } else {
        None
    };
    let mut hints = render_read_hints(
        root,
        conn,
        adapter,
        &path,
        &value,
        task.as_ref(),
        value["write_scope"] == "OUT_OF_SCOPE",
    )?;
    if let Some(ref footer) = footer_hint {
        value["footer"] = json!(footer);
        if hints.is_empty() {
            hints = footer.clone();
        } else {
            hints = format!("{footer}\n{hints}");
        }
    }
    if !hints.is_empty() {
        value["Hints"] = json!(hints);
    }
    if let Some(output) = &request.output_file {
        sensitive(adapter, root, output)?;
        let destination = forge_core::fs::checked_child(root, Path::new(output))?;
        let input_res = resolve_file_path(root, adapter, &path)?;
        ensure!(
            !forge_core::fs::same_filesystem_path(&destination, &input_res),
            "output_file must differ from input"
        );
        let content = exportable_content(&value).context("read has no exportable content")?;
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
    file: &Path,
    baseline: Option<&str>,
    task: Option<&forge_tasks::store::Task>,
) -> Result<String> {
    // Resolve Git from the admitted physical file, including linked repositories.
    // Task authority remains bound to the primary workspace below.
    let git_directory = file.parent().context("delta file has no parent")?;
    let path = file
        .file_name()
        .and_then(|name| name.to_str())
        .context("delta filename is not UTF-8")?;
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
        git_directory,
        &["rev-parse", "--verify", &format!("{revision}^{{commit}}")],
    )?;
    ensure!(resolved.status.success(), "delta baseline is not a commit");
    let commit = String::from_utf8(resolved.stdout)?;
    let output = forge_core::git::git_command(
        git_directory,
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

#[cfg(test)]
mod tests {
    use super::sensitive_with_case;
    use forge_core::config::Adapter;

    #[test]
    fn sensitive_policy_honors_case_insensitive_volume_flag() {
        let adapter = Adapter {
            sensitive: vec![".env*".into(), "secrets/**".into()],
            ..Default::default()
        };

        assert!(sensitive_with_case(&adapter, ".env.local", false).is_err());
        assert!(sensitive_with_case(&adapter, ".ENV.local", false).is_ok());
        assert!(sensitive_with_case(&adapter, ".ENV.local", true).is_err());
        assert!(sensitive_with_case(&adapter, "SECRETS/token.txt", true).is_err());
        assert!(sensitive_with_case(&adapter, "src/lib.rs", true).is_ok());
    }
}
