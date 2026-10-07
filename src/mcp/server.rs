use crate::{db::cache::Identity, engine::scanner};
use anyhow::Result;
use rmcp::ServiceExt;
use rusqlite::OptionalExtension;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const INVENTORY_FRESHNESS_TTL: std::time::Duration = std::time::Duration::from_secs(5);

/// Represents cached connection data.
pub struct CachedConnection {
    identity: Identity,
    connection: rusqlite::Connection,
    adapter_digest: String,
    workspace_root: PathBuf,
    database_path: PathBuf,
    adapter_roots: Vec<PathBuf>,
    linked_workspaces: Vec<scanner::LinkedWorkspace>,
    inventory_checked_at: Option<Instant>,
    freshness: Option<Freshness>,
}

/// Names the connection slot type.
pub type ConnectionSlot = Option<CachedConnection>;

#[derive(Clone, serde::Serialize)]
struct Freshness {
    status: &'static str,
    output_root: String,
    corpus_hash: String,
    checked_at_unix_ms: u128,
    files_checked: usize,
    inventory_scan_ms: f64,
    refresh: &'static str,
}

#[derive(Clone)]
/// Represents server data.
pub struct Server {
    /// The root value.
    pub root: PathBuf,
    /// The db value.
    pub db: PathBuf,
    /// The connection value.
    pub connection: Arc<Mutex<ConnectionSlot>>,
}
impl Server {
    /// Creates a new instance.
    pub fn new(root: PathBuf, db: PathBuf) -> Self {
        Self {
            root,
            db,
            connection: Arc::new(Mutex::new(None)),
        }
    }
    /// Performs ensure fresh.
    pub fn ensure_fresh(&self) -> Result<()> {
        self.read(|_| Ok(serde_json::Value::Null)).map(|_| ())
    }

    fn check_rebuild_needed(&self, current_adapter: &scanner::Adapter) -> Result<bool> {
        match std::fs::symlink_metadata(&self.db) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(error.into()),
            Ok(metadata) if !metadata.is_file() => anyhow::bail!("database must be a regular file"),
            Ok(_) => {}
        }
        let _generation_lock = crate::db::cache::shared_lock(&self.db)?;
        let conn = rusqlite::Connection::open_with_flags(
            &self.db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        conn.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF; BEGIN DEFERRED")?;
        let has_metadata_table: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='metadata')",
            [],
            |row| row.get(0),
        )?;
        if !has_metadata_table {
            crate::db::reader::validate_database_header(&self.db)?;
            crate::db::reader::validate_database_integrity(&conn)?;
            return Ok(true);
        }
        crate::db::reader::validate_workspace(&conn, &self.root)?;
        let engine: String = conn.query_row(
            "SELECT value FROM metadata WHERE key='indexer_engine'",
            [],
            |r| r.get(0),
        )?;
        if engine != "contextunity-forge-mcp-rust" {
            anyhow::bail!("incompatible database engine");
        }
        let schema_version: String = conn.query_row(
            "SELECT value FROM metadata WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )?;
        if schema_version != crate::engine::scanner::ENGINE_SCHEMA_VERSION {
            crate::db::reader::validate_database_integrity(&conn)?;
            return Ok(true);
        }
        let semantics: Option<String> = conn
            .query_row(
                "SELECT value FROM metadata WHERE key='index_semantics_version'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if semantics.as_deref() != Some(crate::engine::scanner::INDEX_SEMANTICS_VERSION) {
            crate::db::reader::validate_database_integrity(&conn)?;
            return Ok(true);
        }
        let admit_rebuild = || -> Result<bool> {
            crate::db::reader::validate_database_integrity(&conn)?;
            crate::db::reader::open(&self.db, &self.root)?;
            Ok(true)
        };
        let persisted_digest: String = conn.query_row(
            "SELECT value FROM metadata WHERE key='adapter_digest'",
            [],
            |r| r.get(0),
        )?;
        if current_adapter.digest != persisted_digest {
            return admit_rebuild();
        }
        let persisted_version: String = conn.query_row(
            "SELECT value FROM metadata WHERE key='adapter_version'",
            [],
            |r| r.get(0),
        )?;
        if current_adapter.adapter_version.as_deref().unwrap_or("") != persisted_version {
            return admit_rebuild();
        }
        let persisted_policy: String = conn.query_row(
            "SELECT value FROM metadata WHERE key='adapter'",
            [],
            |row| row.get(0),
        )?;
        let persisted_policy: serde_json::Value = serde_json::from_str(&persisted_policy)?;
        let persisted_roots: Vec<String> =
            serde_json::from_value(persisted_policy["roots"].clone())?;
        let root = scanner::canonical_root(&self.root)?;
        let current_roots: Vec<_> = current_adapter
            .roots
            .iter()
            .map(|path| {
                path.strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        if current_roots != persisted_roots {
            return admit_rebuild();
        }
        let current_linked_json = serde_json::to_string(&current_adapter.linked_workspaces)?;
        let persisted_linked: Option<String> = conn
            .query_row(
                "SELECT value FROM metadata WHERE key='adapter_linked_workspaces'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        match persisted_linked {
            Some(persisted) => {
                if persisted != current_linked_json {
                    return admit_rebuild();
                }
            }
            None => {
                if !current_adapter.linked_workspaces.is_empty() {
                    return admit_rebuild();
                }
            }
        }
        Ok(false)
    }

    fn admit(&self, slot: &mut ConnectionSlot) -> Result<Freshness> {
        let root = scanner::canonical_root(&self.root)?;
        let mut refresh = "none";
        for _ in 0..3 {
            let adapter = scanner::load_adapter(&root, None)?;
            let admitted_identity = slot
                .as_ref()
                .filter(|cached| {
                    cached.workspace_root == root
                        && cached.database_path == self.db
                        && cached.adapter_digest == adapter.digest
                        && cached.adapter_roots == adapter.roots
                        && cached.linked_workspaces == adapter.linked_workspaces
                })
                .and_then(|cached| {
                    if !std::fs::symlink_metadata(&self.db).is_ok_and(|metadata| metadata.is_file())
                    {
                        return None;
                    }
                    crate::db::cache::identity(&self.db)
                        .ok()
                        .filter(|identity| *identity == cached.identity)
                });
            if admitted_identity.is_none() && self.check_rebuild_needed(&adapter)? {
                *slot = None;
                crate::db::writer::build(&root, &self.db, None)?;
                refresh = "rebuild";
                continue;
            }
            let identity =
                admitted_identity.map_or_else(|| crate::db::cache::identity(&self.db), Ok)?;
            if slot
                .as_ref()
                .is_none_or(|cached| cached.identity != identity)
            {
                *slot = None;
                *slot = Some(CachedConnection {
                    identity: identity.clone(),
                    connection: crate::db::reader::open(&self.db, &root)?,
                    adapter_digest: adapter.digest.clone(),
                    workspace_root: root.clone(),
                    database_path: self.db.clone(),
                    adapter_roots: adapter.roots.clone(),
                    linked_workspaces: adapter.linked_workspaces.clone(),
                    inventory_checked_at: None,
                    freshness: None,
                });
            } else if let Some(cached) = slot.as_ref() {
                cached.connection.execute_batch("BEGIN DEFERRED")?;
            }
            let conn = &slot
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("database unavailable"))?
                .connection;
            if let Some(cached) = slot.as_ref().filter(|cached| {
                cached.adapter_digest == adapter.digest
                    && cached
                        .inventory_checked_at
                        .is_some_and(|checked| checked.elapsed() < INVENTORY_FRESHNESS_TTL)
            }) {
                if let Some(freshness) = &cached.freshness {
                    let mut freshness = freshness.clone();
                    freshness.refresh = "none";
                    freshness.inventory_scan_ms = 0.0;
                    return Ok(freshness);
                }
            }
            let metadata = |key: &str| -> Result<String> {
                Ok(
                    conn.query_row("SELECT value FROM metadata WHERE key=?1", [key], |row| {
                        row.get(0)
                    })?,
                )
            };
            let previous = crate::db::reader::inventory_snapshot(conn)?;
            let started = Instant::now();
            let scan = scanner::scan_reusing(&root, &adapter, &previous)?;
            let manifests =
                crate::engine::languages::manifests::DependencyRegistry::collect_with_adapter(
                    &root,
                    Some(&adapter),
                );
            let previous_manifest_digest = conn.query_row(
                "SELECT value FROM metadata WHERE key='manifest_digest'",
                [],
                |row| row.get::<_, String>(0),
            )?;
            if manifests.digest() != previous_manifest_digest {
                *slot = None;
                crate::db::writer::build(&root, &self.db, adapter.adapter_path.as_deref())?;
                refresh = "rebuild";
                continue;
            }
            let inventory_scan_ms = started.elapsed().as_secs_f64() * 1000.;
            let previous: BTreeMap<_, _> = previous
                .iter()
                .map(|entry| (entry.path.as_str(), entry))
                .collect();
            let current: BTreeMap<_, _> = scan
                .entries
                .iter()
                .map(|entry| (entry.path.as_str(), entry))
                .collect();
            let changed: BTreeSet<_> = previous
                .keys()
                .chain(current.keys())
                .filter(|path| match (previous.get(**path), current.get(**path)) {
                    (Some(old), Some(new)) => {
                        old.digest != new.digest
                            || old.language != new.language
                            || old.is_doc != new.is_doc
                    }
                    _ => true,
                })
                .copied()
                .collect();
            if crate::db::cache::identity(&self.db)? != identity {
                anyhow::bail!("generation changed during source admission; retry against the current snapshot");
            }
            let latest_adapter = scanner::load_adapter(&root, None)?;
            if latest_adapter.digest != adapter.digest
                || latest_adapter.linked_workspaces != adapter.linked_workspaces
            {
                anyhow::bail!(
                    "adapter changed during source admission; retry against the current snapshot"
                );
            }
            if changed.is_empty() {
                let freshness = Freshness {
                    status: "source_inventory_matched",
                    output_root: metadata("output_root")?,
                    corpus_hash: metadata("corpus_hash")?,
                    checked_at_unix_ms: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
                    files_checked: scan.files,
                    inventory_scan_ms,
                    refresh,
                };
                if let Some(cached) = slot.as_mut() {
                    cached.adapter_digest = adapter.digest.clone();
                    cached.inventory_checked_at = Some(Instant::now());
                    cached.freshness = Some(freshness.clone());
                }
                return Ok(freshness);
            }
            let mut rebuild = false;
            for path in &changed {
                if !current.contains_key(path)
                    && scanner::resolve_file_path(&root, &adapter, path)?
                        .symlink_metadata()
                        .is_ok()
                {
                    rebuild = true;
                }
            }
            let modified: Vec<_> = changed.into_iter().map(PathBuf::from).collect();
            *slot = None;
            if rebuild {
                crate::db::writer::build(&root, &self.db, None)?;
                refresh = "rebuild";
            } else {
                match crate::db::writer::delta(&root, &self.db, &modified) {
                    Ok(_) => refresh = "delta",
                    Err(error) if error.is::<crate::db::writer::SourceSnapshotMismatch>() => {
                        continue;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        anyhow::bail!(
            "workspace changed repeatedly during source admission; retry against a stable snapshot"
        )
    }

    /// Performs read.
    pub fn read(
        &self,
        f: impl FnOnce(&rusqlite::Connection) -> Result<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let mut slot = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("database connection lock poisoned"))?;
        let freshness = match self.admit(&mut slot) {
            Ok(freshness) => freshness,
            Err(error) => {
                *slot = None;
                return Err(error);
            }
        };
        let _read_lock = crate::db::cache::shared_lock(&self.db)?;
        let cached = slot
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("database unavailable"))?;
        let result = f(&cached.connection);
        let unchanged = crate::db::cache::identity(&self.db).map(|after| after == cached.identity);
        let ended = cached.connection.execute_batch("ROLLBACK");
        if let Err(error) = ended {
            *slot = None;
            return Err(error.into());
        }
        if !unchanged.as_ref().is_ok_and(|value| *value) {
            *slot = None;
            unchanged?;
            anyhow::bail!("generation changed during query; retry against the current snapshot");
        }
        let mut result = result?;
        if let Some(object) = result.as_object_mut() {
            object.insert("freshness".into(), serde_json::to_value(freshness)?);
        }
        Ok(result)
    }
}
const MCP_SESSION_ENV: &str = "CONTEXTUNITY_FORGE_MCP_SESSION";

fn resumed_initialize_params(server: &Server) -> Option<rmcp::model::InitializeRequestParams> {
    let raw = std::env::var(MCP_SESSION_ENV).ok()?;
    std::env::remove_var(MCP_SESSION_ENV);
    let params = serde_json::from_str::<rmcp::model::InitializeRequestParams>(&raw).ok()?;
    let supported = rmcp::Service::supported_protocol_versions(server);
    supported
        .iter()
        .any(|version| version == &params.protocol_version)
        .then_some(params)
}

fn preserved_session(
    service: &rmcp::service::RunningService<rmcp::RoleServer, Server>,
) -> Option<String> {
    let info = service.peer_info()?;
    match serde_json::to_string(info.as_ref()) {
        Ok(json) => Some(json),
        Err(error) => {
            eprintln!("SIGHUP session preserve failed: {error}");
            None
        }
    }
}

/// Performs serve.
pub async fn serve(root: PathBuf, db: PathBuf) -> Result<()> {
    let server = Server::new(root, db);
    let service = if let Some(params) = resumed_initialize_params(&server) {
        rmcp::service::serve_directly(server, super::response::stdio(), Some(params))
    } else {
        server.serve(super::response::stdio()).await?
    };
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
        let preserved = preserved_session(&service);
        let waiting = service.waiting();
        tokio::pin!(waiting);
        loop {
            tokio::select! {
                result = &mut waiting => {
                    result?;
                    break;
                }
                Some(()) = hangup.recv() => {
                    match &preserved {
                        Some(json) => std::env::set_var(MCP_SESSION_ENV, json),
                        None => std::env::remove_var(MCP_SESSION_ENV),
                    }
                    let result = (|| -> std::io::Result<()> {
                        let mut exe = std::env::current_exe()?;
                        #[cfg(target_os = "linux")]
                        {
                            use std::os::unix::ffi::{OsStrExt, OsStringExt};
                            if let Some(path) = exe.as_os_str().as_bytes().strip_suffix(b" (deleted)") {
                                exe = std::ffi::OsString::from_vec(path.to_vec()).into();
                            }
                        }
                        let args: Vec<_> = std::env::args_os().skip(1).collect();
                        Err(std::process::Command::new(exe).args(args).exec())
                    })();
                    if let Err(error) = result {
                        eprintln!("SIGHUP reload failed: {error}");
                    }
                }
            }
        }
    }
    #[cfg(not(unix))]
    service.waiting().await?;
    Ok(())
}
