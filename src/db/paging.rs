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
        options.generation.as_ref().is_none_or(|previous| {
            previous == &current || (previous.len() >= 8 && current.starts_with(previous.as_str()))
        }),
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
        "items": items, "generation": generation
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
    nodes_with_path(
        alias,
        detail,
        &format!("(SELECT path FROM path_dictionary WHERE path_id={alias}.path_id)"),
    )
}

pub(crate) fn nodes_with_path(alias: &str, detail: Detail, path_expression: &str) -> String {
    let path = format!("{path_expression} AS path");
    if detail == Detail::Full {
        format!(
            "{alias}.node_id,{alias}.id,{alias}.kind,{alias}.name,{alias}.qualname,{alias}.path_id,{path},{alias}.line,{alias}.end_line,{alias}.is_test,{alias}.language,{alias}.generated,{alias}.details,{alias}.node_hash,{alias}.owner_path_id"
        )
    } else {
        format!(
            "{alias}.id,{alias}.kind,{alias}.name,{alias}.qualname,{path},{alias}.line,{alias}.end_line,{alias}.language"
        )
    }
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
    let src = format!("(SELECT id FROM nodes WHERE node_hash={alias}.src_hash) AS src_public_id");
    let dst = format!("(SELECT id FROM nodes WHERE node_hash={alias}.dst_hash) AS dst_public_id");
    let path = format!("(SELECT path FROM path_dictionary WHERE path_id={alias}.path_id) AS path");
    let confidence = format!(
        "(SELECT evidence FROM coverage_evidence WHERE evidence_id={alias}.confidence_id) AS confidence"
    );
    if detail == Detail::Full {
        let evidence = format!(
            "(SELECT evidence FROM coverage_evidence WHERE evidence_id={alias}.evidence_id) AS evidence"
        );
        format!(
            "{src},{dst},{alias}.kind,{path},{alias}.line,{evidence},{confidence},{alias}.occurrence_count"
        )
    } else {
        format!(
            "{src},{dst},{alias}.kind,{path},{alias}.line,{confidence},{alias}.occurrence_count"
        )
    }
}

pub(crate) fn coverage(alias: &str, detail: Detail) -> String {
    let expression = format!(
        "(SELECT expression FROM coverage_expressions WHERE expression_id={alias}.expression_id) AS expression"
    );
    if detail == Detail::Full {
        format!(
            "(SELECT path FROM path_dictionary WHERE path_id={alias}.path_id) AS path,{alias}.line,{expression},{alias}.status,(SELECT evidence FROM coverage_evidence WHERE evidence_id={alias}.evidence_id) AS evidence"
        )
    } else {
        format!("{alias}.line,{expression},{alias}.status")
    }
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
