use contextunity_forge_mcp::core::models::{Coverage, Facts, Graph, Node};
use contextunity_forge_mcp::engine::{ast, linker};
use rusqlite::{backup::Backup, Connection};
use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::Mutex;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct CallObservation {
    pub(super) caller_path: String,
    pub(super) caller_name: String,
    pub(super) caller_is_test: bool,
    pub(super) target_path: String,
    pub(super) target_name: String,
    pub(super) kind: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct CoverageObservation {
    pub(super) path: String,
    pub(super) line: i64,
    pub(super) expression: String,
    pub(super) status: String,
    pub(super) evidence: String,
}

/// Owns disk-backed profile fixtures and returns persisted observations.
pub(crate) struct PersistedWorkspaceHarness {
    workspace: crate::common::Workspace,
    cold_cache: crate::languages::support::ColdFilesystemReferenceCache,
}

impl PersistedWorkspaceHarness {
    pub(super) fn new() -> Self {
        Self {
            workspace: crate::common::Workspace::new(),
            cold_cache: Default::default(),
        }
    }

    pub(super) fn read(&self, path: impl AsRef<std::path::Path>) -> Vec<u8> {
        std::fs::read(self.workspace.path(path)).unwrap()
    }

    pub(super) fn build_and_open(&self) -> Connection {
        self.workspace.build();
        self.workspace.open()
    }

    pub(super) fn open_at(&self, database: impl AsRef<std::path::Path>) -> Connection {
        let conn =
            contextunity_forge_mcp::db::reader::open(database.as_ref(), self.workspace.root())
                .unwrap_or_else(|error| panic!("could not open test workspace index: {error:#}"));
        let _ = conn.execute_batch("COMMIT");
        conn
    }

    pub(super) fn remove(&self, path: impl AsRef<std::path::Path>) {
        self.workspace.remove(path);
    }

    pub(super) fn delta_cold(
        &self,
        path: &str,
    ) -> crate::languages::support::ColdObservations<crate::languages::support::DeltaRows> {
        self.workspace.delta(&[path]);
        self.cold_observations(crate::languages::support::delta_rows)
    }

    pub(super) fn cold_observations<T>(
        &self,
        observe: impl Fn(&Connection) -> T,
    ) -> crate::languages::support::ColdObservations<T> {
        self.cold_cache
            .observe(&self.workspace, self.workspace.open(), observe)
    }

    pub(super) fn calls(&self) -> Vec<CallObservation> {
        let connection = self.workspace.open();
        let mut statement = connection
            .prepare(
                "SELECT caller_path.path,caller.name,caller.is_test,target_path.path,target.name,edge.kind \
                 FROM edges edge \
                 JOIN nodes caller ON caller.node_hash=edge.src_hash \
                 JOIN nodes target ON target.node_hash=edge.dst_hash \
                 JOIN path_dictionary caller_path ON caller_path.path_id=caller.path_id \
                 JOIN path_dictionary target_path ON target_path.path_id=target.path_id \
                 ORDER BY caller_path.path,caller.name,target_path.path,target.name,edge.kind",
            )
            .unwrap();
        statement
            .query_map([], |row| {
                Ok(CallObservation {
                    caller_path: row.get(0)?,
                    caller_name: row.get(1)?,
                    caller_is_test: row.get::<_, i64>(2)? != 0,
                    target_path: row.get(3)?,
                    target_name: row.get(4)?,
                    kind: row.get(5)?,
                })
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    pub(super) fn coverage(&self) -> Vec<CoverageObservation> {
        let connection = self.workspace.open();
        let mut statement = connection
            .prepare(
                "SELECT path.path,coverage.line,expression.expression,coverage.status,evidence.evidence \
                 FROM resolution_coverage coverage \
                 JOIN path_dictionary path ON path.path_id=coverage.path_id \
                 JOIN coverage_expressions expression ON expression.expression_id=coverage.expression_id \
                 JOIN coverage_evidence evidence ON evidence.evidence_id=coverage.evidence_id \
                 ORDER BY path.path,coverage.line,expression.expression,coverage.status,evidence.evidence",
            )
            .unwrap();
        statement
            .query_map([], |row| {
                Ok(CoverageObservation {
                    path: row.get(0)?,
                    line: row.get(1)?,
                    expression: row.get(2)?,
                    status: row.get(3)?,
                    evidence: row.get(4)?,
                })
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }
}

impl Deref for PersistedWorkspaceHarness {
    type Target = crate::common::Workspace;

    fn deref(&self) -> &Self::Target {
        &self.workspace
    }
}

pub(super) fn fixture_workspace(files: &[(&str, &str)]) -> PersistedWorkspaceHarness {
    let workspace = PersistedWorkspaceHarness::new();
    workspace.write_all(files);
    workspace
}

pub(super) fn remove_fixture(workspace: &PersistedWorkspaceHarness, path: &str) {
    workspace.remove(path);
}

pub(super) fn cold_relations(
    workspace: &PersistedWorkspaceHarness,
) -> super::support::ColdObservations<super::support::RelationRows> {
    workspace.cold_observations(super::support::relation_rows)
}

pub(super) fn query_strings(
    connection: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Vec<String> {
    crate::common::sqlite::query_strings(connection, sql, params)
}

pub(super) fn query_scalar<T: rusqlite::types::FromSql>(
    connection: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> T {
    crate::common::sqlite::query_value(connection, sql, params)
}

/// Parser composition only: source and emitted facts stay in memory.
pub(super) struct ExtractorHarness<'a> {
    language: &'a str,
    path: &'a str,
}

/// Copies a real persisted baseline into independent query connections.
pub(super) struct PersistedSnapshot(Mutex<Connection>);

#[derive(Debug)]
pub(super) struct LinkedFacts {
    pub(super) facts: Facts,
    graph: Graph,
}

/// Links several source snippets in memory under their exact workspace paths.
#[derive(Debug)]
pub(super) struct LinkedCatalog {
    pub(super) facts: BTreeMap<String, Facts>,
    pub(super) graph: Graph,
}

impl LinkedCatalog {
    pub(super) fn coverage_line(&self, path: &str, line: usize, expression: &str) -> &str {
        self.graph
            .coverage
            .iter()
            .find(|row| row.path == path && row.line == line && row.expression == expression)
            .map(|row| row.status.as_str())
            .unwrap_or_else(|| panic!("missing coverage {path}:{line} {expression}"))
    }

    fn node_field<'a>(&'a self, id: &str, field: impl Fn(&'a Node) -> &'a str) -> &'a str {
        self.facts
            .values()
            .flat_map(|facts| facts.nodes.iter())
            .find(|node| node.id == id)
            .map(field)
            .unwrap_or_else(|| panic!("missing node {id}"))
    }

    pub(super) fn edge_qualnames(&self, src_qualname: &str, kind: &str) -> Vec<String> {
        let mut names = self
            .graph
            .edges
            .iter()
            .filter(|edge| {
                edge.kind == kind
                    && self.node_field(&edge.src, |node| node.qualname.as_str()) == src_qualname
            })
            .map(|edge| {
                self.node_field(&edge.dst, |node| node.qualname.as_str())
                    .to_owned()
            })
            .collect::<Vec<_>>();
        names.sort();
        names
    }


    pub(super) fn coverage_status(&self, path: &str, expression: &str) -> &str {
        self.graph
            .coverage
            .iter()
            .find(|row| row.path == path && row.expression == expression)
            .map(|row| row.status.as_str())
            .unwrap_or_else(|| panic!("missing coverage {path} {expression}"))
    }
}

impl Deref for LinkedFacts {
    type Target = Graph;

    fn deref(&self) -> &Graph {
        &self.graph
    }
}

impl LinkedFacts {
    pub(super) fn statuses(&self, expression: &str) -> Vec<&str> {
        self.graph
            .coverage
            .iter()
            .filter(|row| row.expression == expression)
            .map(|row| row.status.as_str())
            .collect()
    }

    pub(super) fn coverage<'a>(
        &'a self,
        expression: &'a str,
    ) -> impl Iterator<Item = &'a Coverage> {
        self.graph
            .coverage
            .iter()
            .filter(move |row| row.expression == expression)
    }

    pub(super) fn call_targets(&self, expression: &str) -> Vec<&Node> {
        self.graph
            .edges
            .iter()
            .filter(|edge| edge.kind == "calls" && edge.evidence == expression)
            .filter_map(|edge| self.facts.nodes.iter().find(|node| node.id == edge.dst))
            .collect()
    }
}

impl PersistedSnapshot {
    pub(super) fn from_connection(source: &Connection) -> Self {
        Self(Mutex::new(Self::copy(source)))
    }

    pub(super) fn open(&self) -> Connection {
        Self::copy(&self.0.lock().unwrap())
    }

    fn copy(source: &Connection) -> Connection {
        let mut connection = Connection::open_in_memory().unwrap();
        Backup::new(source, &mut connection)
            .unwrap()
            .run_to_completion(256, std::time::Duration::ZERO, None)
            .unwrap();
        connection
    }
}

impl<'a> ExtractorHarness<'a> {
    pub(super) fn for_lang(language: &'a str) -> Self {
        Self {
            language,
            path: "snippet",
        }
    }

    pub(super) fn at_path(mut self, path: &'a str) -> Self {
        self.path = path;
        self
    }

    pub(super) fn extract(&self, source: &str) -> Facts {
        self.try_extract(source)
            .unwrap_or_else(|error| panic!("{} ({}): {error}", self.path, self.language))
    }

    pub(super) fn try_extract(&self, source: &str) -> anyhow::Result<Facts> {
        ast::extract(self.path, self.language, source)
    }

    pub(super) fn link_graph(facts: &BTreeMap<String, Facts>) -> Graph {
        linker::link(facts)
    }

    pub(super) fn link_graph_with_root(
        facts: &BTreeMap<String, Facts>,
        root: Option<&std::path::Path>,
    ) -> Graph {
        linker::link_with_root(facts, None, root)
    }

    pub(super) fn linked(&self, source: &str) -> LinkedFacts {
        let mut catalog = BTreeMap::from([(self.path.to_owned(), self.extract(source))]);
        let graph = linker::link(&catalog);
        LinkedFacts {
            facts: catalog.remove(self.path).unwrap(),
            graph,
        }
    }

    pub(super) fn linked_with_root(
        language: &str,
        sources: &[(&str, &str)],
        root: Option<&std::path::Path>,
    ) -> LinkedCatalog {
        let facts = sources
            .iter()
            .map(|(path, source)| {
                let parsed = ast::extract(path, language, source)
                    .unwrap_or_else(|error| panic!("{path} ({language}): {error}"));
                ((*path).to_owned(), parsed)
            })
            .collect::<BTreeMap<_, _>>();
        let graph = linker::link_with_root(&facts, None, root);
        LinkedCatalog { facts, graph }
    }
}
