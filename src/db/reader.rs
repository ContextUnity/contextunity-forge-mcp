use super::paging;
use crate::core::response::{Detail, QueryOptions};
use anyhow::{bail, Context, Result};
use rusqlite::backup::Backup;
use rusqlite::{types::ValueRef, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Map, Value};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    ops::Deref,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

#[path = "reader/selectors.rs"]
mod selectors;
pub(crate) use selectors::select_detail;

const MAX_SQLITE_VALUE_BYTES: i32 = 8 * 1024 * 1024;
const MAX_QUERY_RESULT_BYTES: usize = MAX_SQLITE_VALUE_BYTES as usize;
const QUERY_RESULT_TOO_LARGE: &str = "query result exceeds the 8 MiB row budget; narrow the selector or path, request detail='compact' with a smaller limit, or select fewer and smaller SQL columns. Continue paged requests with the returned offset and generation";

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
        /// Exact IDs or paths that disambiguate the target.
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
            } => write!(formatter, "ambiguous selector {selector}; use an exact node id or workspace-relative path; candidates: {}", serde_json::to_string(candidates).map_err(|_| std::fmt::Error)?),
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
    if version != crate::engine::scanner::ENGINE_SCHEMA_VERSION
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
    if semantics.as_deref() != Some(crate::engine::scanner::INDEX_SEMANTICS_VERSION) {
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
pub(crate) fn inventory_snapshot(
    conn: &Connection,
) -> Result<Vec<crate::engine::scanner::FileEntry>> {
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
        _ => Err(SelectorError::Ambiguous {
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
            if matches!(
                name.as_str(),
                "node_hash" | "path_hash" | "path_id" | "owner_path_id"
            ) {
                continue;
            }
            if names[i + 1..].iter().any(|later| later == name) {
                value.entry(name.clone()).or_insert(Value::Null);
                continue;
            }
            let raw = row.get_ref(i)?;
            let special_json_column = matches!(
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
    let generation = paging::generation(conn, options)?;
    let node = select_detail(conn, selector, options.detail)?;
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
    let direction = request.direction;
    let show_doc = request.show_doc;
    let options = request.page;
    let mut result = inspect_paged(conn, selector, show_doc, options)?;
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
            if options.detail == Detail::Compact {
                omit_selected_endpoint(&mut incoming, "dst_public_id");
            }
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
            if options.detail == Detail::Compact {
                omit_selected_endpoint(&mut outgoing, "src_public_id");
            }
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

fn omit_selected_endpoint(page: &mut Value, field: &str) {
    if let Some(items) = page["items"].as_array_mut() {
        for item in items {
            if let Some(edge) = item.as_object_mut() {
                edge.remove(field);
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
    let query = query
        .split_whitespace()
        .map(|s| format!("\"{}\"", s.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ");
    if query.is_empty() {
        bail!("search query is empty");
    }
    let kind = doc_type.unwrap_or("");
    let resolved = resolve_component_prefix(conn, component.unwrap_or(""));
    let component = resolved.as_str();
    let (prefix, end) = path_bounds(component);
    let excerpt = if include_excerpt {
        ",substr(snippet(doc_search,-1,'[',']',' … ',16),1,240) excerpt"
    } else {
        ""
    };
    let sql = format!("SELECT {},bm25(doc_search) rank{excerpt} FROM doc_search JOIN doc_sections d ON d.rowid=doc_search.rowid WHERE doc_search MATCH ?1 AND (?2='' OR d.doc_type=?2) AND (?3='' OR d.path=?3 OR (d.path>=?4 AND d.path<?5)) ORDER BY rank,d.path,d.doc_id", paging::docs("d", options.detail));
    Ok(
        json!({"sections":paging::query(conn, &sql, &[&query,&kind,&component,&prefix,&end], options)?}),
    )
}

/// Returns the doc paged.
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
        let resolution_columns = if options.detail == Detail::Full {
            format!(
                "{},{} cause",
                paging::coverage("c", Detail::Full),
                resolution_cause_sql()
            )
        } else {
            format!(
                "c.line,(SELECT expression FROM coverage_expressions WHERE expression_id=c.expression_id) AS expression,c.status,{} cause",
                resolution_cause_sql()
            )
        };
        result["resolution"] = paging::query(conn, &format!("SELECT {resolution_columns} FROM resolution_coverage c JOIN coverage_evidence e ON e.evidence_id=c.evidence_id WHERE c.status IN('unresolved','ambiguous') AND c.path_id=(SELECT path_id FROM path_dictionary WHERE path=?1) ORDER BY c.line,c.expression_id,c.status,c.evidence_id"), &[&path], options)?;
        result["external_imports"] = paging::query(conn, &format!("SELECT {} FROM resolution_coverage c WHERE c.status='external' AND c.path_id=(SELECT path_id FROM path_dictionary WHERE path=?1) ORDER BY c.line,c.expression_id,c.evidence_id", paging::coverage("c", options.detail)), &[&path], options)?;
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
    let compute_cycles = include_cycles.unwrap_or(false);
    result["cycles"] = if compute_cycles {
        super::cycles::summary(conn, if path.is_empty() { None } else { Some(path) })?
    } else {
        json!({"omitted": true, "hint": "Pass include_cycles=true to compute cyclic dependencies."})
    };
    result["generation"] = json!(generation);
    Ok(result)
}

fn resolution_cause_sql() -> &'static str {
    "CASE WHEN c.status='ambiguous' THEN 'ambiguous_candidates' WHEN e.evidence='computed receiver or dynamic callee; callable identity is unknown' THEN 'dynamic_callee' WHEN e.evidence='callee is shadowed by a parameter or local binding of unknown callable identity' THEN 'shadowed_binding' WHEN e.evidence LIKE 'call through external import %' THEN 'external_import_call' WHEN e.evidence LIKE 'import %: 1 modules, 0 alias targets' THEN 'alias_target_missing' WHEN e.evidence LIKE 'no indexed provider for absolute import %' THEN 'missing_indexed_import' WHEN e.evidence LIKE 'import %: 0 modules, %' THEN 'missing_indexed_import' WHEN e.evidence LIKE '0 lexically justified candidates;%' THEN 'no_lexical_candidate' ELSE 'other' END"
}
