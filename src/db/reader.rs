use anyhow::{bail, Context, Result};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
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
    let stored: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='workspace_root'",
        [],
        |r| r.get(0),
    )?;
    if Path::new(&stored) != root.canonicalize()? {
        bail!("database belongs to a different workspace: {stored}");
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
    let started = std::time::Instant::now();
    conn.progress_handler(
        1000,
        Some(move || started.elapsed() > std::time::Duration::from_millis(2000)),
    );
    struct ProgressGuard<'a>(&'a Connection);
    impl Drop for ProgressGuard<'_> {
        fn drop(&mut self) {
            self.0.progress_handler(0, None::<fn() -> bool>);
        }
    }
    let _guard = ProgressGuard(conn);
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
                bail!("query result exceeds8MiB serialized bound");
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
    if selector.trim().is_empty() {
        bail!("selector is empty");
    }
    let exact = rows(
        conn,
        "SELECT * FROM nodes WHERE id=?1 OR qualname=?1",
        &[&selector],
        101,
    )?;
    let result = if exact.is_empty() {
        let (kind, name) = selector.split_once(':').unwrap_or(("", selector));
        rows(
            conn,
            "SELECT * FROM nodes WHERE ((name=?1 OR qualname=?1) AND (?2='' OR kind=?2)) OR path=?3",
            &[&name, &kind, &selector],
            101,
        )?
    } else {
        exact
    };
    match result.len() {
        0 => bail!("selector not found: {selector}"),
        1 => Ok(result[0].clone()),
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
        json!({"components":rows(conn,"SELECT id,name,path FROM nodes WHERE kind='component' ORDER BY path",&[],10000)?,"counts":rows(conn,"SELECT (SELECT count(*)FROM files)files,(SELECT count(*)FROM nodes)nodes,(SELECT count(*)FROM edges)edges,(SELECT count(*)FROM doc_sections)doc_sections,(SELECT count(*)FROM errors)parse_errors,(SELECT count(*)FROM resolution_coverage WHERE status!='resolved')unresolved",&[],1)?,"languages":rows(conn,"SELECT language,count(*)files FROM files GROUP BY language ORDER BY language",&[],100)?,"generation":rows(conn,"SELECT key,value FROM metadata WHERE key IN('schema_version','output_root','corpus_hash','workspace_root')ORDER BY key",&[],10)?}),
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
    let prefix = format!("{component}/%");
    Ok(
        json!({"sections":rows(conn,"SELECT d.*,bm25(doc_search)rank FROM doc_search JOIN doc_sections d ON d.rowid=doc_search.rowid WHERE doc_search MATCH ?1 AND (?2='' OR d.doc_type=?2)AND(?3='' OR d.path=?3 OR d.path LIKE ?4)ORDER BY rank,d.path LIMIT ?5",&[&query,&kind,&component,&prefix,&(limit as i64)],limit)?}),
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
        .trim_start()
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let indexed_path: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM nodes WHERE path=?1 OR path LIKE ?2)",
        rusqlite::params![target, format!("{}/%", target.trim_end_matches('/'))],
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
    let path = target.trim_end_matches('/');
    let prefix = format!("{path}/%");
    Ok(
        json!({"target":target,"errors":rows(conn,"SELECT * FROM errors WHERE ?1='' OR path=?1 OR path LIKE ?2 ORDER BY path,line",&[&path,&prefix],1000)?,"resolution":rows(conn,"SELECT * FROM resolution_coverage WHERE status!='resolved' AND(?1='' OR path=?1 OR path LIKE ?2)ORDER BY path,line",&[&path,&prefix],1000)?,"cycles":crate::db::traversal::cycles(conn)?}),
    )
}

pub fn validate_limit(limit: usize) -> Result<()> {
    if !(1..=10000).contains(&limit) {
        bail!("limit must be1..10000");
    }
    Ok(())
}
