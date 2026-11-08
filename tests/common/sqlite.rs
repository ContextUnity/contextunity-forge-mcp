use rusqlite::{Connection, Params};

/// Reads the first column of every row. The statement stays on this connection.
pub fn query_strings(connection: &Connection, sql: &str, params: impl Params) -> Vec<String> {
    connection
        .prepare_cached(sql)
        .unwrap_or_else(|error| panic!("could not prepare test query {sql}: {error}"))
        .query_map(params, |row| row.get(0))
        .unwrap_or_else(|error| panic!("could not run test query {sql}: {error}"))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|error| panic!("could not read test query {sql}: {error}"))
}

/// Reads one scalar. The query must return exactly one row.
pub fn query_value<T: rusqlite::types::FromSql>(
    connection: &Connection,
    sql: &str,
    params: impl Params,
) -> T {
    connection
        .query_row(sql, params, |row| row.get(0))
        .unwrap_or_else(|error| panic!("could not read test scalar {sql}: {error}"))
}
