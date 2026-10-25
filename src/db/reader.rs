use super::paging;
use crate::core::response::{Detail, QueryOptions, MAX_PAGE_SIZE};
use anyhow::{bail, ensure, Context, Result};
use rusqlite::backup::Backup;
use rusqlite::{types::ValueRef, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Map, Value};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    ops::Deref,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

#[path = "reader/selectors.rs"]
mod selectors;
pub(crate) use selectors::{select_detail, select_detail_with_path};

const MAX_SQLITE_VALUE_BYTES: i32 = 8 * 1024 * 1024;
const MAX_QUERY_RESULT_BYTES: usize = MAX_SQLITE_VALUE_BYTES as usize;
const QUERY_RESULT_TOO_LARGE: &str = "query result exceeds the 8 MiB row budget; narrow the selector or path, request detail='compact' with a smaller limit, or select fewer and smaller SQL columns. Continue paged requests with the returned offset and generation";
const VALID_OVERVIEW_ASPECTS: [&str; 11] = [
    "counts",
    "components",
    "languages",
    "coverage_by_workspace",
    "cycles",
    "compiled_profiles",
    "metadata",
    "discovery",
    "manifest_components",
    "documentation_hubs",
    "milestones",
];

/// A selector failure that callers can handle without parsing diagnostic text.
#[derive(Debug)]
pub enum SelectorError {
    /// No indexed target matched the selector.
    NotFound {
        /// The requested selector.
        selector: String,
        /// Indexed directory paths that can narrow the request.
        suggestions: Vec<String>,
    },
    /// More than one indexed target matched the selector.
    Ambiguous {
        /// The requested selector.
        selector: String,
        /// Bounded indexed candidates that can disambiguate the target.
        candidates: Vec<AmbiguousCandidate>,
    },
    /// More than one indexed directory path matched a path alias.
    AmbiguousPaths {
        /// The requested path alias.
        selector: String,
        /// Matching workspace-relative paths.
        candidates: Vec<String>,
    },
    /// The selector cannot be interpreted safely.
    InvalidSyntax {
        /// The requested selector.
        selector: String,
        /// The syntax problem.
        reason: String,
    },
    /// A documentation path was supplied to a code selector.
    DocLink {
        /// The documentation target.
        target: String,
    },
}

/// Indexed metadata for one ambiguous selector candidate.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AmbiguousCandidate {
    /// Exact indexed node ID.
    pub id: String,
    /// Workspace-relative file path.
    pub path: String,
    /// Source start line.
    pub line: i64,
    /// Qualified symbol name.
    pub qualname: String,
    /// Indexed declaration signature.
    pub signature: String,
}

impl std::fmt::Display for SelectorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound {
                selector,
                suggestions,
            } if suggestions.is_empty() => write!(formatter, "selector not found: {selector}; use code_map_search to find an indexed symbol id, code_map_overview to inspect indexed paths, or get_doc for Markdown files. File paths are passed without a file: prefix"),
            Self::NotFound {
                selector,
                suggestions,
            } => write!(formatter, "selector not found: {selector}; indexed path suggestions: {}; use a workspace-relative directory or a code_map_search node id", serde_json::to_string(suggestions).map_err(|_| std::fmt::Error)?),
            Self::Ambiguous {
                selector,
                candidates,
            } => write!(formatter, "ambiguous selector {selector}; use an exact node id or workspace-relative path; candidates: {}", serde_json::to_string(&candidates.iter().map(|candidate| candidate.id.as_str()).collect::<Vec<_>>()).map_err(|_| std::fmt::Error)?),
            Self::AmbiguousPaths {
                selector,
                candidates,
            } => write!(formatter, "ambiguous selector path {selector}; use a workspace-relative path; candidates: {}", serde_json::to_string(candidates).map_err(|_| std::fmt::Error)?),
            Self::InvalidSyntax { selector, reason } if selector.trim().is_empty() => write!(formatter, "selector is empty; {reason}"),
            Self::InvalidSyntax { selector, reason } => write!(formatter, "invalid selector {selector}: {reason}"),
            Self::DocLink { target } => write!(formatter, "'{target}' is a Markdown file; use get_doc or search_docs to inspect documentation"),
        }
    }
}

impl std::error::Error for SelectorError {}

struct JsonSizeCounter {
    bytes: usize,
    limit: usize,
    exceeded: bool,
}

impl JsonSizeCounter {
    fn new(limit: usize) -> Self {
        Self {
            bytes: 0,
            limit,
            exceeded: false,
        }
    }
}

impl Write for JsonSizeCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes) {
            self.exceeded = true;
            return Err(std::io::Error::other("serialized JSON exceeds budget"));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct IndexRebuildRequired(&'static str);

impl std::fmt::Display for IndexRebuildRequired {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for IndexRebuildRequired {}

/// Selects overview sections and their common page.
pub struct OverviewOptions<'a> {
    /// Optional set of requested overview sections.
    pub aspects: Option<&'a [String]>,
    /// Bounded page and generation contract.
    pub page: &'a QueryOptions,
}

/// Selects explanation direction, linked documentation, and paging.
pub struct ExplainOptions<'a> {
    /// Incoming, outgoing, or both directions.
    pub direction: Option<&'a str>,
    /// Whether linked documentation is included.
    pub show_doc: bool,
    /// Bounded page and generation contract.
    pub page: &'a QueryOptions,
}

/// Filters and paging for document search.
pub struct DocSearchOptions<'a> {
    /// Optional document type.
    pub doc_type: Option<&'a str>,
    /// Optional workspace component or alias.
    pub component: Option<&'a str>,
    /// Whether to include bounded matching excerpts.
    pub include_excerpt: bool,
    /// Bounded page and generation contract.
    pub page: &'a QueryOptions,
}

static INTEGRITY_SNAPSHOT_NONCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn validate_database_header(path: &Path) -> Result<()> {
    let mut header = [0; 16];
    File::open(path)?.read_exact(&mut header)?;
    if &header != b"SQLite format 3\0" {
        bail!("database has an invalid SQLite header");
    }
    Ok(())
}

struct IntegritySnapshot {
    directory: PathBuf,
    path: PathBuf,
}

impl IntegritySnapshot {
    fn create() -> Result<Self> {
        for _ in 0..128 {
            let nonce = INTEGRITY_SNAPSHOT_NONCE.fetch_add(1, Ordering::Relaxed);
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let directory = std::env::temp_dir().join(format!(
                "contextunity-forge-integrity-{}-{timestamp}-{nonce}",
                std::process::id()
            ));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            builder.mode(0o700);
            match builder.create(&directory) {
                Ok(()) => {
                    let snapshot = Self {
                        path: directory.join("snapshot.sqlite"),
                        directory,
                    };
                    let mut options = OpenOptions::new();
                    options.read(true).write(true).create_new(true);
                    #[cfg(unix)]
                    options.mode(0o600);
                    match options.open(&snapshot.path) {
                        Ok(file) => {
                            drop(file);
                            return Ok(snapshot);
                        }
                        Err(error) => {
                            drop(snapshot);
                            return Err(error.into());
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        bail!("could not create a unique integrity-check snapshot")
    }
}

impl Drop for IntegritySnapshot {
    fn drop(&mut self) {
        for suffix in ["", "-journal", "-wal", "-shm"] {
            let mut path = self.path.as_os_str().to_owned();
            path.push(suffix);
            let _ = fs::remove_file(PathBuf::from(path));
        }
        let _ = fs::remove_dir(&self.directory);
    }
}

pub(crate) fn validate_database_integrity(conn: &Connection) -> Result<()> {
    let source_integrity: String =
        conn.query_row("PRAGMA integrity_check(sqlite_schema)", [], |row| {
            row.get(0)
        })?;
    if source_integrity != "ok" {
        bail!("database integrity check failed: {source_integrity}");
    }
    let snapshot = IntegritySnapshot::create()?;
    let mut copy = Connection::open_with_flags(
        &snapshot.path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    {
        let backup = Backup::new(conn, &mut copy)?;
        backup.run_to_completion(32, Duration::from_millis(5), None)?;
    }
    copy.execute_batch("PRAGMA cache_size=-4096; PRAGMA temp_store=FILE;")?;
    let integrity: String = copy.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        bail!("database integrity check failed: {integrity}");
    }
    Ok(())
}

/// Read connection held together with its generation guard lock.
pub struct LockedConnection {
    connection: Connection,
    _lock: File,
}

impl Deref for LockedConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}

pub(crate) fn open_locked(path: &Path, root: &Path) -> Result<LockedConnection> {
    let lock = super::cache::shared_lock(path)?;
    Ok(LockedConnection {
        connection: open(path, root)?,
        _lock: lock,
    })
}

/// Opens a read-only SQLite snapshot after validating its workspace and Merkle seal.
///
/// The connection starts a deferred transaction so subsequent queries see one
/// generation. A verified file identity may use the cached integrity receipt.
///
/// # Errors
///
/// Rejects incompatible, corrupt, changed, or foreign workspace databases.
pub fn open(path: &Path, root: &Path) -> Result<Connection> {
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        bail!("database must not be a symlink");
    }
    let before = super::cache::identity(path)?;
    validate_database_header(path)?;
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    conn.execute_batch(
        "PRAGMA query_only=ON; PRAGMA trusted_schema=OFF; PRAGMA mmap_size=268435456;",
    )?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    conn.execute_batch("BEGIN DEFERRED")?;
    let has_metadata_table: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='metadata')",
        [],
        |r| r.get(0),
    )?;
    if !has_metadata_table {
        validate_database_integrity(&conn)?;
        return Err(
            IndexRebuildRequired("legacy database missing metadata table; rebuild index").into(),
        );
    }
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
    if version != crate::core::config::ENGINE_SCHEMA_VERSION
        || engine != "contextunity-forge-mcp-rust"
    {
        validate_database_integrity(&conn)?;
        return Err(IndexRebuildRequired("incompatible database schema or engine").into());
    }
    validate_workspace(&conn, root)?;
    let semantics: Option<String> = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='index_semantics_version'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if semantics.as_deref() != Some(crate::core::config::INDEX_SEMANTICS_VERSION) {
        validate_database_integrity(&conn)?;
        return Err(IndexRebuildRequired("incompatible index semantics; rebuild index").into());
    }
    let algorithm: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='commitment_algorithm'",
        [],
        |r| r.get(0),
    )?;
    if algorithm != crate::core::commitments::ALGORITHM {
        validate_database_integrity(&conn)?;
        return Err(
            IndexRebuildRequired("incompatible commitment algorithm; rebuild index").into(),
        );
    }
    let seal: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='output_root'",
        [],
        |r| r.get(0),
    )?;
    let receipt_missing = !super::cache::matches(path, root, &seal, &before);
    if receipt_missing {
        crate::core::commitments::verify(&conn)?;
    }
    if super::cache::identity(path)? != before {
        bail!("database changed during reader admission");
    }
    if receipt_missing {
        let _ = super::cache::publish_verified_identity(path, root, &seal, &before);
    }
    conn.set_limit(
        rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH,
        MAX_SQLITE_VALUE_BYTES,
    );
    conn.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_SQL_LENGTH, 64 * 1024);
    Ok(conn)
}

/// Decodes one persisted local facts payload for integration-level storage checks.
///
/// # Errors
///
/// Returns an error when the payload is corrupt or uses an unsupported encoding.
#[doc(hidden)]
pub fn decode_local_facts(blob: &[u8]) -> Result<crate::core::models::Facts> {
    Ok(crate::core::typed_facts::decode_facts(blob)?.into_public())
}

pub(crate) fn inventory_snapshot(conn: &Connection) -> Result<Vec<crate::core::models::FileEntry>> {
    // Sealed inventory metadata can exceed the public SQL cell limit; restore it before tool queries.
    let previous_limit = conn.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH, i32::MAX);
    let inventory = conn.query_row(
        "SELECT value FROM metadata WHERE key='inventory_snapshot'",
        [],
        |row| row.get::<_, String>(0),
    );
    conn.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH, previous_limit);
    serde_json::from_str(&inventory.context("cannot read inventory snapshot")?)
        .context("invalid inventory snapshot")
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

// Directory selectors are resolved from indexed paths. A suffix is accepted only when it
// identifies one directory; otherwise the caller must supply a workspace-relative path.
pub(crate) fn directory_path(conn: &Connection, selector: &str) -> Result<Option<String>> {
    let selector = selector.trim();
    let unprefixed = selector
        .strip_prefix("file://")
        .or_else(|| selector.strip_prefix("file:"))
        .unwrap_or(selector);
    let selector = unprefixed.strip_prefix("./").unwrap_or(unprefixed);
    let selector = selector.trim_end_matches('/');
    if selector.is_empty()
        || selector.starts_with('/')
        || selector
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(SelectorError::InvalidSyntax {
            selector: selector.to_owned(),
            reason: "directory path must be workspace-relative without empty, '.' or '..' segments"
                .into(),
        }
        .into());
    }
    let (lower, upper) = path_bounds(selector);
    let exact: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM files WHERE path>=?1 AND path<?2)",
        [&lower, &upper],
        |row| row.get(0),
    )?;
    if exact {
        return Ok(Some(selector.to_owned()));
    }
    let needle = format!("/{selector}/");
    let mut statement = conn.prepare(
        "SELECT DISTINCT substr(path,1,instr(path,?1)+length(?1)-2) AS directory \
         FROM files WHERE instr(path,?1)>0 ORDER BY directory LIMIT 11",
    )?;
    let candidates = statement
        .query_map([&needle], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    match candidates.as_slice() {
        [] => Ok(None),
        [path] => Ok(Some(path.clone())),
        _ => Err(SelectorError::AmbiguousPaths {
            selector: selector.to_owned(),
            candidates,
        }
        .into()),
    }
}

pub(crate) fn indexed_path_suggestions(conn: &Connection, selector: &str) -> Result<Vec<String>> {
    let name = selector
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("");
    if name.len() < 2 {
        return Ok(Vec::new());
    }
    let mut statement =
        conn.prepare("SELECT path FROM files WHERE instr(path,?1)>0 ORDER BY path LIMIT 5")?;
    let suggestions = statement
        .query_map([name], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(suggestions)
}

/// Executes bounded read-only SQL and converts rows to JSON values.
///
/// Limits the query to two seconds, the supplied row count, and an 8 MiB
/// serialized result. Each SQLite value must also fit the configured cell limit.
///
/// # Errors
///
/// Rejects writes, oversized queries or results, timeouts, and SQLite errors.
pub fn rows(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
    limit: usize,
) -> Result<Vec<Value>> {
    rows_with_mode(conn, sql, params, limit, RowMode::Structural)
}

pub(crate) fn document_contents_by_ids(
    conn: &Connection,
    doc_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>> {
    if doc_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    ensure!(
        doc_ids.len() <= MAX_PAGE_SIZE,
        "document content lookup exceeds the inspected page bound"
    );
    let placeholders = (1..=doc_ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT doc_id,content FROM doc_sections WHERE doc_id IN ({placeholders}) ORDER BY doc_id"
    );
    let params = doc_ids
        .iter()
        .map(|doc_id| doc_id as &dyn rusqlite::ToSql)
        .collect::<Vec<_>>();
    let rows = rows(conn, &sql, &params, doc_ids.len())?;
    let mut contents = BTreeMap::new();
    for row in rows {
        let doc_id = row
            .get("doc_id")
            .and_then(Value::as_str)
            .context("document content lookup returned a row without doc_id")?;
        let content = row
            .get("content")
            .and_then(Value::as_str)
            .context("document content lookup returned a row without content")?;
        contents.insert(doc_id.to_owned(), content.to_owned());
    }
    ensure!(
        contents.len() == doc_ids.len(),
        "document content lookup could not resolve every inspected section"
    );
    Ok(contents)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RowMode {
    Structural,
    Opaque,
}

pub(super) fn rows_with_mode(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
    limit: usize,
    mode: RowMode,
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
    let mut total_bytes = 2usize;
    while let Some(row) = query.next()? {
        let mut value = Map::new();
        let mut minimum_row_bytes = 2usize;
        let mut property_count = 0usize;
        for (i, name) in names.iter().enumerate() {
            if mode == RowMode::Structural
                && matches!(
                    name.as_str(),
                    "node_hash" | "path_hash" | "path_id" | "owner_path_id"
                )
            {
                continue;
            }
            if names[i + 1..].iter().any(|later| later == name) {
                value.entry(name.clone()).or_insert(Value::Null);
                continue;
            }
            let raw = row.get_ref(i)?;
            let special_json_column = mode == RowMode::Structural
                && matches!(
                    name.as_str(),
                    "details" | "invariants" | "referenced_symbols"
                );
            let minimum_value_bytes = match raw {
                ValueRef::Null => 4,
                ValueRef::Integer(number) => number.to_string().len(),
                ValueRef::Real(_) => 1,
                ValueRef::Text(_) if special_json_column => 1,
                ValueRef::Text(bytes) => bytes.len().saturating_add(2),
                ValueRef::Blob(bytes) => bytes.len().saturating_mul(2).saturating_add(2),
            };
            let minimum_key_bytes = serde_json::to_vec(name)?.len().saturating_add(1);
            minimum_row_bytes = minimum_row_bytes
                .saturating_add(usize::from(property_count > 0))
                .saturating_add(minimum_key_bytes)
                .saturating_add(minimum_value_bytes);
            property_count += 1;
            let row_separator_bytes = usize::from(!result.is_empty());
            let available_row_bytes = MAX_QUERY_RESULT_BYTES
                .saturating_sub(total_bytes)
                .saturating_sub(row_separator_bytes);
            if minimum_row_bytes > available_row_bytes {
                bail!(QUERY_RESULT_TOO_LARGE);
            }

            let cell = match raw {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(v) => json!(v),
                ValueRef::Real(v) => json!(v),
                ValueRef::Text(v) => {
                    let s = String::from_utf8_lossy(v);
                    if special_json_column {
                        serde_json::from_str(&s).unwrap_or_else(|_| json!(s))
                    } else {
                        json!(s)
                    }
                }
                ValueRef::Blob(v) => json!(hex::encode(v)),
            };
            value.insert(name.clone(), cell);
        }

        let row_separator_bytes = usize::from(!result.is_empty());
        let available_row_bytes = MAX_QUERY_RESULT_BYTES
            .saturating_sub(total_bytes)
            .saturating_sub(row_separator_bytes);
        let mut row_size = JsonSizeCounter::new(available_row_bytes);
        if let Err(error) = serde_json::to_writer(&mut row_size, &value) {
            if row_size.exceeded {
                bail!(QUERY_RESULT_TOO_LARGE);
            }
            return Err(error.into());
        }
        total_bytes = total_bytes
            .saturating_add(row_separator_bytes)
            .saturating_add(row_size.bytes);
        result.push(Value::Object(value));
        if result.len() >= limit {
            break;
        }
    }
    Ok(result)
}

/// Resolves one fail-closed code selector and returns its full node projection.
///
/// # Errors
///
/// Returns a typed [`SelectorError`] for missing, ambiguous, or invalid targets.
pub fn select(conn: &Connection, selector: &str) -> Result<Value> {
    select_detail(conn, selector, Detail::Full)
}
fn is_code_path(s: &str) -> bool {
    const EXTENSIONS: &[&str] = &[
        ".py", ".rs", ".ts", ".tsx", ".js", ".jsx", ".vue", ".go", ".c", ".cpp", ".cc", ".cxx",
        ".h", ".hpp", ".cs", ".java", ".kt", ".rb", ".php", ".proto", ".json", ".yaml", ".yml",
        ".toml", ".sql", ".sh", ".bash",
    ];
    EXTENSIONS.iter().any(|ext| s.ends_with(ext))
}

/// Read a bounded overview of selected workspace sections.
pub fn overview_with_options(conn: &Connection, request: &OverviewOptions<'_>) -> Result<Value> {
    let aspects = request.aspects;
    let options = request.page;
    let generation = paging::generation(conn, options)?;
    if let Some(aspect) = aspects.unwrap_or_default().iter().find(|aspect| {
        !VALID_OVERVIEW_ASPECTS
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(aspect))
    }) {
        bail!(
            "unsupported overview aspect '{aspect}'; allowed aspects: {}",
            VALID_OVERVIEW_ASPECTS.join(", ")
        );
    }
    let has_aspect = |name: &str| -> bool {
        match aspects {
            None => name != "cycles",
            Some(list) => {
                if list.is_empty() {
                    name != "cycles"
                } else {
                    list.iter().any(|a| a.eq_ignore_ascii_case(name))
                }
            }
        }
    };

    let mut result = serde_json::Map::new();

    if has_aspect("components") {
        let components = paging::query(
            conn,
            "SELECT n.id,n.name,p.path FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.kind='component' ORDER BY p.path,n.id",
            &[],
            options,
        )?;
        result.insert("components".into(), components);
    }

    if has_aspect("counts") {
        let counts = rows(
            conn,
            "SELECT (SELECT count(*) FROM files) files,(SELECT count(*) FROM nodes) nodes,(SELECT count(*) FROM edges) edges,(SELECT count(*) FROM doc_sections) doc_sections,(SELECT count(*) FROM errors) parse_errors,(SELECT count(*) FROM resolution_coverage WHERE status IN('unresolved','ambiguous')) unresolved,(SELECT count(*) FROM resolution_coverage WHERE status='external') external_imports",
            &[],
            1,
        )?.remove(0);
        result.insert("counts".into(), counts);
    }

    if has_aspect("languages") {
        let lang_sql = "SELECT l.language,coalesce(fc.files,0) files,coalesce(r.resolved,0) resolved,coalesce(r.ambiguous,0) ambiguous,coalesce(r.unresolved,0) unresolved,coalesce(r.external_imports,0) external_imports,coalesce(e.parse_errors,0) parse_errors FROM (SELECT language FROM files UNION SELECT language FROM nodes WHERE kind='island_scope' UNION SELECT language FROM coverage_owner_language) l LEFT JOIN (SELECT language,count(*) files FROM files GROUP BY language) fc ON fc.language=l.language LEFT JOIN (SELECT language,sum(CASE WHEN status='resolved' THEN records ELSE 0 END) resolved,sum(CASE WHEN status='ambiguous' THEN records ELSE 0 END) ambiguous,sum(CASE WHEN status='unresolved' THEN records ELSE 0 END) unresolved,sum(CASE WHEN status='external' THEN records ELSE 0 END) external_imports FROM coverage_language_counts GROUP BY language) r ON r.language=l.language LEFT JOIN (SELECT f.language,count(*) parse_errors FROM errors er JOIN files f ON f.path=er.path GROUP BY f.language) e ON e.language=l.language ORDER BY l.language";
        let language_total = paging::count(conn, "SELECT count(*) FROM (SELECT language FROM files UNION SELECT language FROM nodes WHERE kind='island_scope' UNION SELECT language FROM coverage_owner_language)", &[])?;
        let limit = options.limit as i64;
        let offset = options.offset as i64;
        let languages = rows(
            conn,
            &format!("{lang_sql} LIMIT ?1 OFFSET ?2"),
            &[&limit, &offset],
            options.limit,
        )?;
        result.insert(
            "languages".into(),
            paging::value(languages, language_total, options, &generation),
        );
    }

    if has_aspect("coverage_by_workspace") {
        let scopes = workspace_coverage(conn, options, &generation)?;
        result.insert("coverage_by_workspace".into(), scopes);
    }

    if has_aspect("compiled_profiles") {
        let compiled_profiles: std::collections::BTreeSet<_> = crate::engine::languages::profiles()
            .map(|p| p.id())
            .collect();
        result.insert(
            "compiled_profiles".into(),
            serde_json::to_value(compiled_profiles)?,
        );
    }

    if has_aspect("cycles") {
        let cycles = super::cycles::summary(conn, None)?;
        result.insert("cycles".into(), cycles);
    }

    if has_aspect("metadata") {
        let metadata = rows(
            conn,
            "SELECT key,value FROM metadata WHERE key IN('schema_version','workspace_root') ORDER BY key",
            &[],
            2,
        )?;
        result.insert("metadata".into(), serde_json::Value::Array(metadata));
    }

    result.insert("generation".into(), serde_json::Value::String(generation));

    Ok(serde_json::Value::Object(result))
}

/// Pages one indexed entry point per Markdown document for workspace discovery.
///
/// These rows come only from the existing document index; this read path does not
/// scan the filesystem or promote unresolved documentation candidates.
pub(crate) fn documentation_hubs(conn: &Connection, options: &QueryOptions) -> Result<Value> {
    paging::query(
        conn,
        "SELECT d.doc_id,d.path,d.section_title,d.doc_type,n.line FROM doc_sections d LEFT JOIN nodes n ON n.id=d.doc_id WHERE d.rowid IN (SELECT min(rowid) FROM doc_sections GROUP BY path) ORDER BY d.path,d.doc_id",
        &[],
        options,
    )
}

fn workspace_coverage(
    conn: &Connection,
    options: &QueryOptions,
    generation: &str,
) -> Result<Value> {
    let root: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='workspace_root'",
        [],
        |row| row.get(0),
    )?;
    let linked_json: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='adapter_linked_workspaces'",
            [],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or_else(|| "[]".into());
    let linked: Value = serde_json::from_str(&linked_json)?;
    let mut scopes = vec![json!({"name":"primary","root":root,"path_prefix":""})];
    if let Some(workspaces) = linked.as_array() {
        for workspace in workspaces {
            let (Some(name), Some(path)) = (
                workspace.get("name").and_then(Value::as_str),
                workspace.get("path").and_then(Value::as_str),
            ) else {
                continue;
            };
            scopes.push(json!({
                "name": name,
                "root": path,
                "path_prefix": format!("[{name}]/")
            }));
        }
    }
    scopes.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    let total = scopes.len();
    let linked_json = serde_json::to_string(&linked)?;
    let mut items = Vec::with_capacity(options.limit.min(total.saturating_sub(options.offset)));
    for scope in scopes.into_iter().skip(options.offset).take(options.limit) {
        let prefix = scope["path_prefix"].as_str().unwrap_or_default();
        let counts = if prefix.is_empty() {
            rows(
                conn,
                "SELECT coalesce(sum(CASE WHEN c.status='resolved' THEN c.records ELSE 0 END),0) resolved,coalesce(sum(CASE WHEN c.status='ambiguous' THEN c.records ELSE 0 END),0) ambiguous,coalesce(sum(CASE WHEN c.status='unresolved' THEN c.records ELSE 0 END),0) unresolved,coalesce(sum(CASE WHEN c.status='external' THEN c.records ELSE 0 END),0) external_imports FROM coverage_language_counts c JOIN path_dictionary p ON p.path_id=c.path_id WHERE NOT EXISTS(SELECT 1 FROM json_each(?1) linked WHERE substr(p.path,1,length('['||json_extract(linked.value,'$.name')||']/'))='['||json_extract(linked.value,'$.name')||']/')",
                &[&linked_json],
                1,
            )?
            .remove(0)
        } else {
            rows(
                conn,
                "SELECT coalesce(sum(CASE WHEN c.status='resolved' THEN c.records ELSE 0 END),0) resolved,coalesce(sum(CASE WHEN c.status='ambiguous' THEN c.records ELSE 0 END),0) ambiguous,coalesce(sum(CASE WHEN c.status='unresolved' THEN c.records ELSE 0 END),0) unresolved,coalesce(sum(CASE WHEN c.status='external' THEN c.records ELSE 0 END),0) external_imports FROM coverage_language_counts c JOIN path_dictionary p ON p.path_id=c.path_id WHERE substr(p.path,1,length(?1))=?1",
                &[&prefix],
                1,
            )?
            .remove(0)
        };
        let mut item = scope.as_object().cloned().unwrap_or_default();
        for field in ["resolved", "ambiguous", "unresolved", "external_imports"] {
            item.insert(field.into(), counts[field].clone());
        }
        item.insert("resolution_health".into(), resolution_health(&counts));
        let reasons = match workspace_diagnostic_reason_counts(conn, prefix, &linked_json) {
            Ok(counts) => json!({"availability":"available","counts":counts}),
            Err(error) if is_query_budget_exhaustion(&error) => json!({
                "availability":"unavailable",
                "cause":"canonical reason aggregation exceeded an existing SQLite query budget"
            }),
            Err(error) => return Err(error),
        };
        item.insert("diagnostic_reasons".into(), reasons);
        items.push(Value::Object(item));
    }
    Ok(paging::value(items, total, options, generation))
}

fn resolution_health(counts: &Value) -> Value {
    let status_count = |status: &str| counts[status].as_u64();
    let Some(resolved) = status_count("resolved") else {
        return json!({"availability":"unavailable","cause":"resolution status counts are outside the supported integer range"});
    };
    let Some(unresolved) = status_count("unresolved") else {
        return json!({"availability":"unavailable","cause":"resolution status counts are outside the supported integer range"});
    };
    let Some(ambiguous) = status_count("ambiguous") else {
        return json!({"availability":"unavailable","cause":"resolution status counts are outside the supported integer range"});
    };
    let Some(denominator) = resolved
        .checked_add(unresolved)
        .and_then(|total| total.checked_add(ambiguous))
    else {
        return json!({"availability":"unavailable","cause":"resolution status counts exceed the supported integer range"});
    };
    if denominator == 0 {
        return json!({"availability":"unavailable","cause":"no in-scope resolution references"});
    }
    json!({
        "availability":"available",
        "resolved":resolved,
        "denominator":denominator,
        "percent":resolved as f64 * 100.0 / denominator as f64
    })
}

fn workspace_diagnostic_reason_counts(
    conn: &Connection,
    prefix: &str,
    linked_json: &str,
) -> Result<Value> {
    let cause = resolution_cause_sql();
    let (sql, parameter) = if prefix.is_empty() {
        (
            format!(
                "SELECT c.status,{cause} cause,x.expression,e.evidence FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence e ON e.evidence_id=c.evidence_id WHERE NOT EXISTS(SELECT 1 FROM json_each(?1) linked WHERE substr(p.path,1,length('['||json_extract(linked.value,'$.name')||']/'))='['||json_extract(linked.value,'$.name')||']/')"
            ),
            linked_json,
        )
    } else {
        (
            format!(
                "SELECT c.status,{cause} cause,x.expression,e.evidence FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence e ON e.evidence_id=c.evidence_id WHERE substr(p.path,1,length(?1))=?1"
            ),
            prefix,
        )
    };

    const REASONS: &[&str] = &[
        "dynamic_callback",
        "unresolved_receiver_type",
        "framework_magic",
        "external_unindexed",
        "ambiguous_candidates",
        "shadowed_binding",
        "unknown",
    ];
    const STATUSES: &[&str] = &["resolved", "unresolved", "ambiguous", "external"];
    let mut counts: BTreeMap<String, BTreeMap<String, u64>> = REASONS
        .iter()
        .map(|reason| {
            (
                (*reason).to_owned(),
                STATUSES
                    .iter()
                    .map(|status| ((*status).to_owned(), 0))
                    .collect(),
            )
        })
        .collect();

    let budget = QueryBudget::new(conn);
    let mut statement = conn.prepare(&sql)?;
    if !statement.readonly() {
        bail!("query must be read-only");
    }
    let mut query = statement.query([parameter])?;
    while let Some(row) = query.next()? {
        budget.check()?;
        let status = row.get_ref(0)?.as_str()?;
        let cause = row.get_ref(1)?.as_str()?;
        let expression = row.get_ref(2)?.as_str()?;
        let evidence = row.get_ref(3)?.as_str()?;
        let reason = diagnostic_reason(Some(status), Some(cause), Some(expression), evidence)
            .or_else(|| (status != "resolved").then_some("unknown"));
        if let Some(reason) = reason {
            let count = counts
                .entry(reason.to_owned())
                .or_default()
                .entry(status.to_owned())
                .or_default();
            *count = count
                .checked_add(1)
                .context("diagnostic reason reference count exceeds the supported integer range")?;
        }
    }
    budget.check()?;
    Ok(json!(counts))
}

fn is_query_budget_exhaustion(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.to_string() == "graph query exceeds two second budget"
            || cause
                .downcast_ref::<rusqlite::Error>()
                .is_some_and(|error| {
                    matches!(
                        error,
                        rusqlite::Error::SqliteFailure(sqlite_error, _)
                            if matches!(
                                sqlite_error.code,
                                rusqlite::ErrorCode::OperationInterrupted | rusqlite::ErrorCode::TooBig
                            )
                    )
                })
    })
}

/// Inspects a selected declaration with bounded graph and documentation pages.
///
/// # Errors
///
/// Returns a typed [`SelectorError`] if the selector is not unique or valid.
pub fn inspect_paged(
    conn: &Connection,
    selector: &str,
    show_doc: bool,
    options: &QueryOptions,
) -> Result<Value> {
    inspect_paged_with_path(conn, selector, None, show_doc, options)
}

pub(crate) fn inspect_paged_with_path(
    conn: &Connection,
    selector: &str,
    path: Option<&str>,
    show_doc: bool,
    options: &QueryOptions,
) -> Result<Value> {
    let generation = paging::generation(conn, options)?;
    let node = select_detail_with_path(conn, selector, path, options.detail)?;
    let id = node["id"].as_str().context("invalid node id")?;
    let mut documents = if show_doc {
        paging::query(conn, &format!("SELECT {} FROM doc_sections d JOIN nodes dn ON dn.id=d.doc_id JOIN edges e ON e.dst_hash=dn.node_hash WHERE e.src_hash=?1 AND e.kind='references_doc' ORDER BY d.is_invariant DESC,d.path,d.doc_id", paging::docs("d", options.detail)), &[&crate::core::models::stable_hash64(id)], options)?
    } else {
        paging::value(Vec::new(), 0, options, &generation)
    };
    if documents["total"] == 0 {
        documents = json!({"total": 0});
    }
    let path = node["path"].as_str().context("invalid node path")?;
    let line = node["line"].as_i64().context("invalid start line")?;
    let end_line = node["end_line"].as_i64().context("invalid end line")?;
    let scope: &[&dyn rusqlite::ToSql] = &[&path, &line, &end_line];
    let mut coverage = paging::query(conn, &format!("SELECT {} FROM resolution_coverage c WHERE c.path_id=(SELECT path_id FROM path_dictionary WHERE path=?1) AND c.line BETWEEN ?2 AND ?3 ORDER BY c.line,c.expression_id,c.status,c.evidence_id", paging::coverage("c", options.detail)), scope, options)?;
    coverage["statuses"] = json!(rows(conn,
        "SELECT status,count(*) total FROM resolution_coverage WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path=?1) AND line BETWEEN ?2 AND ?3 GROUP BY status ORDER BY status",
        scope, 20)?);
    if options.detail == Detail::Compact {
        coverage["detail_hint"] = json!("detail='full' includes per-reference evidence; statuses count the entire selected scope.");
    }
    Ok(json!({"node":node, "documents":documents, "coverage":coverage, "generation":generation}))
}

/// Explain indexed relationships for a selected symbol.
pub fn explain_with_options(
    conn: &Connection,
    selector: &str,
    request: &ExplainOptions<'_>,
) -> Result<Value> {
    explain_with_path_options(conn, selector, None, request)
}

pub(crate) fn explain_with_path_options(
    conn: &Connection,
    selector: &str,
    path: Option<&str>,
    request: &ExplainOptions<'_>,
) -> Result<Value> {
    let direction = request.direction;
    let show_doc = request.show_doc;
    let options = request.page;
    let mut result = inspect_paged_with_path(conn, selector, path, show_doc, options)?;
    let id = result["node"]["id"]
        .as_str()
        .context("invalid node id")?
        .to_owned();
    let node_hash = crate::core::models::stable_hash64(&id);
    let dir = match direction.unwrap_or("both") {
        "inbound" => "incoming",
        "outbound" => "outgoing",
        direction => direction,
    };
    match dir {
        "both" | "incoming" => {
            let mut incoming = paging::query(conn, &format!("SELECT {} FROM edges e WHERE e.dst_hash=?1 ORDER BY e.kind,e.src_hash,e.path_id,e.line", paging::edges("e", options.detail)), &[&node_hash], options)?;
            annotate_edge_direction(&mut incoming, "inbound");
            result["incoming"] = incoming;
        }
        "outgoing" => {
            let total = paging::count(
                conn,
                "SELECT count(*) FROM edges WHERE dst_hash=?1",
                &[&node_hash],
            )?;
            result["incoming"] = json!({"total": total, "omitted": true, "hint": "Pass direction='incoming' to page incoming edges."});
        }
        _ => bail!("direction must be both, incoming (inbound), or outgoing (outbound)"),
    }
    match dir {
        "both" | "outgoing" => {
            let mut outgoing = paging::query(conn, &format!("SELECT {} FROM edges e WHERE e.src_hash=?1 ORDER BY e.kind,e.dst_hash,e.path_id,e.line", paging::edges("e", options.detail)), &[&node_hash], options)?;
            annotate_edge_direction(&mut outgoing, "outbound");
            result["outgoing"] = outgoing;
        }
        "incoming" => {
            let total = paging::count(
                conn,
                "SELECT count(*) FROM edges WHERE src_hash=?1",
                &[&node_hash],
            )?;
            result["outgoing"] = json!({"total": total, "omitted": true, "hint": "Pass direction='outgoing' to page outgoing edges."});
        }
        _ => {}
    }
    let implementors = rows(
        conn,
        "SELECT n.id AS src_public_id,e.kind,p.path,e.line,v.evidence,n.name,n.kind AS node_kind FROM edges e JOIN nodes n ON n.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=e.path_id JOIN coverage_evidence v ON v.evidence_id=e.evidence_id WHERE e.dst_hash=?1 AND e.kind IN('implements','overrides','extends') ORDER BY p.path,e.line",
        &[&node_hash],
        50,
    )?;
    if !implementors.is_empty() {
        result["implementors"] = json!(implementors);
    }
    let implements = rows(
        conn,
        "SELECT n.id AS dst_public_id,e.kind,p.path,e.line,v.evidence,n.name,n.kind AS node_kind FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=e.path_id JOIN coverage_evidence v ON v.evidence_id=e.evidence_id WHERE e.src_hash=?1 AND e.kind IN('implements','overrides','extends') ORDER BY p.path,e.line",
        &[&node_hash],
        50,
    )?;
    if !implements.is_empty() {
        result["implements"] = json!(implements);
    }
    result["direction"] = json!(dir);
    if let Some(gen) = result.as_object_mut().and_then(|m| m.remove("generation")) {
        if let Some(m) = result.as_object_mut() {
            m.insert("generation".into(), gen);
        }
    }
    Ok(result)
}

fn annotate_edge_direction(page: &mut Value, direction: &str) {
    if let Some(items) = page["items"].as_array_mut() {
        for item in items {
            if let Some(edge) = item.as_object_mut() {
                edge.insert("direction".into(), json!(direction));
            }
        }
    }
}

/// Resolves component prefix.
pub fn resolve_component_prefix(conn: &Connection, component: &str) -> String {
    let comp = component.trim().trim_end_matches('/');
    if comp.is_empty() {
        return String::new();
    }
    // 1. Direct path check: if any doc begins with `comp/` or equals `comp`, use it as is
    let direct_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM doc_sections WHERE path=?1 OR (path>=?2 AND path<?3))",
            rusqlite::params![comp, format!("{comp}/"), format!("{comp}0")],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if direct_exists {
        return comp.to_string();
    }

    // 2. Check metadata adapter JSON for aliases and owners
    if let Ok(raw) =
        conn.query_row::<String, _, _>("SELECT value FROM metadata WHERE key='adapter'", [], |r| {
            r.get(0)
        })
    {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
            let aliases = val.get("aliases").and_then(|v| v.as_object());
            let owners = val.get("owners").and_then(|v| v.as_object());

            // Target name: either alias resolution or component itself
            let target_owner = aliases
                .and_then(|a| a.get(comp))
                .and_then(|v| v.as_str())
                .unwrap_or(comp);

            // Find matching path in owners
            if let Some(owners_map) = owners {
                for (owner_path, owner_name) in owners_map {
                    if let Some(name_str) = owner_name.as_str() {
                        if name_str.eq_ignore_ascii_case(target_owner)
                            || name_str.eq_ignore_ascii_case(comp)
                        {
                            return owner_path.trim_end_matches('/').to_string();
                        }
                    }
                }
                // Also check if comp matches an owner path directly or ends with /{comp}
                for owner_path in owners_map.keys() {
                    if owner_path.eq_ignore_ascii_case(comp)
                        || owner_path.ends_with(&format!("/{comp}"))
                    {
                        return owner_path.trim_end_matches('/').to_string();
                    }
                }
            }
        }
    }

    // 3. Fallback: check if any doc_sections path contains `/{comp}/`
    let pattern = format!("%/{comp}/%");
    if let Ok(matched_path) = conn.query_row::<String, _, _>(
        "SELECT path FROM doc_sections WHERE path LIKE ?1 ORDER BY path LIMIT 1",
        [&pattern],
        |r| r.get(0),
    ) {
        if let Some((prefix, _)) = matched_path.split_once(&format!("/{comp}/")) {
            return format!("{prefix}/{comp}");
        }
    }

    comp.to_string()
}

/// Search indexed documentation using one bounded options contract.
pub fn search_docs_with_options(
    conn: &Connection,
    query: &str,
    request: &DocSearchOptions<'_>,
) -> Result<Value> {
    let doc_type = request.doc_type;
    let component = request.component;
    let include_excerpt = request.include_excerpt;
    let options = request.page;
    if query.trim().is_empty() {
        bail!("search query is empty");
    }
    let terms = query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .filter(|term| {
            !DOC_SEARCH_ENGLISH_STOP_WORDS
                .iter()
                .any(|stop_word| term.eq_ignore_ascii_case(stop_word))
        })
        .map(|term| format!("\"{term}\""))
        .collect::<Vec<_>>();
    if terms.is_empty() {
        let sql = format!(
            "SELECT {} FROM doc_sections d WHERE 0",
            paging::docs("d", options.detail)
        );
        return Ok(json!({"sections":paging::query(conn, &sql, &[], options)?}));
    }
    let query = terms.join(" OR ");
    let kind = doc_type.unwrap_or("");
    let resolved = resolve_component_prefix(conn, component.unwrap_or(""));
    let component = resolved.as_str();
    let (prefix, end) = path_bounds(component);
    let excerpt = if include_excerpt {
        ",substr(snippet(doc_search,-1,'[',']',' … ',16),1,240) excerpt"
    } else {
        ""
    };
    let sql = format!(
        "SELECT {},bm25(doc_search,3.0,1.0,1.0) rank{excerpt} FROM doc_search JOIN doc_sections d ON d.rowid=doc_search.rowid WHERE doc_search MATCH ?1 AND (?2='' OR d.doc_type=?2) AND (?3='' OR d.path=?3 OR (d.path>=?4 AND d.path<?5)) ORDER BY rank,d.path,d.doc_id",
        paging::docs("d", options.detail)
    );
    Ok(
        json!({"sections":paging::query(conn, &sql, &[&query,&kind,&component,&prefix,&end], options)?}),
    )
}

const DOC_SEARCH_ENGLISH_STOP_WORDS: &[&str] = &[
    "a", "about", "above", "after", "again", "against", "all", "am", "an", "and", "any", "are",
    "as", "at", "be", "because", "been", "before", "being", "below", "between", "both", "but",
    "by", "can", "could", "did", "do", "does", "doing", "down", "during", "each", "few", "for",
    "from", "further", "had", "has", "have", "having", "he", "her", "here", "hers", "him", "his",
    "how", "i", "if", "in", "into", "is", "it", "its", "just", "me", "more", "most", "my", "no",
    "nor", "not", "now", "of", "off", "on", "once", "only", "or", "other", "our", "out", "over",
    "own", "same", "she", "should", "so", "some", "such", "than", "that", "the", "their", "them",
    "then", "there", "these", "they", "this", "those", "through", "to", "too", "under", "until",
    "up", "very", "was", "we", "were", "what", "when", "where", "which", "while", "who", "whom",
    "why", "will", "with", "would", "you", "your",
];

const DOCUMENT_REFERENCE_CANDIDATES_SQL: &str = r#"
WITH source_claims AS MATERIALIZED (
    SELECT
        CAST(claim.key AS INTEGER) AS claim_index,
        json_extract(claim.value,'$.doc_id') AS source_doc_id,
        json_extract(claim.value,'$.source_path') AS source_path,
        json_extract(claim.value,'$.name') AS claim_name,
        json_extract(claim.value,'$.normalized_name') AS normalized_name,
        CAST(json_extract(claim.value,'$.line') AS INTEGER) AS source_line,
        CAST(json_extract(claim.value,'$.explicit_path') AS INTEGER) AS explicit_path,
        json_extract(claim.value,'$.target_path') AS claim_target_path,
        CAST(json_extract(claim.value,'$.suffix_indexed') AS INTEGER) AS suffix_indexed
    FROM json_each(?1) claim
),
fts_targets AS MATERIALIZED (
    SELECT n.node_id,n.id,n.name,n.qualname,n.kind,p.path,n.line
    FROM node_search
    JOIN nodes n ON n.node_hash=node_search.rowid
    JOIN path_dictionary p ON p.path_id=n.path_id
    WHERE node_search MATCH ?2
      AND n.kind NOT IN('document','invariant','component')
      AND (
        ((?3='' AND NOT (
            substr(p.path,1,1)='['
            AND instr(p.path,'/')>1
            AND substr(p.path,instr(p.path,'/')-1,1)=']'
        )) OR (?3!='' AND substr(p.path,1,length(?3))=?3
            AND substr(p.path,length(?3)+1,1)='/'))
        OR EXISTS (
            SELECT 1 FROM source_claims explicit_claim
            WHERE explicit_claim.explicit_path=1
              AND explicit_claim.claim_target_path IS NOT NULL
              AND explicit_claim.claim_target_path=p.path
        )
      )
),
candidate_rows AS (
    SELECT c.claim_index,c.source_doc_id,c.source_path,c.claim_name,c.source_line,
           c.explicit_path,c.claim_target_path,n.node_id AS target_node_id,n.id AS target_id,
           n.name AS target_name,n.qualname AS target_qualname,n.kind AS target_kind,
           p.path AS target_path,n.line AS target_line,0 AS priority
    FROM source_claims c
    JOIN nodes n INDEXED BY idx_nodes_qualname ON n.qualname=c.normalized_name
    JOIN path_dictionary p ON p.path_id=n.path_id
    UNION ALL
    SELECT c.claim_index,c.source_doc_id,c.source_path,c.claim_name,c.source_line,
           c.explicit_path,c.claim_target_path,n.node_id,n.id,n.name,n.qualname,n.kind,
           p.path,n.line,1
    FROM source_claims c
    JOIN nodes n INDEXED BY idx_nodes_name ON n.name=c.claim_name
    JOIN path_dictionary p ON p.path_id=n.path_id
    UNION ALL
    SELECT c.claim_index,c.source_doc_id,c.source_path,c.claim_name,c.source_line,
           c.explicit_path,c.claim_target_path,n.node_id,n.id,n.name,n.qualname,n.kind,
           n.path,n.line,2
    FROM source_claims c
    JOIN fts_targets n
      ON c.suffix_indexed=1
     AND length(c.normalized_name)>0
     AND length(n.qualname)>length(c.normalized_name)
     AND substr(n.qualname,-length(c.normalized_name))=c.normalized_name
     AND substr(n.qualname,length(n.qualname)-length(c.normalized_name),1)='.'
    UNION ALL
    /*TOKENLESS_SUFFIX_FALLBACK*/
),
eligible_candidates AS (
    SELECT *
    FROM candidate_rows
    WHERE target_kind NOT IN('document','invariant','component')
      AND (
        (explicit_path=1 AND claim_target_path IS NOT NULL AND target_path=claim_target_path)
        OR
        (explicit_path=0 AND (
            (?3='' AND NOT (
                substr(target_path,1,1)='['
                AND instr(target_path,'/')>1
                AND substr(target_path,instr(target_path,'/')-1,1)=']'
            ))
            OR
            (?3!='' AND substr(target_path,1,length(?3))=?3
                AND substr(target_path,length(?3)+1,1)='/')
        ))
      )
),
ranked_candidates AS (
    SELECT *,MIN(priority) OVER(
        PARTITION BY claim_index,source_doc_id,source_path,explicit_path,claim_target_path
    ) AS selected_priority
    FROM eligible_candidates
),
winning_candidates AS (
    SELECT * FROM ranked_candidates WHERE priority=selected_priority
),
ambiguous_claims AS (
    SELECT claim_index,source_doc_id,source_path,explicit_path,claim_target_path
    FROM winning_candidates
    GROUP BY claim_index,source_doc_id,source_path,explicit_path,claim_target_path
    HAVING COUNT(DISTINCT target_node_id)>1
)
SELECT DISTINCT
    winner.source_doc_id,winner.source_path,winner.claim_name,winner.source_line,
    winner.target_id,winner.target_name,winner.target_qualname,winner.target_kind,
    winner.target_path,winner.target_line
FROM winning_candidates winner
JOIN ambiguous_claims ambiguous
  ON ambiguous.claim_index=winner.claim_index
 AND ambiguous.source_doc_id=winner.source_doc_id
 AND ambiguous.source_path=winner.source_path
 AND ambiguous.explicit_path=winner.explicit_path
 AND ambiguous.claim_target_path IS winner.claim_target_path
ORDER BY winner.source_path,winner.source_line,winner.source_doc_id,winner.claim_name,
         winner.target_path,winner.target_line,winner.target_id
"#;

fn document_reference_candidate_page(
    conn: &Connection,
    selected_sections: &[Value],
    source_workspace: &str,
    options: &QueryOptions,
) -> Result<Value> {
    let mut claims = Vec::new();
    let mut fts_terms = BTreeSet::new();
    let mut has_tokenless_claim = false;
    for section in selected_sections {
        let (Some(doc_id), Some(source_path)) =
            (section["doc_id"].as_str(), section["path"].as_str())
        else {
            continue;
        };
        let detailed_claims = section["details"]["symbol_claims"]
            .as_array()
            .filter(|claims| !claims.is_empty());
        let source_line = section["node_line"].as_u64().unwrap_or(1);
        let mut append_claim =
            |name: &str, line: u64, explicit_path: bool, target_path: Option<&str>| {
                let normalized_name = name.replace("::", ".");
                let suffix_indexed = normalized_name.chars().any(char::is_alphanumeric);
                if suffix_indexed {
                    fts_terms.insert(normalized_name.clone());
                } else {
                    has_tokenless_claim = true;
                }
                claims.push(json!({
                    "doc_id":doc_id,
                    "source_path":source_path,
                    "name":name,
                    "normalized_name":normalized_name,
                    "line":line,
                    "explicit_path":explicit_path,
                    "target_path":target_path,
                    "suffix_indexed":suffix_indexed,
                }));
            };
        if let Some(detailed_claims) = detailed_claims {
            for claim in detailed_claims {
                if let Some(name) = claim["name"].as_str() {
                    append_claim(
                        name,
                        claim["line"].as_u64().unwrap_or(source_line),
                        claim["explicit_path"].as_bool().unwrap_or(false),
                        claim["target_path"].as_str(),
                    );
                }
            }
        } else if let Some(fallback_claims) = section["referenced_symbols"].as_array() {
            for name in fallback_claims.iter().filter_map(Value::as_str) {
                append_claim(name, source_line, false, None);
            }
        }
    }

    if claims.is_empty() {
        let generation = paging::generation(conn, options)?;
        return Ok(paging::value(Vec::new(), 0, options, &generation));
    }
    let claims_json = serde_json::to_string(&claims)?;
    let fts_query = if fts_terms.is_empty() {
        "\"__forge_document_reference_suffix_no_tokens__\"".to_owned()
    } else {
        fts_terms
            .iter()
            .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" OR ")
    };
    let tokenless_fallback = if has_tokenless_claim {
        "SELECT c.claim_index,c.source_doc_id,c.source_path,c.claim_name,c.source_line,\n           c.explicit_path,c.claim_target_path,n.node_id,n.id,n.name,n.qualname,n.kind,\n           p.path,n.line,2\n    FROM source_claims c\n    JOIN nodes n INDEXED BY idx_nodes_qualname\n      ON c.suffix_indexed=0\n     AND length(c.normalized_name)>0\n     AND length(n.qualname)>length(c.normalized_name)\n     AND substr(n.qualname,-length(c.normalized_name))=c.normalized_name\n     AND substr(n.qualname,length(n.qualname)-length(c.normalized_name),1)='.'\n    JOIN path_dictionary p ON p.path_id=n.path_id"
    } else {
        "SELECT NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,2 WHERE 0"
    };
    let sql = DOCUMENT_REFERENCE_CANDIDATES_SQL
        .replace("/*TOKENLESS_SUFFIX_FALLBACK*/", tokenless_fallback);
    let mut page = paging::query(
        conn,
        &sql,
        &[&claims_json, &fts_query, &source_workspace],
        options,
    )?;
    let items = page["items"].as_array().cloned().unwrap_or_default();
    page["items"] = Value::Array(
        items
            .into_iter()
            .map(|item| {
                json!({
                    "source":{
                        "doc_id":item["source_doc_id"],
                        "path":item["source_path"],
                        "name":item["claim_name"],
                        "line":item["source_line"],
                    },
                    "target":{
                        "id":item["target_id"],
                        "name":item["target_name"],
                        "qualname":item["target_qualname"],
                        "kind":item["target_kind"],
                        "path":item["target_path"],
                        "line":item["target_line"],
                    },
                })
            })
            .collect(),
    );
    Ok(page)
}

/// Returns the doc paged.
pub fn get_doc_paged(
    conn: &Connection,
    path: &str,
    section: Option<&str>,
    options: &QueryOptions,
) -> Result<Value> {
    let section = section.unwrap_or("");
    let status_projection = if options.detail == Detail::Full {
        ",json_extract(n.details,'$.status') AS status"
    } else {
        ""
    };
    let sections = paging::query(conn, &format!("SELECT {},n.line,n.end_line{status_projection} FROM doc_sections d LEFT JOIN nodes n ON n.id=d.doc_id WHERE (d.doc_id=?1 OR d.path=?1) AND (?2='' OR d.section_title=?2) ORDER BY d.path,d.rowid", paging::docs("d", options.detail)), &[&path,&section], options)?;
    if sections["total"] == 0 {
        bail!("document or section not found");
    }
    const MAX_RELATION_SCOPE_SECTIONS: usize = 1000;
    let relation_scope_limit = (MAX_RELATION_SCOPE_SECTIONS + 1) as i64;
    let relation_scope_sql = if options.detail == Detail::Full {
        "SELECT d.doc_id,d.path,n.details,n.line AS node_line,d.referenced_symbols FROM doc_sections d JOIN nodes n ON n.id=d.doc_id WHERE (d.doc_id=?1 OR d.path=?1) AND (?2='' OR d.section_title=?2) ORDER BY d.path,d.rowid LIMIT ?3"
    } else {
        "SELECT d.doc_id,d.path FROM doc_sections d WHERE (d.doc_id=?1 OR d.path=?1) AND (?2='' OR d.section_title=?2) ORDER BY d.path,d.rowid LIMIT ?3"
    };
    let selected_sections = rows(
        conn,
        relation_scope_sql,
        &[&path, &section, &relation_scope_limit],
        MAX_RELATION_SCOPE_SECTIONS + 1,
    )?;
    if selected_sections.len() > MAX_RELATION_SCOPE_SECTIONS {
        bail!("document relation scope exceeds 1000 sections; narrow the section selector");
    }
    let hashes = selected_sections
        .iter()
        .filter_map(|item| item["doc_id"].as_str())
        .map(crate::core::models::stable_hash64)
        .collect::<Vec<_>>();
    let source_workspace = selected_sections
        .first()
        .and_then(|item| item["path"].as_str())
        .map(|path| crate::core::config::workspace_path(path).0.to_owned())
        .unwrap_or_default();
    let encoded_hashes = serde_json::to_string(&hashes)?;
    let cites_doc = paging::query(
        conn,
        "SELECT d.doc_id,d.path,d.section_title,d.doc_type,e.line FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash JOIN doc_sections d ON d.doc_id=n.id WHERE e.kind='cites_doc' AND e.src_hash IN(SELECT CAST(value AS INTEGER) FROM json_each(?1)) ORDER BY d.path,d.doc_id,e.line",
        &[&encoded_hashes],
        options,
    )?;
    let cited_by_doc = paging::query(
        conn,
        "SELECT d.doc_id,d.path,d.section_title,d.doc_type,e.line FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash JOIN doc_sections d ON d.doc_id=n.id WHERE e.kind='cited_by_doc' AND e.src_hash IN(SELECT CAST(value AS INTEGER) FROM json_each(?1)) ORDER BY d.path,d.doc_id,e.line",
        &[&encoded_hashes],
        options,
    )?;
    let referenced_symbols = paging::query(
        conn,
        "SELECT source.id AS doc_id,symbol.id,symbol.name,symbol.kind,symbol.path_id,(SELECT path FROM path_dictionary p WHERE p.path_id=symbol.path_id) AS path,symbol.line,e.line AS citation_line FROM edges e JOIN nodes source ON source.node_hash=e.src_hash JOIN doc_sections d ON d.doc_id=source.id JOIN nodes symbol ON symbol.node_hash=e.dst_hash WHERE e.kind='documents' AND e.src_hash IN(SELECT CAST(value AS INTEGER) FROM json_each(?1)) ORDER BY d.path,d.doc_id,e.line,symbol.path_id,symbol.line,symbol.id",
        &[&encoded_hashes],
        options,
    )?;
    let referenced_symbol_candidates = if options.detail == Detail::Full {
        document_reference_candidate_page(conn, &selected_sections, &source_workspace, options)?
    } else {
        json!({
            "omitted":true,
            "hint":"Request detail=full for documentary reference candidates",
        })
    };
    Ok(
        json!({"sections":sections,"cites_doc":cites_doc,"cited_by_doc":cited_by_doc,"referenced_symbols":referenced_symbols,"referenced_symbol_candidates":referenced_symbol_candidates}),
    )
}

pub(crate) fn governing_invariants(conn: &Connection, node_id: &str) -> Result<Value> {
    const DIRECT_LIMIT: usize = 5;
    const TRANSITIVE_LIMIT: usize = 5;
    let node_hash = crate::core::models::stable_hash64(node_id);
    let direct_total = paging::count(
        conn,
        "SELECT count(*) FROM edges e WHERE e.kind='governs' AND e.dst_hash=?1",
        &[&node_hash],
    )?;
    let direct_rows = rows(
        conn,
        "SELECT n.name AS id,json_extract(n.details,'$.rule') AS rule,p.path,n.line FROM edges e JOIN nodes n ON n.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=n.path_id WHERE e.kind='governs' AND e.dst_hash=?1 ORDER BY n.name,p.path,n.line LIMIT ?2",
        &[&node_hash, &(DIRECT_LIMIT as i64)],
        DIRECT_LIMIT,
    )?;
    let direct = direct_rows
        .iter()
        .map(|item| {
            json!({
                "id":item["id"],
                "rule":item["rule"],
                "source":{"path":item["path"],"line":item["line"]},
            })
        })
        .collect::<Vec<_>>();
    let transitive_total = paging::count(
        conn,
        "WITH neighbors(hash) AS (SELECT e.src_hash FROM edges e WHERE e.dst_hash=?1 AND e.kind IN('calls','possible_calls') UNION SELECT e.dst_hash FROM edges e WHERE e.src_hash=?1 AND e.kind IN('calls','possible_calls')) SELECT count(DISTINCT n.node_hash) FROM edges e JOIN nodes n ON n.node_hash=e.src_hash JOIN neighbors g ON g.hash=e.dst_hash WHERE e.kind='governs'",
        &[&node_hash],
    )?;
    let transitive = rows(
        conn,
        "WITH neighbors(hash) AS (SELECT e.src_hash FROM edges e WHERE e.dst_hash=?1 AND e.kind IN('calls','possible_calls') UNION SELECT e.dst_hash FROM edges e WHERE e.src_hash=?1 AND e.kind IN('calls','possible_calls')) SELECT DISTINCT n.name AS id,json_extract(n.details,'$.rule') AS rule,p.path,n.line FROM edges e JOIN nodes n ON n.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=n.path_id JOIN neighbors g ON g.hash=e.dst_hash WHERE e.kind='governs' ORDER BY n.name,p.path,n.line LIMIT ?2",
        &[&node_hash, &(TRANSITIVE_LIMIT as i64)],
        TRANSITIVE_LIMIT,
    )?;
    let omitted_direct = direct_total.saturating_sub(direct.len());
    let omitted_transitive = transitive_total.saturating_sub(transitive.len());
    let omitted_sources = transitive
        .iter()
        .map(|item| json!({"id":item["id"],"source":{"path":item["path"],"line":item["line"]}}))
        .collect::<Vec<_>>();
    Ok(json!({
        "direct":direct,
        "omitted_direct":omitted_direct,
        "transitive_ids":transitive.iter().map(|item| item["id"].clone()).collect::<Vec<_>>(),
        "omitted_transitive":omitted_transitive,
        "omitted_sources":omitted_sources,
    }))
}

/// Returns bounded governing rules for every indexed symbol in a task scope.
pub(crate) fn governing_invariants_for_scope(conn: &Connection, scope: &[String]) -> Result<Value> {
    const DIRECT_LIMIT: usize = 5;
    const TRANSITIVE_LIMIT: usize = 5;
    if scope.is_empty() {
        return Ok(json!({
            "direct": [],
            "omitted_direct": 0,
            "transitive_ids": [],
            "omitted_transitive": 0,
            "omitted_sources": [],
        }));
    }
    let scope_json = serde_json::to_string(scope)?;
    let scope_nodes = "WITH scope_nodes(hash) AS (SELECT DISTINCT n.node_hash FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE EXISTS(SELECT 1 FROM json_each(?1) s WHERE p.path=trim(s.value,'/') OR substr(p.path,1,length(trim(s.value,'/'))+1)=trim(s.value,'/')||'/'))";
    let direct_total = paging::count(
        conn,
        &format!("{scope_nodes}, direct AS (SELECT DISTINCT e.src_hash FROM edges e JOIN scope_nodes s ON s.hash=e.dst_hash WHERE e.kind='governs') SELECT count(*) FROM direct"),
        &[&scope_json],
    )?;
    let direct_rows = rows(
        conn,
        &format!("{scope_nodes} SELECT DISTINCT n.name AS id,json_extract(n.details,'$.rule') AS rule,p.path,n.line FROM edges e JOIN scope_nodes s ON s.hash=e.dst_hash JOIN nodes n ON n.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=n.path_id WHERE e.kind='governs' ORDER BY n.name,p.path,n.line LIMIT ?2"),
        &[&scope_json, &(DIRECT_LIMIT as i64)],
        DIRECT_LIMIT,
    )?;
    let direct = direct_rows
        .iter()
        .map(|item| {
            json!({
                "id":item["id"],
                "rule":item["rule"],
                "source":{"path":item["path"],"line":item["line"]},
            })
        })
        .collect::<Vec<_>>();

    let neighbors = "neighbors(hash) AS (SELECT e.src_hash FROM edges e JOIN scope_nodes s ON s.hash=e.dst_hash WHERE e.kind IN('calls','possible_calls') UNION SELECT e.dst_hash FROM edges e JOIN scope_nodes s ON s.hash=e.src_hash WHERE e.kind IN('calls','possible_calls'))";
    let transitive_total = paging::count(
        conn,
        &format!("{scope_nodes}, {neighbors}, direct AS (SELECT DISTINCT e.src_hash FROM edges e JOIN scope_nodes s ON s.hash=e.dst_hash WHERE e.kind='governs'), transitive AS (SELECT DISTINCT e.src_hash FROM edges e JOIN neighbors n ON n.hash=e.dst_hash WHERE e.kind='governs' AND NOT EXISTS(SELECT 1 FROM direct d WHERE d.src_hash=e.src_hash)) SELECT count(*) FROM transitive"),
        &[&scope_json],
    )?;
    let transitive_rows = rows(
        conn,
        &format!("{scope_nodes}, {neighbors}, direct AS (SELECT DISTINCT e.src_hash FROM edges e JOIN scope_nodes s ON s.hash=e.dst_hash WHERE e.kind='governs') SELECT DISTINCT n.name AS id,json_extract(n.details,'$.rule') AS rule,p.path,n.line FROM edges e JOIN neighbors g ON g.hash=e.dst_hash JOIN nodes n ON n.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=n.path_id WHERE e.kind='governs' AND NOT EXISTS(SELECT 1 FROM direct d WHERE d.src_hash=e.src_hash) ORDER BY n.name,p.path,n.line LIMIT ?2"),
        &[&scope_json, &(TRANSITIVE_LIMIT as i64)],
        TRANSITIVE_LIMIT,
    )?;
    let transitive_ids = transitive_rows
        .iter()
        .map(|item| item["id"].clone())
        .collect::<Vec<_>>();
    let omitted_sources = transitive_rows
        .iter()
        .map(|item| json!({"id":item["id"],"source":{"path":item["path"],"line":item["line"]}}))
        .collect::<Vec<_>>();
    Ok(json!({
        "direct":direct,
        "omitted_direct":direct_total.saturating_sub(direct.len()),
        "transitive_ids":transitive_ids,
        "omitted_transitive":transitive_total.saturating_sub(transitive_rows.len()),
        "omitted_sources":omitted_sources,
    }))
}

/// Runs a bounded read-only analysis query with generation-aware pagination.
///
/// # Errors
///
/// Rejects unsupported SQL, stale generations, and result budget violations.
pub fn analyze_paged(
    conn: &Connection,
    target: &str,
    include_cycles: Option<bool>,
    options: &QueryOptions,
) -> Result<Value> {
    let generation = paging::generation(conn, options)?;
    let sql = target.trim();
    if let Some(statement_sql) = classify_read_only_sql(sql)? {
        let statement = conn.prepare(statement_sql)?;
        if !statement.readonly() {
            bail!("query must be read-only");
        }
        return Ok(
            json!({"rows":paging::query_opaque(conn, statement_sql, &[], options)?, "ordering":"SQL order is preserved; include a deterministic ORDER BY for stable pagination."}),
        );
    }
    if sql.contains(';') {
        bail!("exactly one SELECT or WITH statement is allowed");
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
    let first = first_sql_keyword(sql)?.unwrap_or("");
    if !indexed_path
        && [
            "insert", "update", "delete", "drop", "alter", "create", "replace", "attach", "detach",
            "pragma", "vacuum", "reindex", "begin", "commit", "rollback",
        ]
        .iter()
        .any(|keyword| first.eq_ignore_ascii_case(keyword))
    {
        bail!("write or administrative SQL is not allowed");
    }
    if !indexed_path {
        bail!("PATH_NOT_INDEXED: '{path}' was not found in the indexed inventory");
    }
    let params: &[&dyn rusqlite::ToSql] = &[&path, &prefix, &end];
    let scope = "(?1='' OR path=?1 OR (path>=?2 AND path<?3))";
    let coverage_scope = "(?1='' OR p.path=?1 OR (p.path>=?2 AND p.path<?3))";
    let total_errors = paging::count(
        conn,
        &format!("SELECT count(*) FROM errors WHERE {scope}"),
        params,
    )?;
    let total_unresolved = paging::count(
        conn,
        &format!("SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id WHERE c.status IN('unresolved','ambiguous') AND {coverage_scope}"),
        params,
    )?;
    let total_external_imports = paging::count(
        conn,
        &format!("SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id WHERE c.status='external' AND {coverage_scope}"),
        params,
    )?;
    let exact_file: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM files WHERE path=?1)",
        [path],
        |r| r.get(0),
    )?;
    let mut result = json!({"target":target,"total_errors":total_errors,"total_unresolved":total_unresolved,"total_external_imports":total_external_imports,"scope":if exact_file {"file"} else {"summary"}});
    let public_surface_sql = format!(
        "WITH RECURSIVE covered(node_hash) AS (SELECT n.node_hash FROM nodes n WHERE n.is_test=1 AND json_extract(n.details,'$.test_role')='test_case' UNION SELECT e.dst_hash FROM edges e JOIN covered c ON c.node_hash=e.src_hash WHERE e.kind='calls') SELECT {} FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE (?1='' OR p.path=?1 OR (p.path>=?2 AND p.path<?3)) AND n.is_test=0 AND json_extract(n.details,'$.is_public')=1 AND NOT EXISTS(SELECT 1 FROM covered c WHERE c.node_hash=n.node_hash) ORDER BY p.path,n.line,n.id",
        paging::nodes_with_path("n", options.detail, "p.path")
    );
    result["untested_public_surface"] = paging::query(conn, &public_surface_sql, params, options)?;
    let (language_sql, language_params): (&str, &[&dyn rusqlite::ToSql]) = if path.is_empty() {
        ("SELECT c.language,sum(CASE WHEN c.status='resolved' THEN c.records ELSE 0 END) resolved,sum(CASE WHEN c.status='ambiguous' THEN c.records ELSE 0 END) ambiguous,sum(CASE WHEN c.status='unresolved' THEN c.records ELSE 0 END) unresolved,sum(CASE WHEN c.status='external' THEN c.records ELSE 0 END) external_imports FROM coverage_language_counts c GROUP BY c.language ORDER BY c.language LIMIT 100", &[])
    } else {
        ("SELECT c.language,sum(CASE WHEN c.status='resolved' THEN c.records ELSE 0 END) resolved,sum(CASE WHEN c.status='ambiguous' THEN c.records ELSE 0 END) ambiguous,sum(CASE WHEN c.status='unresolved' THEN c.records ELSE 0 END) unresolved,sum(CASE WHEN c.status='external' THEN c.records ELSE 0 END) external_imports FROM (SELECT path_id FROM path_dictionary WHERE path=?1 OR (path>=?2 AND path<?3)) p JOIN coverage_language_counts c ON c.path_id=p.path_id GROUP BY c.language ORDER BY c.language LIMIT 100", params)
    };
    let mut resolution_languages = Map::new();
    for row in rows(conn, language_sql, language_params, 100)? {
        if let Some(language) = row["language"].as_str() {
            resolution_languages.insert(
                language.to_owned(),
                json!({
                    "resolved": row["resolved"],
                    "ambiguous": row["ambiguous"],
                    "unresolved": row["unresolved"],
                    "external_imports": row["external_imports"]
                }),
            );
        }
    }
    result["resolution_languages"] = Value::Object(resolution_languages);
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
        let resolution_sql = if options.detail == Detail::Full {
            format!(
                "SELECT {},{} cause,e.evidence AS __evidence FROM resolution_coverage c JOIN coverage_evidence e ON e.evidence_id=c.evidence_id WHERE c.status IN('unresolved','ambiguous') AND c.path_id=(SELECT path_id FROM path_dictionary WHERE path=?1) ORDER BY c.line,c.expression_id,c.status,c.evidence_id",
                paging::coverage("c", Detail::Full),
                resolution_cause_sql()
            )
        } else {
            format!(
                "SELECT c.line,(SELECT expression FROM coverage_expressions WHERE expression_id=c.expression_id) AS expression,c.status,{} cause,e.evidence AS __evidence FROM resolution_coverage c JOIN coverage_evidence e ON e.evidence_id=c.evidence_id WHERE c.status IN('unresolved','ambiguous') AND c.path_id=(SELECT path_id FROM path_dictionary WHERE path=?1) ORDER BY c.line,c.expression_id,c.status,c.evidence_id",
                resolution_cause_sql()
            )
        };
        let mut resolution = paging::query(conn, &resolution_sql, &[&path], options)?;
        let external_sql = format!(
            "SELECT {},e.evidence AS __evidence FROM resolution_coverage c JOIN coverage_evidence e ON e.evidence_id=c.evidence_id WHERE c.status='external' AND c.path_id=(SELECT path_id FROM path_dictionary WHERE path=?1) ORDER BY c.line,c.expression_id,c.evidence_id",
            paging::coverage("c", options.detail)
        );
        let mut external_imports = paging::query(conn, &external_sql, &[&path], options)?;
        let workspace_root: String = conn.query_row(
            "SELECT value FROM metadata WHERE key='workspace_root'",
            [],
            |row| row.get(0),
        )?;
        let source = super::symbols::verified_source(conn, Path::new(&workspace_root), path)?;
        annotate_diagnostic_page(&mut resolution, &source);
        annotate_diagnostic_page(&mut external_imports, &source);
        result["resolution"] = resolution;
        result["external_imports"] = external_imports;
    } else {
        result["top_files"] = json!(rows(conn, &format!("SELECT path,sum(parse_errors) parse_errors,sum(unresolved) unresolved FROM (SELECT path,count(*) parse_errors,0 unresolved FROM errors WHERE {scope} GROUP BY path UNION ALL SELECT p.path,0 parse_errors,count(*) unresolved FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id WHERE c.status IN('unresolved','ambiguous') AND {coverage_scope} GROUP BY p.path) GROUP BY path ORDER BY sum(parse_errors)+sum(unresolved) DESC,path LIMIT 5"), params, 5)?);
        result["resolution_statuses"] = json!(rows(conn, &format!("SELECT c.status,count(*) total FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id WHERE {coverage_scope} GROUP BY c.status ORDER BY c.status LIMIT 20"), params, 20)?);
        result["resolution_causes"] = json!(rows(conn, &format!("SELECT status,cause,count(*) total,count(DISTINCT path) affected_files FROM (SELECT p.path,c.status,{} cause FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_evidence e ON e.evidence_id=c.evidence_id WHERE c.status IN('unresolved','ambiguous') AND {coverage_scope}) GROUP BY status,cause ORDER BY total DESC,status,cause", resolution_cause_sql()), params, 20)?);
        result["parse_error_languages"] = json!(rows(conn, "SELECT f.language,count(*) errors,count(DISTINCT e.path) affected_files FROM errors e JOIN files f ON f.path=e.path WHERE (?1='' OR e.path=?1 OR (e.path>=?2 AND e.path<?3)) GROUP BY f.language ORDER BY errors DESC,f.language", params, 100)?);
        result["cause_note"] = json!("Causes classify recorded resolver evidence; inspect an exact file with detail='full' for evidence and parse error messages.");
        result["continuation_hint"] = json!(
            "Call code_map_analyze with an exact path from top_files to page its diagnostics."
        );
    }
    if exact_file && (path.ends_with(".md") || path.ends_with(".mdx")) {
        result["doc_diagnostics"] = document_diagnostics(conn, path, options, &generation)?;
    }
    let compute_cycles = include_cycles.unwrap_or(false);
    result["cycles"] = if compute_cycles {
        super::cycles::summary(conn, if path.is_empty() { None } else { Some(path) })?
    } else {
        json!({"omitted": true, "hint": "Pass include_cycles=true to compute cyclic dependencies."})
    };
    result["generation"] = json!(generation);
    Ok(result)
}

fn classify_read_only_sql(sql: &str) -> Result<Option<&str>> {
    let Some(keyword) = first_sql_keyword(sql)? else {
        return Ok(None);
    };
    if !keyword.eq_ignore_ascii_case("select") && !keyword.eq_ignore_ascii_case("with") {
        return Ok(None);
    }
    let end = sql_statement_end(sql)?;
    Ok(Some(&sql[..end]))
}

fn first_sql_keyword(sql: &str) -> Result<Option<&str>> {
    let start = skip_sql_trivia(sql, 0)?;
    let bytes = sql.as_bytes();
    let mut end = start;
    while end < bytes.len()
        && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'_' | b'$'))
    {
        end += 1;
    }
    Ok((start < end).then(|| &sql[start..end]))
}

fn sql_statement_end(sql: &str) -> Result<usize> {
    let bytes = sql.as_bytes();
    let mut offset = 0;
    loop {
        offset = skip_sql_trivia(sql, offset)?;
        if offset == bytes.len() {
            return Ok(offset);
        }
        match bytes[offset] {
            b';' => {
                ensure!(
                    skip_sql_trivia(sql, offset + 1)? == bytes.len(),
                    "exactly one SELECT or WITH statement is allowed"
                );
                return Ok(offset);
            }
            b'\'' | b'"' | b'`' => {
                offset = skip_sql_quoted(sql, offset, bytes[offset])?;
            }
            b'[' => {
                offset = skip_sql_quoted(sql, offset, b']')?;
            }
            _ => offset += 1,
        }
    }
}

fn skip_sql_trivia(sql: &str, mut offset: usize) -> Result<usize> {
    let bytes = sql.as_bytes();
    loop {
        while offset < bytes.len() && bytes[offset].is_ascii_whitespace() {
            offset += 1;
        }
        if bytes.get(offset..offset + 2) == Some(b"--") {
            while offset < bytes.len() && bytes[offset] != b'\n' {
                offset += 1;
            }
            continue;
        }
        if bytes.get(offset..offset + 2) == Some(b"/*") {
            let comment_start = offset + 2;
            let Some(comment_end) = sql[comment_start..].find("*/") else {
                bail!("unterminated SQL block comment");
            };
            offset = comment_start + comment_end + 2;
            continue;
        }
        return Ok(offset);
    }
}

fn skip_sql_quoted(sql: &str, mut offset: usize, closing: u8) -> Result<usize> {
    let bytes = sql.as_bytes();
    let doubled_delimiter = closing != b']';
    offset += 1;
    while offset < bytes.len() {
        if bytes[offset] == closing {
            if doubled_delimiter && bytes.get(offset + 1) == Some(&closing) {
                offset += 2;
            } else {
                return Ok(offset + 1);
            }
        } else {
            offset += 1;
        }
    }
    bail!("unterminated quoted value in SQL")
}

fn document_diagnostics(
    conn: &Connection,
    path: &str,
    options: &QueryOptions,
    generation: &str,
) -> Result<Value> {
    const MAX_DOCUMENT_SECTIONS: usize = 1000;
    const MAX_DOCUMENT_DIAGNOSTICS: usize = 2000;
    let section_limit = MAX_DOCUMENT_SECTIONS as i64;
    let mut diagnostics = rows(
        conn,
        "SELECT 'broken_doc_link' AS kind,p.path,CAST(json_extract(c.value,'$.line') AS INTEGER) AS line,json_extract(c.value,'$.target') AS target FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id JOIN json_each(n.details,'$.citations') c WHERE n.kind='document' AND p.path=?1 AND NOT EXISTS(SELECT 1 FROM edges e JOIN coverage_evidence v ON v.evidence_id=e.evidence_id WHERE e.kind='cites_doc' AND e.src_hash=n.node_hash AND e.line=CAST(json_extract(c.value,'$.line') AS INTEGER) AND v.evidence=json_extract(c.value,'$.target')) ORDER BY line,target LIMIT ?2",
        &[&path, &section_limit],
        MAX_DOCUMENT_SECTIONS,
    )?;
    diagnostics.retain(|item| item["target"].as_str().is_some_and(is_document_link_target));
    let document_rows = rows(
        conn,
        "SELECT n.path_id,n.id,n.details FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.kind='document' AND p.path=?1 ORDER BY n.line,n.id LIMIT ?2",
        &[&path, &section_limit],
        MAX_DOCUMENT_SECTIONS,
    )?;
    let workspace_root: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='workspace_root'",
        [],
        |row| row.get(0),
    )?;
    for document in &document_rows {
        let Some(claims) = document["details"]["symbol_claims"].as_array() else {
            continue;
        };
        for claim in claims {
            if claim["label_count"]
                .as_u64()
                .is_some_and(|count| count != 1)
            {
                continue;
            }
            let (Some(name), Some(target_path), Some(start), Some(end), Some(source_line)) = (
                claim["name"].as_str(),
                claim["target_path"].as_str(),
                claim["start_line"].as_u64(),
                claim["end_line"].as_u64(),
                claim["line"].as_u64(),
            ) else {
                continue;
            };
            if target_path.split('/').any(|component| component == "..") {
                continue;
            }
            let normalized_name = name.replace("::", ".");
            let matching = rows(
                conn,
                "WITH candidates AS (SELECT n.id,n.name,n.kind,n.line,n.end_line,p.path,CASE WHEN n.qualname=?3 THEN 0 WHEN n.name=?2 THEN 1 ELSE 2 END AS priority FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.kind NOT IN('document','invariant','module','file') AND (n.qualname=?3 OR n.name=?2 OR (substr(n.qualname,-length(?3))=?3 AND substr(n.qualname,-length(?3)-1,1)='.'))), best AS (SELECT MIN(priority) AS priority FROM candidates) SELECT id,name,kind,line,end_line,path FROM candidates WHERE priority=(SELECT priority FROM best) ORDER BY line,id LIMIT 2",
                &[&target_path, &name, &normalized_name],
                2,
            )?;
            if matching.len() > 1 {
                continue;
            }
            if matching.is_empty() {
                let prefix = format!("{name}%");
                let suggestion = rows(
                    conn,
                    "SELECT n.name,n.line,p.path FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.kind NOT IN('document','invariant','module','file') AND n.name LIKE ?2 ORDER BY n.name,n.line LIMIT 1",
                    &[&target_path, &prefix],
                    1,
                )?;
                let mut diagnostic = json!({
                    "kind":"dead_doc_symbol",
                    "path":path,
                    "line":source_line,
                    "claim":{"name":name,"target_path":target_path,"start_line":start,"end_line":end},
                });
                if let Some(candidate) = suggestion.first() {
                    diagnostic["suggested_symbol"] = json!({
                        "name":candidate["name"],
                        "path":candidate["path"],
                        "line":candidate["line"],
                    });
                }
                diagnostics.push(diagnostic);
                continue;
            }
            let target = &matching[0];
            let source = match super::symbols::verified_source(
                conn,
                Path::new(&workspace_root),
                target_path,
            ) {
                Ok(source) => source,
                Err(_) => continue,
            };
            let source_lines = source.lines().count().max(1) as u64;
            if start == 0 || end < start || end > source_lines {
                continue;
            }
            let target_start = target["line"].as_u64().unwrap_or(0);
            let target_end = target["end_line"].as_u64().unwrap_or(target_start);
            if end < target_start || start > target_end {
                diagnostics.push(json!({
                    "kind":"stale_doc_sections",
                    "reason":"documented_symbol_range_mismatch",
                    "path":path,
                    "line":source_line,
                    "claim":{"name":name,"target_path":target_path,"start_line":start,"end_line":end},
                    "target":{"id":target["id"],"path":target_path,"line":target_start,"end_line":target_end},
                    "generation":generation,
                }));
            }
        }
    }
    diagnostics.sort_by(|left, right| {
        left["line"]
            .as_u64()
            .cmp(&right["line"].as_u64())
            .then_with(|| left["kind"].as_str().cmp(&right["kind"].as_str()))
            .then_with(|| left.to_string().cmp(&right.to_string()))
    });
    diagnostics.truncate(MAX_DOCUMENT_DIAGNOSTICS);
    let total = diagnostics.len();
    let items = diagnostics
        .into_iter()
        .skip(options.offset)
        .take(options.limit)
        .collect();
    Ok(paging::value(items, total, options, generation))
}

fn is_document_link_target(target: &str) -> bool {
    if target.starts_with('#') {
        return true;
    }
    if target.contains("://") || target.starts_with("mailto:") {
        return false;
    }
    let linked_path = target.split('#').next().unwrap_or_default();
    match Path::new(linked_path)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some(extension) => matches!(extension.to_ascii_lowercase().as_str(), "md" | "mdx"),
        None => true,
    }
}

fn resolution_cause_sql() -> &'static str {
    "CASE WHEN c.status='ambiguous' THEN 'ambiguous_candidates' WHEN e.evidence='computed receiver or dynamic callee; callable identity is unknown' THEN 'dynamic_callee' WHEN e.evidence='callee is shadowed by a parameter or local binding of unknown callable identity' THEN 'shadowed_binding' WHEN e.evidence LIKE 'call through external import %' THEN 'external_import_call' WHEN e.evidence LIKE 'import %: 1 modules, 0 alias targets' THEN 'alias_target_missing' WHEN e.evidence LIKE 'no indexed provider for absolute import %' THEN 'missing_indexed_import' WHEN e.evidence LIKE 'import %: 0 modules, %' THEN 'missing_indexed_import' WHEN e.evidence LIKE '0 lexically justified candidates;%' THEN 'no_lexical_candidate' ELSE 'other' END"
}

fn annotate_diagnostic_page(page: &mut Value, source: &str) {
    let Some(items) = page["items"].as_array_mut() else {
        return;
    };
    for item in items {
        let evidence = item
            .get("__evidence")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let reason = diagnostic_reason(
            item.get("status").and_then(Value::as_str),
            item.get("cause").and_then(Value::as_str),
            item.get("expression").and_then(Value::as_str),
            &evidence,
        );
        let snippet = item
            .get("line")
            .and_then(Value::as_u64)
            .filter(|line| *line > 0)
            .and_then(|line| {
                source
                    .split_inclusive('\n')
                    .nth(line.saturating_sub(1) as usize)
            })
            .map(|line| line.trim_end_matches(['\r', '\n']))
            .map(bounded_diagnostic_snippet);
        if let Some(object) = item.as_object_mut() {
            object.remove("__evidence");
            object.insert(
                "reason".into(),
                reason.map_or(Value::Null, |reason| json!(reason)),
            );
            object.insert(
                "snippet".into(),
                snippet.map_or(Value::Null, |snippet| json!(snippet)),
            );
        }
    }
}

fn bounded_diagnostic_snippet(line: &str) -> String {
    if line.len() <= 512 {
        return line.to_owned();
    }
    let mut end = 512;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    line[..end].to_owned()
}

fn diagnostic_reason(
    status: Option<&str>,
    cause: Option<&str>,
    expression: Option<&str>,
    evidence: &str,
) -> Option<&'static str> {
    if status == Some("ambiguous") {
        return Some("ambiguous_candidates");
    }
    if evidence.contains("diagnostic_reason=framework_magic") {
        return Some("framework_magic");
    }
    if evidence
        .contains("callee is shadowed by a parameter or local binding of unknown callable identity")
    {
        return Some(
            if expression.is_some_and(|expression| expression.contains('.')) {
                "unresolved_receiver_type"
            } else {
                "dynamic_callback"
            },
        );
    }
    if evidence.starts_with("call through external import ")
        || cause == Some("external_import_call")
    {
        return Some("external_unindexed");
    }
    match cause {
        Some("shadowed_binding") => Some("shadowed_binding"),
        _ => None,
    }
}
