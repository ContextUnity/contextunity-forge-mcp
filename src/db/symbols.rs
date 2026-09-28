use super::{paging, reader};
use crate::core::response::{QueryOptions, SourceOptions};
use crate::engine::scanner;
use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{collections::HashMap, fs::File, io::Read, path::Path};

fn admit_test_mapping(conn: &Connection, id: &str, inbound: bool) -> Result<()> {
    let seeds = paging::count(
        conn,
        "SELECT count(*) FROM edges WHERE src_public_id=?1 AND kind='contains'",
        &[&id],
    )?;
    let dependencies = super::traversal::immediate_links(conn, id, inbound, false)?;
    let immediate = seeds.saturating_add(dependencies);
    if immediate > 1000 {
        bail!("test mapping from {id} starts with {immediate} direct graph links before its unbounded dependency walk. Select a narrower module or symbol; reducing limit alone does not reduce traversal work");
    }
    Ok(())
}

pub fn inspect(
    conn: &Connection,
    root: &Path,
    selector: &str,
    show_doc: bool,
    show_source: bool,
) -> Result<Value> {
    let mut result = reader::inspect(conn, selector, show_doc)?;
    if !show_source {
        return Ok(result);
    }
    let node = &result["node"];
    let path = node["path"].as_str().context("invalid node path")?;
    let start = node["line"].as_u64().context("invalid start line")? as usize;
    let end = node["end_line"].as_u64().context("invalid end line")? as usize;
    if start == 0 || end < start {
        bail!("node has no source range");
    }
    let source = verified_source(conn, root, path)?;
    let lines: Vec<_> = source.split_inclusive('\n').collect();
    if end > lines.len() {
        bail!("source range exceeds indexed file");
    }
    let snippet = lines[start - 1..end].concat();
    if serde_json::to_vec(&snippet)?.len() > 8 * 1024 * 1024 {
        bail!("source exceeds serialized byte limit");
    }
    result["source"] = json!(snippet);
    Ok(result)
}

fn verified_source(conn: &Connection, root: &Path, path: &str) -> Result<String> {
    let digest: String = conn.query_row("SELECT digest FROM files WHERE path=?1", [path], |r| {
        r.get(0)
    })?;
    let adapter = scanner::load_adapter(root, None)?;
    let resolved = scanner::resolve_file_path(root, &adapter, path)?;
    let file = File::open(resolved).context("source file unavailable; rebuild index")?;
    if !file.metadata()?.is_file() {
        bail!("source must be a regular file");
    }
    let mut bytes = Vec::new();
    file.take(scanner::MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > scanner::MAX_FILE_BYTES {
        bail!("source exceeds file byte limit");
    }
    if scanner::digest(&bytes) != digest {
        bail!("source changed since indexing; update or rebuild index");
    }
    String::from_utf8(bytes).context("source is not UTF-8")
}

pub fn search(conn: &Connection, pattern: &str, kind: Option<&str>, limit: usize) -> Result<Value> {
    reader::validate_limit(limit)?;
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern.len() > 1024 || pattern.chars().all(|c| c == '*') {
        bail!("pattern must contain a symbol fragment and be at most 1024 bytes");
    }
    let kind = kind.unwrap_or("");
    let size = limit + 1;
    let mut nodes = if pattern.contains('*') {
        // Only '*' has pattern semantics; SQL wildcards remain literal.
        let like = pattern
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
            .replace('*', "%");
        if let Some(prefix) = pattern
            .strip_suffix('*')
            .filter(|p| !p.is_empty() && p.chars().all(char::is_alphanumeric))
        {
            let query = format!("\"{prefix}\"*");
            reader::rows(conn, "SELECT n.* FROM node_search JOIN nodes n ON n.node_id=node_search.rowid WHERE node_search MATCH ?4 AND (n.name LIKE ?1 ESCAPE '\\' OR n.qualname LIKE ?1 ESCAPE '\\') AND (?2='' OR n.kind=?2) ORDER BY CASE WHEN n.name LIKE ?1 ESCAPE '\\' THEN 0 ELSE 1 END,n.path,n.line,n.id LIMIT ?3", &[&like, &kind, &size, &query], size)?
        } else {
            reader::rows(conn, "SELECT * FROM nodes WHERE (name LIKE ?1 ESCAPE '\\' OR qualname LIKE ?1 ESCAPE '\\') AND (?2='' OR kind=?2) ORDER BY CASE WHEN name LIKE ?1 ESCAPE '\\' THEN 0 ELSE 1 END,path,line,id LIMIT ?3", &[&like, &kind, &size], size)?
        }
    } else {
        let query = pattern
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(" AND ");
        if query.is_empty() {
            bail!("pattern must contain a symbol fragment");
        }
        reader::rows(conn, "SELECT n.* FROM node_search JOIN nodes n ON n.node_id=node_search.rowid WHERE node_search MATCH ?1 AND (?2='' OR n.kind=?2) ORDER BY n.path,n.line,n.id LIMIT ?3", &[&query, &kind, &size], size)?
    };
    let truncated = nodes.len() > limit;
    nodes.truncate(limit);
    Ok(json!({"pattern":pattern,"nodes":nodes,"truncated":truncated,"limit":limit}))
}

pub fn tests(conn: &Connection, selector: &str, direction: &str, limit: usize) -> Result<Value> {
    reader::validate_limit(limit)?;
    let (inbound, is_test) = match direction {
        "inbound" => (true, 1),
        "outbound" => (false, 0),
        _ => bail!("direction must be inbound or outbound"),
    };
    let node = reader::select(conn, selector)?;
    let id = node["id"].as_str().context("invalid node id")?;
    admit_test_mapping(conn, id, inbound)?;
    // Containment expands only the selected scope; UNION visits each dependency once.
    let steps = super::traversal::dependency_steps(inbound, false, false);
    let sql = format!("WITH RECURSIVE seeds(id) AS (SELECT ?1 UNION SELECT e.dst_public_id FROM seeds s JOIN edges e ON e.src_public_id=s.id WHERE e.kind='contains'), walk(id) AS (SELECT id FROM seeds UNION {steps}) SELECT n.* FROM walk w JOIN nodes n ON n.id=w.id WHERE n.is_test=?2 AND n.kind IN ('function','method','class','struct') AND n.id!=?1 ORDER BY n.path,n.line,n.id LIMIT ?3");
    let size = limit + 1;
    let mut nodes = reader::rows(conn, &sql, &[&id, &is_test, &size], size)?;
    let truncated = nodes.len() > limit;
    nodes.truncate(limit);
    Ok(
        json!({"selector":node,"direction":direction,"nodes":nodes,"truncated":truncated,"scope":"indexed static dependencies; unresolved references can hide tests"}),
    )
}

pub fn search_paged(
    conn: &Connection,
    pattern: &str,
    kind: Option<&str>,
    options: &QueryOptions,
) -> Result<Value> {
    search_paged_in_path(conn, pattern, kind, None, options)
}

pub fn search_paged_in_path(
    conn: &Connection,
    pattern: &str,
    kind: Option<&str>,
    path: Option<&str>,
    options: &QueryOptions,
) -> Result<Value> {
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern.len() > 1024 || pattern.chars().all(|c| c == '*') {
        bail!("pattern must contain a symbol fragment and be at most 1024 bytes");
    }
    let kind = kind.unwrap_or("");
    let path = path.map(|value| {
        value
            .trim()
            .strip_prefix("file://")
            .or_else(|| value.trim().strip_prefix("file:"))
            .unwrap_or(value.trim())
            .trim_start_matches("./")
            .trim_end_matches('/')
    });
    if let Some(path) = path {
        if path.is_empty()
            || path.starts_with('/')
            || path.contains('\\')
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            bail!("path must be a nonempty workspace-relative file or directory");
        }
    }
    let path = path.unwrap_or("");
    let (path_start, path_end) = reader::path_bounds(path);
    let columns = paging::nodes("n", options.detail);
    let mut nodes = if pattern.contains('*') {
        let like = pattern
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
            .replace('*', "%");
        if let Some(prefix) = pattern
            .strip_suffix('*')
            .filter(|p| !p.is_empty() && p.chars().all(char::is_alphanumeric))
        {
            let query = format!("\"{prefix}\"*");
            paging::query(conn, &format!("SELECT {columns},CASE WHEN n.name LIKE ?1 ESCAPE '\\' THEN 'name_pattern' ELSE 'qualified_pattern' END match_reason FROM node_search JOIN nodes n ON n.node_id=node_search.rowid WHERE node_search MATCH ?3 AND (n.name LIKE ?1 ESCAPE '\\' OR n.qualname LIKE ?1 ESCAPE '\\') AND (?2='' OR n.kind=?2) AND (?4='' OR n.path=?4 OR (n.path>=?5 AND n.path<?6)) ORDER BY CASE WHEN n.name LIKE ?1 ESCAPE '\\' THEN 0 ELSE 1 END,n.path,n.line,n.id"), &[&like,&kind,&query,&path,&path_start,&path_end], options)?
        } else {
            paging::query(conn, &format!("SELECT {columns},CASE WHEN n.name LIKE ?1 ESCAPE '\\' THEN 'name_pattern' ELSE 'qualified_pattern' END match_reason FROM nodes n WHERE (n.name LIKE ?1 ESCAPE '\\' OR n.qualname LIKE ?1 ESCAPE '\\') AND (?2='' OR n.kind=?2) AND (?3='' OR n.path=?3 OR (n.path>=?4 AND n.path<?5)) ORDER BY CASE WHEN n.name LIKE ?1 ESCAPE '\\' THEN 0 ELSE 1 END,n.path,n.line,n.id"), &[&like,&kind,&path,&path_start,&path_end], options)?
        }
    } else {
        let query = pattern
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(" AND ");
        if query.is_empty() {
            bail!("pattern must contain a symbol fragment");
        }
        let escaped = pattern
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let prefix = format!("{escaped}%");
        let fragment = format!("%{escaped}%");
        let rank = "CASE WHEN n.name=?6 COLLATE NOCASE THEN 0 WHEN n.name LIKE ?7 ESCAPE '\\' THEN 1 WHEN n.name LIKE ?8 ESCAPE '\\' THEN 2 WHEN n.qualname=?6 COLLATE NOCASE THEN 3 WHEN n.qualname LIKE ?7 ESCAPE '\\' THEN 4 WHEN n.qualname LIKE ?8 ESCAPE '\\' THEN 5 ELSE 6 END";
        paging::query(conn, &format!("SELECT {columns},{rank} match_rank FROM node_search JOIN nodes n ON n.node_id=node_search.rowid WHERE node_search MATCH ?1 AND (?2='' OR n.kind=?2) AND (?3='' OR n.path=?3 OR (n.path>=?4 AND n.path<?5)) ORDER BY match_rank,n.path,n.line,n.id"), &[&query,&kind,&path,&path_start,&path_end,&pattern,&prefix,&fragment], options)?
    };
    if let Some(items) = nodes["items"].as_array_mut() {
        for item in items {
            if options.detail == crate::core::response::Detail::Compact
                && item["match_reason"] == "name_pattern"
            {
                item.as_object_mut()
                    .context("invalid search result")?
                    .remove("match_reason");
            }
            if let Some(rank) = item
                .as_object_mut()
                .and_then(|item| item.remove("match_rank"))
            {
                let reason = match rank.as_u64() {
                    Some(0) => "exact_name",
                    Some(1) => "name_prefix",
                    Some(2) => "name_fragment",
                    Some(3) => "exact_qualified_name",
                    Some(4) => "qualified_name_prefix",
                    Some(5) => "qualified_name_fragment",
                    _ => "indexed_text",
                };
                if options.detail == crate::core::response::Detail::Full
                    || !matches!(rank.as_u64(), Some(0..=2))
                {
                    item["match_reason"] = json!(reason);
                }
            }
        }
    }
    Ok(json!({"pattern":pattern,"nodes":nodes}))
}

pub fn tests_paged(
    conn: &Connection,
    selector: &str,
    direction: &str,
    options: &QueryOptions,
) -> Result<Value> {
    let (inbound, is_test) = match direction {
        "inbound" => (true, 1),
        "outbound" => (false, 0),
        _ => bail!("direction must be inbound or outbound"),
    };
    paging::generation(conn, options)?;
    let node = reader::select_detail(conn, selector, options.detail)?;
    let id = node["id"].as_str().context("invalid node id")?;
    admit_test_mapping(conn, id, inbound)?;
    let path = node["path"].as_str().unwrap_or("");
    let start_line = node["line"].as_i64().unwrap_or(0);
    let end_line = node["end_line"].as_i64().unwrap_or(i64::MAX);

    let unresolved_count: usize = conn
        .query_row(
            "SELECT count(*) FROM resolution_coverage WHERE status IN('unresolved','ambiguous') AND path=?1 AND line BETWEEN ?2 AND ?3",
            rusqlite::params![path, start_line, end_line],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let steps = super::traversal::dependency_steps(inbound, false, false);
    let (forward_candidate, forward_selected, reverse_candidate, reverse_selected) = if inbound {
        (
            "src_public_id",
            "dst_public_id",
            "dst_public_id",
            "src_public_id",
        )
    } else {
        (
            "dst_public_id",
            "src_public_id",
            "src_public_id",
            "dst_public_id",
        )
    };
    let sql = format!("WITH RECURSIVE seeds(id) AS (SELECT ?1 UNION SELECT e.dst_public_id FROM seeds s JOIN edges e ON e.src_public_id=s.id WHERE e.kind='contains'), walk(id) AS (SELECT id FROM seeds UNION {steps}) SELECT {} FROM walk w JOIN nodes n ON n.id=w.id WHERE n.is_test=?2 AND n.kind IN ('function','method','class','struct') AND n.id!=?1 ORDER BY CASE WHEN EXISTS(SELECT 1 FROM edges e WHERE e.{forward_selected}=?1 AND e.{forward_candidate}=n.id AND e.kind IN({})) OR EXISTS(SELECT 1 FROM edges e WHERE e.{reverse_selected}=?1 AND e.{reverse_candidate}=n.id AND e.kind IN({})) THEN 0 ELSE 1 END,n.path,n.line,n.id", paging::nodes("n", options.detail), super::traversal::FORWARD_DEPENDENCIES, super::traversal::REVERSE_DEPENDENCIES);
    let mut nodes = paging::query(conn, &sql, &[&id, &is_test], options)?;
    annotate_test_connections(
        conn,
        &mut nodes,
        id,
        path,
        inbound,
        options.detail == crate::core::response::Detail::Compact,
    )?;
    let scope_note = if unresolved_count > 0 {
        format!("indexed static dependencies; {unresolved_count} unresolved reference(s) within symbol scope may conceal dynamic test callers")
    } else {
        "indexed static dependencies; no unresolved references in symbol scope".to_string()
    };
    Ok(
        json!({"selector":node,"direction":direction,"nodes":nodes,"unresolved_references":unresolved_count,"scope":scope_note}),
    )
}

fn annotate_test_connections(
    conn: &Connection,
    page: &mut Value,
    selected_id: &str,
    selected_path: &str,
    inbound: bool,
    compact: bool,
) -> Result<()> {
    let items = page["items"]
        .as_array_mut()
        .context("missing test result items")?;
    if items.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = items
        .iter()
        .map(|item| {
            item["id"]
                .as_str()
                .context("invalid test result id")
                .map(str::to_owned)
        })
        .collect::<Result<_>>()?;
    let placeholders = (2..=ids.len() + 1)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let (forward_candidate, forward_selected, reverse_candidate, reverse_selected) = if inbound {
        (
            "src_public_id",
            "dst_public_id",
            "dst_public_id",
            "src_public_id",
        )
    } else {
        (
            "dst_public_id",
            "src_public_id",
            "src_public_id",
            "dst_public_id",
        )
    };
    let sql = format!(
        "SELECT e.{forward_candidate} related_id,e.kind,e.path,e.line FROM edges e WHERE e.{forward_selected}=?1 AND e.{forward_candidate} IN ({placeholders}) AND e.kind IN({}) UNION ALL SELECT e.{reverse_candidate} related_id,e.kind,e.path,e.line FROM edges e WHERE e.{reverse_selected}=?1 AND e.{reverse_candidate} IN ({placeholders}) AND e.kind IN({}) ORDER BY related_id,kind,path,line",
        super::traversal::FORWARD_DEPENDENCIES,
        super::traversal::REVERSE_DEPENDENCIES,
    );
    let bindings: Vec<&dyn rusqlite::ToSql> = std::iter::once(&selected_id as &dyn rusqlite::ToSql)
        .chain(ids.iter().map(|id| id as &dyn rusqlite::ToSql))
        .collect();
    // At most seven dependency kinds are persisted per selected/candidate pair.
    let direct = reader::rows(conn, &sql, &bindings, ids.len() * 7)?;
    let mut reasons = HashMap::new();
    for edge in direct {
        let related_id = edge["related_id"]
            .as_str()
            .context("invalid related node id")?
            .to_owned();
        reasons.entry(related_id).or_insert_with(|| {
            json!({
                "relation": "direct",
                "edge_kind": edge["kind"],
                "path": edge["path"],
                "line": edge["line"],
            })
        });
    }
    for item in items {
        let id = item["id"].as_str().context("invalid test result id")?;
        let mut connection = reasons
            .remove(id)
            .unwrap_or_else(|| json!({"relation":"scope_or_transitive"}));
        if compact && connection["relation"] == "direct" {
            let evidence_path = connection["path"].as_str().unwrap_or("");
            if evidence_path == selected_path
                || evidence_path == item["path"].as_str().unwrap_or("")
            {
                connection
                    .as_object_mut()
                    .context("invalid direct connection")?
                    .remove("path");
            }
        }
        item["connection"] = connection;
    }
    Ok(())
}

pub fn inspect_paged(
    conn: &Connection,
    root: &Path,
    selector: &str,
    show_doc: bool,
    source: &SourceOptions,
    options: &QueryOptions,
) -> Result<Value> {
    with_source(
        conn,
        root,
        reader::inspect_paged(conn, selector, show_doc, options)?,
        source,
        options,
    )
}

pub fn snippet_paged(
    conn: &Connection,
    root: &Path,
    selector: &str,
    source: &SourceOptions,
    options: &QueryOptions,
) -> Result<Value> {
    let generation = paging::generation(conn, options)?;
    let node = reader::select_detail(conn, selector, options.detail)?;
    with_source(
        conn,
        root,
        json!({"node":node,"generation":generation}),
        source,
        options,
    )
}

pub fn explain_paged(
    conn: &Connection,
    root: &Path,
    selector: &str,
    direction: Option<&str>,
    source: &SourceOptions,
    options: &QueryOptions,
) -> Result<Value> {
    explain_paged_with_docs(conn, root, selector, direction, true, source, options)
}

pub fn explain_paged_with_docs(
    conn: &Connection,
    root: &Path,
    selector: &str,
    direction: Option<&str>,
    show_doc: bool,
    source: &SourceOptions,
    options: &QueryOptions,
) -> Result<Value> {
    with_source(
        conn,
        root,
        reader::explain_paged_with_docs(conn, selector, direction, show_doc, options)?,
        source,
        options,
    )
}

fn with_source(
    conn: &Connection,
    root: &Path,
    mut result: Value,
    source: &SourceOptions,
    options: &QueryOptions,
) -> Result<Value> {
    if !source.enabled {
        return Ok(result);
    }
    anyhow::ensure!(
        source.leading_lines <= 20 && (1..=100).contains(&source.max_body_lines),
        "invalid source preview bounds"
    );
    anyhow::ensure!(
        source.offset == 0 || options.generation.is_some(),
        "source continuation requires generation from the previous page"
    );
    let node = &result["node"];
    let path = node["path"].as_str().context("invalid node path")?;
    let text = verified_source(conn, root, path)?;
    let (snippet, mut preview) = source_preview(conn, node, &text, source)?;
    preview["generation"] = result["generation"].clone();
    result["source"] = json!(snippet);
    result["source_preview"] = preview;
    Ok(result)
}

fn source_preview(
    conn: &Connection,
    node: &Value,
    text: &str,
    options: &SourceOptions,
) -> Result<(String, Value)> {
    const SOURCE_BYTES: usize = 8192;
    let path = node["path"].as_str().context("invalid node path")?;
    let start = node["line"].as_u64().context("invalid start line")? as usize;
    let end = node["end_line"].as_u64().context("invalid end line")? as usize;
    let lines: Vec<_> = text.split_inclusive('\n').collect();
    anyhow::ensure!(
        start > 0 && end >= start && end <= lines.len(),
        "node has no valid source range"
    );
    let line_start: usize = lines[..start - 1].iter().map(|line| line.len()).sum();
    let line_end: usize = line_start
        + lines[start - 1..end]
            .iter()
            .map(|line| line.len())
            .sum::<usize>();
    let mut range = symbol_range(node, text)?.unwrap_or(line_start..line_end);
    anyhow::ensure!(
        range.start >= line_start && range.end <= line_end,
        "source boundary differs from indexed symbol"
    );
    let starts_on_clean_line = text[line_start..range.start].trim().is_empty();
    if text[line_start..range.start].trim().is_empty() {
        range.start = line_start;
    }
    if text[range.end..line_end].trim().is_empty() {
        range.end = line_end;
    }
    let body: Vec<_> = text[range].split_inclusive('\n').collect();
    anyhow::ensure!(
        options.offset <= body.len(),
        "source_offset exceeds symbol body"
    );
    let previous: usize = conn.query_row("SELECT coalesce(max(end_line),0) FROM nodes WHERE path=?1 AND end_line<?2 AND kind NOT IN('file','module','component')", rusqlite::params![path,start], |r| r.get(0))?;
    let leading_start = (start - 1)
        .saturating_sub(options.leading_lines)
        .max(previous);
    let available_leading = if options.offset == 0 && starts_on_clean_line {
        start - 1 - leading_start
    } else {
        0
    };
    let mut leading = String::new();
    let mut leading_count = 0;
    if options.offset == 0 && starts_on_clean_line {
        for line in lines[leading_start..start - 1].iter().rev() {
            if escaped_cost(line) + escaped_cost(&leading) > SOURCE_BYTES / 4 {
                break;
            }
            leading.insert_str(0, line);
            leading_count += 1;
        }
    }
    let mut output = leading;
    let mut cost = escaped_cost(&output);
    let mut emitted = 0;
    let mut truncated_line = false;
    for line in body
        .iter()
        .skip(options.offset)
        .take(options.max_body_lines)
    {
        let line_cost = escaped_cost(line);
        if cost + line_cost > SOURCE_BYTES - 2 {
            if emitted == 0 {
                let available = SOURCE_BYTES - 2 - cost;
                let mut used = 0;
                for ch in line.chars() {
                    let next = escaped_char_cost(ch);
                    if used + next > available {
                        break;
                    }
                    output.push(ch);
                    used += next;
                }
                truncated_line = true;
            }
            break;
        }
        output.push_str(line);
        cost += line_cost;
        emitted += 1;
    }
    let next = options.offset + emitted;
    let has_more = next < body.len();
    let hint = if truncated_line {
        Some(format!("Line {} exceeds the source preview byte budget; read that line from local file {path}. No line continuation can return the omitted characters.",start+next))
    } else if has_more {
        Some("Repeat the same tool and selector with show_source=true, next_source_offset as source_offset, and this generation.".to_owned())
    } else {
        None
    };
    let start_line = (!output.is_empty()).then_some(start + options.offset - leading_count);
    let end_line = (!output.is_empty()).then_some(if emitted == 0 {
        start + options.offset
    } else {
        start + next - 1
    });
    Ok((
        output,
        json!({"start_line":start_line,"end_line":end_line,"total_body_lines":body.len(),"body_offset":options.offset,"body_lines":emitted,"leading_lines":leading_count,"next_source_offset":if has_more && !truncated_line {Some(next)} else {None},"has_more":has_more,"omitted_leading_lines":available_leading.saturating_sub(leading_count),"omitted_body_lines":body.len().saturating_sub(next),"truncated_line":truncated_line,"continuation_hint":hint}),
    ))
}

fn escaped_char_cost(ch: char) -> usize {
    match ch {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{08}' | '\u{0c}' => 2,
        ch if ch <= '\u{1f}' => 6,
        ch => ch.len_utf8(),
    }
}
fn escaped_cost(text: &str) -> usize {
    text.chars().map(escaped_char_cost).sum()
}

fn symbol_range(node: &Value, text: &str) -> Result<Option<std::ops::Range<usize>>> {
    let kind = node["kind"].as_str().unwrap_or("");
    if matches!(kind, "file" | "module" | "component") {
        return Ok(None);
    }
    let path = node["path"].as_str().context("invalid node path")?;
    let language = node["language"].as_str().context("invalid language")?;
    let profile = crate::engine::languages::require(language)?;
    let start = node["line"].as_u64().context("invalid start line")? as usize;
    let name = node["name"].as_str().context("invalid node name")?;
    let mut parser = profile.create_parser(path)?;
    parser.set_timeout_micros(2_000_000);
    let tree = parser
        .parse(text, None)
        .context("source preview parse exceeds time budget")?;
    let mut stack = vec![tree.root_node()];
    let mut matches = Vec::new();
    while let Some(syntax) = stack.pop() {
        if syntax.start_position().row + 1 > start || syntax.end_position().row + 1 < start {
            continue;
        }
        if syntax.start_position().row + 1 == start && profile.symbol(syntax) == Some(kind) {
            let candidate = profile
                .symbol_name(syntax, text)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    format!("anonymous@{}:{}", start, syntax.start_position().column + 1)
                });
            if candidate == name {
                matches.push(syntax.byte_range());
            }
        }
        let mut cursor = syntax.walk();
        stack.extend(syntax.named_children(&mut cursor));
    }
    matches.sort_by_key(|range| range.start);
    let base = format!("{}:{path}:{start}:{name}", profile.node_prefix(kind));
    let id = node["id"].as_str().context("invalid node id")?;
    let selected = if id == base {
        matches.into_iter().next()
    } else {
        matches.into_iter().find(|range| {
            id == format!(
                "{base}:{}",
                text[..range.start].rsplit('\n').next().unwrap_or("").len()
            )
        })
    };
    selected
        .map(Some)
        .context("indexed symbol boundary is unavailable; read the local file instead")
}
