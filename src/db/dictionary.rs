use super::*;

#[derive(Default)]
pub struct PathDictionaryCache {
    cache: hashbrown::HashMap<String, i64>,
}

impl PathDictionaryCache {
    pub fn new() -> Self {
        Self {
            cache: hashbrown::HashMap::new(),
        }
    }

    pub fn load_from_db(&mut self, conn: &Connection) -> Result<()> {
        let mut stmt = conn.prepare("SELECT path, path_id FROM path_dictionary")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for r in rows {
            let (path, id) = r?;
            self.cache.insert(path, id);
        }
        Ok(())
    }

    pub fn bulk_insert(
        &mut self,
        conn: &Connection,
        paths: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<()> {
        self.load_from_db(conn)?;
        let mut next_id: i64 = conn.query_row(
            "SELECT coalesce(max(path_id),0) FROM path_dictionary",
            [],
            |row| row.get(0),
        )?;
        let mut batch = MultiValueBatch::<2, BorrowedSqlValue<'_>>::new(
            conn,
            "INSERT INTO path_dictionary(path_id,path) VALUES",
        )?;
        for p in paths {
            let path_ref = p.as_ref();
            if !self.cache.contains_key(path_ref) {
                next_id = next_id
                    .checked_add(1)
                    .context("path dictionary ID overflow")?;
                self.cache.insert(path_ref.to_owned(), next_id);
                batch.push([
                    BorrowedSqlValue::Integer(next_id),
                    BorrowedSqlValue::OwnedText(path_ref.to_owned()),
                ])?;
            }
        }
        batch.flush()?;
        Ok(())
    }

    pub fn get_or_insert(&mut self, conn: &Connection, path: &str) -> Result<i64> {
        if let Some(&id) = self.cache.get(path) {
            return Ok(id);
        }
        conn.execute(
            "INSERT OR IGNORE INTO path_dictionary(path) VALUES(?1)",
            [path],
        )?;
        let id: i64 = conn.query_row(
            "SELECT path_id FROM path_dictionary WHERE path=?1",
            [path],
            |row| row.get(0),
        )?;
        self.cache.insert(path.to_owned(), id);
        Ok(id)
    }
}

pub struct SharedKeyCache<'conn> {
    cache: hashbrown::HashMap<Arc<str>, i64>,
    pub(super) hash_to_key: hashbrown::HashMap<i64, Arc<str>>,
    stmt_insert: rusqlite::Statement<'conn>,
    stmt_select: Option<rusqlite::Statement<'conn>>,
    pending: Option<MultiValueBatch<'conn, 2, BorrowedSqlValue<'static>>>,
}

impl<'conn> SharedKeyCache<'conn> {
    pub fn new(conn: &'conn Connection, is_delta: bool) -> Result<Self> {
        let stmt_insert = conn.prepare("INSERT INTO shared_keys(key_hash,key) VALUES(?1,?2)")?;
        let stmt_select = if is_delta {
            Some(conn.prepare("SELECT CASE WHEN typeof(k.key)='integer' THEN n.qualname ELSE k.key END FROM shared_keys k LEFT JOIN nodes n ON n.node_id=k.key AND typeof(k.key)='integer' WHERE k.key_hash=?1")?)
        } else {
            None
        };
        Ok(Self {
            cache: hashbrown::HashMap::new(),
            hash_to_key: hashbrown::HashMap::new(),
            stmt_insert,
            stmt_select,
            pending: None,
        })
    }

    pub(super) fn new_batched(conn: &'conn Connection) -> Result<Self> {
        let mut cache = Self::new(conn, false)?;
        cache.pending = Some(MultiValueBatch::new(
            conn,
            "INSERT INTO shared_keys(key_hash,key) VALUES",
        )?);
        Ok(cache)
    }

    pub(super) fn new_batched_delta(conn: &'conn Connection) -> Result<Self> {
        let mut cache = Self::new(conn, true)?;
        cache.pending = Some(MultiValueBatch::new(
            conn,
            "INSERT INTO shared_keys(key_hash,key) VALUES",
        )?);
        Ok(cache)
    }

    pub(super) fn flush(&mut self) -> Result<()> {
        if let Some(batch) = self.pending.as_mut() {
            batch.flush()?;
        }
        Ok(())
    }

    pub fn get_or_insert(&mut self, key: &str) -> Result<i64> {
        self.get_or_insert_value(key, None)
    }

    pub(super) fn get_or_insert_node(&mut self, key: &str, node_id: i64) -> Result<i64> {
        self.get_or_insert_value(key, Some(node_id))
    }

    fn get_or_insert_value(&mut self, key: &str, node_id: Option<i64>) -> Result<i64> {
        if let Some(&hash) = self.cache.get(key) {
            return Ok(hash);
        }
        let hash = stable_hash64(key);
        if let Some(existing) = self.hash_to_key.get(&hash) {
            if existing.as_ref() != key {
                bail!("shared key hash collision detected");
            }
        } else {
            if let Some(stmt) = self.stmt_select.as_mut() {
                let existing: Option<String> =
                    stmt.query_row([hash], |row| row.get(0)).optional()?;
                if let Some(existing) = existing {
                    if existing != key {
                        bail!("shared key hash collision detected");
                    }
                    let shared_key: Arc<str> = Arc::from(existing);
                    self.hash_to_key.insert(hash, Arc::clone(&shared_key));
                    self.cache.insert(shared_key, hash);
                    return Ok(hash);
                }
            }
            let shared_key: Arc<str> = Arc::from(key);
            if let Some(batch) = self.pending.as_mut() {
                batch.push([
                    BorrowedSqlValue::Integer(hash),
                    node_id.map_or_else(
                        || BorrowedSqlValue::SharedText(Arc::clone(&shared_key)),
                        BorrowedSqlValue::Integer,
                    ),
                ])?;
            } else {
                if let Some(node_id) = node_id {
                    self.stmt_insert.execute(params![hash, node_id])?;
                } else {
                    self.stmt_insert.execute(params![hash, key])?;
                }
            }
            self.cache.insert(Arc::clone(&shared_key), hash);
            self.hash_to_key.insert(hash, shared_key);
            return Ok(hash);
        }
        let shared_key = Arc::clone(
            self.hash_to_key
                .get(&hash)
                .expect("shared key is present after lookup or insertion"),
        );
        self.cache.insert(shared_key, hash);
        Ok(hash)
    }
}

pub(super) fn prefill_coverage_dictionary<'a>(
    conn: &Connection,
    table: &str,
    column: &str,
    prefix: &'static str,
    values: impl Iterator<Item = &'a str>,
) -> Result<hashbrown::HashMap<&'a str, i64>> {
    let mut cache = hashbrown::HashMap::new();
    let mut batch = MultiValueBatch::<2, BorrowedSqlValue<'_>>::new(conn, prefix)?;
    let stored: hashbrown::HashMap<String, i64> = conn
        .prepare_cached(&format!("SELECT {column}_id,{column} FROM {table}"))?
        .query_map([], |row| Ok((row.get(1)?, row.get(0)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut next_id = stored
        .values()
        .copied()
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .context("dictionary ID overflow")?;
    for value in values {
        if let hashbrown::hash_map::Entry::Vacant(entry) = cache.entry(value) {
            if let Some(id) = stored.get(value) {
                entry.insert(*id);
                continue;
            }
            entry.insert(next_id);
            batch.push([
                BorrowedSqlValue::Integer(next_id),
                BorrowedSqlValue::Text(value),
            ])?;
            next_id = next_id
                .checked_add(1)
                .context("coverage dictionary ID overflow")?;
        }
    }
    batch.flush()?;
    Ok(cache)
}

pub(super) fn prefill_changed_coverage_expressions<'a>(
    conn: &Connection,
    values: impl Iterator<Item = &'a str>,
) -> Result<hashbrown::HashMap<&'a str, i64>> {
    let values: Vec<_> = values.collect();
    if values.is_empty() {
        return Ok(hashbrown::HashMap::new());
    }
    let stored: hashbrown::HashMap<String, i64> = conn
        .prepare_cached("SELECT expression,expression_id FROM coverage_expressions WHERE expression IN (SELECT value FROM json_each(?1))")?
        .query_map([serde_json::to_string(&values)?], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut next_id: i64 = conn.query_row(
        "SELECT coalesce(max(expression_id),0) FROM coverage_expressions",
        [],
        |row| row.get(0),
    )?;
    let mut cache = hashbrown::HashMap::with_capacity(values.len());
    let mut batch = MultiValueBatch::<2, BorrowedSqlValue<'_>>::new(
        conn,
        "INSERT INTO coverage_expressions(expression_id,expression) VALUES",
    )?;
    for value in values {
        if let hashbrown::hash_map::Entry::Vacant(entry) = cache.entry(value) {
            let id = if let Some(&existing_id) = stored.get(value) {
                existing_id
            } else {
                next_id = next_id
                    .checked_add(1)
                    .context("coverage expression ID overflow")?;
                batch.push([
                    BorrowedSqlValue::Integer(next_id),
                    BorrowedSqlValue::Text(value),
                ])?;
                next_id
            };
            entry.insert(id);
        }
    }
    batch.flush()?;
    Ok(cache)
}

pub(super) struct CoverageDictionaryCache;

impl CoverageDictionaryCache {
    pub(super) fn prefill_evidence<'a>(
        conn: &Connection,
        graph: &'a CompactGraph,
    ) -> Result<hashbrown::HashMap<&'a str, i64>> {
        prefill_coverage_dictionary(
            conn,
            "coverage_evidence",
            "evidence",
            "INSERT INTO coverage_evidence(evidence_id,evidence) VALUES",
            graph
                .edges
                .iter()
                .map(|edge| edge.confidence.as_str())
                .chain(graph.edges.iter().map(|edge| edge.evidence.as_str()))
                .chain(
                    graph
                        .coverage
                        .iter()
                        .map(|coverage| coverage.evidence.as_str()),
                ),
        )
    }

    pub(super) fn prefill_expressions<'a>(
        conn: &Connection,
        graph: &'a CompactGraph,
        cold_build: bool,
    ) -> Result<hashbrown::HashMap<&'a str, i64>> {
        let values = graph
            .coverage
            .iter()
            .map(|coverage| coverage.expression.as_str());
        if cold_build {
            prefill_coverage_dictionary(
                conn,
                "coverage_expressions",
                "expression",
                "INSERT INTO coverage_expressions(expression_id,expression) VALUES",
                values,
            )
        } else {
            prefill_changed_coverage_expressions(conn, values)
        }
    }
}

#[cfg(test)]
pub struct CoverageExpressionCache<'conn> {
    pub(super) cache: hashbrown::HashMap<String, i64>,
    stmt_insert: rusqlite::Statement<'conn>,
    stmt_select: rusqlite::Statement<'conn>,
}

#[cfg(test)]
impl<'conn> CoverageExpressionCache<'conn> {
    pub fn new(conn: &'conn Connection) -> Result<Self> {
        let stmt_insert =
            conn.prepare("INSERT OR IGNORE INTO coverage_expressions(expression) VALUES(?1)")?;
        let stmt_select =
            conn.prepare("SELECT expression_id FROM coverage_expressions WHERE expression=?1")?;
        Ok(Self {
            cache: hashbrown::HashMap::new(),
            stmt_insert,
            stmt_select,
        })
    }

    pub fn get_or_insert(&mut self, expression: &str) -> Result<i64> {
        if let Some(&id) = self.cache.get(expression) {
            return Ok(id);
        }
        self.stmt_insert.execute([expression])?;
        let id: i64 = self.stmt_select.query_row([expression], |row| row.get(0))?;
        self.cache.insert(expression.to_owned(), id);
        Ok(id)
    }
}

#[cfg(test)]
pub struct CoverageEvidenceCache<'conn> {
    pub(super) cache: hashbrown::HashMap<String, i64>,
    stmt_insert: rusqlite::Statement<'conn>,
    stmt_select: rusqlite::Statement<'conn>,
}

#[cfg(test)]
impl<'conn> CoverageEvidenceCache<'conn> {
    pub fn new(conn: &'conn Connection) -> Result<Self> {
        let stmt_insert =
            conn.prepare("INSERT OR IGNORE INTO coverage_evidence(evidence) VALUES(?1)")?;
        let stmt_select =
            conn.prepare("SELECT evidence_id FROM coverage_evidence WHERE evidence=?1")?;
        Ok(Self {
            cache: hashbrown::HashMap::new(),
            stmt_insert,
            stmt_select,
        })
    }

    pub fn get_or_insert(&mut self, evidence: &str) -> Result<i64> {
        if let Some(&id) = self.cache.get(evidence) {
            return Ok(id);
        }
        self.stmt_insert.execute([evidence])?;
        let id: i64 = self.stmt_select.query_row([evidence], |row| row.get(0))?;
        self.cache.insert(evidence.to_owned(), id);
        Ok(id)
    }
}
