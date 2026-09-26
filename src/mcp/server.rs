use anyhow::Result;
use rmcp::ServiceExt;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
#[derive(Clone)]
pub struct Server {
    pub root: PathBuf,
    pub db: PathBuf,
    pub connection: Arc<Mutex<Option<(crate::db::cache::Identity, rusqlite::Connection)>>>,
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
        let rebuild = self.check_rebuild_needed().unwrap_or(true);
        if rebuild {
            eprintln!("[contextunity-forge-mcp] adapter or schema changed, reindexing automatically...");
            let mut slot = self
                .connection
                .lock()
                .map_err(|_| anyhow::anyhow!("database connection lock poisoned"))?;
            *slot = None;
            crate::db::writer::build(&self.root, &self.db, None)?;
        }
        Ok(())
    }

    fn check_rebuild_needed(&self) -> Result<bool> {
        if !self.db.exists() {
            return Ok(true);
        }
        let current_adapter = crate::engine::scanner::load_adapter(&self.root, None)?;
        let conn = rusqlite::Connection::open_with_flags(
            &self.db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        let schema_version: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap_or_default();
        if schema_version != crate::engine::scanner::ENGINE_SCHEMA_VERSION {
            return Ok(true);
        }
        let persisted_digest: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key='adapter_digest'",
                [],
                |r| r.get(0),
            )
            .unwrap_or_default();
        if current_adapter.digest != persisted_digest {
            return Ok(true);
        }
        let persisted_version: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key='adapter_version'",
                [],
                |r| r.get(0),
            )
            .unwrap_or_default();
        if current_adapter.adapter_version.as_deref().unwrap_or("") != persisted_version {
            return Ok(true);
        }
        let current_linked_json =
            serde_json::to_string(&current_adapter.linked_workspaces).unwrap_or_default();
        let persisted_linked: Result<String, _> = conn.query_row(
            "SELECT value FROM metadata WHERE key='adapter_linked_workspaces'",
            [],
            |r| r.get(0),
        );
        match persisted_linked {
            Ok(persisted) => {
                if persisted != current_linked_json {
                    return Ok(true);
                }
            }
            Err(_) => {
                if !current_adapter.linked_workspaces.is_empty() {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    pub fn read(
        &self,
        f: impl FnOnce(&rusqlite::Connection) -> Result<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        self.ensure_fresh()?;
        let identity = crate::db::cache::identity(&self.db)?;
        let mut slot = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("database connection lock poisoned"))?;
        let fresh = slot.as_ref().is_none_or(|(old, _)| *old != identity);
        if fresh {
            let conn = crate::db::reader::open(&self.db, &self.root)?;
            *slot = Some((identity.clone(), conn));
        }
        let conn = &slot
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("database unavailable"))?
            .1;
        if !fresh {
            conn.execute_batch("BEGIN DEFERRED")?;
        }
        let result = f(conn);
        let unchanged = crate::db::cache::identity(&self.db).map(|after| after == identity);
        let ended = conn.execute_batch("ROLLBACK");
        if let Err(error) = ended {
            *slot = None;
            return Err(error.into());
        }
        if !unchanged? {
            *slot = None;
            anyhow::bail!("generation changed during query; retry against the current snapshot");
        }
        result
    }
}
pub async fn serve(root: PathBuf, db: PathBuf) -> Result<()> {
    let service = Server::new(root, db)
        .serve(rmcp::transport::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}
