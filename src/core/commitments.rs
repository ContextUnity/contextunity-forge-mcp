use anyhow::{bail, Context, Result};
use hashbrown::HashMap;
use rusqlite::{params, types::ValueRef, Connection};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

mod coverage_leaves;
mod domain_leaves;
mod snapshot;

pub(crate) use snapshot::seal_snapshot;
/// Domain-separated owner commitment algorithm persisted in index metadata.
pub const ALGORITHM: &str = "forge-owner-sha256-v2";
/// Logical table domains included in the deterministic output root.
pub const DOMAINS: &[&str] = &[
    "source_inventory",
    "files",
    "nodes",
    "edges",
    "local_facts",
    "edge_occurrences",
    "dependencies",
    "shared_owners",
    "errors",
    "resolution_coverage",
    "coverage_owner_language",
    "coverage_language_counts",
    "doc_sections",
];
const FTS: &[(&str, &str)] = &[
    ("node_search_postings", "term,doc,col,offset"),
    ("node_search_config", "k"),
    ("doc_search_postings", "term,doc,col,offset"),
    ("doc_search_config", "k"),
];
/// Returns a lowercase SHA-256 digest of the supplied bytes.
pub fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn encode_row(
    hasher: &mut impl sha2::digest::Update,
    row: &rusqlite::Row<'_>,
    columns: usize,
) -> Result<()> {
    hasher.update(&[0xff]);
    for i in 0..columns {
        match row.get_ref(i)? {
            ValueRef::Null => hasher.update(b"n"),
            ValueRef::Integer(n) => {
                hasher.update(b"i");
                hasher.update(&n.to_le_bytes());
            }
            ValueRef::Real(n) => {
                hasher.update(b"r");
                hasher.update(&n.to_bits().to_le_bytes());
            }
            ValueRef::Text(s) => {
                hasher.update(b"t");
                hasher.update(&(s.len() as u64).to_le_bytes());
                hasher.update(s);
            }
            ValueRef::Blob(s) => {
                hasher.update(b"b");
                hasher.update(&(s.len() as u64).to_le_bytes());
                hasher.update(s);
            }
        }
    }
    Ok(())
}
fn append_text(buffer: &mut Vec<u8>, text: &[u8]) {
    buffer.push(b't');
    buffer.extend_from_slice(&(text.len() as u64).to_le_bytes());
    buffer.extend_from_slice(text);
}
fn append_integer(buffer: &mut Vec<u8>, value: i64) {
    buffer.push(b'i');
    buffer.extend_from_slice(&value.to_le_bytes());
}
fn append_fts_posting(
    buffer: &mut Vec<u8>,
    term: &[u8],
    identity: FtsRowIdentityRef<'_>,
    column: &[u8],
    offset: i64,
) {
    buffer.push(0xff);
    append_text(buffer, term);
    match identity {
        FtsRowIdentityRef::NodeHash(node_hash) => append_integer(buffer, node_hash),
        FtsRowIdentityRef::DocId(doc_id) => append_text(buffer, doc_id),
    }
    append_text(buffer, column);
    append_integer(buffer, offset);
}
fn new_leaf_hasher(table: &str, owner: &str) -> Sha256 {
    let mut h = Sha256::new();
    h.update(ALGORITHM);
    h.update(table);
    h.update((owner.len() as u64).to_le_bytes());
    h.update(owner.as_bytes());
    h
}
fn key(table: &str, owner: &str) -> String {
    format!("leaf:{}", serde_json::json!([table, owner]))
}
fn shape(table: &str) -> (&'static str, &'static str, &'static str) {
    match table {
        "errors" => ("t.path", "", "t.line,t.message"),
        "doc_sections" => ("t.path", "", "t.doc_id"),
        _ => ("t.path", "", "t.path"),
    }
}
#[inline]
fn encode_text(h: &mut impl sha2::digest::Update, s: &str) {
    h.update(b"t");
    h.update(&(s.len() as u64).to_le_bytes());
    h.update(s.as_bytes());
}

#[inline]
fn encode_int(h: &mut impl sha2::digest::Update, n: i64) {
    h.update(b"i");
    h.update(&n.to_le_bytes());
}

#[inline]
fn encode_null(h: &mut impl sha2::digest::Update) {
    h.update(b"n");
}

#[derive(Clone, Copy)]
struct TextSpan {
    start: usize,
    end: usize,
}

#[derive(Default)]
struct TextArena(String);

impl TextArena {
    fn push(&mut self, text: &str) -> TextSpan {
        let start = self.0.len();
        self.0.push_str(text);
        TextSpan {
            start,
            end: self.0.len(),
        }
    }

    fn get(&self, span: TextSpan) -> &str {
        &self.0[span.start..span.end]
    }
}

fn leaves(
    conn: &Connection,
    table: &str,
    owners: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, String>> {
    match table {
        "nodes" => return domain_leaves::leaves_nodes(conn, owners),
        "edge_occurrences" => return domain_leaves::leaves_edge_occurrences(conn, owners),
        "edges" => return domain_leaves::leaves_edges(conn, owners),
        "shared_owners" => return domain_leaves::leaves_shared_owners(conn, owners),
        "dependencies" => return domain_leaves::leaves_dependencies(conn, owners),
        "resolution_coverage" => {
            if let Some(leaves) = coverage_leaves::canonical_coverage_leaves(conn, owners)? {
                return Ok(leaves);
            }
        }
        "coverage_owner_language" => {
            let leaves = coverage_leaves::canonical_owner_language_leaves(conn, owners)?
                .context("coverage_owner_language rows require canonical text")?;
            return Ok(leaves);
        }
        "coverage_language_counts" => {
            let leaves = coverage_leaves::canonical_language_count_leaves(conn, owners)?
                .context("coverage_language_counts rows require canonical text")?;
            return Ok(leaves);
        }
        _ => {}
    }
    domain_leaves::leaves_generic(conn, table, owners)
}

fn ensure_fts_vocab(conn: &Connection, index: &str) -> Result<&'static str> {
    let (table, create_sql) = match index {
        "node_search" => (
            "node_search_commit_vocab",
            "CREATE VIRTUAL TABLE IF NOT EXISTS temp.node_search_commit_vocab \
             USING fts5vocab(main,node_search,'instance')",
        ),
        "doc_search" => (
            "doc_search_commit_vocab",
            "CREATE VIRTUAL TABLE IF NOT EXISTS temp.doc_search_commit_vocab \
             USING fts5vocab(main,doc_search,'instance')",
        ),
        _ => anyhow::bail!("unsupported FTS commitment index: {index}"),
    };
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_temp_master WHERE type='table' AND name=?1)",
        [table],
        |row| row.get(0),
    )?;
    if exists {
        return Ok(table);
    }

    // Snapshot hashing opens read-only connections and marks them query-only.
    // SQLite permits a temp vocab view on a read-only main database, but only
    // when query_only is briefly disabled while the temp object is created.
    let query_only: i64 = conn.query_row("PRAGMA query_only", [], |row| row.get(0))?;
    if query_only != 0 {
        conn.pragma_update(None, "query_only", false)?;
    }
    let create_result = conn.execute_batch(create_sql);
    let restore_result = if query_only != 0 {
        conn.pragma_update(None, "query_only", true)
    } else {
        Ok(())
    };
    create_result?;
    restore_result?;
    Ok(table)
}

fn decode_fts5_size_array(bytes: &[u8], columns: usize) -> Result<Vec<u32>> {
    let mut offset = 0;
    let mut sizes = Vec::with_capacity(columns);
    for _ in 0..columns {
        let mut value = 0u32;
        let mut encoded_bytes = 0;
        loop {
            let byte = *bytes
                .get(offset)
                .context("truncated FTS5 document-size vector")?;
            offset += 1;
            encoded_bytes += 1;
            anyhow::ensure!(encoded_bytes <= 5, "oversized FTS5 document-size value");
            value = value
                .checked_mul(128)
                .and_then(|value| value.checked_add(u32::from(byte & 0x7f)))
                .context("FTS5 document-size value overflow")?;
            if byte & 0x80 == 0 {
                break;
            }
        }
        sizes.push(value);
    }
    anyhow::ensure!(
        offset == bytes.len(),
        "FTS5 document-size vector has trailing bytes"
    );
    Ok(sizes)
}

struct FtsCommitmentSpec {
    docsize_table: &'static str,
    fts_table: &'static str,
    owner_join: &'static str,
    identity: &'static str,
    identity_kind: FtsIdentityKind,
    columns: &'static [&'static str],
}

#[derive(Clone, Copy)]
enum FtsIdentityKind {
    NodeHash,
    DocId,
}

enum FtsRowIdentity {
    NodeHash(i64),
    DocId(Vec<u8>),
}

enum FtsRowIdentityRef<'a> {
    NodeHash(i64),
    DocId(&'a [u8]),
}

struct FtsDocument {
    identity_sort_key: Option<i64>,
    size_blob: Vec<u8>,
    token_counts: [u32; 3],
}

type FtsDocumentMap = HashMap<i64, FtsDocument>;
type FtsOwnerIds = Vec<Vec<u8>>;
type FtsTerms = Vec<Vec<u8>>;
type FtsTermRanks = HashMap<Vec<u8>, u32>;

struct FtsPosting {
    term: u32,
    identity_sort_key: i64,
    column_index: u8,
    column_sort_key: u8,
    offset: i64,
}

fn fts_commitment_spec(index: &str) -> Result<FtsCommitmentSpec> {
    Ok(match index {
        "node_search" => FtsCommitmentSpec {
            docsize_table: "node_search_docsize",
            fts_table: "node_search",
            owner_join: "LEFT JOIN nodes AS n ON n.node_hash=f.rowid",
            identity: "n.node_hash",
            identity_kind: FtsIdentityKind::NodeHash,
            columns: &["search_text"],
        },
        "doc_search" => FtsCommitmentSpec {
            docsize_table: "doc_search_docsize",
            fts_table: "doc_search",
            owner_join: "LEFT JOIN doc_sections AS s ON s.rowid=f.rowid",
            identity: "s.doc_id",
            identity_kind: FtsIdentityKind::DocId,
            columns: &["section_title", "content", "invariants"],
        },
        _ => anyhow::bail!("unsupported FTS commitment index: {index}"),
    })
}

fn load_fts_documents(
    conn: &Connection,
    spec: &FtsCommitmentSpec,
) -> Result<(FtsDocumentMap, FtsOwnerIds)> {
    let mut stmt = conn.prepare(&format!(
        "SELECT f.rowid,{identity},d.sz
         FROM {fts} AS f
         {owner_join}
         LEFT JOIN {docsize} AS d ON d.id=f.rowid",
        identity = spec.identity,
        fts = spec.fts_table,
        owner_join = spec.owner_join,
        docsize = spec.docsize_table,
    ))?;
    let mut rows = stmt.query([])?;
    let mut documents = Vec::new();
    while let Some(row) = rows.next()? {
        let doc: i64 = row.get(0)?;
        let identity = match (spec.identity_kind, row.get_ref(1)?) {
            (FtsIdentityKind::NodeHash, ValueRef::Integer(node_hash)) => {
                Some(FtsRowIdentity::NodeHash(node_hash))
            }
            (FtsIdentityKind::DocId, ValueRef::Text(doc_id)) => {
                Some(FtsRowIdentity::DocId(doc_id.to_vec()))
            }
            (_, ValueRef::Null) => None,
            _ => anyhow::bail!("invalid FTS5 owner identity: {}:{doc}", spec.fts_table),
        };
        let size_blob: Option<Vec<u8>> = row.get(2)?;
        let size_blob = size_blob.with_context(|| {
            format!(
                "missing active FTS5 docsize row: {}:{doc}",
                spec.docsize_table
            )
        })?;
        documents.push((doc, identity, size_blob));
    }

    let mut owner_ids: Vec<Vec<u8>> = documents
        .iter()
        .filter_map(|(_, identity, _)| match identity {
            Some(FtsRowIdentity::DocId(doc_id)) => Some(doc_id.clone()),
            _ => None,
        })
        .collect();
    owner_ids.sort_unstable();
    owner_ids.dedup();
    let owner_ranks: HashMap<Vec<u8>, i64> = owner_ids
        .iter()
        .enumerate()
        .map(|(rank, doc_id)| Ok((doc_id.clone(), i64::try_from(rank)?)))
        .collect::<Result<_>>()?;

    let mut by_doc = HashMap::with_capacity(documents.len());
    for (doc, identity, size_blob) in documents {
        let identity_sort_key = match identity.as_ref() {
            Some(FtsRowIdentity::NodeHash(node_hash)) => Some(*node_hash),
            Some(FtsRowIdentity::DocId(doc_id)) => owner_ranks.get(doc_id).copied(),
            None => None,
        };
        by_doc.insert(
            doc,
            FtsDocument {
                identity_sort_key,
                size_blob,
                token_counts: [0; 3],
            },
        );
    }
    Ok((by_doc, owner_ids))
}

fn load_fts_terms(conn: &Connection, vocab: &str) -> Result<(FtsTerms, FtsTermRanks)> {
    let mut stmt = conn.prepare(&format!("SELECT DISTINCT term FROM temp.{vocab}"))?;
    let mut rows = stmt.query([])?;
    let mut terms = Vec::new();
    while let Some(row) = rows.next()? {
        match row.get_ref(0)? {
            ValueRef::Text(term) => terms.push(term.to_vec()),
            _ => anyhow::bail!("invalid FTS5 vocabulary term: {vocab}"),
        }
    }
    terms.sort_unstable();
    let term_ranks = terms
        .iter()
        .enumerate()
        .map(|(rank, term)| Ok((term.clone(), u32::try_from(rank)?)))
        .collect::<Result<_>>()?;
    Ok((terms, term_ranks))
}

fn validate_fts_docsize(
    spec: &FtsCommitmentSpec,
    documents: &HashMap<i64, FtsDocument>,
) -> Result<()> {
    for (doc, document) in documents {
        let actual =
            decode_fts5_size_array(&document.size_blob, spec.columns.len()).with_context(|| {
                format!("invalid FTS5 document sizes: {}:{doc}", spec.docsize_table)
            })?;
        for (column_index, column) in spec.columns.iter().enumerate() {
            anyhow::ensure!(
                actual[column_index] == document.token_counts[column_index],
                "FTS5 document size mismatch: {}:{doc} column {column}",
                spec.docsize_table
            );
        }
    }
    Ok(())
}

fn fts_digest(conn: &Connection, table: &str, order_by: &str) -> Result<String> {
    let mut h = Sha256::new();
    h.update(table);
    if let Some(index) = table.strip_suffix("_postings") {
        let vocab = ensure_fts_vocab(conn, index)?;
        let spec = fts_commitment_spec(index)?;
        let (mut documents, owner_ids) = load_fts_documents(conn, &spec)?;
        let (terms, term_ranks) = load_fts_terms(conn, vocab)?;
        let mut column_order: Vec<usize> = (0..spec.columns.len()).collect();
        column_order.sort_unstable_by_key(|column| spec.columns[*column].as_bytes());
        let mut column_sort_keys = vec![0; spec.columns.len()];
        for (rank, column) in column_order.into_iter().enumerate() {
            column_sort_keys[column] = u8::try_from(rank)?;
        }
        let mut stmt = conn.prepare(&format!(
            "SELECT v.term,v.col,v.offset,v.doc FROM temp.{vocab} AS v"
        ))?;
        let mut rows = stmt.query([])?;
        let mut postings = Vec::new();
        while let Some(row) = rows.next()? {
            let term = match row.get_ref(0)? {
                ValueRef::Text(term) => term,
                _ => anyhow::bail!("invalid FTS5 vocabulary term in {table}"),
            };
            let term_rank = term_ranks
                .get(term)
                .copied()
                .with_context(|| format!("FTS5 vocabulary term was not interned: {table}"))?;
            let column = match row.get_ref(1)? {
                ValueRef::Text(column) => column,
                _ => anyhow::bail!("invalid FTS5 document-size column in {table}"),
            };
            let column_index = spec
                .columns
                .iter()
                .position(|candidate| candidate.as_bytes() == column)
                .with_context(|| {
                    format!(
                        "unknown FTS5 document-size column: {index}:{}",
                        String::from_utf8_lossy(column)
                    )
                })?;
            let doc: i64 = row.get(3)?;
            let document = documents
                .get_mut(&doc)
                .with_context(|| format!("FTS5 postings refer to missing owner rows: {table}"))?;
            let identity_sort_key = document
                .identity_sort_key
                .with_context(|| format!("FTS5 postings refer to missing owner rows: {table}"))?;
            document.token_counts[column_index] = document.token_counts[column_index]
                .checked_add(1)
                .context("FTS5 token count exceeds document-size range")?;
            postings.push(FtsPosting {
                term: term_rank,
                identity_sort_key,
                column_index: u8::try_from(column_index)?,
                column_sort_key: column_sort_keys[column_index],
                offset: row.get(2)?,
            });
        }
        drop(rows);
        drop(stmt);
        postings.sort_unstable_by_key(|posting| {
            (
                posting.term,
                posting.identity_sort_key,
                posting.column_sort_key,
                posting.offset,
            )
        });
        let mut encoded_rows = Vec::with_capacity(256 * 1024);
        for posting in postings {
            let identity = match spec.identity_kind {
                FtsIdentityKind::NodeHash => FtsRowIdentityRef::NodeHash(posting.identity_sort_key),
                FtsIdentityKind::DocId => {
                    let rank = usize::try_from(posting.identity_sort_key)?;
                    FtsRowIdentityRef::DocId(
                        owner_ids
                            .get(rank)
                            .context("sorted FTS5 posting lost its document identity")?,
                    )
                }
            };
            append_fts_posting(
                &mut encoded_rows,
                terms
                    .get(posting.term as usize)
                    .context("sorted FTS5 posting lost its term")?,
                identity,
                spec.columns[posting.column_index as usize].as_bytes(),
                posting.offset,
            );
            if encoded_rows.len() >= 256 * 1024 {
                h.update(&encoded_rows);
                encoded_rows.clear();
            }
        }
        h.update(&encoded_rows);
        validate_fts_docsize(&spec, &documents)?;
    } else {
        let mut stmt = conn.prepare(&format!("SELECT * FROM {table} ORDER BY {order_by}"))?;
        let columns = stmt.column_count();
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            encode_row(&mut h, row, columns)?;
        }
    }
    Ok(hex::encode(h.finalize()))
}
fn root(conn: &Connection) -> Result<String> {
    let mut h = Sha256::new();
    h.update(ALGORITHM);
    let mut stmt=conn.prepare("SELECT domain,CASE WHEN typeof(digest)='blob' THEN lower(hex(digest)) ELSE digest END FROM domain_commitments WHERE owner_id=0 ORDER BY domain")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (k, v) = row?;
        h.update((k.len() as u64).to_le_bytes());
        h.update(k);
        h.update(v);
    }
    let mut stmt=conn.prepare("SELECT key,value FROM metadata WHERE key IN('schema_version','index_semantics_version','indexer_engine','workspace_root','adapter','corpus_hash','commitment_algorithm','inventory_snapshot','manifest_digest')ORDER BY key")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (k, v) = row?;
        h.update((k.len() as u64).to_le_bytes());
        h.update(k);
        h.update((v.len() as u64).to_le_bytes());
        h.update(v);
    }
    Ok(hex::encode(h.finalize()))
}
fn aggregate(table: &str, leaves: &BTreeMap<String, String>) -> String {
    let mut h = Sha256::new();
    h.update(ALGORITHM);
    h.update(table);
    for (k, v) in leaves {
        h.update((k.len() as u64).to_le_bytes());
        h.update(k);
        h.update(v);
    }
    hex::encode(h.finalize())
}

fn leaf_owner<'a>(key: &'a str, prefix: &str) -> Result<std::borrow::Cow<'a, str>> {
    let encoded = key
        .strip_prefix(prefix)
        .and_then(|s| s.strip_suffix(']'))
        .context("invalid commitment leaf key")?;
    Ok(match serde_json::from_str::<&str>(encoded) {
        Ok(owner) => std::borrow::Cow::Borrowed(owner),
        Err(_) => std::borrow::Cow::Owned(serde_json::from_str::<String>(encoded)?),
    })
}

fn batch_insert_commitments(
    conn: &Connection,
    table: &str,
    entries: &BTreeMap<String, String>,
) -> Result<()> {
    let chunk_size =
        (usize::try_from(conn.limit(rusqlite::limits::Limit::SQLITE_LIMIT_VARIABLE_NUMBER))? / 3)
            .min(250);
    anyhow::ensure!(
        chunk_size > 0,
        "SQLite variable limit cannot fit a commitment row"
    );
    let prefix = format!("leaf:[\"{table}\",");
    let mut paths: HashMap<String, i64> = conn
        .prepare_cached("SELECT path,path_id FROM path_dictionary")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let missing: BTreeSet<_> = entries
        .keys()
        .map(|key| leaf_owner(key, &prefix))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|owner| !paths.contains_key(owner.as_ref()))
        .collect();
    if !missing.is_empty() {
        let missing: Vec<_> = missing.into_iter().collect();
        for chunk in missing.chunks(chunk_size) {
            let sql = format!(
                "INSERT OR IGNORE INTO path_dictionary(path) VALUES{}",
                std::iter::repeat_n("(?)", chunk.len())
                    .collect::<Vec<_>>()
                    .join(",")
            );
            conn.prepare_cached(&sql)?
                .execute(rusqlite::params_from_iter(
                    chunk.iter().map(|path| path.as_ref()),
                ))?;
        }
        paths = conn
            .prepare_cached("SELECT path,path_id FROM path_dictionary")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
    }
    let mut chunk = Vec::with_capacity(chunk_size);
    for (k, v) in entries {
        let path = leaf_owner(k, &prefix)?;
        let owner_id = *paths
            .get(path.as_ref())
            .context("missing commitment owner path")?;
        let mut digest = [0u8; 32];
        hex::decode_to_slice(v, &mut digest)?;
        chunk.push((owner_id, digest));
        if chunk.len() == chunk_size {
            insert_commitment_chunk(conn, table, &chunk)?;
            chunk.clear();
        }
    }
    if !chunk.is_empty() {
        insert_commitment_chunk(conn, table, &chunk)?;
    }
    Ok(())
}

fn insert_commitment_chunk(
    conn: &Connection,
    table: &str,
    chunk: &[(i64, [u8; 32])],
) -> Result<()> {
    if chunk.is_empty() {
        return Ok(());
    }
    let mut sql = String::from("INSERT OR REPLACE INTO domain_commitments VALUES");
    for i in 0..chunk.len() {
        if i > 0 {
            sql.push(',');
        }
        sql.push_str("(?,?,?)");
    }
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut params_vec: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(chunk.len() * 3);
    for (k, v) in chunk {
        params_vec.push(&table);
        params_vec.push(k);
        params_vec.push(v);
    }
    stmt.execute(rusqlite::params_from_iter(params_vec))?;
    Ok(())
}
fn stored(conn: &Connection, table: &str) -> Result<BTreeMap<String, String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT p.path,CASE WHEN typeof(d.digest)='blob' THEN lower(hex(d.digest)) ELSE d.digest END FROM domain_commitments d LEFT JOIN path_dictionary p ON p.path_id=d.owner_id WHERE d.domain=?1 AND d.owner_id!=0",
    )?;
    let rows = stmt
        .query_map([table], |r| {
            let owner: String = r.get(0)?;
            Ok((key(table, &owner), r.get(1)?))
        })?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
    Ok(rows)
}
/// Recomputes every owner leaf and stores the deterministic output root.
///
/// # Errors
///
/// Fails if indexed rows or dictionary references are malformed, or SQLite fails.
pub fn seal(conn: &Connection) -> Result<String> {
    seal_owners(conn, None)
}

/// Reseals selected owner leaves and recomputes their domain and output roots.
///
/// `None` seals the complete index. The caller owns the SQLite transaction and
/// must keep the source rows stable until the seal has completed.
///
/// # Errors
///
/// Fails on malformed graph references, hashing errors, or SQLite failures.
pub fn seal_owners(conn: &Connection, owners: Option<&BTreeSet<String>>) -> Result<String> {
    conn.execute(
        "INSERT OR REPLACE INTO metadata VALUES('commitment_algorithm',?1)",
        [ALGORITHM],
    )?;
    let encoded_owners = owners.map(serde_json::to_string).transpose()?;
    for table in DOMAINS {
        let changed = leaves(conn, table, owners)?;
        if let Some(encoded) = &encoded_owners {
            conn.execute(
                "DELETE FROM domain_commitments WHERE domain=?1 AND owner_id IN(SELECT path_id FROM path_dictionary WHERE path IN(SELECT value FROM json_each(?2)))",
                params![table, encoded],
            )?;
        } else {
            conn.execute(
                "DELETE FROM domain_commitments WHERE domain=?1 AND owner_id!=0",
                [table],
            )?;
        }
        batch_insert_commitments(conn, table, &changed)?;
        let digest = if owners.is_none() {
            aggregate(table, &changed)
        } else {
            aggregate(table, &stored(conn, table)?)
        };
        conn.prepare_cached("INSERT OR REPLACE INTO domain_commitments VALUES(?1,0,unhex(?2))")?
            .execute(params![table, digest])?;
    }
    for &(table, order_by) in FTS {
        conn.execute(
            "INSERT OR REPLACE INTO domain_commitments VALUES(?1,0,unhex(?2))",
            params![table, fts_digest(conn, table, order_by)?],
        )?;
    }
    let root = root(conn)?;
    conn.execute(
        "INSERT OR REPLACE INTO metadata VALUES('output_root',?1)",
        [&root],
    )?;
    Ok(root)
}
/// Verifies every stored owner leaf, domain root, and output root.
///
/// # Errors
///
/// Returns a mismatch error when the committed index differs from its seal.
pub fn verify(conn: &Connection) -> Result<()> {
    verify_owners(conn, None)
}
/// Verifies selected owner leaves and all affected domain and output roots.
///
/// `None` verifies the complete index.
///
/// # Errors
///
/// Returns a mismatch error when the stored commitment differs from source rows.
pub fn verify_owners(conn: &Connection, owners: Option<&BTreeSet<String>>) -> Result<()> {
    let algorithm: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='commitment_algorithm'",
        [],
        |r| r.get(0),
    )?;
    if algorithm != ALGORITHM {
        bail!("incompatible commitment algorithm; rebuild index");
    }
    for table in DOMAINS {
        let actual = leaves(conn, table, owners)?;
        let all = stored(conn, table)?;
        let expected = if let Some(owners) = owners {
            let keys: BTreeSet<_> = owners.iter().map(|o| key(table, o)).collect();
            all.iter()
                .filter(|(k, _)| keys.contains(*k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        } else {
            all.clone()
        };
        if actual != expected {
            bail!("commitment mismatch: {table}");
        }
        let stored: String = conn.query_row(
            "SELECT CASE WHEN typeof(digest)='blob' THEN lower(hex(digest)) ELSE digest END FROM domain_commitments WHERE domain=?1 AND owner_id=0",
            [table],
            |r| r.get(0),
        )?;
        if aggregate(table, &all) != stored {
            bail!("domain root mismatch: {table}");
        }
    }
    for &(table, order_by) in FTS {
        let stored: String = conn.query_row(
            "SELECT CASE WHEN typeof(digest)='blob' THEN lower(hex(digest)) ELSE digest END FROM domain_commitments WHERE domain=?1 AND owner_id=0",
            [table],
            |r| r.get(0),
        )?;
        if fts_digest(conn, table, order_by)? != stored {
            bail!("FTS commitment mismatch: {table}");
        }
    }
    let stored: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='output_root'",
        [],
        |r| r.get(0),
    )?;
    if root(conn)? != stored {
        bail!("output root mismatch");
    }
    Ok(())
}
