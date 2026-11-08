use crate::common::Workspace;
use contextunity_forge_mcp::db::reader;
use rusqlite::Connection;
use serde_json::Value;
use std::path::Path;

pub enum MetadataVersion {
    Schema,
    Semantics,
}

#[derive(Debug)]
pub enum IndexCorruption {
    Freelist,
    DocumentSearchData,
}

/// Mutates the real persisted index while leaving admission to production callers.
pub struct IndexFixture(Connection);

impl IndexFixture {
    pub fn open(database: &Path) -> Self {
        Self(Connection::open(database).unwrap())
    }

    pub fn outdated(&self, version: MetadataVersion) {
        let key = match version {
            MetadataVersion::Schema => "schema_version",
            MetadataVersion::Semantics => "index_semantics_version",
        };
        self.0
            .execute("UPDATE metadata SET value='0' WHERE key=?1", [key])
            .unwrap();
    }

    pub fn seal(&self) {
        contextunity_forge_mcp::core::commitments::seal(&self.0).unwrap();
    }

    pub fn tamper_function_names(&self, name: &str) -> anyhow::Result<()> {
        self.0
            .execute("UPDATE nodes SET name=?1 WHERE kind='function'", [name])?;
        Ok(())
    }

    pub fn checkpoint(&self) {
        self.0
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
    }

    pub fn corrupt_file(database: &Path, corruption: IndexCorruption) -> Vec<u8> {
        let mut damaged = std::fs::read(database).unwrap();
        match corruption {
            IndexCorruption::Freelist => damaged[36..40].copy_from_slice(&1_u32.to_be_bytes()),
            IndexCorruption::DocumentSearchData => {
                let connection = Connection::open(database).unwrap();
                let page_size = connection
                    .query_row("PRAGMA page_size", [], |row| row.get::<_, usize>(0))
                    .unwrap();
                let root_page = connection
                    .query_row(
                        "SELECT rootpage FROM sqlite_schema WHERE name='doc_search_data'",
                        [],
                        |row| row.get::<_, usize>(0),
                    )
                    .unwrap();
                damaged[(root_page - 1) * page_size] = 0xff;
            }
        }
        std::fs::write(database, damaged).unwrap();
        std::fs::read(database).unwrap()
    }
}

/// A real task-store fixture for navigation's independence from task claims.
pub struct TaskClaimsFixture {
    _connection: Connection,
}

impl TaskClaimsFixture {
    pub fn worktrees_only(workspace: &Workspace, database: &str, worktrees: &[&Path]) -> Self {
        let database = workspace.path(database);
        std::fs::create_dir_all(database.parent().unwrap()).unwrap();
        let connection = Connection::open(database).unwrap();
        connection
            .execute_batch("CREATE TABLE task_claims(worktree TEXT, ended INTEGER);")
            .unwrap();
        for worktree in worktrees {
            connection
                .execute(
                    "INSERT INTO task_claims VALUES(?1, 0)",
                    [worktree.to_str().unwrap()],
                )
                .unwrap();
        }
        Self {
            _connection: connection,
        }
    }
}

pub fn stored_node_count(connection: &Connection) -> anyhow::Result<i64> {
    Ok(connection.query_row("SELECT count(*) FROM nodes", [], |row| row.get(0))?)
}

pub fn claim_session_fingerprint(
    workspace: &Workspace,
    task_id: &str,
    revision: u64,
) -> anyhow::Result<String> {
    let store = contextunity_forge_mcp::engine::tasks::store_for_task(workspace.root(), task_id)?;
    Ok(store.connection.query_row(
        "SELECT session_fingerprint FROM task_claims WHERE task_id=?1 AND revision=?2",
        rusqlite::params![task_id, revision],
        |row| row.get(0),
    )?)
}

pub fn manifest_digest_at(database: &Path) -> rusqlite::Result<String> {
    let connection = Connection::open(database)?;
    connection.query_row(
        "SELECT value FROM metadata WHERE key='manifest_digest'",
        [],
        |row| row.get(0),
    )
}

pub fn first_function_name(connection: &Connection) -> rusqlite::Result<String> {
    connection.query_row("SELECT name FROM nodes WHERE kind='function'", [], |row| {
        row.get(0)
    })
}

pub fn output_root(connection: &Connection) -> rusqlite::Result<String> {
    connection.query_row(
        "SELECT value FROM metadata WHERE key='output_root'",
        [],
        |row| row.get(0),
    )
}

pub fn mark_legacy_index_semantics(database: &Path) -> anyhow::Result<()> {
    let connection = Connection::open(database)?;
    connection.execute(
        "UPDATE metadata SET value='10:compact-storage-v8:legacy' WHERE key='index_semantics_version'",
        [],
    )?;
    contextunity_forge_mcp::core::commitments::seal(&connection)?;
    Ok(())
}

pub fn edge_rows(workspace: &Workspace) -> anyhow::Result<Vec<Value>> {
    let connection = workspace.open();
    reader::rows(
        &connection,
        "SELECT a.name src,b.name dst,e.kind FROM edges e JOIN nodes a ON a.node_hash=e.src_hash JOIN nodes b ON b.node_hash=e.dst_hash",
        &[],
        1000,
    )
}

pub fn edge_occurrence_snapshot(
    connection: &Connection,
    path: &str,
) -> rusqlite::Result<Vec<String>> {
    let mut statement = connection.prepare(
        "SELECT (SELECT id FROM nodes WHERE node_hash=edge_occurrences.src_hash)||'|'||(SELECT id FROM nodes WHERE node_hash=edge_occurrences.dst_hash)||'|'||kind FROM edge_occurrences WHERE (SELECT path FROM path_dictionary WHERE path_id=edge_occurrences.owner_id)=?1 ORDER BY (SELECT id FROM nodes WHERE node_hash=edge_occurrences.src_hash),(SELECT id FROM nodes WHERE node_hash=edge_occurrences.dst_hash),kind",
    )?;
    let rows = statement
        .query_map([path], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(rows)
}
