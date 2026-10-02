use contextunity_forge_mcp::{
    core::models::{Edge, Facts},
    db::{reader, writer},
    engine::{docs, linker, scanner},
    mcp::server::Server,
};
use rusqlite::{limits::Limit, Connection};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct ScopedWorkspace(PathBuf);
#[path = "core_basics/tasks.rs"]
mod tasks;
impl ScopedWorkspace {
    fn new(prefix: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("{prefix}_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, content: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
}

#[test]
fn public_linker_preserves_arbitrary_edge_tags_and_orders_edges_stably() {
    let unusual = Edge {
        src: "source-α".into(),
        dst: "target-β".into(),
        kind: "custom-kind-雪".into(),
        path: "z/文件.rs".into(),
        line: 17,
        evidence: "evidence-дані".into(),
        confidence: "confidence-🧭".into(),
    };
    let earlier = Edge {
        path: "a/файл.rs".into(),
        ..unusual.clone()
    };
    let facts = BTreeMap::from([(
        "input.rs".to_owned(),
        Facts {
            edges: vec![unusual.clone(), earlier.clone()],
            ..Facts::default()
        },
    )]);

    let linked = linker::link(&facts);
    assert_eq!(linked.edges.len(), 2);
    assert_eq!(linked.edges[0].path, earlier.path);
    assert_eq!(linked.edges[1].path, unusual.path);
    assert_eq!(
        serde_json::to_vec(&linked.edges[0]).unwrap(),
        serde_json::to_vec(&earlier).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(&linked.edges[1]).unwrap(),
        serde_json::to_vec(&unusual).unwrap()
    );
}
impl Drop for ScopedWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn test_markdown_doc_extractor() {
    let source = r#"# Architecture Overview

General system architecture description.

## Invariants
1. Services MUST NEVER import internal database directly.
2. ContextRouter routes via verified JWT tokens.

## References
Consult `contextunity.core.tokens` and `ServiceClient`.
"#;
    let sections =
        docs::extract("docs/architecture.md", source, 1000.0).expect("markdown extract failed");
    assert!(!sections.is_empty());
    assert_eq!(sections[0].path, "docs/architecture.md");
    assert!(sections.iter().any(
        |s| s.section_title.contains("Architecture") || s.section_title.contains("Invariants")
    ));
}

#[test]
fn default_scope_excludes_common_generated_directories() {
    let workspace = ScopedWorkspace::new("forge_scanner_scope");
    workspace.write("src/main.py", "def source(): pass\n");
    workspace.write("src/build_helpers.py", "def source(): pass\n");
    for directory in [
        "target",
        "node_modules",
        "build",
        "dist",
        "coverage",
        ".cache",
        ".next",
        ".nuxt",
        ".svelte-kit",
        ".pytest_cache",
        ".mypy_cache",
        ".ruff_cache",
        ".gradle",
        "__pycache__",
    ] {
        workspace.write(&format!("{directory}/generated.py"), "def source(): pass\n");
    }

    let report = scanner::scan(&workspace.0, None).unwrap();
    let paths: Vec<_> = report
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    assert_eq!(paths, ["src/build_helpers.py", "src/main.py"]);
}

#[test]
fn database_roundtrip_build_and_query() {
    let ws = ScopedWorkspace::new("forge_roundtrip");
    ws.write(
        "src/main.py",
        r#"
class Service:
    def execute(self):
        return helper()

def helper():
    return 42
"#,
    );
    ws.write(
        "README.md",
        r#"# Workspace Documentation
## Standards
All handlers must be asynchronous and tested.
"#,
    );

    let db_path = ws.0.join("code_map.sqlite");
    let report = writer::build(&ws.0, &db_path, None).expect("cold build failed");
    assert!(report["nodes"].as_i64().unwrap_or(0) > 0);
    assert!(report["files"].as_i64().unwrap_or(0) >= 2);

    let conn = reader::open(&db_path, &ws.0).expect("reader open failed");
    let overview = reader::overview(&conn).expect("overview failed");
    assert!(overview["counts"][0]["files"].as_i64().unwrap_or(0) >= 2);

    let docs = reader::search_docs(&conn, "Standards", None, None, 10).expect("search docs failed");
    assert!(!docs["sections"].as_array().unwrap().is_empty());
}

#[cfg(feature = "lang-rust")]
#[test]
fn cold_build_preserves_extracted_storage_across_full_and_remainder_batches() {
    use contextunity_forge_mcp::core::{commitments, models::stable_hash64};

    let ws = ScopedWorkspace::new("forge_storage_batches");
    for index in 0..251 {
        ws.write(
            &format!("src/unit_{index:03}.rs"),
            &format!(
                "/// Unicode λ, quotes \"double\", slash \\, and control \0.\npub fn unit_{index:03}() {{}}\n"
            ),
        );
    }
    let markdown: String = (0..251)
        .map(|index| {
            format!(
                "# Section {index:03} λ\n\nMUST preserve \"quotes\", slash \\, and control \0. See `unit_{index:03}`.\n\n"
            )
        })
        .collect();
    ws.write("README.md", &markdown);

    let adapter = scanner::load_adapter(&ws.0, None).unwrap();
    let scan = scanner::scan_with_adapter(&ws.0, &adapter).unwrap();
    let facts: Vec<_> = scan
        .entries
        .iter()
        .map(|entry| writer::extract(&ws.0, entry, &adapter).unwrap())
        .collect();
    assert_eq!(scan.entries.len(), 252);
    assert_eq!(facts.iter().map(|fact| fact.docs.len()).sum::<usize>(), 251);

    let db_path = ws.0.join(".forge/storage.sqlite");
    writer::build(&ws.0, &db_path, None).unwrap();
    let conn = Connection::open(&db_path).unwrap();
    let inventory: Vec<(String, String, String, u64)> = conn
        .prepare("SELECT path,status,digest,size FROM source_inventory ORDER BY rowid")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let expected_inventory: Vec<_> = scan
        .entries
        .iter()
        .map(|entry| {
            (
                entry.path.clone(),
                "indexed".to_owned(),
                entry.digest.clone(),
                entry.bytes,
            )
        })
        .collect();
    assert_eq!(inventory, expected_inventory);

    type LocalFactsRow = (String, String, String, bool, bool, Vec<u8>, String);
    let local_facts: Vec<LocalFactsRow> = conn
        .prepare("SELECT path,source_digest,language,is_test,generated,facts_blob,typeof(facts_blob) FROM local_facts ORDER BY rowid")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(local_facts.len(), scan.entries.len());
    let mut durable_facts = Vec::<Facts>::new();
    for (entry, (path, digest, language, is_test, generated, blob, storage_type)) in
        scan.entries.iter().zip(local_facts)
    {
        assert_eq!(path, entry.path);
        assert_eq!(digest, entry.digest);
        assert_eq!(language, entry.language);
        assert_eq!(
            is_test,
            contextunity_forge_mcp::core::models::is_test(&entry.path)
        );
        assert!(!generated);
        assert_eq!(storage_type, "blob");
        let durable_json = zstd::stream::decode_all(blob.as_slice()).unwrap();
        let fact: Facts = serde_json::from_slice(&durable_json).unwrap();
        let json = serde_json::to_vec(&fact).unwrap();
        assert_eq!(json, durable_json);
        assert_eq!(blob, zstd::stream::encode_all(json.as_slice(), 1).unwrap());
        durable_facts.push(fact);
    }

    for (index, node) in durable_facts
        .iter()
        .flat_map(|fact| &fact.nodes)
        .enumerate()
    {
        let (node_id, details): (i64, String) = conn
            .query_row(
                "SELECT node_id,details FROM nodes WHERE id=?1",
                [&node.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(node_id, index as i64 + 1);
        assert_eq!(details, serde_json::to_string(&node.details).unwrap());
    }
    let durable_nodes: BTreeMap<_, _> = durable_facts
        .iter()
        .flat_map(|fact| &fact.nodes)
        .map(|node| (node.id.as_str(), node))
        .collect();
    for node in facts.iter().flat_map(|fact| &fact.nodes) {
        let actual = durable_nodes.get(node.id.as_str()).unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(node).unwrap()
        );
    }

    let doc_rows: Vec<(i64, contextunity_forge_mcp::core::models::DocSection, String)> = conn
        .prepare("SELECT rowid,doc_id,path,section_title,doc_type,content,invariants,referenced_symbols,mtime,size,is_invariant,typeof(mtime) FROM doc_sections ORDER BY rowid")
        .unwrap()
        .query_map([], |row| {
            let invariants: String = row.get(6)?;
            let referenced_symbols: String = row.get(7)?;
            Ok((row.get(0)?, contextunity_forge_mcp::core::models::DocSection {
                doc_id: row.get(1)?,
                path: row.get(2)?,
                section_title: row.get(3)?,
                doc_type: row.get(4)?,
                content: row.get(5)?,
                invariants: serde_json::from_str(&invariants).unwrap(),
                referenced_symbols: serde_json::from_str(&referenced_symbols).unwrap(),
                mtime: row.get(8)?,
                size: row.get(9)?,
                is_invariant: row.get(10)?,
            }, row.get(11)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let expected_docs: Vec<_> = facts.iter().flat_map(|fact| &fact.docs).collect();
    assert_eq!(doc_rows.len(), expected_docs.len());
    for (index, ((rowid, actual, storage_type), expected)) in
        doc_rows.iter().zip(expected_docs).enumerate()
    {
        assert_eq!(*rowid, index as i64 + 1);
        assert_eq!(storage_type, "real");
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }

    let shared_keys: Vec<(i64, String)> = conn
        .prepare("SELECT key_hash,key FROM shared_keys ORDER BY key_hash")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(shared_keys.len() > 250);
    let keys_by_text: BTreeMap<_, _> = shared_keys
        .iter()
        .map(|(hash, key)| (key.as_str(), *hash))
        .collect();
    for node in facts.iter().flat_map(|fact| &fact.nodes) {
        assert_eq!(
            keys_by_text.get(node.qualname.as_str()),
            Some(&stable_hash64(&node.qualname))
        );
    }
    for (hash, key) in shared_keys {
        assert_eq!(hash, stable_hash64(&key));
    }
    commitments::verify(&conn).unwrap();
}

#[test]
fn rows_enforces_the_exact_serialized_json_limit() {
    const MAX_SERIALIZED_BYTES: usize = 8 * 1024 * 1024;
    const SQL: &str = r#"SELECT ?1 AS "a\b",0 AS integer_value,1.25 AS real_value,NULL AS null_value,X'00ff' AS blob_value,'[0]' AS details UNION ALL SELECT '',0,1.25,NULL,X'00ff','[0]'"#;

    let conn = Connection::open_in_memory().unwrap();
    conn.set_limit(Limit::SQLITE_LIMIT_LENGTH, MAX_SERIALIZED_BYTES as i32);

    let empty_rows = reader::rows(&conn, SQL, &[&""], 2).unwrap();
    let empty_size = serde_json::to_vec(&empty_rows).unwrap().len();

    let safe_value = "a".repeat(MAX_SERIALIZED_BYTES - empty_size);
    let safe_exact = reader::rows(&conn, SQL, &[&safe_value], 2)
        .ok()
        .map(|rows| serde_json::to_vec(&rows).unwrap().len());
    let mut safe_over_value = safe_value;
    safe_over_value.push('a');
    let safe_over = reader::rows(&conn, SQL, &[&safe_over_value], 2);
    let safe_over_rejected = match safe_over {
        Err(error) => error.to_string().contains("exceeds the 8 MiB row budget"),
        Ok(_) => false,
    };

    let remaining_bytes = MAX_SERIALIZED_BYTES - empty_size;
    let escaped_control_bytes = serde_json::to_vec(&"\u{0001}").unwrap().len() - 2;
    let control_chars = remaining_bytes / escaped_control_bytes;
    let plain_chars = remaining_bytes % escaped_control_bytes;
    let mut exact_value = "\u{0001}".repeat(control_chars);
    exact_value.push_str(&"a".repeat(plain_chars));

    let escaped_exact = reader::rows(&conn, SQL, &[&exact_value], 2)
        .ok()
        .map(|rows| serde_json::to_vec(&rows).unwrap().len());

    exact_value.push('a');
    let escaped_over = reader::rows(&conn, SQL, &[&exact_value], 2);
    let escaped_over_rejected = match escaped_over {
        Err(error) => error.to_string().contains("exceeds the 8 MiB row budget"),
        Ok(_) => false,
    };

    let duplicate_conn = Connection::open_in_memory().unwrap();
    let replaced_value = "a".repeat(MAX_SERIALIZED_BYTES - 1);
    let duplicate_rows = reader::rows(
        &duplicate_conn,
        "SELECT ?1 AS x, 'ok' AS x",
        &[&replaced_value],
        1,
    )
    .unwrap();

    assert!(
        safe_exact == Some(MAX_SERIALIZED_BYTES)
            && safe_over_rejected
            && escaped_exact == Some(MAX_SERIALIZED_BYTES)
            && escaped_over_rejected
            && duplicate_rows[0]["x"] == "ok"
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn test_adapter_change_auto_rebuild() {
    let ws = ScopedWorkspace::new("test_forge_adapter");
    ws.write("src/main.rs", "pub fn hello() {}\n");
    ws.write(
        "forge-mcp.yaml",
        "format: forge-code-map-policy/v1\nadapter_version: 1\nroots:\n  - src\n",
    );

    let db_path = ws.0.join(".forge/code-map.sqlite");
    let server = Server::new(ws.0.clone(), db_path.clone());

    let res = server.read(|conn| {
        let ver: String = conn.query_row(
            "SELECT value FROM metadata WHERE key='adapter_version'",
            [],
            |r| r.get(0),
        )?;
        Ok(serde_json::Value::String(ver))
    });
    assert_eq!(res.unwrap(), serde_json::Value::String("1".into()));

    ws.write(
        "forge-mcp.yaml",
        "format: forge-code-map-policy/v1\nadapter_version: 2\nroots:\n  - src\n",
    );

    let res2 = server.read(|conn| {
        let ver: String = conn.query_row(
            "SELECT value FROM metadata WHERE key='adapter_version'",
            [],
            |r| r.get(0),
        )?;
        Ok(serde_json::Value::String(ver))
    });
    assert_eq!(res2.unwrap(), serde_json::Value::String("2".into()));

    ws.write(
        "forge-mcp.yaml",
        "format: forge-code-map-policy/v1\nadapter_version: 2\nroots:\n  - src\nignore:\n  - target\n",
    );

    let res3 = server.read(|conn| {
        let digest: String = conn.query_row(
            "SELECT value FROM metadata WHERE key='adapter_digest'",
            [],
            |r| r.get(0),
        )?;
        Ok(serde_json::Value::String(digest))
    });
    assert!(res3.is_ok());
}
