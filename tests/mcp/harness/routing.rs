use crate::common::Workspace;
use contextunity_forge_mcp::{db::reader, engine::pipeline};
use rusqlite::Connection;
use serde_json::Value;

pub struct RoutingFixture {
    workspace: Workspace,
    imports: &'static str,
}

pub struct RouteSnapshot {
    pub incremental: Connection,
    cold: Connection,
}

pub struct FunctionPair {
    pub entrypoint: Option<String>,
    pub incremental_details: String,
    pub cold_details: String,
}

impl RoutingFixture {
    pub fn new(
        path: &str,
        source: &str,
        imports: &'static str,
        routes: &str,
    ) -> anyhow::Result<Self> {
        let workspace = Workspace::new();
        workspace.write("requirements.txt", "django\n");
        workspace.write(path, source);
        workspace.write("urls.py", &format!("{imports}{routes}\n"));
        pipeline::build_with_cache_dir(
            workspace.root(),
            &workspace.db(),
            None,
            &workspace.path(".forge/extraction-cache"),
        )?;
        Ok(Self { workspace, imports })
    }

    pub fn change_routes(&self, routes: &str) -> anyhow::Result<Value> {
        self.workspace
            .write("urls.py", &format!("{}{routes}\n", self.imports));
        pipeline::delta_with_cache_dir(
            self.workspace.root(),
            &self.workspace.db(),
            &["urls.py".into()],
            &self.workspace.path(".forge/extraction-cache"),
        )
    }

    pub fn build_cold(&self) -> anyhow::Result<Value> {
        pipeline::build_with_cache_dir(
            self.workspace.root(),
            &self.workspace.path(".forge/cold.sqlite"),
            None,
            &self.workspace.path(".forge/extraction-cache"),
        )
    }

    pub fn snapshot(&self) -> anyhow::Result<RouteSnapshot> {
        Ok(RouteSnapshot {
            incremental: reader::open(&self.workspace.db(), self.workspace.root())?,
            cold: Connection::open(self.workspace.path(".forge/cold.sqlite"))?,
        })
    }
}

impl RouteSnapshot {
    pub fn function(&self, name: &str) -> rusqlite::Result<FunctionPair> {
        let entrypoint = self.incremental.query_row(
            "SELECT json_extract(details,'$.entrypoint') FROM nodes WHERE name=?1 AND kind='function'", [name], |row| row.get(0),
        )?;
        let details = |db: &Connection| {
            db.query_row(
                "SELECT details FROM nodes WHERE name=?1 AND kind='function'",
                [name],
                |row| row.get(0),
            )
        };
        Ok(FunctionPair {
            entrypoint,
            incremental_details: details(&self.incremental)?,
            cold_details: details(&self.cold)?,
        })
    }
}
