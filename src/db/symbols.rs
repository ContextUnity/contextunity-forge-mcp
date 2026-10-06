use super::{paging, reader};
use crate::core::{
    models::stable_hash64,
    response::{CoverageOptions, Detail, QueryOptions, SourceOptions},
};
use crate::engine::scanner;
use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{fs::File, io::Read, path::Path};

#[path = "symbols/search.rs"]
mod search;
pub use search::search_with_options;

#[path = "symbols/test_discovery.rs"]
mod test_discovery;
pub use test_discovery::tests_paged;

/// Filters and paging for a symbol search.
pub struct SearchOptions<'a> {
    /// Optional symbol kind.
    pub kind: Option<&'a str>,
    /// Workspace-relative file or directory scope.
    pub path: Option<&'a str>,
    /// Whether Markdown document symbols are included.
    pub include_docs: bool,
    /// Whether the name or qualified name must match exactly.
    pub exact: bool,
    /// Bounded page and generation contract.
    pub page: &'a QueryOptions,
}

/// Output controls for symbol inspection.
pub struct InspectOptions<'a> {
    /// Whether linked documentation is included.
    pub show_doc: bool,
    /// Optional bounded source preview.
    pub source: &'a SourceOptions,
    /// Optional resolution coverage.
    pub coverage: CoverageOptions,
    /// Bounded page and generation contract.
    pub page: &'a QueryOptions,
}

/// Output controls for symbol explanation.
pub struct ExplainOptions<'a> {
    /// Incoming, outgoing, or both directions.
    pub direction: Option<&'a str>,
    /// Whether linked documentation is included.
    pub show_doc: bool,
    /// Optional bounded source preview.
    pub source: &'a SourceOptions,
    /// Optional resolution coverage.
    pub coverage: CoverageOptions,
    /// Bounded page and generation contract.
    pub page: &'a QueryOptions,
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

/// Inspect one indexed symbol with bounded documentation, coverage, and source.
pub fn inspect_with_options(
    conn: &Connection,
    root: &Path,
    selector: &str,
    options: &InspectOptions<'_>,
) -> Result<Value> {
    let result = inspect_paged_response(
        conn,
        selector,
        options.show_doc,
        options.page,
        options.coverage,
    )?;
    with_source(conn, root, result, options.source, options.page)
}

/// Performs snippet paged.
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

/// Explain direct relationships of one indexed symbol.
pub fn explain_with_options(
    conn: &Connection,
    root: &Path,
    selector: &str,
    options: &ExplainOptions<'_>,
) -> Result<Value> {
    let result = explain_paged_response(
        conn,
        selector,
        options.direction,
        options.show_doc,
        options.page,
        options.coverage,
    )?;
    with_source(conn, root, result, options.source, options.page)
}

pub(crate) fn inspect_paged_response(
    conn: &Connection,
    selector: &str,
    show_doc: bool,
    options: &QueryOptions,
    coverage: CoverageOptions,
) -> Result<Value> {
    let result = if coverage.include_coverage {
        reader::inspect_paged(conn, selector, show_doc, options)?
    } else {
        inspect_without_coverage(conn, selector, show_doc, options)?
    };
    summarize_symbol(conn, result, coverage.include_coverage, options.detail)
}

pub(crate) fn explain_paged_response(
    conn: &Connection,
    selector: &str,
    direction: Option<&str>,
    show_doc: bool,
    options: &QueryOptions,
    coverage: CoverageOptions,
) -> Result<Value> {
    let result = if coverage.include_coverage {
        reader::explain_with_options(
            conn,
            selector,
            &reader::ExplainOptions {
                direction,
                show_doc,
                page: options,
            },
        )?
    } else {
        explain_without_coverage(conn, selector, direction, show_doc, options)?
    };
    summarize_symbol(conn, result, coverage.include_coverage, options.detail)
}

fn inspect_without_coverage(
    conn: &Connection,
    selector: &str,
    show_doc: bool,
    options: &QueryOptions,
) -> Result<Value> {
    let generation = paging::generation(conn, options)?;
    let node = reader::select_detail(conn, selector, options.detail)?;
    let id = node["id"].as_str().context("invalid node id")?;
    let mut documents = if show_doc {
        paging::query(
            conn,
            &format!(
                "SELECT {} FROM doc_sections d JOIN nodes dn ON dn.id=d.doc_id JOIN edges e ON e.dst_hash=dn.node_hash WHERE e.src_hash=?1 AND e.kind='references_doc' ORDER BY d.is_invariant DESC,d.path,d.doc_id",
                paging::docs("d", options.detail)
            ),
            &[&stable_hash64(id)],
            options,
        )?
    } else {
        paging::value(Vec::new(), 0, options, &generation)
    };
    if documents["total"] == 0 {
        documents = json!({"total": 0});
    }
    Ok(json!({"node":node,"documents":documents,"generation":generation}))
}

fn explain_without_coverage(
    conn: &Connection,
    selector: &str,
    direction: Option<&str>,
    show_doc: bool,
    options: &QueryOptions,
) -> Result<Value> {
    let mut result = inspect_without_coverage(conn, selector, show_doc, options)?;
    let id = result["node"]["id"]
        .as_str()
        .context("invalid node id")?
        .to_owned();
    let node_hash = stable_hash64(&id);
    let direction = match direction.unwrap_or("both") {
        "inbound" => "incoming",
        "outbound" => "outgoing",
        direction => direction,
    };
    match direction {
        "both" | "incoming" => {
            let mut incoming = paging::query(
                conn,
                &format!(
                    "SELECT {} FROM edges e WHERE e.dst_hash=?1 ORDER BY e.kind,e.src_hash,e.path_id,e.line",
                    paging::edges("e", options.detail)
                ),
                &[&node_hash],
                options,
            )?;
            if options.detail == Detail::Compact {
                omit_compact_endpoint(&mut incoming, "dst_public_id");
            }
            result["incoming"] = incoming;
        }
        "outgoing" => {
            let total = paging::count(
                conn,
                "SELECT count(*) FROM edges WHERE dst_hash=?1",
                &[&node_hash],
            )?;
            result["incoming"] = json!({"total":total,"omitted":true,"hint":"Pass direction='incoming' to page incoming edges."});
        }
        _ => bail!("direction must be both, incoming (inbound), or outgoing (outbound)"),
    }
    match direction {
        "both" | "outgoing" => {
            let mut outgoing = paging::query(
                conn,
                &format!(
                    "SELECT {} FROM edges e WHERE e.src_hash=?1 ORDER BY e.kind,e.dst_hash,e.path_id,e.line",
                    paging::edges("e", options.detail)
                ),
                &[&node_hash],
                options,
            )?;
            if options.detail == Detail::Compact {
                omit_compact_endpoint(&mut outgoing, "src_public_id");
            }
            result["outgoing"] = outgoing;
        }
        "incoming" => {
            let total = paging::count(
                conn,
                "SELECT count(*) FROM edges WHERE src_hash=?1",
                &[&node_hash],
            )?;
            result["outgoing"] = json!({"total":total,"omitted":true,"hint":"Pass direction='outgoing' to page outgoing edges."});
        }
        _ => {}
    }
    let implementors = reader::rows(
        conn,
        "SELECT n.id AS src_public_id,e.kind,p.path,e.line,v.evidence,n.name,n.kind AS node_kind FROM edges e JOIN nodes n ON n.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=e.path_id JOIN coverage_evidence v ON v.evidence_id=e.evidence_id WHERE e.dst_hash=?1 AND e.kind IN('implements','overrides','extends') ORDER BY p.path,e.line",
        &[&node_hash],
        50,
    )?;
    if !implementors.is_empty() {
        result["implementors"] = json!(implementors);
    }
    let implements = reader::rows(
        conn,
        "SELECT n.id AS dst_public_id,e.kind,p.path,e.line,v.evidence,n.name,n.kind AS node_kind FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=e.path_id JOIN coverage_evidence v ON v.evidence_id=e.evidence_id WHERE e.src_hash=?1 AND e.kind IN('implements','overrides','extends') ORDER BY p.path,e.line",
        &[&node_hash],
        50,
    )?;
    if !implements.is_empty() {
        result["implements"] = json!(implements);
    }
    result["direction"] = json!(direction);
    Ok(result)
}

fn omit_compact_endpoint(page: &mut Value, field: &str) {
    if let Some(items) = page["items"].as_array_mut() {
        for item in items {
            if let Some(edge) = item.as_object_mut() {
                edge.remove(field);
            }
        }
    }
}

fn summarize_symbol(
    conn: &Connection,
    mut result: Value,
    include_coverage: bool,
    detail: crate::core::response::Detail,
) -> Result<Value> {
    if !include_coverage {
        result
            .as_object_mut()
            .context("invalid symbol result")?
            .remove("coverage");
    }
    if detail == Detail::Compact {
        let summary = compact_summary(conn, &result["node"])?;
        result["summary"] = summary;
    }
    Ok(result)
}

fn compact_summary(conn: &Connection, node: &Value) -> Result<Value> {
    const PREVIEW_LIMIT: i64 = 5;
    let id = node["id"].as_str().context("selected node has no id")?;
    let (
        signature,
        docstring,
        receiver_type,
        receiver,
        receiver_name,
        is_method,
        is_static,
    ) = conn.query_row(
        "SELECT json_extract(details,'$.signature'),json_extract(details,'$.doc'),json_extract(details,'$.receiver_type'),json_extract(details,'$.receiver'),json_extract(details,'$.receiver_name'),json_extract(details,'$.is_method'),json_extract(details,'$.is_static') FROM nodes WHERE id=?1",
        [id],
        |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<bool>>(5)?,
                row.get::<_, Option<bool>>(6)?,
            ))
        },
    )?;
    let mut container = reader::rows(
        conn,
        "SELECT n.id,n.kind,n.name,n.qualname,p.path,n.line FROM edges e JOIN nodes n ON n.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=n.path_id WHERE e.dst_hash=?1 AND e.kind='contains' ORDER BY CASE WHEN n.kind IN('module','file') THEN 1 ELSE 0 END,n.line DESC,n.end_line,n.id LIMIT 1",
        &[&stable_hash64(id)],
        1,
    )?
    .into_iter()
    .next()
    .unwrap_or(Value::Null);
    let receiver = receiver_type.or(receiver);
    let is_method = node["kind"] == "method" || is_method.unwrap_or(false);
    let is_type_container = matches!(
        container["kind"].as_str(),
        Some("class" | "enum" | "interface" | "record" | "struct" | "trait" | "type")
    );
    if is_method && !is_type_container {
        if let Some(receiver) = receiver.as_deref() {
            let path = node["path"].as_str().unwrap_or("");
            let candidates = reader::rows(
                conn,
                "SELECT n.id,n.kind,n.name,n.qualname,p.path,n.line FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.name=?2 COLLATE NOCASE AND n.name=?2 COLLATE BINARY AND n.kind IN('class','enum','interface','record','struct','trait','type') ORDER BY n.line,n.id LIMIT 101",
                &[&path, &receiver],
                101,
            )?;
            let receiver_container = if candidates.len() == 1 {
                candidates.into_iter().next()
            } else if (2..=100).contains(&candidates.len()) {
                let method_qualname = node["qualname"].as_str().unwrap_or("");
                let module_qualname = container["qualname"].as_str().unwrap_or("");
                let mut scoped = candidates
                    .into_iter()
                    .filter(|candidate| {
                        let qualname = candidate["qualname"].as_str().unwrap_or("");
                        let impl_owner = !module_qualname.is_empty() && qualname == module_qualname;
                        let method_owner = [".", "::"].iter().any(|separator| {
                            method_qualname.starts_with(&format!("{qualname}{separator}"))
                        });
                        let module_owner = [".", "::"].iter().any(|separator| {
                            qualname == format!("{module_qualname}{separator}{receiver}")
                        });
                        impl_owner || method_owner || module_owner
                    })
                    .collect::<Vec<_>>();
                if scoped.len() > 1 {
                    let method_line = node["line"].as_u64().unwrap_or_default();
                    let nearest_line = scoped
                        .iter()
                        .filter_map(|candidate| candidate["line"].as_u64())
                        .map(|line| line.abs_diff(method_line))
                        .min();
                    if let Some(nearest_line) = nearest_line {
                        scoped.retain(|candidate| {
                            candidate["line"]
                                .as_u64()
                                .is_some_and(|line| line.abs_diff(method_line) == nearest_line)
                        });
                    }
                }
                if scoped.len() == 1 {
                    scoped.pop()
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(receiver_container) = receiver_container {
                container = receiver_container;
            } else {
                container = json!({
                    "kind": "receiver",
                    "name": receiver,
                    "path": node["path"],
                    "line": node["line"]
                });
            }
        }
    }
    let inbound = direct_call_summary(conn, id, true, PREVIEW_LIMIT)?;
    let outbound = direct_call_summary(conn, id, false, PREVIEW_LIMIT)?;
    let mut summary = json!({
        "signature": signature,
        "docstring": docstring,
        "container": container,
        "calls": {
            "inbound": inbound,
            "outbound": outbound
        }
    });
    if is_method {
        summary["method"] = json!({
            "receiver": receiver,
            "receiver_name": receiver_name,
            "is_static": is_static
        });
    }
    Ok(summary)
}

fn direct_call_summary(
    conn: &Connection,
    id: &str,
    inbound: bool,
    preview_limit: i64,
) -> Result<Value> {
    let (count_sql, preview_sql) = if inbound {
        (
            "SELECT count(*),coalesce(sum(occurrence_count),0) FROM edges WHERE dst_hash=?1 AND kind='calls'",
            "SELECT n.id,n.kind,n.name,n.qualname,p.path,n.line FROM edges e JOIN nodes n ON n.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=n.path_id WHERE e.dst_hash=?1 AND e.kind='calls' ORDER BY p.path,n.line,n.id LIMIT ?2",
        )
    } else {
        (
            "SELECT count(*),coalesce(sum(occurrence_count),0) FROM edges WHERE src_hash=?1 AND kind='calls'",
            "SELECT n.id,n.kind,n.name,n.qualname,p.path,n.line FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=n.path_id WHERE e.src_hash=?1 AND e.kind='calls' ORDER BY p.path,n.line,n.id LIMIT ?2",
        )
    };
    let (count, occurrences) = {
        let budget = reader::QueryBudget::new(conn);
        let counts = conn.query_row(count_sql, [stable_hash64(id)], |row| {
            Ok((row.get::<_, usize>(0)?, row.get::<_, usize>(1)?))
        })?;
        budget.check()?;
        counts
    };
    let preview = reader::rows(
        conn,
        preview_sql,
        &[&stable_hash64(id), &preview_limit],
        preview_limit as usize,
    )?;
    Ok(json!({"count":count,"occurrences":occurrences,"preview":preview}))
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
    let (snippet, preview) = source_preview(conn, node, &text, source)?;
    let gen = result.as_object_mut().and_then(|m| m.remove("generation"));
    result["source"] = json!(snippet);
    result["source_preview"] = preview;
    if let Some(gen) = gen {
        if let Some(m) = result.as_object_mut() {
            m.insert("generation".into(), gen);
        }
    }
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
    let previous: usize = conn.query_row("SELECT coalesce(max(n.end_line),0) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.end_line<?2 AND n.kind NOT IN('file','module','component')", rusqlite::params![path,start], |r| r.get(0))?;
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
