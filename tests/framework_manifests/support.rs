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
pub(super) use std::{collections::BTreeMap, fs, path::PathBuf};

pub(super) use crate::support::Workspace;

#[allow(dead_code)]
pub(super) fn write_files(workspace: &Workspace, files: &[(&str, &str)]) {
    for &(path, content) in files {
        workspace.write(path, content);
    }
}

#[allow(dead_code)]
pub(super) fn query_rows(conn: &Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[allow(dead_code)]
pub(super) fn query_rows_params<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> Vec<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map(params, |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[allow(dead_code)]
pub(super) fn query_count<P: rusqlite::Params>(conn: &Connection, sql: &str, params: P) -> i64 {
    conn.query_row(sql, params, |row| row.get(0)).unwrap()
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
    conn.query_row(
        "SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.name=?2 ORDER BY n.id",
        params![path, name],
        |row| row.get(0),
    )
    .unwrap()
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
    conn.prepare(
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
    conn.prepare(
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
    conn.prepare(
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
pub(super) fn read_facts_json(conn: &Connection, path: &str) -> Value {
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT facts_blob FROM local_facts WHERE path=?1",
            [path],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::to_value(reader::decode_local_facts(&blob).unwrap()).unwrap()
}

#[allow(dead_code)]
pub(super) fn all_local_facts(conn: &Connection) -> Vec<(String, Value)> {
    conn.prepare("SELECT path, facts_blob FROM local_facts ORDER BY path")
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

#[allow(dead_code, clippy::too_many_arguments)]
pub(super) fn assert_edge_canonical(
    conn: &Connection,
    source_path: &str,
    source_name: &str,
    kind: &str,
    target_path: &str,
    target_name: &str,
    active: bool,
    pack_name: &str,
) {
    let actual: Vec<(String, String)> = conn
        .prepare(
            "SELECT s.id, d.id FROM edge_occurrences e \
             JOIN nodes s ON s.node_hash=e.src_hash \
             JOIN nodes d ON d.node_hash=e.dst_hash \
             JOIN path_dictionary sp ON sp.path_id=s.path_id \
             JOIN path_dictionary dp ON dp.path_id=d.path_id \
             WHERE sp.path=?1 AND s.name=?2 AND e.kind=?3 AND dp.path=?4 AND d.name=?5 \
             ORDER BY s.id, d.id",
        )
        .unwrap()
        .query_map(
            params![source_path, source_name, kind, target_path, target_name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
        .map(Result::unwrap)
        .collect();
    if active {
        assert_eq!(
            actual.len(),
            1,
            "{pack_name}: persisted {source_path}:{source_name} --{kind}--> {target_path}:{target_name}: {actual:?}"
        );
        for (path, name, id) in [
            (source_path, source_name, &actual[0].0),
            (target_path, target_name, &actual[0].1),
        ] {
            let canonical = query_rows_params(
                conn,
                "SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.name=?2 ORDER BY n.id",
                params![path, name],
            );
            assert_eq!(
                canonical.as_slice(),
                std::slice::from_ref(id),
                "unique canonical declaration"
            );
        }
    } else {
        assert!(actual.is_empty(), "{pack_name} noneligible {actual:?}");
    }
}

#[allow(dead_code)]
pub(super) fn assert_manifest_build_rejected(
    manifest_rel_path: &str,
    manifest_content: &str,
    extra_files: &[(&str, &str)],
) -> Result<(), String> {
    let workspace = Workspace::new();
    workspace.write(manifest_rel_path, manifest_content);
    write_files(&workspace, extra_files);
    let result = writer::build(&workspace.0, &workspace.db(), None);
    if result.is_ok() {
        return Err(format!("{manifest_rel_path} was accepted but should fail"));
    }
    if workspace.db().exists() {
        return Err(format!("{manifest_rel_path} published an index database"));
    }
    Ok(())
}

#[allow(dead_code)]
pub(super) fn custom_adapter_config(
    enabled_path: &std::path::Path,
    disabled_path: &std::path::Path,
) -> String {
    format!(
        "roots: [.]\nexcluded_directory_names: [excluded]\nlinked_workspaces:\n  - name: enabled\n    path: '{}'\n    roots: [.]\n  - name: disabled\n    path: '{}'\n    enabled: false\n    roots: [.]\n",
        enabled_path.display(),
        disabled_path.display()
    )
}
