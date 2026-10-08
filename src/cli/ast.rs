use crate::core::response::{Detail, QueryOptions, ResponsePolicy};
use crate::engine::{ast, scanner};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::Path;
/// Performs search.
pub fn search(
    conn: &rusqlite::Connection,
    root: &Path,
    pattern: &str,
    language: &str,
    path: Option<&str>,
    limit: usize,
) -> Result<Value> {
    if limit == 0 || limit > 10000 {
        bail!("limit must be1..10000");
    }
    let mut matches = Vec::new();
    let mut offset = 0;
    let mut generation = None;
    let mut truncated = false;
    while matches.len() < limit {
        let page_limit = (limit - matches.len()).min(100);
        let options = QueryOptions::resolve(
            &ResponsePolicy::default(),
            Some(page_limit),
            offset,
            Some(Detail::Compact),
            generation.clone(),
        )?;
        let page = search_paged(conn, root, pattern, language, path, &options)?;
        if page.get("diagnostic").is_some() {
            return Ok(page);
        }
        let payload = &page["matches"];
        if let Some(items) = payload["items"].as_array() {
            matches.extend(items.iter().cloned());
        }
        if payload["computation_truncated"] == true {
            truncated = true;
            break;
        }
        if payload["has_more"] != true {
            break;
        }
        if matches.len() >= limit {
            truncated = true;
            break;
        }
        generation = payload["generation"].as_str().map(str::to_owned);
        offset = payload["next_offset"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .context("AST continuation offset is missing")?;
    }
    Ok(json!({"matches":matches,"limit":limit,"truncated":truncated}))
}

/// Performs search paged.
pub fn search_paged(
    conn: &rusqlite::Connection,
    root: &Path,
    pattern: &str,
    language: &str,
    path: Option<&str>,
    options: &QueryOptions,
) -> Result<Value> {
    crate::engine::languages::require(language)?;
    anyhow::ensure!((1..=100).contains(&options.limit), "limit must be 1..=100");
    anyhow::ensure!(
        options.offset < ast::SEARCH_MATCH_HORIZON,
        "AST search offset must be below 10000; narrow path or pattern"
    );
    let limit = options
        .limit
        .min(ast::SEARCH_MATCH_HORIZON - options.offset);
    anyhow::ensure!(
        options.offset == 0 || options.generation.is_some(),
        "AST continuation requires generation"
    );
    let root = scanner::canonical_root(root)?;
    let adapter = scanner::load_adapter(&root, None)?;
    let generation = crate::db::paging::generation(conn, options)?;
    if let Some(path) = path {
        scanner::resolve_file_path(&root, &adapter, path)?;
    }
    let path = path.unwrap_or("");
    let (path_start, path_end) = crate::db::reader::path_bounds(path);
    let declaration = ast::pattern_symbol_name(pattern, language)?;
    let files = if let Some(name) = declaration.as_deref() {
        crate::db::reader::rows(
            conn,
            "SELECT DISTINCT f.path,f.digest FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id JOIN files f ON f.path=p.path WHERE n.name=?1 COLLATE NOCASE AND f.status='indexed' AND f.language=?2 AND (?3='' OR f.path=?3 OR (f.path>=?4 AND f.path<?5)) ORDER BY f.path",
            &[&name, &language, &path, &path_start, &path_end],
            500000,
        )?
    } else {
        crate::db::reader::rows(
        conn,
        "SELECT path,digest FROM files WHERE status='indexed' AND language=?1 AND (?2='' OR path=?2 OR (path>=?3 AND path<?4)) ORDER BY path",
        &[&language, &path, &path_start, &path_end],
        500000,
    )?
    };
    let mut items = Vec::new();
    let mut matched = 0;
    let mut complete = true;
    let mut work_limited = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    for file in files {
        let file_path = file["path"].as_str().unwrap_or_default();
        let file_digest = file["digest"].as_str().unwrap_or_default();
        if std::time::Instant::now() >= deadline {
            complete = false;
            work_limited = true;
            break;
        }
        let source =
            std::fs::read_to_string(scanner::resolve_file_path(&root, &adapter, file_path)?)?;
        anyhow::ensure!(
            scanner::digest(source.as_bytes()) == file_digest,
            "source changed during AST query; retry"
        );
        let remaining = limit - items.len();
        if remaining == 0 {
            complete = false;
            break;
        }
        let page = match ast::search_page(
            &source,
            file_path,
            language,
            pattern,
            options.offset.saturating_sub(matched),
            remaining,
            deadline,
        ) {
            Ok(page) => page,
            Err(error) => {
                if let Some(diagnostic) = error.downcast_ref::<ast::PatternSearchDiagnostic>() {
                    return Ok(json!({"diagnostic":diagnostic}));
                }
                return Err(error);
            }
        };
        matched += page.matched;
        items.extend(page.items);
        if !page.complete {
            complete = false;
            work_limited = page.work_limited;
            break;
        }
    }
    let next = options.offset + items.len();
    let has_more = !complete;
    work_limited |= has_more && next >= ast::SEARCH_MATCH_HORIZON;
    let can_continue = has_more && !work_limited;
    let hint = if work_limited {
        Some("AST computation limit reached; narrow path or pattern. Totals are unavailable and continuation cannot advance beyond this work horizon.")
    } else {
        None
    };
    Ok(
        json!({"matches":{"total":if complete {Some(matched)} else {None},"total_status":if complete {"exact"} else {"unavailable"},"offset":options.offset,"limit":limit,"items":items,"has_more":has_more,"next_offset":if can_continue {Some(next)} else {None},"generation":generation,"computation_truncated":work_limited,"continuation_hint":hint},"generation":generation}),
    )
}
