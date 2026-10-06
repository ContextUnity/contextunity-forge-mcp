use super::{paging, reader, SearchOptions};
use crate::core::response::QueryOptions;
use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use serde_json::{json, Value};

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
    let columns = paging::nodes_with_path("n", options.detail, "pd.path");
    let graph_boost = "CASE WHEN EXISTS(SELECT 1 FROM edges e WHERE e.dst_hash=n.node_hash AND e.kind NOT IN('contains','documents','references_doc')) THEN 50 ELSE 0 END";
    let mut nodes = if exact {
        let fts_query = pattern
            .split(|c: char| !c.is_alphanumeric())
            .filter(|part| !part.is_empty())
            .map(|part| format!("\"{part}\""))
            .collect::<Vec<_>>()
            .join(" AND ");
        if fts_query.is_empty() {
            let sql = format!("SELECT {columns},0 AS match_rank,0.0 AS bm25_rank,0 AS graph_boost FROM nodes n JOIN path_dictionary pd ON pd.path_id=n.path_id WHERE (n.name=?1 OR n.qualname=?1) AND (?2='' OR n.kind=?2 OR (?2='method' AND n.kind='function') OR (?2='function' AND n.kind='method')) AND (?3='' OR pd.path=?3 OR (pd.path>=?4 AND pd.path<?5)) AND (?6=1 OR n.language!='markdown') ORDER BY pd.path,n.line,n.id");
            paging::query(
                conn,
                &sql,
                &[
                    &pattern,
                    &kind,
                    &path,
                    &path_start,
                    &path_end,
                    &include_docs,
                ],
                options,
            )?
        } else {
            let sql = format!("SELECT {columns},0 AS match_rank,0.0 AS bm25_rank,0 AS graph_boost FROM node_search JOIN nodes n ON n.node_id=node_search.rowid JOIN path_dictionary pd ON pd.path_id=n.path_id WHERE node_search MATCH ?1 AND (n.name=?2 COLLATE NOCASE OR n.qualname=?2 COLLATE NOCASE) AND (?3='' OR n.kind=?3 OR (?3='method' AND n.kind='function') OR (?3='function' AND n.kind='method')) AND (?4='' OR pd.path=?4 OR (pd.path>=?5 AND pd.path<?6)) AND (?7=1 OR n.language!='markdown') ORDER BY pd.path,n.line,n.id");
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
        }
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
            let from = "FROM node_search JOIN nodes n ON n.node_id=node_search.rowid JOIN path_dictionary pd ON pd.path_id=n.path_id WHERE node_search MATCH ?3 AND (n.name LIKE ?1 ESCAPE '\\' OR n.qualname LIKE ?1 ESCAPE '\\' OR n.qualname LIKE '%::' || ?1 ESCAPE '\\' OR n.qualname LIKE '%.' || ?1 ESCAPE '\\') AND (?2='' OR n.kind=?2 OR (?2='method' AND n.kind='function') OR (?2='function' AND n.kind='method')) AND (?4='' OR pd.path=?4 OR (pd.path>=?5 AND pd.path<?6)) AND (?7=1 OR n.language!='markdown')";
            let probe = format!("SELECT n.node_id {from} LIMIT 1001");
            let sql = format!(
                "WITH fts_scored AS MATERIALIZED (
                    SELECT n.node_id,pd.path,n.line,n.id,(CASE WHEN n.name LIKE ?1 ESCAPE '\\' THEN 500 ELSE 0 END - bm25(node_search)) AS score
                    {from}
                    ORDER BY score DESC,pd.path,n.line,n.id
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
                FROM ranked c JOIN nodes n ON n.node_id=c.node_id JOIN path_dictionary pd ON pd.path_id=n.path_id
                ORDER BY c.graph_group,(c.score+c.graph_boost) DESC,pd.path,n.line,n.id"
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
            paging::query(conn, &format!("SELECT {columns},CASE WHEN n.name LIKE ?1 ESCAPE '\\' THEN 'name_pattern' ELSE 'qualified_pattern' END match_reason FROM nodes n JOIN path_dictionary pd ON pd.path_id=n.path_id WHERE (n.name LIKE ?1 ESCAPE '\\' OR n.qualname LIKE ?1 ESCAPE '\\') AND (?2='' OR n.kind=?2 OR (?2='method' AND n.kind='function') OR (?2='function' AND n.kind='method')) AND (?3='' OR pd.path=?3 OR (pd.path>=?4 AND pd.path<?5)) AND (?6=1 OR n.language!='markdown') ORDER BY CASE WHEN n.name LIKE ?1 ESCAPE '\\' THEN 500 ELSE 0 END DESC,pd.path,n.line,n.id"), &[&like,&kind,&path,&path_start,&path_end,&include_docs], options)?
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
        let filters = "(?2='' OR n.kind=?2 OR (?2='method' AND n.kind='function') OR (?2='function' AND n.kind='method')) AND (?3='' OR pd.path=?3 OR (pd.path>=?4 AND pd.path<?5)) AND (?8=1 OR n.language!='markdown')";
        let structural_score = "CASE WHEN n.name=?6 COLLATE NOCASE OR n.qualname=?6 COLLATE NOCASE THEN 1000 WHEN n.name LIKE ?7 ESCAPE '\\' THEN 500 ELSE 0 END";
        let fts_score = "CASE WHEN n.name=?6 COLLATE NOCASE OR n.qualname=?6 COLLATE NOCASE THEN 1000 WHEN n.name LIKE ?7 ESCAPE '\\' THEN 500 ELSE 0 END - bm25(node_search)";
        let sql = format!(
            "WITH
            fts AS MATERIALIZED (
                SELECT n.node_id,pd.path,n.line,n.id,({fts_score}) AS score
                FROM node_search JOIN nodes n ON n.node_id=node_search.rowid JOIN path_dictionary pd ON pd.path_id=n.path_id
                WHERE node_search MATCH ?1 AND {filters}
                ORDER BY score DESC,pd.path,n.line,n.id LIMIT ?9
            ), fts_probe AS MATERIALIZED (
                SELECT n.node_id FROM node_search JOIN nodes n ON n.node_id=node_search.rowid JOIN path_dictionary pd ON pd.path_id=n.path_id
                WHERE node_search MATCH ?1 AND {filters} LIMIT 1001
            ), structural_all AS MATERIALIZED (
                SELECT n.node_id,pd.path,n.line,n.id,({structural_score}) AS score
                FROM ({structural_candidates}) s JOIN nodes n ON n.node_id=s.node_id JOIN path_dictionary pd ON pd.path_id=n.path_id
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
                SELECT c.node_id,c.score FROM candidates c JOIN nodes n ON n.node_id=c.node_id JOIN path_dictionary pd ON pd.path_id=n.path_id
                ORDER BY c.score DESC,pd.path,n.line,n.id LIMIT 50
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
            ,c.graph_boost,c.search_total FROM ranked c JOIN nodes n ON n.node_id=c.node_id JOIN path_dictionary pd ON pd.path_id=n.path_id
            ORDER BY c.graph_group,(c.score+c.graph_boost) DESC,pd.path,n.line,n.id"
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
    let total = if let Some(total) = items.first().and_then(|item| item.get("search_total")) {
        total.clone()
    } else if options.offset == 0 {
        json!(0)
    } else {
        let first_limit = 1_i64;
        let first_offset = 0_i64;
        let mut first_page_bindings = params.to_vec();
        first_page_bindings.extend([
            &first_limit as &dyn rusqlite::ToSql,
            &first_offset as &dyn rusqlite::ToSql,
        ]);
        let first_page = reader::rows(
            conn,
            &format!(
                "SELECT * FROM ({sql}) LIMIT ?{} OFFSET ?{}",
                params.len() + 1,
                params.len() + 2
            ),
            &first_page_bindings,
            1,
        )?;
        first_page
            .first()
            .and_then(|item| item.get("search_total"))
            .cloned()
            .unwrap_or_else(|| json!(0))
    };
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
