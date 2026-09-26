use crate::engine::{ast, scanner};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::path::Path;
pub fn search(
    root: &Path,
    pattern: &str,
    language: &str,
    path: Option<&str>,
    limit: usize,
) -> Result<Value> {
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
