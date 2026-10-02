use super::{paging, reader};
use crate::core::{
    models::stable_hash64,
    response::{CoverageOptions, Detail, QueryOptions, SourceOptions},
};
use crate::engine::scanner;
use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{collections::HashMap, fs::File, io::Read, path::Path};

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

fn admit_test_mapping(conn: &Connection, id: &str, inbound: bool) -> Result<()> {
    let seeds = paging::count(
        conn,
        "SELECT count(*) FROM edges WHERE src_hash=?1 AND kind='contains'",
        &[&stable_hash64(id)],
    )?;
    let dependencies = super::traversal::immediate_links(conn, id, inbound, false)?;
    let immediate = seeds.saturating_add(dependencies);
    if immediate > 1000 {
        bail!("test mapping from {id} starts with {immediate} direct graph links before its unbounded dependency walk. Select a narrower module or symbol; reducing limit alone does not reduce traversal work");
    }
    Ok(())
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

/// Search indexed symbols using one bounded options contract.
pub fn search_with_options(
    conn: &Connection,
    pattern: &str,
    options: &SearchOptions<'_>,
) -> Result<Value> {
    let SearchOptions {
        kind,
        path,
        include_docs,
        exact,
        page: options,
    } = *options;
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
    let graph_boost = "CASE WHEN EXISTS(SELECT 1 FROM edges e WHERE e.dst_hash=n.node_hash AND e.kind NOT IN('contains','documents','references_doc')) THEN 50 ELSE 0 END";
    let mut nodes = if exact {
        let fts_query = pattern
            .split(|c: char| !c.is_alphanumeric())
            .filter(|part| !part.is_empty())
            .map(|part| format!("\"{part}\""))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!("SELECT {columns},0 AS match_rank,0.0 AS bm25_rank,0 AS graph_boost FROM node_search JOIN nodes n ON n.node_id=node_search.rowid WHERE node_search MATCH ?1 AND (n.name=?2 COLLATE NOCASE OR n.qualname=?2 COLLATE NOCASE) AND (?3='' OR n.kind=?3 OR (?3='method' AND n.kind='function') OR (?3='function' AND n.kind='method')) AND (?4='' OR n.path=?4 OR (n.path>=?5 AND n.path<?6)) AND (?7=1 OR n.language!='markdown') ORDER BY n.path,n.line,n.id");
        paging::query(
            conn,
            &sql,
            &[
                &fts_query,
                &pattern,
                &kind,
                &path,
                &path_start,
                &path_end,
                &include_docs,
            ],
            options,
        )?
    } else if pattern.contains('*') {
        let like = pattern
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
            .replace('*', "%");
        if let Some(prefix) = pattern
            .strip_suffix('*')
            .filter(|p| !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || c == '_'))
        {
            let query = format!("\"{prefix}\"*");
            let from = "FROM node_search JOIN nodes n ON n.node_id=node_search.rowid WHERE node_search MATCH ?3 AND (n.name LIKE ?1 ESCAPE '\\' OR n.qualname LIKE ?1 ESCAPE '\\' OR n.qualname LIKE '%::' || ?1 ESCAPE '\\' OR n.qualname LIKE '%.' || ?1 ESCAPE '\\') AND (?2='' OR n.kind=?2 OR (?2='method' AND n.kind='function') OR (?2='function' AND n.kind='method')) AND (?4='' OR n.path=?4 OR (n.path>=?5 AND n.path<?6)) AND (?7=1 OR n.language!='markdown')";
            let probe = format!("SELECT n.node_id {from} LIMIT 1001");
            let sql = format!(
                "WITH fts_scored AS MATERIALIZED (
                    SELECT n.node_id,n.path,n.line,n.id,(CASE WHEN n.name LIKE ?1 ESCAPE '\\' THEN 500 ELSE 0 END - bm25(node_search)) AS score
                    {from}
                    ORDER BY score DESC,n.path,n.line,n.id
                ), total_probe AS MATERIALIZED ({probe}), result_total AS (
                    SELECT CASE WHEN (SELECT count(*) FROM total_probe)>=1001 THEN NULL
                        ELSE (SELECT count(*) FROM total_probe) END AS search_total
                ), graph_window AS MATERIALIZED (
                    SELECT node_id,score FROM fts_scored ORDER BY score DESC,path,line,id LIMIT 50
                ), boosted AS MATERIALIZED (
                    SELECT p.node_id,{graph_boost} AS graph_boost
                    FROM graph_window p JOIN nodes n ON n.node_id=p.node_id
                ), ranked AS (
                    SELECT f.node_id,f.score,CASE WHEN b.node_id IS NULL THEN 1 ELSE 0 END AS graph_group,
                           COALESCE(b.graph_boost,0) AS graph_boost,t.search_total
                    FROM fts_scored f LEFT JOIN boosted b ON b.node_id=f.node_id CROSS JOIN result_total t
                ) SELECT {columns},CASE WHEN n.name LIKE ?1 ESCAPE '\\' THEN 'name_pattern' ELSE 'qualified_pattern' END match_reason,c.graph_boost,c.search_total
                FROM ranked c JOIN nodes n ON n.node_id=c.node_id
                ORDER BY c.graph_group,(c.score+c.graph_boost) DESC,n.path,n.line,n.id"
            );
            ranked_search_page(
                conn,
                &sql,
                &[
                    &like,
                    &kind,
                    &query,
                    &path,
                    &path_start,
                    &path_end,
                    &include_docs,
                ],
                options,
            )?
        } else {
            paging::query(conn, &format!("SELECT {columns},CASE WHEN n.name LIKE ?1 ESCAPE '\\' THEN 'name_pattern' ELSE 'qualified_pattern' END match_reason FROM nodes n WHERE (n.name LIKE ?1 ESCAPE '\\' OR n.qualname LIKE ?1 ESCAPE '\\') AND (?2='' OR n.kind=?2 OR (?2='method' AND n.kind='function') OR (?2='function' AND n.kind='method')) AND (?3='' OR n.path=?3 OR (n.path>=?4 AND n.path<?5)) AND (?6=1 OR n.language!='markdown') ORDER BY CASE WHEN n.name LIKE ?1 ESCAPE '\\' THEN 500 ELSE 0 END DESC,n.path,n.line,n.id"), &[&like,&kind,&path,&path_start,&path_end,&include_docs], options)?
        }
    } else {
        let query = pattern
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(" OR ");
        if query.is_empty() {
            bail!("pattern must contain a symbol fragment");
        }
        let escaped = pattern
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let prefix = format!("{escaped}%");
        let candidate_limit = i64::try_from(
            options
                .offset
                .saturating_add(options.limit)
                .saturating_add(1)
                .max(500),
        )
        .context("search offset exceeds SQLite integer range")?;
        let structural_candidates = "SELECT node_id FROM nodes WHERE name LIKE ?7 ESCAPE '\\' UNION SELECT node_id FROM nodes WHERE qualname=?6 COLLATE NOCASE";
        let filters = "(?2='' OR n.kind=?2 OR (?2='method' AND n.kind='function') OR (?2='function' AND n.kind='method')) AND (?3='' OR n.path=?3 OR (n.path>=?4 AND n.path<?5)) AND (?8=1 OR n.language!='markdown')";
        let structural_score = "CASE WHEN n.name=?6 COLLATE NOCASE OR n.qualname=?6 COLLATE NOCASE THEN 1000 WHEN n.name LIKE ?7 ESCAPE '\\' THEN 500 ELSE 0 END";
        let fts_score = "CASE WHEN n.name=?6 COLLATE NOCASE OR n.qualname=?6 COLLATE NOCASE THEN 1000 WHEN n.name LIKE ?7 ESCAPE '\\' THEN 500 ELSE 0 END - bm25(node_search)";
        let sql = format!(
            "WITH
            fts AS MATERIALIZED (
                SELECT n.node_id,n.path,n.line,n.id,({fts_score}) AS score
                FROM node_search JOIN nodes n ON n.node_id=node_search.rowid
                WHERE node_search MATCH ?1 AND {filters}
                ORDER BY score DESC,n.path,n.line,n.id LIMIT ?9
            ), fts_probe AS MATERIALIZED (
                SELECT n.node_id FROM node_search JOIN nodes n ON n.node_id=node_search.rowid
                WHERE node_search MATCH ?1 AND {filters} LIMIT 1001
            ), structural_all AS MATERIALIZED (
                SELECT n.node_id,n.path,n.line,n.id,({structural_score}) AS score
                FROM ({structural_candidates}) s JOIN nodes n ON n.node_id=s.node_id
                WHERE {filters}
            ), structural AS MATERIALIZED (
                SELECT node_id,path,line,id,score FROM structural_all
                ORDER BY score DESC,path,line,id LIMIT ?9
            ), candidates AS (
                SELECT node_id,max(score) AS score FROM (
                    SELECT node_id,score FROM fts UNION ALL SELECT node_id,score FROM structural
                ) GROUP BY node_id
            ), result_total AS (
                SELECT CASE WHEN (SELECT count(*) FROM fts_probe)>=1001 THEN NULL
                    ELSE (SELECT count(*) FROM (
                        SELECT node_id FROM fts_probe UNION SELECT node_id FROM structural_all
                    )) END AS search_total
            ), graph_window AS MATERIALIZED (
                SELECT c.node_id,c.score FROM candidates c JOIN nodes n ON n.node_id=c.node_id
                ORDER BY c.score DESC,n.path,n.line,n.id LIMIT 50
            ), boosted AS MATERIALIZED (
                SELECT p.node_id,{graph_boost} AS graph_boost
                FROM graph_window p JOIN nodes n ON n.node_id=p.node_id
            ), ranked AS (
                SELECT c.node_id,c.score,CASE WHEN b.node_id IS NULL THEN 1 ELSE 0 END AS graph_group,
                       COALESCE(b.graph_boost,0) AS graph_boost,t.search_total
                FROM candidates c LEFT JOIN boosted b ON b.node_id=c.node_id CROSS JOIN result_total t
            ) SELECT {columns},CASE WHEN n.name=?6 COLLATE NOCASE THEN 0
                WHEN n.qualname=?6 COLLATE NOCASE THEN 2
                WHEN n.name LIKE ?7 ESCAPE '\\' THEN 1
                WHEN n.name LIKE '%' || ?7 ESCAPE '\\' THEN 3 ELSE 4 END AS match_rank
            ,c.graph_boost,c.search_total FROM ranked c JOIN nodes n ON n.node_id=c.node_id
            ORDER BY c.graph_group,(c.score+c.graph_boost) DESC,n.path,n.line,n.id"
        );
        ranked_search_page(
            conn,
            &sql,
            &[
                &query,
                &kind,
                &path,
                &path_start,
                &path_end,
                &pattern,
                &prefix,
                &include_docs,
                &candidate_limit,
            ],
            options,
        )?
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
                    Some(2) => "exact_qualified_name",
                    Some(3) => "name_fragment",
                    _ => "indexed_text",
                };
                if options.detail == crate::core::response::Detail::Full
                    || !matches!(rank.as_u64(), Some(0..=1 | 3))
                {
                    item["match_reason"] = json!(reason);
                }
                item.as_object_mut()
                    .context("invalid search result")?
                    .remove("bm25_rank");
                item.as_object_mut()
                    .context("invalid search result")?
                    .remove("graph_boost");
            }
        }
    }
    Ok(json!({"pattern":pattern,"nodes":nodes}))
}

fn ranked_search_page(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
    options: &QueryOptions,
) -> Result<Value> {
    let generation = paging::generation(conn, options)?;
    let limit = i64::try_from(options.limit.saturating_add(1))
        .context("search limit exceeds SQLite integer range")?;
    let offset = options.offset as i64;
    let mut bindings = params.to_vec();
    bindings.extend([&limit as &dyn rusqlite::ToSql, &offset]);
    let limit_param = params.len() + 1;
    let mut items = reader::rows(
        conn,
        &format!("{sql} LIMIT ?{} OFFSET ?{}", limit_param, limit_param + 1),
        &bindings,
        options.limit.saturating_add(1),
    )?;
    let total = items
        .first()
        .and_then(|item| item.get("search_total"))
        .cloned()
        .unwrap_or_else(|| {
            if options.offset == 0 && items.is_empty() {
                json!(0)
            } else {
                Value::Null
            }
        });
    for item in &mut items {
        item.as_object_mut()
            .context("invalid search result")?
            .remove("search_total");
    }
    let has_more = items.len() > options.limit;
    items.truncate(options.limit);
    Ok(json!({
        "total": total,
        "offset": options.offset,
        "limit": options.limit,
        "has_more": has_more,
        "next_offset": if has_more { Some(options.offset.saturating_add(items.len())) } else { None },
        "items": items,
        "generation": generation
    }))
}

/// Performs tests paged.
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
    let id_hash = stable_hash64(id);
    admit_test_mapping(conn, id, inbound)?;
    let path = node["path"].as_str().unwrap_or("");
    let start_line = node["line"].as_i64().unwrap_or(0);
    let end_line = node["end_line"].as_i64().unwrap_or(i64::MAX);

    let unresolved_count: usize = conn
        .query_row(
            "SELECT count(*) FROM resolution_coverage WHERE status IN('unresolved','ambiguous') AND path_id=(SELECT path_id FROM path_dictionary WHERE path=?1) AND line BETWEEN ?2 AND ?3",
            rusqlite::params![path, start_line, end_line],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let steps = super::traversal::dependency_steps(inbound, true, false);
    let (forward_candidate, forward_selected, reverse_candidate, reverse_selected) = if inbound {
        ("src_hash", "dst_hash", "dst_hash", "src_hash")
    } else {
        ("dst_hash", "src_hash", "src_hash", "dst_hash")
    };
    let max_depth = 4;
    let sql = format!("WITH RECURSIVE seeds(id,depth) AS (SELECT ?1,0 UNION SELECT e.dst_hash,s.depth+1 FROM seeds s JOIN edges e ON e.src_hash=s.id WHERE e.kind='contains' AND s.depth<?2), walk(id,depth) AS (SELECT id,depth FROM seeds UNION {steps}), reached(id) AS (SELECT id FROM walk GROUP BY id) SELECT {} FROM reached w JOIN nodes n ON n.node_hash=w.id WHERE n.is_test=?3 AND n.kind IN ('function','method','class','struct') AND n.node_hash!=?1 ORDER BY CASE WHEN EXISTS(SELECT 1 FROM edges e WHERE e.{forward_selected}=?1 AND e.{forward_candidate}=n.node_hash AND e.kind IN({})) OR EXISTS(SELECT 1 FROM edges e WHERE e.{reverse_selected}=?1 AND e.{reverse_candidate}=n.node_hash AND e.kind IN({})) THEN 0 ELSE 1 END,n.path,n.line,n.id", paging::nodes("n", options.detail), super::traversal::FORWARD_DEPENDENCIES, super::traversal::REVERSE_DEPENDENCIES);
    let mut nodes = paging::query(conn, &sql, &[&id_hash, &max_depth, &is_test], options)?;
    let mut discovery_method = "graph";
    if inbound && nodes["total"].as_u64() == Some(0) {
        let name = node["name"].as_str().unwrap_or_default();
        let mut words = Vec::new();
        let mut current = String::new();
        for ch in name.chars() {
            if ch.is_uppercase() && !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            if ch.is_alphanumeric() {
                current.push(ch.to_ascii_lowercase());
            } else if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            words.push(current);
        }
        let snake = words.join("_");
        let lexical_query = if words.len() > 1 {
            format!(
                "\"{}\" OR ({})",
                name,
                words
                    .iter()
                    .map(|w| format!("\"{w}\""))
                    .collect::<Vec<_>>()
                    .join(" AND ")
            )
        } else {
            format!("\"{name}\"")
        };
        if !lexical_query.is_empty() {
            nodes = paging::query(
                conn,
                &format!("SELECT {} FROM node_search JOIN nodes n ON n.node_id=node_search.rowid JOIN files f ON f.path=n.path WHERE node_search MATCH ?1 AND n.is_test=1 AND f.is_test=1 AND n.kind IN('function','method','class','struct') AND n.id!=?2 AND (n.name=?3 COLLATE NOCASE OR n.name=('test_'||?3) COLLATE NOCASE OR n.name LIKE ('%'||?4||'%') OR f.path LIKE ('%'||?4||'%')) ORDER BY bm25(node_search),n.path,n.line,n.id", paging::nodes("n", options.detail)),
                &[&lexical_query, &id, &name, &snake],
                options,
            )?;
            if nodes["total"].as_u64().is_some_and(|total| total > 0) {
                discovery_method = "lexical_fallback";
            }
        }
    }
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
        json!({"selector":node,"direction":direction,"nodes":nodes,"discovery_method":discovery_method,"unresolved_references":unresolved_count,"scope":scope_note}),
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
        ("src_hash", "dst_hash", "dst_hash", "src_hash")
    } else {
        ("dst_hash", "src_hash", "src_hash", "dst_hash")
    };
    let sql = format!(
        "SELECT e.{forward_candidate} related_hash,e.kind,p.path,e.line FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE e.{forward_selected}=?1 AND e.{forward_candidate} IN ({placeholders}) AND e.kind IN({}) UNION ALL SELECT e.{reverse_candidate} related_hash,e.kind,p.path,e.line FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE e.{reverse_selected}=?1 AND e.{reverse_candidate} IN ({placeholders}) AND e.kind IN({}) ORDER BY related_hash,kind,path,line",
        super::traversal::FORWARD_DEPENDENCIES,
        super::traversal::REVERSE_DEPENDENCIES,
    );
    let selected_hash = stable_hash64(selected_id);
    let candidate_hashes: Vec<i64> = ids.iter().map(|id| stable_hash64(id)).collect();
    let bindings: Vec<&dyn rusqlite::ToSql> =
        std::iter::once(&selected_hash as &dyn rusqlite::ToSql)
            .chain(
                candidate_hashes
                    .iter()
                    .map(|hash| hash as &dyn rusqlite::ToSql),
            )
            .collect();
    // At most seven dependency kinds are persisted per selected/candidate pair.
    let direct = reader::rows(conn, &sql, &bindings, ids.len() * 7)?;
    let mut reasons = HashMap::new();
    for edge in direct {
        let related_hash = edge["related_hash"]
            .as_i64()
            .context("invalid related node hash")?;
        reasons.entry(related_hash).or_insert_with(|| {
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
            .remove(&stable_hash64(id))
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
        "SELECT n.id,n.kind,n.name,n.qualname,n.path,n.line FROM edges e JOIN nodes n ON n.node_hash=e.src_hash WHERE e.dst_hash=?1 AND e.kind='contains' ORDER BY CASE WHEN n.kind IN('module','file') THEN 1 ELSE 0 END,n.line DESC,n.end_line,n.id LIMIT 1",
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
                "SELECT id,kind,name,qualname,path,line FROM nodes WHERE path=?1 AND name=?2 COLLATE NOCASE AND name=?2 COLLATE BINARY AND kind IN('class','enum','interface','record','struct','trait','type') ORDER BY line,id LIMIT 101",
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
            "SELECT n.id,n.kind,n.name,n.qualname,n.path,n.line FROM edges e JOIN nodes n ON n.node_hash=e.src_hash WHERE e.dst_hash=?1 AND e.kind='calls' ORDER BY n.path,n.line,n.id LIMIT ?2",
        )
    } else {
        (
            "SELECT count(*),coalesce(sum(occurrence_count),0) FROM edges WHERE src_hash=?1 AND kind='calls'",
            "SELECT n.id,n.kind,n.name,n.qualname,n.path,n.line FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash WHERE e.src_hash=?1 AND e.kind='calls' ORDER BY n.path,n.line,n.id LIMIT ?2",
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
