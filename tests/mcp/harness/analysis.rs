use crate::common::Workspace;
use crate::harness::{HarnessResult, McpTestHarness};
use contextunity_forge_mcp::db::reader;
use serde_json::Value;
use std::sync::OnceLock;

static INDEXED_ANALYSIS_FIXTURE: OnceLock<AnalysisFixture> = OnceLock::new();

/// Owns the real workspace used by MCP analysis contract tests.
pub struct AnalysisFixture {
    workspace: Workspace,
}

impl AnalysisFixture {
    pub fn from_workspace(workspace: Workspace) -> Self {
        Self { workspace }
    }

    pub fn indexed_files(files: &[(&str, &str)]) -> Self {
        let workspace = Workspace::new();
        workspace.write_all(files);
        workspace.build();
        Self { workspace }
    }

    pub fn indexed() -> &'static Self {
        INDEXED_ANALYSIS_FIXTURE.get_or_init(|| {
            let workspace = analysis_workspace();
            workspace.build();
            Self { workspace }
        })
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn start_mcp(&self) -> HarnessResult<McpTestHarness> {
        McpTestHarness::new(&self.workspace)
    }

    pub fn rows(
        &self,
        query: &str,
        parameters: &[&dyn rusqlite::ToSql],
        limit: usize,
    ) -> anyhow::Result<Vec<Value>> {
        let connection = self.workspace.open();
        reader::rows(&connection, query, parameters, limit)
    }
}

/// Guards against mutations in tight loops using a retained read-only SQLite connection.
pub struct PersistedWriteMarker {
    connection: rusqlite::Connection,
    baseline: (i64, i64, String),
}

impl PersistedWriteMarker {
    pub fn new(workspace: &Workspace) -> Self {
        let connection = rusqlite::Connection::open_with_flags(
            workspace.db_path(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .unwrap_or_else(|error| panic!("could not open test workspace index read-only: {error:#}"));
        let baseline = Self::query_marker(&connection);
        Self {
            connection,
            baseline,
        }
    }

    fn query_marker(connection: &rusqlite::Connection) -> (i64, i64, String) {
        let data_version: i64 = connection
            .query_row("PRAGMA data_version", [], |row| row.get(0))
            .unwrap_or_else(|error| panic!("could not read PRAGMA data_version: {error:#}"));
        let schema_version: i64 = connection
            .query_row("PRAGMA schema_version", [], |row| row.get(0))
            .unwrap_or_else(|error| panic!("could not read PRAGMA schema_version: {error:#}"));
        let output_root: String = connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'output_root'",
                [],
                |row| row.get(0),
            )
            .unwrap_or_else(|error| panic!("could not read output_root metadata: {error:#}"));
        (data_version, schema_version, output_root)
    }

    pub fn current(&self) -> (i64, i64, String) {
        Self::query_marker(&self.connection)
    }

    pub fn baseline(&self) -> &(i64, i64, String) {
        &self.baseline
    }
}

/// Reads a stable full-state snapshot for non-mutation assertions.
pub fn persisted_state(workspace: &Workspace) -> anyhow::Result<Value> {
    let connection = workspace.open();
    let schema = reader::rows(
        &connection,
        "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name,tbl_name",
        &[],
        100_000,
    )?;
    let table_names = {
        let mut statement = connection.prepare(
            "SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let names = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        names
    };
    let tables = table_names
        .into_iter()
        .map(|name| {
            let quoted_name = name.replace('"', "\"\"");
            let mut rows = reader::rows(
                &connection,
                &format!("SELECT * FROM \"{quoted_name}\""),
                &[],
                100_000,
            )?;
            rows.sort_by_key(|row| row.to_string());
            Ok((name, rows))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(serde_json::json!({
        "schema": schema,
        "tables": tables,
        "pragmas": {
            "application_id": connection.query_row("PRAGMA application_id", [], |row| row.get::<_, i64>(0))?,
            "freelist_count": connection.query_row("PRAGMA freelist_count", [], |row| row.get::<_, i64>(0))?,
            "journal_mode": connection.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))?,
            "page_count": connection.query_row("PRAGMA page_count", [], |row| row.get::<_, i64>(0))?,
            "schema_version": connection.query_row("PRAGMA schema_version", [], |row| row.get::<_, i64>(0))?,
            "user_version": connection.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))?,
        },
    }))
}

fn analysis_workspace() -> Workspace {
    let workspace = Workspace::new();
    workspace.write("forge-mcp.yaml", "tasks_db: .forge/tasks.sqlite\n");
    workspace.write("cycle.py", "def a(): b()\ndef b(): a()\n");
    workspace.write("other.py", "def idle(): pass\n");
    workspace
}
