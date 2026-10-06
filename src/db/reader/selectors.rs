use super::{is_code_path, paging, rows, SelectorError};
use crate::core::response::Detail;
use anyhow::{Context, Result};
use rusqlite::Connection;
use serde_json::Value;

pub(crate) fn select_detail(conn: &Connection, selector: &str, detail: Detail) -> Result<Value> {
    let trimmed = selector.trim();
    if trimmed.is_empty() {
        return Err(SelectorError::InvalidSyntax {
            selector: selector.to_owned(),
            reason: "use an exact node id or code_map_search to find symbols".into(),
        }
        .into());
    }
    // 1. Normalize prefix: strip file:// or file: or leading ./
    let unpeeled = if let Some(stripped) = trimmed.strip_prefix("file://") {
        stripped
    } else if let Some(stripped) = trimmed.strip_prefix("file:") {
        stripped
    } else {
        trimmed
    };
    let unpeeled = unpeeled.strip_prefix("./").unwrap_or(unpeeled);

    // 1b. Check line anchor: #L123 or #123
    let (clean_selector, line_anchor) = if let Some((base, hash_part)) = unpeeled.split_once('#') {
        let line_str = hash_part.trim_start_matches('L');
        if let Ok(l) = line_str.parse::<i64>() {
            (base, Some(l))
        } else {
            (unpeeled, None)
        }
    } else {
        (unpeeled, None)
    };

    // 1c. Check path:line syntax (e.g. src/foo.rs:42)
    let (normalized, target_line) = if let Some(l) = line_anchor {
        (clean_selector, Some(l))
    } else if let Some((base, num_part)) = clean_selector.rsplit_once(':') {
        if base.contains('/') || base.contains('\\') || is_code_path(base) {
            if let Ok(l) = num_part.parse::<i64>() {
                (base, Some(l))
            } else {
                (clean_selector, None)
            }
        } else {
            (clean_selector, None)
        }
    } else {
        (clean_selector, None)
    };
    if normalized.ends_with(".md") {
        return Err(SelectorError::DocLink {
            target: normalized.to_owned(),
        }
        .into());
    }

    // 1d. If a specific line number was targeted, find the enclosing node
    if let Some(line) = target_line {
        return select_line_anchor(conn, selector, normalized, line, detail);
    }

    // 2. Exact match by id
    let exact_id = rows(
        conn,
        "SELECT id, kind FROM nodes WHERE id=?1 LIMIT 1",
        &[&normalized],
        1,
    )?;
    if !exact_id.is_empty() {
        return Ok(rows(
            conn,
            &format!(
                "SELECT {} FROM nodes n WHERE n.id=?1 LIMIT 1",
                paging::nodes("n", detail)
            ),
            &[&exact_id[0]["id"].as_str().context("invalid node id")?],
            1,
        )?
        .remove(0));
    }

    // 3. Match by path:symbol, kind:name, or name/path/module:path
    let (prefix_part, symbol_part) = if let Some((p, s)) = normalized.split_once("::") {
        (Some(p), s)
    } else if let Some((p, s)) = normalized.split_once(':') {
        (Some(p), s)
    } else {
        (None, normalized)
    };

    let mut result = if let Some(prefix) = prefix_part {
        let is_path = prefix.contains('/') || prefix.contains('\\') || is_code_path(prefix);
        let is_known_kind = matches!(
            prefix,
            "function"
                | "fn"
                | "class"
                | "module"
                | "struct"
                | "method"
                | "trait"
                | "interface"
                | "type"
                | "component"
        );
        if is_path {
            let mut candidates = rows(
                conn,
                "SELECT n.id,n.kind FROM path_dictionary p JOIN nodes n ON n.path_id=p.path_id WHERE p.path=?1 AND ((n.name=?2 COLLATE NOCASE AND n.name=?2 COLLATE BINARY) OR (n.qualname=?2 COLLATE NOCASE AND n.qualname=?2 COLLATE BINARY)) ORDER BY n.id LIMIT 101",
                &[&prefix, &symbol_part],
                101,
            )?;
            if candidates.is_empty() {
                candidates = rows(
                    conn,
                    "SELECT n.id,n.kind FROM path_dictionary p JOIN nodes n ON n.path_id=p.path_id WHERE substr(p.path,-length(?1))=?1 AND ((n.name=?2 COLLATE NOCASE AND n.name=?2 COLLATE BINARY) OR (n.qualname=?2 COLLATE NOCASE AND n.qualname=?2 COLLATE BINARY)) ORDER BY n.id LIMIT 101",
                    &[&prefix, &symbol_part],
                    101,
                )?;
            }
            if candidates.is_empty() {
                candidates = rows(
                    conn,
                    "SELECT n.id,n.kind FROM path_dictionary p JOIN nodes n ON n.path_id=p.path_id WHERE (p.path=?1 OR substr(p.path,-length(?1))=?1) AND substr(n.qualname,-length(?2))=?2 ORDER BY n.id LIMIT 101",
                    &[&prefix, &symbol_part],
                    101,
                )?;
            }
            let non_modules: Vec<_> = candidates
                .iter()
                .filter(|v| {
                    !matches!(
                        v["kind"].as_str(),
                        Some("module") | Some("file") | Some("component")
                    )
                })
                .cloned()
                .collect();
            if non_modules.is_empty() {
                candidates
            } else {
                non_modules
            }
        } else if is_known_kind {
            let kind = if prefix == "fn" { "function" } else { prefix };
            rows(
                conn,
                "SELECT id, kind FROM nodes WHERE name=?1 COLLATE NOCASE AND name=?1 COLLATE BINARY AND kind=?2 UNION SELECT id, kind FROM nodes WHERE qualname=?1 COLLATE NOCASE AND qualname=?1 COLLATE BINARY AND kind=?2 ORDER BY id LIMIT 101",
                &[&symbol_part, &kind],
                101,
            )?
        } else {
            let base_name = symbol_part.strip_prefix("./").unwrap_or(symbol_part);
            rows(
                conn,
                "SELECT id,kind FROM nodes WHERE name=?1 COLLATE NOCASE AND name=?1 COLLATE BINARY AND (?2='' OR kind=?2) UNION SELECT id,kind FROM nodes WHERE qualname=?1 COLLATE NOCASE AND qualname=?1 COLLATE BINARY AND (?2='' OR kind=?2) UNION SELECT id,kind FROM nodes WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path=?3) UNION SELECT id,kind FROM nodes WHERE id=?4 ORDER BY id LIMIT 101",
                &[&base_name, &prefix, &normalized, &format!("module:{normalized}")],
                101,
            )?
        }
    } else {
        let base_name = normalized.strip_prefix("./").unwrap_or(normalized);
        rows(
            conn,
            "SELECT id,kind FROM nodes WHERE name=?1 COLLATE NOCASE AND name=?1 COLLATE BINARY UNION SELECT id,kind FROM nodes WHERE qualname=?1 COLLATE NOCASE AND qualname=?1 COLLATE BINARY UNION SELECT id,kind FROM nodes WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path=?2) UNION SELECT id,kind FROM nodes WHERE id=?3 ORDER BY id LIMIT 101",
            &[&base_name, &normalized, &format!("module:{normalized}")],
            101,
        )?
    };

    // 4. Smart suffix fallback (for import paths like contextunity.shield.cli or Class.method like FormLoginFetcher.fetch)
    if result.is_empty() {
        let clean_target = normalized
            .trim_start_matches("module:")
            .trim_start_matches("function:")
            .trim_start_matches("class:");
        let suffix_matches = rows(
            conn,
            "SELECT n.id,n.kind FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE substr(n.qualname,-length(?1))=?1 OR substr(p.path,-length(?1))=?1 ORDER BY n.id LIMIT 101",
            &[&clean_target],
            101,
        )?;
        if suffix_matches.len() == 1 {
            result = suffix_matches;
        } else if suffix_matches.len() > 1 {
            let exact_suffix: Vec<_> = suffix_matches
                .iter()
                .filter(|v| {
                    v["id"]
                        .as_str()
                        .is_some_and(|id| id.ends_with(clean_target))
                })
                .cloned()
                .collect();
            if exact_suffix.len() == 1 {
                result = exact_suffix;
            } else {
                result = suffix_matches;
            }
        }
    }

    // 5. Disambiguate file/module paths:
    // If a file path matched all its functions and classes, but has exactly one module/file node, pick the module node!
    // CRITICAL SAFETY INVARIANT: Only disambiguate to module when the selector is clearly a path or file,
    // NEVER when it is a bare symbol name (to prevent semantic hijacking of functions/classes having the same name as a file).
    let is_path_like = normalized.contains('/')
        || normalized.contains('\\')
        || is_code_path(normalized)
        || normalized.starts_with("module:")
        || conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM files WHERE path=?1)",
                [&normalized],
                |r| r.get::<_, bool>(0),
            )
            .unwrap_or(false);

    if is_path_like && result.len() > 1 {
        let modules: Vec<_> = result
            .iter()
            .filter(|v| {
                matches!(
                    v["kind"].as_str(),
                    Some("module") | Some("file") | Some("component")
                )
            })
            .cloned()
            .collect();
        if modules.len() == 1 {
            result = modules;
        }
    }

    if result.len() > 1 {
        // Disambiguation pipeline:
        // 1. If there is a method/function and field with the same name — automatically pick method/function.
        let has_callable = result
            .iter()
            .any(|v| matches!(v["kind"].as_str(), Some("method" | "function")));
        let has_fields = result.iter().any(|v| v["kind"].as_str() == Some("field"));
        if has_callable && has_fields {
            result.retain(|v| v["kind"].as_str() != Some("field"));
        }
    }

    if result.len() > 1 {
        // Fetch full node metadata (id, kind, name, qualname, path, line, end_line, details) for candidates
        let mut enriched_nodes = Vec::new();
        for cand in &result {
            let id = cand["id"].as_str().context("invalid candidate node id")?;
            let mut row_vals = rows(
                conn,
                "SELECT n.id,n.kind,n.name,n.qualname,p.path,n.line,n.end_line,n.details FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.id=?1 LIMIT 1",
                &[&id],
                1,
            )?;
            enriched_nodes.push(
                row_vals
                    .pop()
                    .context("candidate node disappeared during selector disambiguation")?,
            );
        }

        // 2. If there is a runtime method and a stub with the same name — pick the runtime method.
        if enriched_nodes.len() > 1 {
            let non_stubs: Vec<_> = enriched_nodes
                .iter()
                .filter(|v| {
                    let details_val = match &v["details"] {
                        Value::String(s) => serde_json::from_str::<Value>(s).unwrap_or(Value::Null),
                        other => other.clone(),
                    };
                    let is_stub = details_val
                        .get("is_stub")
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false);
                    let is_overload = details_val
                        .get("is_overload")
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false);
                    !is_stub && !is_overload
                })
                .cloned()
                .collect();
            if !non_stubs.is_empty() && non_stubs.len() < enriched_nodes.len() {
                enriched_nodes = non_stubs;
            }
        }

        // 3. If selector still has duplicates in the same class — pick the runtime node with body instead of crashing.
        if enriched_nodes.len() > 1 {
            let with_body: Vec<_> = enriched_nodes
                .iter()
                .filter(|v| {
                    let line = v["line"].as_i64().unwrap_or(0);
                    let end_line = v["end_line"].as_i64().unwrap_or(0);
                    end_line > line
                })
                .cloned()
                .collect();
            if with_body.len() == 1 {
                enriched_nodes = with_body;
            } else if with_body.len() > 1 {
                let first_path = enriched_nodes[0]["path"].as_str().unwrap_or("");
                let first_qual_parent = enriched_nodes[0]["qualname"]
                    .as_str()
                    .and_then(|q| q.rsplit_once('.').map(|(p, _)| p))
                    .unwrap_or("");
                let same_class = enriched_nodes.iter().all(|n| {
                    n["path"].as_str() == Some(first_path)
                        && n["qualname"]
                            .as_str()
                            .and_then(|q| q.rsplit_once('.').map(|(p, _)| p))
                            == Some(first_qual_parent)
                });
                if same_class {
                    if let Some(best) = with_body.into_iter().max_by_key(|v| {
                        let line = v["line"].as_i64().unwrap_or(0);
                        let end_line = v["end_line"].as_i64().unwrap_or(0);
                        (end_line - line, line)
                    }) {
                        enriched_nodes = vec![best];
                    }
                }
            }
        }

        if !enriched_nodes.is_empty() {
            result = enriched_nodes;
        }
    }

    match result.len() {
        0 => Err(SelectorError::NotFound {
            selector: selector.to_owned(),
            suggestions: Vec::new(),
        }
        .into()),
        1 => Ok(rows(
            conn,
            &format!(
                "SELECT {} FROM nodes n WHERE n.id=?1 LIMIT 1",
                paging::nodes("n", detail)
            ),
            &[&result[0]["id"].as_str().context("invalid node id")?],
            1,
        )?
        .remove(0)),
        _ => Err(SelectorError::Ambiguous {
            selector: selector.to_owned(),
            candidates: result
                .iter()
                .filter_map(|value| value["id"].as_str().map(str::to_owned))
                .collect(),
        }
        .into()),
    }
}

fn select_line_anchor(
    conn: &Connection,
    selector: &str,
    normalized_path: &str,
    line: i64,
    detail: Detail,
) -> Result<Value> {
    let exact_path_match = rows(
        conn,
        "SELECT n.id FROM path_dictionary p JOIN nodes n ON n.path_id=p.path_id WHERE p.path=?1 AND n.line<=?2 AND n.end_line>=?2 ORDER BY CASE WHEN n.kind IN ('module', 'file', 'component') THEN 1 ELSE 0 END ASC, (n.end_line - n.line) ASC, n.id LIMIT 1",
        &[&normalized_path, &line],
        1,
    )?;
    if let Some(candidate) = exact_path_match.first() {
        let id = candidate["id"].as_str().context("invalid node id")?;
        return load_selected_node(conn, id, detail);
    }

    let exact_file_exists = !rows(
        conn,
        "SELECT path FROM files WHERE path=?1 LIMIT 1",
        &[&normalized_path],
        1,
    )?
    .is_empty();
    if exact_file_exists {
        return Err(SelectorError::NotFound {
            selector: selector.to_owned(),
            suggestions: Vec::new(),
        }
        .into());
    }

    // Suffix matching is a fallback only and must start at a path-component boundary.
    let suffix_paths = rows(
        conn,
        "SELECT DISTINCT p.path FROM path_dictionary p JOIN nodes n ON n.path_id=p.path_id WHERE substr(p.path,-length(?1))=?1 AND (length(p.path)=length(?1) OR substr(p.path,-length(?1)-1,1)='/') AND n.line<=?2 AND n.end_line>=?2 ORDER BY p.path LIMIT 2",
        &[&normalized_path, &line],
        2,
    )?;
    if suffix_paths.is_empty() {
        return Err(SelectorError::NotFound {
            selector: selector.to_owned(),
            suggestions: Vec::new(),
        }
        .into());
    }

    let mut candidates = Vec::with_capacity(suffix_paths.len());
    for path_row in &suffix_paths {
        let path = path_row["path"]
            .as_str()
            .context("invalid candidate path")?;
        let best_match = rows(
            conn,
            "SELECT n.id FROM path_dictionary p JOIN nodes n ON n.path_id=p.path_id WHERE p.path=?1 AND n.line<=?2 AND n.end_line>=?2 ORDER BY CASE WHEN n.kind IN ('module', 'file', 'component') THEN 1 ELSE 0 END ASC, (n.end_line - n.line) ASC, n.id LIMIT 1",
            &[&path, &line],
            1,
        )?;
        candidates.push(
            best_match
                .first()
                .and_then(|candidate| candidate["id"].as_str())
                .context("line-anchor path has no matching node")?
                .to_owned(),
        );
    }

    if candidates.len() > 1 {
        return Err(SelectorError::Ambiguous {
            selector: selector.to_owned(),
            candidates,
        }
        .into());
    }

    load_selected_node(conn, &candidates[0], detail)
}

fn load_selected_node(conn: &Connection, id: &str, detail: Detail) -> Result<Value> {
    rows(
        conn,
        &format!(
            "SELECT {} FROM nodes n WHERE n.id=?1 LIMIT 1",
            paging::nodes("n", detail)
        ),
        &[&id],
        1,
    )?
    .pop()
    .context("selected node disappeared during selector resolution")
}
