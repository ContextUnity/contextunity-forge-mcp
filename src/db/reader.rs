use super::paging;
use crate::core::response::{Detail, QueryOptions};
use anyhow::{bail, Context, Result};
use rusqlite::{types::ValueRef, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Map, Value};
use std::path::Path;
pub fn open(path: &Path, root: &Path) -> Result<Connection> {
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        bail!("database must not be a symlink");
    }
    let before = super::cache::identity(path)?;
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    conn.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    conn.execute_batch("BEGIN DEFERRED")?;
    let version: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='schema_version'",
        [],
        |r| r.get(0),
    )?;
    let engine: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='indexer_engine'",
        [],
        |r| r.get(0),
    )?;
    if version != "2" || engine != "contextunity-forge-mcp-rust" {
        bail!("incompatible database schema or engine");
    }
    validate_workspace(&conn, root)?;
    let semantics: Option<String> = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='index_semantics_version'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if semantics.as_deref() != Some(crate::engine::scanner::INDEX_SEMANTICS_VERSION) {
        bail!("incompatible index semantics; rebuild index");
    }
    let algorithm: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='commitment_algorithm'",
        [],
        |r| r.get(0),
    )?;
    if algorithm != crate::core::commitments::ALGORITHM {
        bail!("incompatible commitment algorithm; rebuild index");
    }
    let seal: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='output_root'",
        [],
        |r| r.get(0),
    )?;
    if !super::cache::matches(path, root, &seal, &before) {
        crate::core::commitments::verify(&conn)?;
    }
    if super::cache::identity(path)? != before {
        bail!("database changed during reader admission");
    }
    conn.set_limit(
        rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH,
        8 * 1024 * 1024,
    );
    conn.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_SQL_LENGTH, 64 * 1024);
    Ok(conn)
}
pub(crate) fn validate_workspace(conn: &Connection, root: &Path) -> Result<()> {
    let stored: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='workspace_root'",
        [],
        |r| r.get(0),
    )?;
    if Path::new(&stored) != root.canonicalize()? {
        bail!("database belongs to a different workspace: {stored}");
    }
    Ok(())
}

pub(crate) struct QueryBudget<'a> {
    conn: &'a Connection,
    started: std::time::Instant,
}
impl<'a> QueryBudget<'a> {
    pub(crate) fn new(conn: &'a Connection) -> Self {
        let started = std::time::Instant::now();
        conn.progress_handler(
            1000,
            Some(move || started.elapsed() > std::time::Duration::from_secs(2)),
        );
        Self { conn, started }
    }
    pub(crate) fn check(&self) -> Result<()> {
        if self.started.elapsed() > std::time::Duration::from_secs(2) {
            bail!("graph query exceeds two second budget");
        }
        Ok(())
    }
}
impl Drop for QueryBudget<'_> {
    fn drop(&mut self) {
        self.conn.progress_handler(0, None::<fn() -> bool>);
    }
}

// SQLite BINARY ordering makes [path + '/', path + '0') a literal descendant range.
pub(crate) fn path_bounds(path: &str) -> (String, String) {
    let path = path.trim_end_matches('/');
    (format!("{path}/"), format!("{path}0"))
}

pub fn rows(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
    limit: usize,
) -> Result<Vec<Value>> {
    if limit == 0 || limit > 500000 {
        bail!("internal row budget exceeded");
    }
    if sql.len() > 64 * 1024 {
        bail!("SQL exceeds64KiB");
    }
    let _budget = QueryBudget::new(conn);
    let mut statement = conn.prepare(sql)?;
    if !statement.readonly() {
        bail!("query must be read-only");
    }
    let names: Vec<_> = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut query = statement.query(params)?;
    let mut result = Vec::new();
    let mut total_bytes = 0usize;
    while let Some(row) = query.next()? {
        let mut value = Map::new();
        for (i, name) in names.iter().enumerate() {
            let raw = row.get_ref(i)?;
            total_bytes = total_bytes.saturating_add(
                name.len()
                    + match raw {
                        ValueRef::Text(v) | ValueRef::Blob(v) => v.len().saturating_mul(6),
                        _ => 32,
                    },
            );
            if total_bytes > 8 * 1024 * 1024 {
                bail!("query result exceeds the 8 MiB row budget; narrow the selector or path, request detail='compact' with a smaller limit, or select fewer and smaller SQL columns. Continue paged requests with the returned offset and generation");
            }
            let cell = match raw {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(v) => json!(v),
                ValueRef::Real(v) => json!(v),
                ValueRef::Text(v) => {
                    let s = String::from_utf8_lossy(v);
                    if matches!(
                        name.as_str(),
                        "details" | "invariants" | "referenced_symbols"
                    ) {
                        serde_json::from_str(&s).unwrap_or_else(|_| json!(s))
                    } else {
                        json!(s)
                    }
                }
                ValueRef::Blob(v) => json!(hex::encode(v)),
            };
            value.insert(name.clone(), cell);
        }
        result.push(Value::Object(value));
        if result.len() >= limit {
            break;
        }
    }
    Ok(result)
}
pub fn select(conn: &Connection, selector: &str) -> Result<Value> {
    select_detail(conn, selector, Detail::Full)
}
pub(crate) fn select_detail(conn: &Connection, selector: &str, detail: Detail) -> Result<Value> {
    if selector.trim().is_empty() {
        bail!("selector is empty");
    }
    let exact = rows(
        conn,
        "SELECT id FROM nodes WHERE id=?1 OR qualname=?1 ORDER BY id LIMIT 101",
        &[&selector],
        101,
    )?;
    let result = if exact.is_empty() {
        let (kind, name) = selector.split_once(':').unwrap_or(("", selector));
        rows(
            conn,
            "SELECT id FROM nodes WHERE ((name=?1 OR qualname=?1) AND (?2='' OR kind=?2)) OR path=?3 ORDER BY id LIMIT 101",
            &[&name, &kind, &selector],
            101,
        )?
    } else {
        exact
    };
    match result.len() {
        0 => bail!("selector not found: {selector}; use code_map_search to find an indexed symbol id, code_map_overview to inspect indexed paths, or get_doc for Markdown files. File paths are passed without a file: prefix"),
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
        _ => bail!(
            "ambiguous selector {selector}; use an exact node id; candidates: {}",
            serde_json::to_string(
                &result
                    .iter()
                    .filter_map(|v| v["id"].as_str())
                    .collect::<Vec<_>>()
            )?
        ),
    }
}
pub fn overview(conn: &Connection) -> Result<Value> {
    Ok(
        json!({"components":rows(conn,"SELECT id,name,path FROM nodes WHERE kind='component' ORDER BY path",&[],10000)?,"counts":rows(conn,"SELECT (SELECT count(*)FROM files)files,(SELECT count(*)FROM nodes)nodes,(SELECT count(*)FROM edges)edges,(SELECT count(*)FROM doc_sections)doc_sections,(SELECT count(*)FROM errors)parse_errors,(SELECT count(*)FROM resolution_coverage WHERE status IN('unresolved','ambiguous'))unresolved,(SELECT count(*)FROM resolution_coverage WHERE status='external')external_imports",&[],1)?,"languages":rows(conn,"SELECT language,count(*)files FROM files GROUP BY language ORDER BY language",&[],100)?,"generation":rows(conn,"SELECT key,value FROM metadata WHERE key IN('schema_version','output_root','corpus_hash','workspace_root')ORDER BY key",&[],10)?}),
    )
}
pub fn inspect(conn: &Connection, selector: &str, show_doc: bool) -> Result<Value> {
    let node = select(conn, selector)?;
    let id = node["id"].as_str().context("invalid node id")?;
    let docs = if show_doc {
        rows(conn,"SELECT d.* FROM doc_sections d JOIN edges e ON e.dst_public_id=d.doc_id WHERE e.src_public_id=?1 AND e.kind='references_doc' ORDER BY d.is_invariant DESC,d.path,d.doc_id",&[&id],1000)?
    } else {
        Vec::new()
    };
    Ok(
        json!({"node":node,"documents":docs,"coverage":rows(conn,"SELECT * FROM resolution_coverage WHERE path=?1 AND line BETWEEN ?2 AND ?3 ORDER BY line",&[&node["path"].as_str().unwrap_or(""),&node["line"].as_i64().unwrap_or(0),&node["end_line"].as_i64().unwrap_or(i64::MAX)],1000)?}),
    )
}
pub fn explain(conn: &Connection, selector: &str) -> Result<Value> {
    let mut result = inspect(conn, selector, true)?;
    let id = result["node"]["id"]
        .as_str()
        .context("invalid node id")?
        .to_owned();
    result["incoming"] = json!(rows(
        conn,
        "SELECT * FROM edges WHERE dst_public_id=?1 ORDER BY kind,src_public_id",
        &[&id],
        1000
    )?);
    result["outgoing"] = json!(rows(
        conn,
        "SELECT * FROM edges WHERE src_public_id=?1 ORDER BY kind,dst_public_id",
        &[&id],
        1000
    )?);
    Ok(result)
}
pub fn search_docs(
    conn: &Connection,
    query: &str,
    doc_type: Option<&str>,
    component: Option<&str>,
    limit: usize,
) -> Result<Value> {
    validate_limit(limit)?;
    let query = query
        .split_whitespace()
        .map(|s| format!("\"{}\"", s.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ");
    if query.is_empty() {
        bail!("search query is empty");
    }
    let kind = doc_type.unwrap_or("");
    let component = component.unwrap_or("").trim_end_matches('/');
    let (prefix, end) = path_bounds(component);
    Ok(
        json!({"sections":rows(conn,"SELECT d.*,bm25(doc_search)rank FROM doc_search JOIN doc_sections d ON d.rowid=doc_search.rowid WHERE doc_search MATCH ?1 AND (?2='' OR d.doc_type=?2)AND(?3='' OR d.path=?3 OR (d.path>=?4 AND d.path<?5))ORDER BY rank,d.path LIMIT ?6",&[&query,&kind,&component,&prefix,&end,&(limit as i64)],limit)?}),
    )
}
pub fn get_doc(conn: &Connection, path: &str, section: Option<&str>) -> Result<Value> {
    let section = section.unwrap_or("");
    let result=rows(conn,"SELECT * FROM doc_sections WHERE (doc_id=?1 OR path=?1)AND(?2='' OR section_title=?2)ORDER BY rowid",&[&path,&section],10000)?;
    if result.is_empty() {
        bail!("document or section not found");
    }
    Ok(json!({"sections":result}))
}
pub fn analyze(conn: &Connection, target: &str) -> Result<Value> {
    let first = target
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let path = target.trim_end_matches('/');
    let (prefix, end) = path_bounds(path);
    let indexed_path: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM nodes WHERE path=?1 OR (path>=?2 AND path<?3))",
        rusqlite::params![path, prefix, end],
        |r| r.get(0),
    )?;
    if !indexed_path
        && [
            "insert", "update", "delete", "drop", "alter", "create", "replace", "attach", "detach",
            "pragma", "vacuum", "reindex", "begin", "commit", "rollback",
        ]
        .contains(&first.as_str())
    {
        bail!("write or administrative SQL is not allowed");
    }

    let sql = target.trim();
    if sql.to_ascii_lowercase().starts_with("select ")
        || sql.to_ascii_lowercase().starts_with("with ")
    {
        if sql.contains(';') {
            bail!("exactly one SELECT or WITH statement is allowed");
        }
        return Ok(json!({"rows":rows(conn,sql,&[],1000)?}));
    }
    let total_errors: usize = conn.query_row(
        "SELECT count(*) FROM errors WHERE ?1='' OR path=?1 OR (path>=?2 AND path<?3)",
        rusqlite::params![path, prefix, end],
        |r| r.get(0),
    )?;
    let mut errors = rows(
        conn,
        "SELECT * FROM errors WHERE ?1='' OR path=?1 OR (path>=?2 AND path<?3) ORDER BY path,line LIMIT 1001",
        &[&path, &prefix, &end],
        1001,
    )?;
    let errors_truncated = errors.len() > 1000;
    errors.truncate(1000);

    let total_unresolved: usize = conn.query_row(
        "SELECT count(*) FROM resolution_coverage WHERE status IN('unresolved','ambiguous') AND (?1='' OR path=?1 OR (path>=?2 AND path<?3))",
        rusqlite::params![path, prefix, end],
        |r| r.get(0),
    )?;
    let mut resolution = rows(
        conn,
        "SELECT * FROM resolution_coverage WHERE status IN('unresolved','ambiguous') AND (?1='' OR path=?1 OR (path>=?2 AND path<?3)) ORDER BY path,line LIMIT 1001",
        &[&path, &prefix, &end],
        1001,
    )?;
    let resolution_truncated = resolution.len() > 1000;
    resolution.truncate(1000);
    let total_external_imports: usize = conn.query_row(
        "SELECT count(*) FROM resolution_coverage WHERE status='external' AND (?1='' OR path=?1 OR (path>=?2 AND path<?3))",
        rusqlite::params![path, prefix, end],
        |r| r.get(0),
    )?;

    let cycles =
        crate::db::traversal::cycles(conn, if path.is_empty() { None } else { Some(path) })?;

    Ok(json!({
        "target": target,
        "errors": errors,
        "total_errors": total_errors,
        "errors_truncated": errors_truncated,
        "resolution": resolution,
        "total_unresolved": total_unresolved,
        "total_external_imports": total_external_imports,
        "resolution_truncated": resolution_truncated,
        "truncated": errors_truncated || resolution_truncated,
        "cycles": cycles
    }))
}

pub fn validate_limit(limit: usize) -> Result<()> {
    if !(1..=10000).contains(&limit) {
        bail!("limit must be1..10000");
    }
    Ok(())
}

pub fn overview_paged(conn: &Connection, options: &QueryOptions) -> Result<Value> {
    let generation = paging::generation(conn, options)?;
    let lang_sql = "SELECT l.language,l.files,coalesce(r.resolved,0) resolved,coalesce(r.unresolved,0) unresolved,coalesce(r.external_imports,0) external_imports,coalesce(e.parse_errors,0) parse_errors FROM (SELECT language,count(*) files FROM files GROUP BY language) l LEFT JOIN (SELECT f.language,count(CASE WHEN rc.status='resolved' THEN 1 END) resolved,count(CASE WHEN rc.status IN('unresolved','ambiguous') THEN 1 END) unresolved,count(CASE WHEN rc.status='external' THEN 1 END) external_imports FROM resolution_coverage rc JOIN files f ON f.path=rc.path GROUP BY f.language) r ON r.language=l.language LEFT JOIN (SELECT f.language,count(*) parse_errors FROM errors er JOIN files f ON f.path=er.path GROUP BY f.language) e ON e.language=l.language ORDER BY l.language";
    let compiled_profiles: std::collections::BTreeSet<_> = crate::engine::languages::profiles()
        .map(|p| p.id())
        .collect();
    Ok(json!({
        "generation": generation,
        "components": paging::query(conn, "SELECT id,name,path FROM nodes WHERE kind='component' ORDER BY path,id", &[], options)?,
        "counts": rows(conn,"SELECT (SELECT count(*) FROM files) files,(SELECT count(*) FROM nodes) nodes,(SELECT count(*) FROM edges) edges,(SELECT count(*) FROM doc_sections) doc_sections,(SELECT count(*) FROM errors) parse_errors,(SELECT count(*) FROM resolution_coverage WHERE status IN('unresolved','ambiguous')) unresolved,(SELECT count(*) FROM resolution_coverage WHERE status='external') external_imports", &[], 1)?.remove(0),
        "languages": paging::query(conn, lang_sql, &[], options)?,
        "compiled_profiles": compiled_profiles,
        "metadata": rows(conn,"SELECT key,value FROM metadata WHERE key IN('schema_version','output_root','corpus_hash','workspace_root') ORDER BY key", &[], 4)?
    }))
}

pub fn inspect_paged(
    conn: &Connection,
    selector: &str,
    show_doc: bool,
    options: &QueryOptions,
) -> Result<Value> {
    let generation = paging::generation(conn, options)?;
    let node = select_detail(conn, selector, options.detail)?;
    let id = node["id"].as_str().context("invalid node id")?;
    let documents = if show_doc {
        paging::query(conn, &format!("SELECT {} FROM doc_sections d JOIN edges e ON e.dst_public_id=d.doc_id WHERE e.src_public_id=?1 AND e.kind='references_doc' ORDER BY d.is_invariant DESC,d.path,d.doc_id", paging::docs("d", options.detail)), &[&id], options)?
    } else {
        paging::value(Vec::new(), 0, options, &generation)
    };
    let coverage = paging::query(conn, &format!("SELECT {} FROM resolution_coverage c WHERE c.path=?1 AND c.line BETWEEN ?2 AND ?3 ORDER BY c.line,c.expression,c.status,c.evidence", paging::coverage("c", options.detail)), &[&node["path"].as_str().unwrap_or(""), &node["line"].as_i64().unwrap_or(0), &node["end_line"].as_i64().unwrap_or(i64::MAX)], options)?;
    Ok(json!({"node":node, "documents":documents, "coverage":coverage, "generation":generation}))
}

pub fn explain_paged(
    conn: &Connection,
    selector: &str,
    direction: Option<&str>,
    options: &QueryOptions,
) -> Result<Value> {
    let mut result = inspect_paged(conn, selector, true, options)?;
    let id = result["node"]["id"]
        .as_str()
        .context("invalid node id")?
        .to_owned();
    let dir = direction.unwrap_or("both");
    match dir {
        "both" | "incoming" => {
            result["incoming"] = paging::query(conn, &format!("SELECT {} FROM edges e WHERE e.dst_public_id=?1 ORDER BY e.kind,e.src_public_id,e.edge_id", paging::edges("e", options.detail)), &[&id], options)?;
        }
        "outgoing" => {
            let total = paging::count(
                conn,
                "SELECT count(*) FROM edges WHERE dst_public_id=?1",
                &[&id],
            )?;
            result["incoming"] = json!({"total": total, "omitted": true, "hint": "Pass direction='incoming' to page incoming edges."});
        }
        _ => bail!("direction must be both, incoming, or outgoing"),
    }
    match dir {
        "both" | "outgoing" => {
            result["outgoing"] = paging::query(conn, &format!("SELECT {} FROM edges e WHERE e.src_public_id=?1 ORDER BY e.kind,e.dst_public_id,e.edge_id", paging::edges("e", options.detail)), &[&id], options)?;
        }
        "incoming" => {
            let total = paging::count(
                conn,
                "SELECT count(*) FROM edges WHERE src_public_id=?1",
                &[&id],
            )?;
            result["outgoing"] = json!({"total": total, "omitted": true, "hint": "Pass direction='outgoing' to page outgoing edges."});
        }
        _ => {}
    }
    result["direction"] = json!(dir);
    Ok(result)
}

pub fn search_docs_paged(
    conn: &Connection,
    query: &str,
    doc_type: Option<&str>,
    component: Option<&str>,
    options: &QueryOptions,
) -> Result<Value> {
    let query = query
        .split_whitespace()
        .map(|s| format!("\"{}\"", s.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ");
    if query.is_empty() {
        bail!("search query is empty");
    }
    let kind = doc_type.unwrap_or("");
    let component = component.unwrap_or("").trim_end_matches('/');
    let (prefix, end) = path_bounds(component);
    let sql = format!("SELECT {},bm25(doc_search) rank FROM doc_search JOIN doc_sections d ON d.rowid=doc_search.rowid WHERE doc_search MATCH ?1 AND (?2='' OR d.doc_type=?2) AND (?3='' OR d.path=?3 OR (d.path>=?4 AND d.path<?5)) ORDER BY rank,d.path,d.doc_id", paging::docs("d", options.detail));
    Ok(
        json!({"sections":paging::query(conn, &sql, &[&query,&kind,&component,&prefix,&end], options)?}),
    )
}

pub fn get_doc_paged(
    conn: &Connection,
    path: &str,
    section: Option<&str>,
    options: &QueryOptions,
) -> Result<Value> {
    let section = section.unwrap_or("");
    let sections = paging::query(conn, &format!("SELECT {} FROM doc_sections d WHERE (d.doc_id=?1 OR d.path=?1) AND (?2='' OR d.section_title=?2) ORDER BY d.path,d.rowid", paging::docs("d", options.detail)), &[&path,&section], options)?;
    if sections["total"] == 0 {
        bail!("document or section not found");
    }
    Ok(json!({"sections":sections}))
}

pub fn analyze_paged(
    conn: &Connection,
    target: &str,
    include_cycles: Option<bool>,
    options: &QueryOptions,
) -> Result<Value> {
    let generation = paging::generation(conn, options)?;
    let sql = target.trim();
    let first = sql
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(first.as_str(), "select" | "with") {
        if sql.contains(';') {
            bail!("exactly one SELECT or WITH statement is allowed");
        }
        let statement = conn.prepare(sql)?;
        if !statement.readonly() {
            bail!("query must be read-only");
        }
        return Ok(
            json!({"rows":paging::query(conn, sql, &[], options)?, "ordering":"SQL order is preserved; include a deterministic ORDER BY for stable pagination."}),
        );
    }
    let path = target.trim_end_matches('/');
    let (prefix, end) = path_bounds(path);
    let indexed_path: bool = path.is_empty()
        || conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM files WHERE path=?1 OR (path>=?2 AND path<?3))",
            rusqlite::params![path, prefix, end],
            |r| r.get(0),
        )?;
    if !indexed_path && matches!(path, "diagnostics" | "cycles") {
        bail!("analysis target '{path}' is interpreted as a path, not an analysis mode. Use target='' for workspace diagnostics; pass include_cycles=true to compute cycles for the workspace or a narrower indexed path");
    }
    if !indexed_path
        && [
            "insert", "update", "delete", "drop", "alter", "create", "replace", "attach", "detach",
            "pragma", "vacuum", "reindex", "begin", "commit", "rollback",
        ]
        .contains(&first.as_str())
    {
        bail!("write or administrative SQL is not allowed");
    }
    let params: &[&dyn rusqlite::ToSql] = &[&path, &prefix, &end];
    let scope = "(?1='' OR path=?1 OR (path>=?2 AND path<?3))";
    let total_errors = paging::count(
        conn,
        &format!("SELECT count(*) FROM errors WHERE {scope}"),
        params,
    )?;
    let total_unresolved = paging::count(
        conn,
        &format!("SELECT count(*) FROM resolution_coverage WHERE status IN('unresolved','ambiguous') AND {scope}"),
        params,
    )?;
    let total_external_imports = paging::count(
        conn,
        &format!("SELECT count(*) FROM resolution_coverage WHERE status='external' AND {scope}"),
        params,
    )?;
    let exact_file: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM files WHERE path=?1)",
        [path],
        |r| r.get(0),
    )?;
    let mut result = json!({"target":target,"generation":generation,"total_errors":total_errors,"total_unresolved":total_unresolved,"total_external_imports":total_external_imports,"scope":if exact_file {"file"} else {"summary"}});
    if exact_file {
        let error_columns = if options.detail == Detail::Full {
            "*"
        } else {
            "path,line,substr(message,1,512) message,length(message)>512 message_truncated"
        };
        result["errors"] = paging::query(
            conn,
            &format!(
                "SELECT {error_columns} FROM errors WHERE path=?1 ORDER BY line,message,rowid"
            ),
            &[&path],
            options,
        )?;
        result["resolution"] = paging::query(conn, &format!("SELECT {} FROM resolution_coverage c WHERE c.status IN('unresolved','ambiguous') AND c.path=?1 ORDER BY c.line,c.expression,c.status,c.evidence", paging::coverage("c", options.detail)), &[&path], options)?;
        result["external_imports"] = paging::query(conn, &format!("SELECT {} FROM resolution_coverage c WHERE c.status='external' AND c.path=?1 ORDER BY c.line,c.expression,c.evidence", paging::coverage("c", options.detail)), &[&path], options)?;
    } else {
        result["top_files"] = json!(rows(conn, &format!("SELECT path,sum(parse_errors) parse_errors,sum(unresolved) unresolved FROM (SELECT path,count(*) parse_errors,0 unresolved FROM errors WHERE {scope} GROUP BY path UNION ALL SELECT path,0 parse_errors,count(*) unresolved FROM resolution_coverage WHERE status IN('unresolved','ambiguous') AND {scope} GROUP BY path) GROUP BY path ORDER BY sum(parse_errors)+sum(unresolved) DESC,path LIMIT 5"), params, 5)?);
        result["resolution_statuses"] = json!(rows(conn, &format!("SELECT status,count(*) total FROM resolution_coverage WHERE {scope} GROUP BY status ORDER BY status LIMIT 20"), params, 20)?);
        result["continuation_hint"] = json!(
            "Call code_map_analyze with an exact path from top_files to page its diagnostics."
        );
    }
    let compute_cycles = include_cycles.unwrap_or(false);
    result["cycles"] = if compute_cycles {
        super::cycles::summary(conn, if path.is_empty() { None } else { Some(path) })?
    } else {
        json!({"omitted": true, "hint": "Pass include_cycles=true to compute cyclic dependencies."})
    };
    Ok(result)
}
