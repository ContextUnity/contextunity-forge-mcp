use crate::common::Workspace;
use contextunity_forge_mcp::{
    core::{commitments, models::DocSection},
    db::{reader, writer},
    engine::{pipeline, scanner},
};
use rayon::prelude::*;
use rusqlite::Connection;
use std::collections::BTreeMap;

#[cfg(feature = "lang-rust")]
#[test]
fn cold_build_preserves_extracted_storage_across_full_and_remainder_batches() -> anyhow::Result<()>
{
    use contextunity_forge_mcp::core::models::stable_hash64;

    let item_count = 251;
    let workspace = Workspace::new();
    for index in 0..item_count {
        workspace.write(
            format!("src/unit_{index:03}.rs"),
            &format!(
                "/// Unicode λ, quotes \"double\", slash \\, and control \0.\npub fn unit_{index:03}() {{}}\n"
            ),
        );
        workspace.write(
            format!("docs/unit_{index:03}.md"),
            &format!(
                "# Section {index:03} λ\n\nMUST preserve \"quotes\", slash \\, and control \0. See `unit_{index:03}`.\n\n"
            ),
        );
    }

    let adapter = scanner::load_adapter(workspace.root(), None)?;
    let scan = scanner::scan_with_adapter(workspace.root(), &adapter)?;
    let sources = &scan.entries;
    let extraction_pool = rayon::ThreadPoolBuilder::new().num_threads(4).build()?;
    let extracted_facts = extraction_pool.install(|| {
        scan.entries
            .par_iter()
            .map(|entry| writer::extract(workspace.root(), entry, &adapter))
            .collect::<anyhow::Result<Vec<_>>>()
    })?;

    let db_path = workspace.root().join(".forge/storage.sqlite");
    extraction_pool.install(|| {
        pipeline::build_with_cache_dir(
            workspace.root(),
            &db_path,
            None,
            &workspace.path(".forge/extraction-cache"),
        )
    })?;
    let conn = Connection::open(&db_path)?;

    let inventory: Vec<(String, String, String, u64)> = conn
        .prepare("SELECT path,status,digest,size FROM source_inventory ORDER BY rowid")?
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?;

    #[allow(clippy::type_complexity)]
    let local_facts: Vec<(String, String, String, bool, bool, Vec<u8>, String)> = conn
        .prepare("SELECT path,source_digest,language,is_test,generated,facts_blob,typeof(facts_blob) FROM local_facts ORDER BY rowid")?
        .query_map([], |row| {
            let blob: Vec<u8> = row.get(5)?;
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                blob,
                row.get(6)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let nodes: Vec<(i64, String, String)> = conn
        .prepare("SELECT node_id,id,details FROM nodes ORDER BY node_id")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let doc_rows = conn
        .prepare("SELECT rowid,doc_id,path,section_title,doc_type,content,invariants,referenced_symbols,mtime,size,is_invariant,typeof(mtime) FROM doc_sections ORDER BY rowid")?
        .query_map([], |row| {
            let invariants: String = row.get(6)?;
            let referenced_symbols: String = row.get(7)?;
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                invariants,
                referenced_symbols,
                row.get(8)?,
                row.get(9)?,
                row.get::<_, bool>(10)?,
                row.get::<_, String>(11)?,
            ))
        })
        ?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let docs = doc_rows
        .into_iter()
        .map(
            |(
                rowid,
                doc_id,
                path,
                section_title,
                doc_type,
                content,
                invariants,
                referenced_symbols,
                mtime,
                size,
                is_invariant,
                storage_type,
            )| {
                Ok((
                    rowid,
                    DocSection {
                        doc_id,
                        path,
                        section_title,
                        doc_type,
                        content,
                        invariants: serde_json::from_str(&invariants)?,
                        referenced_symbols: serde_json::from_str(&referenced_symbols)?,
                        mtime,
                        size,
                        is_invariant,
                    },
                    storage_type,
                ))
            },
        )
        .collect::<anyhow::Result<Vec<_>>>()?;
    let shared_keys: Vec<(i64, String)> = conn
        .prepare("SELECT k.key_hash,CASE WHEN typeof(k.key)='integer' THEN n.qualname ELSE k.key END FROM shared_keys k LEFT JOIN nodes n ON n.node_id=k.key AND typeof(k.key)='integer' ORDER BY k.key_hash")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        ?
        .collect::<rusqlite::Result<_>>()?;
    let commitments_verification = commitments::verify(&conn);
    let expected_inventory: Vec<_> = sources
        .iter()
        .map(|source| {
            (
                source.path.clone(),
                "indexed".to_owned(),
                source.digest.clone(),
                source.bytes,
            )
        })
        .collect();
    let decoded_local_facts = local_facts
        .iter()
        .map(|stored| reader::decode_local_facts(&stored.5))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let durable: std::collections::BTreeMap<_, _> = decoded_local_facts
        .iter()
        .flat_map(|facts| facts.nodes.iter().cloned())
        .map(|node| (node.id.clone(), node))
        .collect();
    let live_versus_durable_nodes: Vec<(
        contextunity_forge_mcp::core::models::Node,
        Option<contextunity_forge_mcp::core::models::Node>,
    )> = extracted_facts
        .iter()
        .flat_map(|facts| facts.nodes.iter().cloned())
        .map(|live| {
            let durable = durable.get(&live.id).cloned();
            (live, durable)
        })
        .collect();
    let extracted_docs: Vec<_> = extracted_facts
        .iter()
        .flat_map(|facts| facts.docs.clone())
        .collect();

    let facts = &extracted_facts;
    assert_eq!(sources.len(), item_count * 2);
    assert_eq!(
        facts.len(),
        sources.len(),
        "every source has a live writer::extract result"
    );
    for language in ["rust", "markdown"] {
        assert_eq!(
            sources
                .iter()
                .filter(|source| source.language == language)
                .count(),
            item_count
        );
    }
    assert_eq!(
        facts.iter().map(|fact| fact.docs.len()).sum::<usize>(),
        item_count
    );
    assert_eq!(inventory, expected_inventory);

    assert_eq!(local_facts.len(), sources.len());
    for (source, stored) in sources.iter().zip(local_facts) {
        assert_eq!(stored.0, source.path);
        assert_eq!(stored.1, source.digest);
        assert_eq!(stored.2, source.language);
        assert_eq!(
            stored.3,
            contextunity_forge_mcp::core::models::is_test(&source.path)
        );
        assert!(!stored.4);
        assert_eq!(stored.6, "blob");
        assert!(stored.5.starts_with(b"FORGE-LOCAL-FACTS\0\x03"));
        assert!(stored.5["FORGE-LOCAL-FACTS\0\x03".len()..].starts_with(&[0x28, 0xb5, 0x2f, 0xfd]));
    }

    let expected_nodes: Vec<_> = decoded_local_facts
        .iter()
        .flat_map(|facts| &facts.nodes)
        .collect();
    assert_eq!(
        nodes.len(),
        expected_nodes.len(),
        "every persisted node must have a decoded local-facts witness"
    );
    let persisted_nodes: BTreeMap<_, _> = nodes.iter().map(|row| (row.1.as_str(), row)).collect();
    assert_eq!(
        persisted_nodes.len(),
        nodes.len(),
        "persisted node IDs must be unique"
    );
    for (index, node) in expected_nodes.iter().enumerate() {
        let persisted = *persisted_nodes
            .get(node.id.as_str())
            .expect("each decoded fact node has a persisted row");
        assert_eq!(
            persisted.0,
            index as i64 + 1,
            "persisted insertion order for {}",
            node.id
        );
        assert_eq!(persisted.1, node.id, "persisted row identity");
        let navigation: serde_json::Value = serde_json::from_str(&persisted.2).unwrap();
        assert!(
            !navigation
                .as_object()
                .is_some_and(serde_json::Map::is_empty),
            "empty node details must be stored as null for {}",
            node.id
        );
        for key in ["signature", "doc", "receiver_name"] {
            if node.details[key]
                .as_str()
                .is_some_and(|value| !value.is_empty())
            {
                assert_eq!(navigation[key], node.details[key]);
            }
        }
    }
    for (live, durable) in &live_versus_durable_nodes {
        assert_eq!(
            serde_json::to_value(
                durable
                    .as_ref()
                    .expect("live extracted node has a durable twin")
            )
            .unwrap(),
            serde_json::to_value(live).unwrap()
        );
    }

    assert_eq!(docs.len(), extracted_docs.len());
    for (index, (actual, expected)) in docs.iter().zip(&extracted_docs).enumerate() {
        assert_eq!(actual.0, index as i64 + 1);
        assert_eq!(actual.2, "real");
        assert_eq!(
            serde_json::to_value(&actual.1).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }

    assert!(shared_keys.len() >= item_count);
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
    commitments_verification
        .as_ref()
        .expect("durable cold-build commitments verify");
    Ok(())
}

#[test]
fn cli_memory_budget_precedence_fallback_and_rejection_are_observable() -> anyhow::Result<()> {
    let configure_memory_budget = |workspace: &Workspace, memory_budget_mb: Option<&str>| {
        let limits = memory_budget_mb.map_or_else(String::new, |value| {
            format!("limits:\n  memory_budget_mb: {value}\n")
        });
        workspace.write(
            "forge-mcp.yaml",
            &format!("roots: [src]\ndocs: []\n{limits}"),
        );
    };
    let read_persisted_commitments =
        |database: &std::path::Path| -> anyhow::Result<Vec<(String, i64, String)>> {
            let connection = Connection::open(database)?;
            let mut statement = connection.prepare(
        "SELECT domain,owner_id,lower(hex(digest)) FROM domain_commitments ORDER BY domain,owner_id",
    )?;
            let rows =
                statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        };
    let run_memory_budget_build =
        |workspace: &Workspace, label: &str, env_budget_mb: Option<&str>| -> anyhow::Result<_> {
            use std::process::Command;

            let database = workspace
                .root()
                .join(".forge")
                .join(format!("{label}.sqlite"));
            let mut command = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"));
            command
                .arg("--format")
                .arg("json")
                .arg("build")
                .arg(workspace.root())
                .arg("--output")
                .arg(&database)
                .env(
                    "FORGE_EXTRACTION_CACHE_DIR",
                    workspace.path(".forge/extraction-cache"),
                )
                .env_remove("FORGE_MEMORY_BUDGET_MB");
            if let Some(value) = env_budget_mb {
                command.env("FORGE_MEMORY_BUDGET_MB", value);
            }
            let output = command.output()?;
            Ok((label.to_owned(), output, database))
        };
    let workspace = Workspace::new();
    workspace.write("src/lib.rs", "pub fn stable() -> u8 { 1 }\n");

    configure_memory_budget(&workspace, Some("50"));
    let yaml = run_memory_budget_build(&workspace, "yaml", Some("1000"))?;

    configure_memory_budget(&workspace, None);
    let environment = run_memory_budget_build(&workspace, "environment", Some("200"))?;
    let environment_high = run_memory_budget_build(&workspace, "environment-high", Some("1000"))?;
    let adaptive = run_memory_budget_build(&workspace, "adaptive", None)?;

    let invalid = [
        ("invalid-environment-text", None, Some("many")),
        ("invalid-environment-zero", None, Some("0")),
        (
            "invalid-environment-overflow",
            None,
            Some("18446744073709551615"),
        ),
        ("invalid-yaml-text", Some("many"), Some("200")),
        ("invalid-yaml-zero", Some("0"), Some("200")),
        (
            "invalid-yaml-overflow",
            Some("18446744073709551615"),
            Some("200"),
        ),
    ]
    .into_iter()
    .map(|(label, yaml_budget, env_budget)| {
        configure_memory_budget(&workspace, yaml_budget);
        run_memory_budget_build(&workspace, label, env_budget)
    })
    .collect::<anyhow::Result<Vec<_>>>()?;

    for build in [&yaml, &environment, &environment_high, &adaptive] {
        assert!(
            build.1.status.success(),
            "{} CLI build failed: {}",
            build.0,
            String::from_utf8_lossy(&build.1.stderr)
        );
        assert!(build.2.exists(), "{} did not publish a database", build.0);
    }
    let yaml_report: serde_json::Value = serde_json::from_slice(&yaml.1.stdout)?;
    assert_eq!(
        yaml_report["memory_budget_bytes"].as_u64(),
        Some(50 * 1024 * 1024)
    );

    let env_report: serde_json::Value = serde_json::from_slice(&environment.1.stdout)?;
    assert_eq!(
        env_report["memory_budget_bytes"].as_u64(),
        Some(200 * 1024 * 1024)
    );
    let env_high_report: serde_json::Value = serde_json::from_slice(&environment_high.1.stdout)?;
    assert_eq!(
        env_high_report["memory_budget_bytes"].as_u64(),
        Some(1000 * 1024 * 1024)
    );
    assert_eq!(
        env_report["output_root"], env_high_report["output_root"],
        "the transient environment budget must not alter the adapter policy fingerprint"
    );

    let adaptive_report: serde_json::Value = serde_json::from_slice(&adaptive.1.stdout)?;
    let adaptive = adaptive_report["memory_budget_bytes"].as_u64().unwrap();
    assert!((64 * 1024 * 1024..=16 * 1024 * 1024 * 1024).contains(&adaptive));

    let yaml_commitments = read_persisted_commitments(&yaml.2)?;
    for build in [&environment, &environment_high] {
        assert_eq!(
            yaml_commitments,
            read_persisted_commitments(&build.2)?,
            "{}",
            build.0
        );
    }

    for (label, output, database) in &invalid {
        assert!(!output.status.success(), "{} unexpectedly built", label);
        assert!(!database.exists(), "{} published a database", label);
    }
    Ok(())
}

#[cfg(feature = "lang-rust")]
#[test]
fn public_extraction_rejects_a_file_entry_digest_mismatch() -> anyhow::Result<()> {
    let workspace = Workspace::new();
    workspace.write("src/lib.rs", "pub fn current() -> u8 { 1 }\n");
    let adapter = scanner::load_adapter(workspace.root(), None)?;
    let mut entry = scanner::scan_with_adapter(workspace.root(), &adapter)?
        .entries
        .into_iter()
        .find(|entry| entry.path == "src/lib.rs")
        .ok_or_else(|| anyhow::anyhow!("scanner did not return src/lib.rs"))?;
    entry.digest = "deliberately-stale-digest".to_owned();
    let error = writer::extract(workspace.root(), &entry, &adapter)
        .expect_err("stale public extraction must fail closed");
    assert!(
        error
            .to_string()
            .contains("source changed during extraction"),
        "stale public extraction must fail closed: {error:#}"
    );
    Ok(())
}
