use contextunity_forge_mcp::db::reader;
use rusqlite::{limits::Limit, Connection};

const MAX_SERIALIZED_BYTES: usize = 8 * 1024 * 1024;
const SQL: &str = r#"SELECT ?1 AS "a\b",0 AS integer_value,1.25 AS real_value,NULL AS null_value,X'00ff' AS blob_value,'[0]' AS details UNION ALL SELECT '',0,1.25,NULL,X'00ff','[0]'"#;

#[test]
fn rows_enforces_the_exact_serialized_json_limit() {
    let conn = Connection::open_in_memory().unwrap();
    conn.set_limit(Limit::SQLITE_LIMIT_LENGTH, MAX_SERIALIZED_BYTES as i32);

    let empty_rows = reader::rows(&conn, SQL, &[&""], 2).unwrap();
    let empty_size = serde_json::to_vec(&empty_rows).unwrap().len();

    let safe_value = "a".repeat(MAX_SERIALIZED_BYTES - empty_size);
    let safe_exact = reader::rows(&conn, SQL, &[&safe_value], 2)
        .ok()
        .map(|rows| serde_json::to_vec(&rows).unwrap().len());
    let mut safe_over_value = safe_value;
    safe_over_value.push('a');
    let safe_over = reader::rows(&conn, SQL, &[&safe_over_value], 2);
    let safe_over_rejected = match safe_over {
        Err(error) => error.to_string().contains("exceeds the 8 MiB row budget"),
        Ok(_) => false,
    };

    let remaining_bytes = MAX_SERIALIZED_BYTES - empty_size;
    let escaped_control_bytes = serde_json::to_vec(&"\u{0001}").unwrap().len() - 2;
    let control_chars = remaining_bytes / escaped_control_bytes;
    let plain_chars = remaining_bytes % escaped_control_bytes;
    let mut exact_value = "\u{0001}".repeat(control_chars);
    exact_value.push_str(&"a".repeat(plain_chars));

    let escaped_exact = reader::rows(&conn, SQL, &[&exact_value], 2)
        .ok()
        .map(|rows| serde_json::to_vec(&rows).unwrap().len());

    exact_value.push('a');
    let escaped_over = reader::rows(&conn, SQL, &[&exact_value], 2);
    let escaped_over_rejected = match escaped_over {
        Err(error) => error.to_string().contains("exceeds the 8 MiB row budget"),
        Ok(_) => false,
    };

    let duplicate_conn = Connection::open_in_memory().unwrap();
    let replaced_value = "a".repeat(MAX_SERIALIZED_BYTES - 1);
    let duplicate_rows = reader::rows(
        &duplicate_conn,
        "SELECT ?1 AS x, 'ok' AS x",
        &[&replaced_value],
        1,
    )
    .unwrap();

    assert!(
        safe_exact == Some(MAX_SERIALIZED_BYTES)
            && safe_over_rejected
            && escaped_exact == Some(MAX_SERIALIZED_BYTES)
            && escaped_over_rejected
            && duplicate_rows[0]["x"] == "ok",
        "safe exact: {safe_exact:?}; safe over rejected: {safe_over_rejected}; escaped exact: {escaped_exact:?}; escaped over rejected: {escaped_over_rejected}; duplicate alias retained final value: {}",
        duplicate_rows[0]["x"] == "ok"
    );
}
