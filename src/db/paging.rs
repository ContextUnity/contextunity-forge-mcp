use super::reader::{rows, QueryBudget};
use crate::core::response::{Detail, QueryOptions, MAX_PAGE_SIZE};
use anyhow::{ensure, Result};
use rusqlite::{Connection, ToSql};
use serde_json::{json, Value};

pub(crate) fn generation(conn: &Connection, options: &QueryOptions) -> Result<String> {
    ensure!(
        (1..=MAX_PAGE_SIZE).contains(&options.limit),
        "limit must be 1..=100"
    );
    ensure!(
        options
            .offset
            .checked_add(options.limit)
            .is_some_and(|end| end <= i64::MAX as usize),
        "offset is too large"
    );
    ensure!(
        options.offset == 0 || options.generation.is_some(),
        "continuation requires generation from the previous page"
    );
    let current: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='output_root'",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        options
            .generation
            .as_ref()
            .is_none_or(|previous| previous == &current),
        "index generation changed; restart at offset 0 without generation"
    );
    Ok(current)
}

pub(crate) fn value(
    items: Vec<Value>,
    total: usize,
    options: &QueryOptions,
    generation: &str,
) -> Value {
    let next = options.offset.saturating_add(items.len());
    let has_more = next < total;
    json!({
        "total": total, "offset": options.offset, "limit": options.limit,
        "has_more": has_more, "next_offset": if has_more { Some(next) } else { None },
        "items": items, "generation": generation,
        "continuation_hint": if has_more { Some("Repeat the same query with next_offset as offset and this generation.") } else { None }
    })
}

pub(crate) fn count(conn: &Connection, sql: &str, params: &[&dyn ToSql]) -> Result<usize> {
    let _budget = QueryBudget::new(conn);
    let mut statement = conn.prepare(sql)?;
    ensure!(statement.readonly(), "query must be read-only");
    let total: i64 = statement.query_row(params, |r| r.get(0))?;
    Ok(usize::try_from(total)?)
}

pub(crate) fn query(
    conn: &Connection,
    sql: &str,
    params: &[&dyn ToSql],
    options: &QueryOptions,
) -> Result<Value> {
    let generation = generation(conn, options)?;
    let total = count(conn, &format!("SELECT count(*) FROM ({sql})"), params)?;
    let limit = options.limit as i64;
    let offset = options.offset as i64;
    let mut bindings = params.to_vec();
    bindings.extend([&limit as &dyn ToSql, &offset as &dyn ToSql]);
    let bounded = format!(
        "SELECT * FROM ({sql}) LIMIT ?{} OFFSET ?{}",
        params.len() + 1,
        params.len() + 2
    );
    let items = rows(conn, &bounded, &bindings, options.limit)?;
    Ok(value(items, total, options, &generation))
}

pub(crate) fn nodes(alias: &str, detail: Detail) -> String {
    columns(
        alias,
        detail,
        &["id", "kind", "name", "path", "line", "end_line", "language"],
    )
}

pub(crate) fn docs(alias: &str, detail: Detail) -> String {
    columns(
        alias,
        detail,
        &[
            "doc_id",
            "path",
            "section_title",
            "doc_type",
            "size",
            "is_invariant",
        ],
    )
}

pub(crate) fn edges(alias: &str, detail: Detail) -> String {
    columns(
        alias,
        detail,
        &[
            "src_public_id",
            "dst_public_id",
            "kind",
            "path",
            "line",
            "confidence",
            "occurrence_count",
        ],
    )
}

pub(crate) fn coverage(alias: &str, detail: Detail) -> String {
    columns(alias, detail, &["line", "expression", "status"])
}

fn columns(alias: &str, detail: Detail, names: &[&str]) -> String {
    if detail == Detail::Full {
        format!("{alias}.*")
    } else {
        names
            .iter()
            .map(|name| format!("{alias}.{name}"))
            .collect::<Vec<_>>()
            .join(",")
    }
}
