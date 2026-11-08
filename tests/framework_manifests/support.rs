pub(super) use crate::common::Workspace;
pub(super) use contextunity_forge_mcp::{
    core::commitments,
    db::{reader, writer},
    engine::{
        ast,
        languages::{self, dependency_registry::DependencyRegistry, LanguageFamily},
        linker,
    },
};
pub(super) use rusqlite::{params, Connection};
pub(super) use serde_json::Value;
pub(super) use std::collections::BTreeMap;

pub(super) mod cross_pack_fixture;
mod dependency_discovery;
pub(super) mod fastapi_rows;
pub(super) use cross_pack_fixture::{
    cross_pack_sources, django_include_sources, GRIDVIEW_RECEIVER_CASES,
};
mod source_registry;
mod virtual_workspace;
pub(super) use virtual_workspace::VirtualWorkspace;

pub(super) struct FrameworkHarness {
    pub(super) registry: DependencyRegistry,
}
impl FrameworkHarness {
    pub(super) fn persisted(files: &[(&str, &str)]) -> Workspace {
        let workspace = Workspace::new();
        workspace.write_all(files);
        workspace
    }

    pub(super) fn from_manifests(files: &[(&str, &str)]) -> anyhow::Result<Self> {
        use languages::dependency_registry::{ManifestSource, WorkspaceManifestSources};
        let manifests: Vec<_> = files
            .iter()
            .map(|&(path, source)| ManifestSource { path, source })
            .collect();
        let registry = DependencyRegistry::try_collect_from_sources([WorkspaceManifestSources {
            name: "",
            manifests: &manifests,
        }])?;
        Ok(Self { registry })
    }

    pub(super) fn published(files: &[(&str, &str)]) -> Workspace {
        let workspace = Self::persisted(files);
        workspace.build();
        workspace
    }

    pub(super) fn publish_virtual(sources: VirtualWorkspace) -> Workspace {
        let workspace = Self::persisted(&[]);
        for (path, source) in sources.files {
            workspace.write(path, &source);
        }
        workspace.build();
        workspace
    }

    pub(super) fn link_sources(
        &self,
        files: &[(&str, &str, &str)],
    ) -> anyhow::Result<contextunity_forge_mcp::core::models::Graph> {
        let facts = Self::extract_with_registry(&self.registry, files)?;
        Ok(linker::link_compact_with_typed_registry(&facts, None, None, &self.registry).into())
    }

    pub(super) fn extract_with_registry(
        registry: &DependencyRegistry,
        files: &[(&str, &str, &str)],
    ) -> anyhow::Result<BTreeMap<String, contextunity_forge_mcp::forge_core::typed_facts::TypedFacts>>
    {
        files
            .iter()
            .map(|&(path, language, source)| {
                ast::extract_typed_with_registry(path, language, source, registry)
                    .map(|facts| (path.to_owned(), facts))
            })
            .collect()
    }

    pub(super) fn bundled_registry() -> &'static DependencyRegistry {
        static REGISTRY: std::sync::OnceLock<DependencyRegistry> = std::sync::OnceLock::new();
        REGISTRY.get_or_init(|| DependencyRegistry::collect(None))
    }

    pub(super) fn extract(
        path: &str,
        language: &str,
        source: &str,
    ) -> anyhow::Result<contextunity_forge_mcp::core::models::Facts> {
        ast::extract_typed_with_registry(path, language, source, Self::bundled_registry())
            .map(|facts| facts.into_public())
    }

    pub(super) fn link(
        facts: &BTreeMap<String, contextunity_forge_mcp::core::models::Facts>,
    ) -> contextunity_forge_mcp::core::models::Graph {
        linker::link_with_registry(facts, None, None, Self::bundled_registry())
    }
}

#[allow(dead_code)]
pub(super) fn query_rows(conn: &Connection, sql: &str) -> Vec<String> {
    crate::common::sqlite::query_strings(conn, sql, [])
}

pub(super) fn publication_commitments(conn: &Connection) -> Vec<String> {
    query_rows(
        conn,
        "SELECT json_array(domain,owner_id,hex(digest)) FROM domain_commitments ORDER BY domain,owner_id",
    )
}

pub(super) fn error_messages(conn: &Connection) -> Vec<String> {
    query_rows(conn, "SELECT message FROM errors")
}

pub(super) fn named_node_ids(conn: &Connection, path: &str, name: &str) -> Vec<String> {
    query_rows_params(
        conn,
        "SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.name=?2 ORDER BY n.id",
        params![path, name],
    )
}

pub(super) fn route_names(conn: &Connection) -> Vec<String> {
    query_rows(
        conn,
        "SELECT name FROM nodes WHERE kind='route' ORDER BY name",
    )
}

pub(super) fn call_target_names(
    conn: &Connection,
    source_path: Option<&str>,
    source: &str,
) -> Vec<String> {
    query_rows_params(
        conn,
        "SELECT d.name FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE (?1 IS NULL OR p.path=?1) AND s.name=?2 AND e.kind='calls'",
        params![source_path, source],
    )
}

pub(super) fn route_handle_labels(conn: &Connection) -> Vec<String> {
    query_rows(
        conn,
        "SELECT n.name||'->'||d.name FROM edge_occurrences e JOIN nodes n ON n.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='handles' ORDER BY n.name",
    )
}

pub(super) fn owner_commitments(conn: &Connection, path: &str) -> Vec<String> {
    query_rows_params(
        conn,
        "SELECT domain||'|'||hex(digest) FROM domain_commitments WHERE owner_id=(SELECT path_id FROM path_dictionary WHERE path=?1) ORDER BY domain",
        [path],
    )
}

pub(super) fn coverage_details(conn: &Connection) -> Vec<(String, String, String)> {
    conn.prepare_cached("SELECT (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)")
        .unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(Result::unwrap).collect()
}

pub(super) fn coverage_for_expression(
    conn: &Connection,
    path: &str,
    expression: &str,
    line: Option<usize>,
) -> Vec<(String, String)> {
    conn.prepare_cached("SELECT status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2 AND (?3 IS NULL OR line=?3)")
        .unwrap().query_map(params![path,expression,line], |row| Ok((row.get(0)?,row.get(1)?))).unwrap().map(Result::unwrap).collect()
}

#[allow(dead_code)]
pub(super) fn persisted_graph_rows(conn: &Connection) -> Vec<Vec<String>> {
    vec![
        query_rows(
            conn,
            "SELECT (SELECT id FROM nodes WHERE node_hash=src_hash)||'|'||(SELECT id FROM nodes WHERE node_hash=dst_hash)||'|'||kind||'|'||(SELECT path FROM path_dictionary WHERE path_id=edges.path_id)||'|'||line FROM edges ORDER BY (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind,(SELECT path FROM path_dictionary WHERE path_id=edges.path_id),line",
        ),
        query_rows(
            conn,
            "SELECT (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)||'|'||line||'|'||(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)||'|'||status||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)",
        ),
    ]
}

#[allow(dead_code)]
pub(super) fn query_rows_params<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> Vec<String> {
    crate::common::sqlite::query_strings(conn, sql, params)
}

#[allow(dead_code)]
pub(super) fn query_count<P: rusqlite::Params>(conn: &Connection, sql: &str, params: P) -> i64 {
    crate::common::sqlite::query_value(conn, sql, params)
}

#[allow(dead_code)]
pub(super) fn query_count_usize<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> usize {
    conn.query_row(sql, params, |row| row.get(0)).unwrap()
}

#[allow(dead_code)]
pub(super) fn find_node_id(conn: &Connection, path: &str, name: &str) -> String {
    named_node_ids(conn, path, name).into_iter().next().unwrap()
}

#[allow(dead_code)]
pub(super) fn node_id_by_kind(conn: &Connection, path: &str, kind: &str, name: &str) -> String {
    conn.query_row(
        "SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.kind=?2 AND n.name=?3 ORDER BY n.id",
        params![path, kind, name],
        |row| row.get(0),
    )
    .unwrap()
}

#[allow(dead_code)]
pub(super) fn edge_dst_ids(
    conn: &Connection,
    src_path: &str,
    src_name: &str,
    kind: &str,
) -> Vec<String> {
    conn.prepare_cached(
        "SELECT d.id FROM edge_occurrences e \
         JOIN nodes s ON s.node_hash=e.src_hash \
         JOIN nodes d ON d.node_hash=e.dst_hash \
         JOIN path_dictionary p ON p.path_id=s.path_id \
         WHERE p.path=?1 AND s.name=?2 AND e.kind=?3 ORDER BY d.id",
    )
    .unwrap()
    .query_map(params![src_path, src_name, kind], |row| row.get(0))
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

#[allow(dead_code)]
pub(super) fn coverage_status(conn: &Connection, path: &str, expression: &str) -> String {
    conn.query_row(
        "SELECT c.status FROM resolution_coverage c \
         JOIN path_dictionary p ON p.path_id=c.path_id \
         JOIN coverage_expressions x ON x.expression_id=c.expression_id \
         WHERE p.path=?1 AND x.expression=?2",
        params![path, expression],
        |row| row.get(0),
    )
    .unwrap()
}

#[allow(dead_code)]
pub(super) fn coverage_status_and_evidence(
    conn: &Connection,
    path: &str,
    expression: &str,
) -> (String, String) {
    conn.query_row(
        "SELECT c.status, e.evidence FROM resolution_coverage c \
         JOIN coverage_expressions x ON x.expression_id=c.expression_id \
         JOIN coverage_evidence e ON e.evidence_id=c.evidence_id \
         JOIN path_dictionary p ON p.path_id=c.path_id \
         WHERE p.path=?1 AND x.expression=?2",
        params![path, expression],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap()
}

#[allow(dead_code)]
pub(super) fn coverage_statuses(
    conn: &Connection,
    path: &str,
    line: usize,
    expression_pattern: &str,
) -> Vec<String> {
    conn.prepare_cached(
        "SELECT DISTINCT c.status FROM resolution_coverage c \
         JOIN path_dictionary p ON p.path_id=c.path_id \
         JOIN coverage_expressions x ON x.expression_id=c.expression_id \
         WHERE p.path=?1 AND c.line=?2 AND x.expression LIKE ?3 ORDER BY c.status",
    )
    .unwrap()
    .query_map(params![path, line, expression_pattern], |r| r.get(0))
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

#[allow(dead_code)]
pub(super) fn coverage_statuses_for_path(
    conn: &Connection,
    path: &str,
    expression: &str,
) -> Vec<String> {
    conn.prepare_cached(
        "SELECT DISTINCT c.status FROM resolution_coverage c \
         JOIN path_dictionary p ON p.path_id=c.path_id \
         JOIN coverage_expressions x ON x.expression_id=c.expression_id \
         WHERE p.path=?1 AND x.expression=?2 ORDER BY c.status",
    )
    .unwrap()
    .query_map(params![path, expression], |row| row.get(0))
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

#[allow(dead_code)]
pub(super) fn named_bindings_for_domain<'a>(facts: &'a Value, domain: &str) -> Vec<&'a Value> {
    facts["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|node| {
            node["details"]["value_flow"]["named_bindings"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .filter(|fact| fact["domain"] == domain)
        .collect()
}

#[allow(dead_code)]
pub(super) fn all_local_facts(conn: &Connection) -> Vec<(String, Value)> {
    conn.prepare_cached("SELECT path, facts_blob FROM local_facts ORDER BY path")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .unwrap()
        .map(|row| {
            let (path, blob) = row.unwrap();
            (
                path,
                serde_json::to_value(reader::decode_local_facts(&blob).unwrap()).unwrap(),
            )
        })
        .collect()
}
