use crate::core::response::QueryOptions;
use crate::engine::{ast, scanner};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
pub fn search(
    root: &Path,
    pattern: &str,
    language: &str,
    path: Option<&str>,
    limit: usize,
) -> Result<Value> {
    crate::engine::languages::require(language)?;
    if limit == 0 || limit > 10000 {
        bail!("limit must be1..10000");
    }
    let root = scanner::canonical_root(root)?;
    let adapter = scanner::load_adapter(&root, None)?;
    let scan = scanner::scan_with_adapter(&root, &adapter)?;
    if let Some(path) = path {
        scanner::checked_child(&root, Path::new(path))?;
    }
    let prefix = path.map(|p| format!("{}/", p.trim_end_matches('/')));
    let mut matches = Vec::new();
    for file in scan.entries {
        if file.language != language {
            continue;
        }
        if path.is_some_and(|p| {
            file.path != p && !file.path.starts_with(prefix.as_deref().unwrap_or(""))
        }) {
            continue;
        }
        let source =
            std::fs::read_to_string(scanner::checked_child(&root, Path::new(&file.path))?)?;
        matches.extend(ast::search(
            &source,
            &file.path,
            language,
            pattern,
            limit - matches.len(),
        )?);
        if matches.len() >= limit {
            break;
        }
    }
    Ok(json!({"matches":matches,"limit":limit,"truncated":matches.len()>=limit}))
}

pub fn search_paged(
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
    let mut scan = scanner::scan_with_adapter(&root, &adapter)?;
    scan.entries
        .sort_by(|left, right| left.path.cmp(&right.path));
    let mut hash = Sha256::new();
    hash.update(scanner::INDEX_SEMANTICS_VERSION);
    hash.update(&adapter.digest);
    for file in &scan.entries {
        hash.update(file.path.as_bytes());
        hash.update([0]);
        hash.update(file.digest.as_bytes());
    }
    let generation = format!("{:x}", hash.finalize());
    anyhow::ensure!(
        options
            .generation
            .as_ref()
            .is_none_or(|old| old == &generation),
        "source generation changed; restart AST search at offset 0 without generation"
    );
    if let Some(path) = path {
        scanner::resolve_file_path(&root, &adapter, path)?;
    }
    let prefix = path.map(|p| format!("{}/", p.trim_end_matches('/')));
    let mut items = Vec::new();
    let mut matched = 0;
    let mut complete = true;
    let mut work_limited = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    for file in scan.entries {
        if file.language != language
            || path.is_some_and(|p| {
                file.path != p && !file.path.starts_with(prefix.as_deref().unwrap_or(""))
            })
        {
            continue;
        }
        if std::time::Instant::now() >= deadline {
            complete = false;
            work_limited = true;
            break;
        }
        let source =
            std::fs::read_to_string(scanner::resolve_file_path(&root, &adapter, &file.path)?)?;
        anyhow::ensure!(
            scanner::digest(source.as_bytes()) == file.digest,
            "source changed during AST query; retry"
        );
        let remaining = limit - items.len();
        if remaining == 0 {
            complete = false;
            break;
        }
        let page = ast::search_page(
            &source,
            &file.path,
            language,
            pattern,
            options.offset.saturating_sub(matched),
            remaining,
            deadline,
        )?;
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
    } else if has_more {
        Some("Repeat ast_grep_search with the same language, path and pattern, next_offset as offset, and this generation.")
    } else {
        None
    };
    Ok(
        json!({"generation":generation,"matches":{"total":if complete {Some(matched)} else {None},"total_status":if complete {"exact"} else {"unavailable"},"offset":options.offset,"limit":limit,"items":items,"has_more":has_more,"next_offset":if can_continue {Some(next)} else {None},"generation":generation,"computation_truncated":work_limited,"continuation_hint":hint}}),
    )
}
