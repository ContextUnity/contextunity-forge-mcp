use super::*;

pub(super) fn multi_value_batch_rows<const COLUMNS: usize>(conn: &Connection) -> Result<usize> {
    let variable_limit = usize::try_from(conn.limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER))
        .context("SQLite variable limit must be non-negative")?;
    let rows = (variable_limit / COLUMNS).min(MAX_MULTI_VALUE_BATCH_ROWS);
    if rows == 0 {
        bail!("SQLite variable limit cannot fit a {COLUMNS}-column insert");
    }
    Ok(rows)
}

pub(super) enum BorrowedSqlValue<'a> {
    Integer(i64),
    Real(f64),
    Text(&'a str),
    OwnedText(String),
    OwnedJsonText(Vec<u8>),
    SharedText(Arc<str>),
    Blob(&'a [u8]),
    OwnedBlob(Vec<u8>),
    Null,
}

impl ToSql for BorrowedSqlValue<'_> {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        use rusqlite::types::{ToSqlOutput, ValueRef};
        Ok(ToSqlOutput::Borrowed(match self {
            Self::Integer(value) => ValueRef::Integer(*value),
            Self::Real(value) => ValueRef::Real(*value),
            Self::Text(value) => ValueRef::Text(value.as_bytes()),
            Self::OwnedText(value) => ValueRef::Text(value.as_bytes()),
            Self::OwnedJsonText(value) => ValueRef::Text(value),
            Self::SharedText(value) => ValueRef::Text(value.as_bytes()),
            Self::Blob(value) => ValueRef::Blob(value),
            Self::OwnedBlob(value) => ValueRef::Blob(value),
            Self::Null => ValueRef::Null,
        }))
    }
}

pub(super) struct MultiValueBatch<'conn, const COLUMNS: usize, T: ToSql = SqlValue> {
    conn: &'conn Connection,
    prefix: &'static str,
    rows: Vec<[T; COLUMNS]>,
    max_rows: usize,
    full_statement: rusqlite::Statement<'conn>,
}

impl<'conn, const COLUMNS: usize, T: ToSql> MultiValueBatch<'conn, COLUMNS, T> {
    pub(super) fn new(conn: &'conn Connection, prefix: &'static str) -> Result<Self> {
        let max_rows = multi_value_batch_rows::<COLUMNS>(conn)?;
        Ok(Self {
            conn,
            prefix,
            rows: Vec::with_capacity(max_rows),
            max_rows,
            full_statement: conn.prepare(&multi_value_insert_sql::<COLUMNS>(prefix, max_rows))?,
        })
    }

    pub(super) fn push(&mut self, row: [T; COLUMNS]) -> Result<()> {
        self.rows.push(row);
        if self.rows.len() == self.max_rows {
            self.flush()?;
        }
        Ok(())
    }

    pub(super) fn flush(&mut self) -> Result<()> {
        if self.rows.len() == self.max_rows {
            self.full_statement.execute(params_from_iter(
                self.rows.iter().flat_map(|row| row.iter()),
            ))?;
        } else {
            insert_multi_value_batch(self.conn, self.prefix, &self.rows)?;
        }
        self.rows.clear();
        Ok(())
    }
}

fn multi_value_insert_sql<const COLUMNS: usize>(prefix: &str, rows: usize) -> String {
    let mut row_template = String::with_capacity(COLUMNS * 2 + 1);
    row_template.push('(');
    for column in 0..COLUMNS {
        if column > 0 {
            row_template.push(',');
        }
        row_template.push('?');
    }
    row_template.push(')');
    let mut sql = String::with_capacity(prefix.len() + rows * (row_template.len() + 1));
    sql.push_str(prefix);
    for row in 0..rows {
        if row > 0 {
            sql.push(',');
        }
        sql.push_str(&row_template);
    }
    sql
}

pub(super) fn insert_multi_value_batch<const COLUMNS: usize>(
    tx: &Connection,
    prefix: &str,
    batch: &[[impl ToSql; COLUMNS]],
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }

    let max_rows = multi_value_batch_rows::<COLUMNS>(tx)?;
    for rows in batch.chunks(max_rows) {
        let sql = multi_value_insert_sql::<COLUMNS>(prefix, rows.len());
        let values = rows.iter().flat_map(|row| row.iter());
        tx.prepare_cached(&sql)?.execute(params_from_iter(values))?;
    }
    Ok(())
}
