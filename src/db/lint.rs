use super::{paging, reader};
use crate::core::response::{Detail, QueryOptions};
use anyhow::{ensure, Result};
use rusqlite::{Connection, ToSql};
use serde_json::{json, Value};

/// Performs syntax paged.
pub fn syntax_paged(conn: &Connection, target: &str, options: &QueryOptions) -> Result<Value> {
    let generation = paging::generation(conn, options)?;
    let normalized = target.trim();
    let path = normalized.trim_end_matches('/');
    ensure!(
        !normalized.starts_with('/') && !path.split('/').any(|part| part == ".."),
        "lint target must be an indexed workspace-relative path or empty for the workspace"
    );
    let (prefix, end) = reader::path_bounds(path);
    let profiles: Vec<_> = crate::engine::languages::profiles()
        .map(|profile| profile.id())
        .collect();
    let profiles_json = serde_json::to_string(&profiles)?;
    let params: &[&dyn ToSql] = &[&path, &prefix, &end, &profiles_json];
    let scope = "(?1='' OR f.path=?1 OR (f.path>=?2 AND f.path<?3))";
    let supported = "f.language IN (SELECT value FROM json_each(?4))";
    let source_files = paging::count(
        conn,
        &format!("SELECT count(*) FROM files f WHERE {scope} AND {supported}"),
        params,
    )?;
    ensure!(source_files > 0, "lint target has no indexed source files with a compiled language profile; excluded, unsupported, and documentation files are not syntax-checked");
    let indexed_files = paging::count(
        conn,
        &format!("SELECT count(*) FROM files f WHERE {scope}"),
        &params[..3],
    )?;
    let mut languages = reader::rows(conn, &format!("SELECT f.language,count(*) files FROM files f WHERE {scope} AND {supported} GROUP BY f.language ORDER BY f.language"), params, profiles.len())?;
    for language in &mut languages {
        language["diagnostic_scope"] = json!(match language["language"].as_str() {
            Some("vue") => "embedded JavaScript/TypeScript scripts; template and style syntax are not checked",
            Some("html") if crate::engine::languages::by_id("javascript").is_some() => "HTML grammar and executable inline JavaScript; styles and template expressions are not checked",
            Some("html") => "HTML grammar only; inline JavaScript requires the JavaScript profile",
            _ => "stored Tree-sitter syntax diagnostics",
        });
    }
    let message = if options.detail == Detail::Full {
        "e.message"
    } else {
        "substr(e.message,1,512)"
    };
    let diagnostics = paging::query(conn, &format!("SELECT 'syntax.parse' rule_id,'error' severity,f.language,e.path,e.line,{message} message,{} message_truncated FROM errors e JOIN files f ON f.path=e.path WHERE {scope} AND {supported} ORDER BY e.path,e.line,e.message,e.rowid", if options.detail == Detail::Full { "0" } else { "length(e.message)>512" }), params, options)?;
    let status = if diagnostics["total"] == 0 {
        "no_stored_syntax_diagnostics"
    } else {
        "diagnostics_found"
    };
    Ok(json!({
        "target": target, "mode": "lint", "status": status,
        "diagnostics": diagnostics,
        "coverage": {
            "scope": "indexed_sources_only", "indexed_source_files": source_files,
            "other_indexed_files": indexed_files - source_files, "languages": languages,
            "unindexed_files": "not_enumerated", "checks": ["stored_parser_diagnostics"],
            "limitations": "No style, type, semantic, or security rules. Excluded and unsupported files are not checked; absence of stored diagnostics does not prove syntactic validity."
        },
        "generation": generation,
    }))
}
