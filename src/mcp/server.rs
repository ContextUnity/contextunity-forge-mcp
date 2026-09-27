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

type ConnectionSlot = Option<(Identity, rusqlite::Connection)>;

#[derive(serde::Serialize)]
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
pub struct Server {
    pub root: PathBuf,
    pub db: PathBuf,
    pub connection: Arc<Mutex<ConnectionSlot>>,
}
impl Server {
    pub fn new(root: PathBuf, db: PathBuf) -> Self {
        Self {
            root,
            db,
            connection: Arc::new(Mutex::new(None)),
        }
    }
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
        let conn = rusqlite::Connection::open_with_flags(
            &self.db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        conn.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF; BEGIN DEFERRED")?;
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
            return Ok(true);
        }
        let admit_rebuild = || -> Result<bool> {
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
            if self.check_rebuild_needed(&adapter)? {
                *slot = None;
                crate::db::writer::build(&root, &self.db, None)?;
                refresh = "rebuild";
                continue;
            }
            let identity = crate::db::cache::identity(&self.db)?;
            if slot.as_ref().is_none_or(|(old, _)| *old != identity) {
                *slot = None;
                *slot = Some((identity.clone(), crate::db::reader::open(&self.db, &root)?));
            } else if let Some((_, conn)) = slot.as_ref() {
                conn.execute_batch("BEGIN DEFERRED")?;
            }
            let conn = &slot
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("database unavailable"))?
                .1;
            let metadata = |key: &str| -> Result<String> {
                Ok(
                    conn.query_row("SELECT value FROM metadata WHERE key=?1", [key], |row| {
                        row.get(0)
                    })?,
                )
            };
            let previous: Vec<scanner::FileEntry> =
                serde_json::from_str(&metadata("inventory_snapshot")?)?;
            let started = Instant::now();
            let scan = scanner::scan_reusing(&root, &adapter, &previous)?;
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
                return Ok(Freshness {
                    status: "source_inventory_matched",
                    output_root: metadata("output_root")?,
                    corpus_hash: metadata("corpus_hash")?,
                    checked_at_unix_ms: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
                    files_checked: scan.files,
                    inventory_scan_ms,
                    refresh,
                });
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
                crate::db::writer::delta(&root, &self.db, &modified)?;
                refresh = "delta";
            }
        }
        anyhow::bail!(
            "workspace changed repeatedly during source admission; retry against a stable snapshot"
        )
    }

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
        let (identity, conn) = slot
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("database unavailable"))?;
        let result = f(conn);
        let unchanged = crate::db::cache::identity(&self.db).map(|after| after == *identity);
        let ended = conn.execute_batch("ROLLBACK");
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
pub async fn serve(root: PathBuf, db: PathBuf) -> Result<()> {
    let service = Server::new(root, db)
        .serve(super::response::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}
