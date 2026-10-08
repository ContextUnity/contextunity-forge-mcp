use crate::common::Workspace;
use contextunity_forge_mcp::{core::models::Facts, db::writer, engine::scanner};
use rusqlite::Connection;
use std::collections::BTreeMap;

#[cfg(feature = "lang-rust")]
#[test]
fn cold_build_preserves_extracted_storage_across_full_and_remainder_batches() {
    use contextunity_forge_mcp::core::{commitments, models::stable_hash64};

    let ws = Workspace::new();
    for index in 0..251 {
        ws.write(
            format!("src/unit_{index:03}.rs"),
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

    let adapter = scanner::load_adapter(ws.root(), None).unwrap();
    let scan = scanner::scan_with_adapter(ws.root(), &adapter).unwrap();
    let facts: Vec<_> = scan
        .entries
        .iter()
        .map(|entry| writer::extract(ws.root(), entry, &adapter).unwrap())
        .collect();
    assert_eq!(scan.entries.len(), 252);
    assert_eq!(facts.iter().map(|fact| fact.docs.len()).sum::<usize>(), 251);

    let db_path = ws.root().join(".forge/storage.sqlite");
    writer::build(ws.root(), &db_path, None).unwrap();
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
        assert!(blob.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]));
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
        let navigation: serde_json::Value = serde_json::from_str(&details).unwrap();
        for key in ["signature", "doc", "receiver_name"] {
            if node.details[key]
                .as_str()
                .is_some_and(|value| !value.is_empty())
            {
                assert_eq!(navigation[key], node.details[key]);
            }
        }
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
        .prepare("SELECT k.key_hash,CASE WHEN typeof(k.key)='integer' THEN n.qualname ELSE k.key END FROM shared_keys k LEFT JOIN nodes n ON n.node_id=k.key AND typeof(k.key)='integer' ORDER BY k.key_hash")
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
