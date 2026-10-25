use super::*;
use crate::core::typed_facts::encode_facts;
use std::borrow::Cow;

const COLD_LOCAL_FACTS_BATCH_ROWS: usize = 500;

fn compare_coverage_rows(
    left: &crate::core::models::compact_graph::Coverage,
    right: &crate::core::models::compact_graph::Coverage,
) -> std::cmp::Ordering {
    left.path
        .as_bytes()
        .cmp(right.path.as_bytes())
        .then_with(|| left.line.cmp(&right.line))
        .then_with(|| left.expression.as_bytes().cmp(right.expression.as_bytes()))
        .then_with(|| left.status.cmp(&right.status))
        .then_with(|| left.evidence.as_bytes().cmp(right.evidence.as_bytes()))
}

fn flush_owner_language_counts<'a>(
    path_id: i64,
    pending: &mut Vec<(&'a str, &'a str, i64)>,
    language_counts: &mut BTreeMap<(i64, &'a str, &'a str), i64>,
) {
    for (language, status, count) in pending.drain(..) {
        *language_counts
            .entry((path_id, language, status))
            .or_default() += count;
    }
}

#[derive(Debug)]
pub(super) struct FilePersistenceTimings {
    pub(super) node_id: i64,
    pub(super) node_row_count: usize,
    pub(super) docs_count: usize,
    pub(super) fts_insert_ms: f64,
    pub(super) node_row_insert_ms: Option<f64>,
    pub(super) encode_facts_ms: f64,
    pub(super) local_facts_batch_rows: usize,
    pub(super) local_facts_leaves: std::collections::BTreeMap<String, String>,
    pub(super) node_leaves: std::collections::BTreeMap<String, String>,
}

pub(super) fn persist_files(
    tx: &Connection,
    path_cache: &mut PathDictionaryCache,
    entries: &[FileEntry],
    facts: &BTreeMap<String, TypedFacts>,
    bulk_search: bool,
) -> Result<FilePersistenceTimings> {
    persist_files_impl(tx, path_cache, entries, facts, bulk_search, None)
}

pub(super) struct LoadedCatalogFile {
    pub(super) facts: TypedFacts,
    pub(super) encoded: Vec<u8>,
    pub(super) local_facts_leaf: (String, String),
    pub(super) search_texts: Vec<String>,
    pub(super) reference_keys: Vec<String>,
}

type ChunkLoader<'a> = &'a mut dyn FnMut(
    &[FileEntry],
    &BTreeMap<String, TypedFacts>,
) -> Result<Vec<LoadedCatalogFile>>;

pub(super) fn persist_catalog_files(
    tx: &Connection,
    path_cache: &mut PathDictionaryCache,
    entries: &[FileEntry],
    facts: &BTreeMap<String, TypedFacts>,
    loader: ChunkLoader<'_>,
) -> Result<FilePersistenceTimings> {
    persist_files_impl(tx, path_cache, entries, facts, true, Some(loader))
}

fn persist_files_impl(
    tx: &Connection,
    path_cache: &mut PathDictionaryCache,
    entries: &[FileEntry],
    facts: &BTreeMap<String, TypedFacts>,
    bulk_search: bool,
    mut loader: Option<ChunkLoader<'_>>,
) -> Result<FilePersistenceTimings> {
    let mut node_id: i64 = tx.query_row("SELECT coalesce(max(node_id),0)FROM nodes", [], |r| {
        r.get(0)
    })?;
    let initial_node_id = node_id;
    let mut docs_count = 0;
    let mut encode_facts_ms = 0.0;
    let mut local_facts_leaves = std::collections::BTreeMap::new();

    let mut stmt_source_inv = (!bulk_search)
        .then(|| tx.prepare("INSERT INTO source_inventory VALUES(?1,'indexed',?2,?3)"))
        .transpose()?;
    let mut stmt_files = (!bulk_search)
        .then(|| tx.prepare("INSERT INTO files VALUES(?1,'indexed',?2,?3,?4,?5,0,?6)"))
        .transpose()?;
    let mut stmt_local_facts = (!bulk_search)
        .then(|| tx.prepare("INSERT INTO local_facts VALUES(?1,?2,?3,?4,0,?5)"))
        .transpose()?;
    let mut stmt_nodes = (!bulk_search)
        .then(|| {
            tx.prepare("INSERT INTO nodes VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)")
        })
        .transpose()?;
    let mut stmt_node_search = (!bulk_search)
        .then(|| tx.prepare("INSERT INTO node_search(rowid,search_text)VALUES(?1,?2)"))
        .transpose()?;
    let mut shared_owner_batch = MultiValueBatch::<4>::new(
        tx,
        "INSERT OR IGNORE INTO shared_owners(kind_id,key_hash,owner_id,ordinal) VALUES",
    )?;
    let mut shared_key_cache = if bulk_search {
        SharedKeyCache::new_batched(tx)?
    } else {
        SharedKeyCache::new_batched_delta(tx)?
    };
    let mut stmt_doc_sec = (!bulk_search)
        .then(|| tx.prepare("INSERT INTO doc_sections VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"))
        .transpose()?;
    let mut stmt_doc_search = (!bulk_search)
        .then(|| {
            tx.prepare(
                "INSERT INTO doc_search(rowid,section_title,content,invariants)SELECT rowid,section_title,content,invariants FROM doc_sections WHERE path=?1",
            )
        })
        .transpose()?;
    let mut stmt_errors = tx.prepare("INSERT INTO errors VALUES(?1,?2,?3)")?;

    let mut source_inventory_batch = bulk_search
        .then(|| {
            MultiValueBatch::<4, BorrowedSqlValue<'_>>::new(
                tx,
                "INSERT INTO source_inventory VALUES",
            )
        })
        .transpose()?;
    let local_facts_batch_rows = bulk_search
        .then(|| multi_value_batch_rows::<6>(tx).map(|rows| rows.min(COLD_LOCAL_FACTS_BATCH_ROWS)))
        .transpose()?;
    let mut file_batch = Vec::<[SqlValue; 8]>::new();
    let mut reusable_detail_buffers = Vec::<Vec<u8>>::new();
    let mut owned_search_batch = Vec::<SearchRow>::new();
    let mut fts_insert_ms = 0.0;
    let mut node_row_insert_ms = bulk_search.then_some(0.0);
    let mut reusable_search_buffers = Vec::<String>::new();
    let mut incremental_search_buffer = String::new();
    let mut local_facts_batch = Vec::<(&FileEntry, Vec<u8>)>::new();
    let mut doc_sections_batch = bulk_search
        .then(|| {
            MultiValueBatch::<10, BorrowedSqlValue<'static>>::new(
                tx,
                "INSERT INTO doc_sections VALUES",
            )
        })
        .transpose()?;
    let mut shared_owner_rows = Vec::<[i64; 4]>::new();
    let mut node_domain = seal::NodeDomain::new();
    let (file_batch_rows, node_batch_rows, owned_search_batch_rows) = if bulk_search {
        (
            Some(multi_value_batch_rows::<8>(tx)?),
            Some(multi_value_batch_rows::<14>(tx)?),
            Some(multi_value_batch_rows::<2>(tx)?),
        )
    } else {
        (None, None, None)
    };

    let chunk_rows = local_facts_batch_rows.unwrap_or(1);
    for file_chunk in entries.chunks(chunk_rows) {
        let mut loaded = loader
            .as_mut()
            .map(|loader| loader(file_chunk, facts))
            .transpose()?;
        let mut node_batch = Vec::with_capacity(node_batch_rows.unwrap_or_default());
        let mut encoded_chunk: Vec<(Vec<u8>, (String, String))> =
            if let Some(loaded) = loaded.as_mut() {
                loaded
                    .iter_mut()
                    .map(|file| {
                        (
                            std::mem::take(&mut file.encoded),
                            std::mem::take(&mut file.local_facts_leaf),
                        )
                    })
                    .collect()
            } else if local_facts_batch_rows.is_some() {
                let started = Instant::now();
                let encode = |file: &FileEntry| {
                    let facts = facts.get(&file.path).context("missing extracted facts")?;
                    let blob = encode_facts(facts)?;
                    let leaf = seal::local_facts_leaf(
                        &file.path,
                        &file.digest,
                        &file.language,
                        is_test(&file.path),
                        &blob,
                    )?;
                    Ok((blob, leaf))
                };
                let encoded = if file_chunk.len() == 1 {
                    file_chunk.iter().map(encode).collect::<Result<Vec<_>>>()?
                } else {
                    file_chunk
                        .par_iter()
                        .map(encode)
                        .collect::<Result<Vec<_>>>()?
                };
                encode_facts_ms += started.elapsed().as_secs_f64() * 1000.0;
                encoded
            } else {
                Vec::new()
            };
        let mut loaded_search_texts = loaded.as_mut().map(|loaded| {
            loaded
                .iter_mut()
                .map(|file| std::mem::take(&mut file.search_texts))
                .collect::<Vec<_>>()
        });
        let mut prepared_node_details = file_chunk
            .par_iter()
            .enumerate()
            .map(|(chunk_index, file)| -> Result<Vec<(usize, Vec<u8>)>> {
                let f = loaded
                    .as_ref()
                    .map(|loaded| &loaded[chunk_index].facts)
                    .or_else(|| facts.get(&file.path))
                    .context("missing catalog facts for node details")?;
                let detail_facts = facts
                    .get(&file.path)
                    .context("missing navigation facts for node details")?;
                let mut node_order = (0..f.nodes.len()).collect::<Vec<_>>();
                node_order.par_sort_unstable_by(|left, right| {
                    f.nodes[*left]
                        .id
                        .cmp(&f.nodes[*right].id)
                        .then_with(|| left.cmp(right))
                });
                node_order
                    .par_iter()
                    .map(|&node_index| -> Result<(usize, Vec<u8>)> {
                        let n = &f.nodes[node_index];
                        let navigation_node = detail_facts
                            .nodes
                            .get(node_index)
                            .filter(|node| node.id == n.id)
                            .or_else(|| detail_facts.nodes.iter().find(|node| node.id == n.id))
                            .context("staged node is missing matching navigation facts")?;
                        let details = serde_json::to_vec(
                            &navigation_node.navigation_details_with_documentation(&n.details),
                        )?;
                        Ok((node_index, details))
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?;
        for (chunk_index, file) in file_chunk.iter().enumerate() {
            let (f, mut search_texts, prepared_references) = if let Some(loaded) = loaded.as_ref() {
                let loaded = &loaded[chunk_index];
                (
                    &loaded.facts,
                    loaded_search_texts
                        .as_mut()
                        .map(|search_texts| &mut search_texts[chunk_index]),
                    Some(&loaded.reference_keys),
                )
            } else {
                let extracted = facts.get(&file.path).context("missing extracted facts")?;
                (extracted, None, None)
            };
            let owner_id = path_cache.get_or_insert(tx, &file.path)?;
            if let Some(batch) = source_inventory_batch.as_mut() {
                batch.push([
                    BorrowedSqlValue::Text(&file.path),
                    BorrowedSqlValue::Text("indexed"),
                    BorrowedSqlValue::Text(&file.digest),
                    BorrowedSqlValue::Integer(i64::try_from(file.bytes)?),
                ])?;
            } else {
                stmt_source_inv
                    .as_mut()
                    .context("missing incremental source inventory insert")?
                    .execute(params![file.path, file.digest, file.bytes])?;
            }
            if bulk_search {
                file_batch.push([
                    SqlValue::Text(file.path.clone()),
                    SqlValue::Text("indexed".into()),
                    SqlValue::Text(file.digest.clone()),
                    SqlValue::Integer(i64::try_from(file.bytes)?),
                    SqlValue::Text(file.language.clone()),
                    SqlValue::Integer(i64::from(is_test(&file.path))),
                    SqlValue::Integer(0),
                    SqlValue::Integer(stable_hash64(&file.path)),
                ]);
                if file_batch.len() == file_batch_rows.context("missing cold file batch size")? {
                    insert_multi_value_batch(tx, "INSERT INTO files VALUES", &file_batch)?;
                    file_batch.clear();
                }
            } else {
                stmt_files
                    .as_mut()
                    .context("missing incremental file insert")?
                    .execute(params![
                        file.path,
                        file.digest,
                        file.bytes,
                        file.language,
                        is_test(&file.path),
                        stable_hash64(&file.path)
                    ])?;
            }
            if let Some(batch_rows) = local_facts_batch_rows {
                let (blob, leaf) = &mut encoded_chunk[chunk_index];
                local_facts_leaves.insert(std::mem::take(&mut leaf.0), std::mem::take(&mut leaf.1));
                local_facts_batch.push((file, std::mem::take(blob)));
                if local_facts_batch.len() == batch_rows {
                    flush_local_facts_batch(tx, &local_facts_batch)?;
                    local_facts_batch.clear();
                }
            } else {
                let started = Instant::now();
                let blob = encode_facts(f)?;
                encode_facts_ms += started.elapsed().as_secs_f64() * 1000.0;
                let (leaf_key, leaf_hash) = seal::local_facts_leaf(
                    &file.path,
                    &file.digest,
                    &file.language,
                    is_test(&file.path),
                    &blob,
                )?;
                local_facts_leaves.insert(leaf_key, leaf_hash);
                stmt_local_facts
                    .as_mut()
                    .context("missing incremental local facts insert")?
                    .execute(params![
                        file.path,
                        file.digest,
                        file.language,
                        is_test(&file.path),
                        blob.as_slice()
                    ])?;
            }
            let node_id_start = node_id;
            node_id = node_id
                .checked_add(i64::try_from(f.nodes.len())?)
                .context("node ID overflow")?;
            if search_texts
                .as_ref()
                .is_some_and(|search| search.len() != f.nodes.len())
            {
                search_texts = None;
            }
            for (node_index, details_bytes) in
                std::mem::take(&mut prepared_node_details[chunk_index])
            {
                let n = &f.nodes[node_index];
                let row_node_id = node_id_start
                    .checked_add(i64::try_from(node_index + 1)?)
                    .context("node ID overflow")?;
                let search_id = stable_hash64(&n.id);
                let node_path_id = if n.path == file.path {
                    owner_id
                } else {
                    path_cache.get_or_insert(tx, &n.path)?
                };
                if bulk_search {
                    node_domain.push(&file.path, n, &details_bytes)?;
                    node_batch.push([
                        BorrowedSqlValue::Integer(row_node_id),
                        BorrowedSqlValue::Text(n.id.as_str()),
                        BorrowedSqlValue::Text(n.kind.as_str()),
                        BorrowedSqlValue::Text(n.name.as_str()),
                        BorrowedSqlValue::Text(n.qualname.as_str()),
                        BorrowedSqlValue::Integer(node_path_id),
                        BorrowedSqlValue::Integer(i64::try_from(n.line)?),
                        BorrowedSqlValue::Integer(i64::try_from(n.end_line)?),
                        BorrowedSqlValue::Integer(i64::from(n.is_test)),
                        BorrowedSqlValue::Text(n.language.as_str()),
                        BorrowedSqlValue::Integer(i64::from(n.generated)),
                        BorrowedSqlValue::OwnedJsonText(details_bytes),
                        BorrowedSqlValue::Integer(search_id),
                        BorrowedSqlValue::Integer(owner_id),
                    ]);
                    if node_batch.len()
                        == node_batch_rows.context("missing cold node batch size")?
                    {
                        let insert_ms =
                            flush_node_batch(tx, &mut node_batch, &mut reusable_detail_buffers)?;
                        if let Some(total_ms) = node_row_insert_ms.as_mut() {
                            *total_ms += insert_ms;
                        }
                    }
                } else {
                    node_domain.push(&file.path, n, &details_bytes)?;
                    let details = std::str::from_utf8(&details_bytes)
                        .context("serialized node details are not UTF-8")?;
                    stmt_nodes
                        .as_mut()
                        .context("missing incremental node insert")?
                        .execute(params![
                            row_node_id,
                            n.id,
                            n.kind,
                            n.name,
                            n.qualname,
                            node_path_id,
                            n.line,
                            n.end_line,
                            n.is_test,
                            n.language,
                            n.generated,
                            details,
                            search_id,
                            owner_id
                        ])?;
                }
                if bulk_search {
                    let search = if let Some(prepared) = search_texts.as_mut() {
                        reusable_search_buffers.pop();
                        std::mem::take(&mut prepared[node_index])
                    } else {
                        let mut search = reusable_search_buffers.pop().unwrap_or_default();
                        write_node_search_text(&mut search, n, f);
                        search
                    };
                    owned_search_batch.push(SearchRow {
                        search_id,
                        text: search,
                    });
                    if owned_search_batch.len()
                        == owned_search_batch_rows.context("missing cold search batch size")?
                    {
                        fts_insert_ms += flush_owned_search_batch(
                            tx,
                            &mut owned_search_batch,
                            &mut reusable_search_buffers,
                        )?;
                    }
                } else {
                    write_node_search_text(&mut incremental_search_buffer, n, f);
                    if let Some(stmt_node_search) = stmt_node_search.as_mut() {
                        stmt_node_search.execute(params![search_id, &incremental_search_buffer])?;
                    }
                    incremental_search_buffer.clear();
                }
                let key_hash = shared_key_cache.get_or_insert_node(&n.qualname, row_node_id)?;
                shared_owner_rows.push([0, key_hash, owner_id, row_node_id]);
            }
            let keys = prepared_references.map_or_else(
                || f.reference_keys(),
                |keys| keys.iter().map(String::as_str).collect(),
            );
            for key in keys {
                let key_hash = shared_key_cache.get_or_insert(key)?;
                shared_owner_rows.push([1, key_hash, owner_id, 0]);
            }
            for n in &f.nodes {
                if n.details["default_export"] == true {
                    let key_hash = shared_key_cache.get_or_insert(&n.id)?;
                    shared_owner_rows.push([2, key_hash, owner_id, 0]);
                }
            }
            if shared_owner_rows.len() >= 8192 {
                shared_owner_rows.par_sort_unstable();
            } else {
                shared_owner_rows.sort_unstable();
            }
            for row in shared_owner_rows.drain(..) {
                shared_owner_batch.push(row.map(SqlValue::Integer))?;
            }
            for d in &f.docs {
                docs_count += 1;
                if let Some(batch) = doc_sections_batch.as_mut() {
                    batch.push([
                        BorrowedSqlValue::OwnedText(d.doc_id.clone()),
                        BorrowedSqlValue::OwnedText(d.path.clone()),
                        BorrowedSqlValue::OwnedText(d.section_title.clone()),
                        BorrowedSqlValue::OwnedText(d.doc_type.clone()),
                        BorrowedSqlValue::OwnedText(d.content.clone()),
                        BorrowedSqlValue::OwnedText(serde_json::to_string(&d.invariants)?),
                        BorrowedSqlValue::OwnedText(serde_json::to_string(&d.referenced_symbols)?),
                        BorrowedSqlValue::Real(d.mtime),
                        BorrowedSqlValue::Integer(i64::try_from(d.size)?),
                        BorrowedSqlValue::Integer(i64::from(d.is_invariant)),
                    ])?;
                } else {
                    stmt_doc_sec
                        .as_mut()
                        .context("missing incremental document section insert")?
                        .execute(params![
                            d.doc_id,
                            d.path,
                            d.section_title,
                            d.doc_type,
                            d.content,
                            serde_json::to_string(&d.invariants)?,
                            serde_json::to_string(&d.referenced_symbols)?,
                            d.mtime,
                            d.size,
                            d.is_invariant
                        ])?;
                }
            }
            if !f.docs.is_empty() {
                if let Some(stmt) = stmt_doc_search.as_mut() {
                    stmt.execute([&file.path])?;
                }
            }
            for e in &f.errors {
                stmt_errors.execute(params![e.path, e.line, e.message])?;
            }
        }
        let insert_ms = flush_node_batch(tx, &mut node_batch, &mut reusable_detail_buffers)?;
        if let Some(total_ms) = node_row_insert_ms.as_mut() {
            *total_ms += insert_ms;
        }
        drop(node_batch);
        drop(loaded_search_texts);
        drop(loaded);
    }

    flush_local_facts_batch(tx, &local_facts_batch)?;
    if let Some(batch) = doc_sections_batch.as_mut() {
        batch.flush()?;
    }
    let node_leaves = node_domain.finish()?;
    insert_multi_value_batch(tx, "INSERT INTO files VALUES", &file_batch)?;
    fts_insert_ms +=
        flush_owned_search_batch(tx, &mut owned_search_batch, &mut reusable_search_buffers)?;
    if let Some(batch) = source_inventory_batch.as_mut() {
        batch.flush()?;
    }

    shared_key_cache.flush()?;
    shared_owner_batch.flush()?;
    Ok(FilePersistenceTimings {
        node_id,
        node_row_count: usize::try_from(node_id - initial_node_id)?,
        docs_count,
        fts_insert_ms,
        node_row_insert_ms,
        encode_facts_ms,
        local_facts_batch_rows: local_facts_batch_rows.unwrap_or(1),
        local_facts_leaves,
        node_leaves,
    })
}

fn flush_local_facts_batch(tx: &Connection, batch: &[(&FileEntry, Vec<u8>)]) -> Result<()> {
    let rows: Vec<_> = batch
        .iter()
        .map(|(file, blob)| {
            [
                BorrowedSqlValue::Text(&file.path),
                BorrowedSqlValue::Text(&file.digest),
                BorrowedSqlValue::Text(&file.language),
                BorrowedSqlValue::Integer(i64::from(is_test(&file.path))),
                BorrowedSqlValue::Integer(0),
                BorrowedSqlValue::Blob(blob),
            ]
        })
        .collect();
    insert_multi_value_batch(tx, "INSERT INTO local_facts VALUES", &rows)
}

fn flush_node_batch<'a>(
    tx: &Connection,
    batch: &mut Vec<[BorrowedSqlValue<'a>; 14]>,
    reusable_buffers: &mut Vec<Vec<u8>>,
) -> Result<f64> {
    if batch.is_empty() {
        return Ok(0.0);
    }
    let started = Instant::now();
    insert_multi_value_batch(tx, "INSERT INTO nodes VALUES", batch)?;
    let insert_ms = started.elapsed().as_secs_f64() * 1000.;
    for row in batch.drain(..) {
        for value in row {
            if let BorrowedSqlValue::OwnedJsonText(buffer) = value {
                reusable_buffers.push(buffer);
            }
        }
    }
    Ok(insert_ms)
}

pub(super) fn write_node_search_text(search: &mut String, node: &Node, facts: &Facts) {
    search.clear();
    let doc = node
        .details
        .get("doc")
        .and_then(Value::as_str)
        .unwrap_or("");
    if doc.is_empty() {
        let _ = write!(search, "{} {} {}", node.name, node.qualname, node.path);
    } else {
        let _ = write!(
            search,
            "{} {} {} {}",
            node.name, node.qualname, node.path, doc
        );
    }
    if node.kind == "module" {
        let mut expressions: Vec<_> = facts
            .references
            .iter()
            .map(|reference| reference.expression.as_str())
            .collect();
        if expressions.len() >= 8192 {
            expressions.par_sort_unstable();
        } else {
            expressions.sort_unstable();
        }
        expressions.dedup();
        for expression in expressions {
            search.push(' ');
            search.push_str(expression);
        }
    }
}

struct SearchRow {
    search_id: i64,
    text: String,
}

fn flush_owned_search_batch(
    tx: &Connection,
    batch: &mut Vec<SearchRow>,
    reusable_buffers: &mut Vec<String>,
) -> Result<f64> {
    batch.sort_unstable_by_key(|row| row.search_id);
    let rows: Vec<_> = batch
        .iter()
        .map(|row| {
            [
                BorrowedSqlValue::Integer(row.search_id),
                BorrowedSqlValue::Text(&row.text),
            ]
        })
        .collect();
    let started = Instant::now();
    insert_multi_value_batch(
        tx,
        "INSERT INTO node_search(rowid,search_text) VALUES",
        &rows,
    )?;
    let fts_ms = started.elapsed().as_secs_f64() * 1000.;
    for row in batch.drain(..) {
        reusable_buffers.push(row.text);
    }
    Ok(fts_ms)
}

pub(super) struct CachedNodePath {
    path: Arc<str>,
    node_hash: i64,
    path_hash: i64,
}

impl CachedNodePath {
    pub(super) fn new(id: &str, path: String) -> Self {
        Self::from_shared(id, Arc::<str>::from(path))
    }

    pub(super) fn from_shared(id: &str, path: Arc<str>) -> Self {
        Self {
            node_hash: stable_hash64(id),
            path_hash: stable_hash64(&path),
            path,
        }
    }
}

pub(super) fn graph_endpoint_hashes(
    node_paths: &mut hashbrown::HashMap<Cow<'_, str>, Option<CachedNodePath>>,
    stmt_select_path: &mut rusqlite::Statement<'_>,
    endpoint: &str,
    owner: &str,
    kind: &str,
) -> Result<(i64, Option<i64>)> {
    if let Some(node) = node_paths.get(endpoint) {
        let node = node
            .as_ref()
            .with_context(|| format!("edge {kind} has missing endpoint {endpoint}"))?;
        return Ok((
            node.node_hash,
            (node.path.as_ref() != owner).then_some(node.path_hash),
        ));
    }
    let path: Option<String> = stmt_select_path
        .query_row([endpoint], |row| row.get(0))
        .optional()?;
    let node = node_paths
        .entry(Cow::Owned(endpoint.to_owned()))
        .or_insert_with(|| path.map(|path| CachedNodePath::new(endpoint, path)));
    let node = node
        .as_ref()
        .with_context(|| format!("edge {kind} has missing endpoint {endpoint}"))?;
    Ok((
        node.node_hash,
        (node.path.as_ref() != owner).then_some(node.path_hash),
    ))
}

struct StoredEdge<'a> {
    src: i64,
    dst: i64,
    kind: &'a str,
    path_id: i64,
    line: usize,
    evidence: i64,
    confidence: i64,
    count: i64,
    first: usize,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct StoredDependency<'a> {
    owner: i64,
    target: Option<i64>,
    kind: &'a str,
    symbol: &'a str,
    resolution: &'a str,
}

#[derive(Default)]
struct DependencyInput<'a> {
    edges: Vec<(usize, &'a [crate::core::models::compact_graph::Edge])>,
    coverage: Vec<&'a [crate::core::models::compact_graph::Coverage]>,
}

fn persist_dependencies(
    tx: &Connection,
    path_cache: &mut PathDictionaryCache,
    graph: &CompactGraph,
    facts: &BTreeMap<String, TypedFacts>,
    dependency_owners: Option<&std::collections::BTreeSet<String>>,
    resolved_edge_paths: &[(Option<i64>, Option<i64>)],
) -> Result<()> {
    let mut inputs = BTreeMap::<&str, DependencyInput<'_>>::new();
    for path in facts.keys() {
        inputs.entry(path).or_default();
    }
    let mut edge_offset = 0usize;
    for edges in graph.edges.chunk_by(|left, right| left.path == right.path) {
        inputs
            .entry(&edges[0].path)
            .or_default()
            .edges
            .push((edge_offset, edges));
        edge_offset = edge_offset
            .checked_add(edges.len())
            .context("edge occurrence offset overflow")?;
    }
    for rows in graph
        .coverage
        .chunk_by(|left, right| left.path == right.path)
    {
        inputs.entry(&rows[0].path).or_default().coverage.push(rows);
    }
    let mut batch =
        MultiValueBatch::<6, BorrowedSqlValue<'_>>::new(tx, "INSERT INTO dependencies VALUES")?;
    for (path, input) in inputs {
        let owner = path_cache.get_or_insert(tx, path)?;
        let mut dependencies = Vec::new();
        #[cfg(feature = "lang-html")]
        if dependency_owners.is_none_or(|owners| owners.contains(path)) {
            if let Some(file_facts) = facts.get(path) {
                for node in file_facts.nodes.iter().filter(|node| {
                    node.language == "python"
                        && matches!(node.kind.as_str(), "module" | "function" | "method")
                }) {
                    let Some(flow) = file_facts.flows.get(node) else {
                        continue;
                    };
                    for context in flow.template_contexts() {
                        let Some(target) = context.target_path.as_ref() else {
                            continue;
                        };
                        if crate::core::config::workspace_path(path).0
                            != crate::core::config::workspace_path(target).0
                        {
                            continue;
                        }
                        dependencies.push(StoredDependency {
                            owner,
                            target: Some(stable_hash64(target)),
                            kind: "renders",
                            symbol: &context.target,
                            resolution: "resolved",
                        });
                    }
                }
            }
        }
        #[cfg(not(feature = "lang-html"))]
        let _ = dependency_owners;
        for (edge_offset, edges) in input.edges {
            for (index, edge) in edges.iter().enumerate() {
                let (src_target, dst_target) = *resolved_edge_paths
                    .get(edge_offset + index)
                    .context("missing resolved edge endpoint paths")?;
                for target in [src_target, dst_target].into_iter().flatten() {
                    dependencies.push(StoredDependency {
                        owner,
                        target: Some(target),
                        kind: edge.kind.as_str(),
                        symbol: edge.evidence.as_str(),
                        resolution: "resolved",
                    });
                }
            }
        }
        for row in input
            .coverage
            .into_iter()
            .flatten()
            .filter(|row| row.status != "resolved")
        {
            dependencies.push(StoredDependency {
                owner,
                target: None,
                kind: "unresolved",
                symbol: &row.expression,
                resolution: row.status.as_str(),
            });
        }
        dependencies.sort_unstable();
        dependencies.dedup();
        for (ordinal, dependency) in dependencies.into_iter().enumerate() {
            batch.push([
                BorrowedSqlValue::Integer(owner),
                BorrowedSqlValue::Integer(
                    i64::try_from(ordinal).context("dependency ordinal overflow")?,
                ),
                dependency
                    .target
                    .map_or(BorrowedSqlValue::Null, BorrowedSqlValue::Integer),
                BorrowedSqlValue::Text(dependency.kind),
                BorrowedSqlValue::Text(dependency.symbol),
                BorrowedSqlValue::Text(dependency.resolution),
            ])?;
        }
    }
    batch.flush()
}

pub(super) struct GraphPersistenceTimings {
    pub(super) occurrences: f64,
    pub(super) edges: f64,
    pub(super) dictionaries: f64,
    pub(super) coverage: f64,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn persist_graph(
    tx: &Connection,
    path_cache: &mut PathDictionaryCache,
    graph: &CompactGraph,
    facts: &BTreeMap<String, TypedFacts>,
    dependency_owners: Option<&std::collections::BTreeSet<String>>,
    known_file_entries: Option<&[FileEntry]>,
    node_paths: &mut hashbrown::HashMap<Cow<'_, str>, Option<CachedNodePath>>,
    materialize_edges: bool,
) -> Result<GraphPersistenceTimings> {
    let phase = Instant::now();
    let evidence_ids = CoverageDictionaryCache::prefill_evidence(tx, graph)?;
    let evidence_dictionary_ms = phase.elapsed().as_secs_f64() * 1000.;
    let phase = Instant::now();
    let mut ordinals: hashbrown::HashMap<&str, usize> = hashbrown::HashMap::new();
    let mut current_path: Option<&str> = None;
    let mut current_path_id = None;
    let mut current_ordinal = 0usize;
    let mut occurrence_batch =
        MultiValueBatch::<8, BorrowedSqlValue<'_>>::new(tx, "INSERT INTO edge_occurrences VALUES")?;
    let mut unique_edges = Vec::<StoredEdge<'_>>::with_capacity(if materialize_edges {
        graph.edges.len()
    } else {
        0
    });
    let mut stmt_select_path = tx.prepare_cached(
        "SELECT p.path FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.id=?1",
    )?;
    let mut resolved_edge_paths = Vec::with_capacity(graph.edges.len());
    let mut coverage_batch = MultiValueBatch::<5, BorrowedSqlValue<'_>>::new(
        tx,
        "INSERT OR IGNORE INTO resolution_coverage(path_id,line,expression_id,status,evidence_id) VALUES",
    )?;
    let mut owner_language_batch = MultiValueBatch::<6, BorrowedSqlValue<'_>>::new(
        tx,
        "INSERT OR IGNORE INTO coverage_owner_language(path_id,line,expression_id,status,evidence_id,language) VALUES",
    )?;
    for (first, e) in graph.edges.iter().enumerate() {
        if current_path != Some(e.path.as_ref()) {
            if let Some(previous_path) = current_path {
                ordinals.insert(previous_path, current_ordinal);
            }
            current_path = Some(e.path.as_ref());
            current_path_id = Some(path_cache.get_or_insert(tx, &e.path)?);
            current_ordinal = ordinals.get(e.path.as_ref()).copied().unwrap_or_default();
        }
        let owner_id = current_path_id.context("edge path ID cache is missing the current path")?;
        let ordinal = current_ordinal;
        let evidence_id = *evidence_ids
            .get(e.evidence.as_str())
            .context("missing edge evidence")?;
        let confidence_id = *evidence_ids
            .get(e.confidence.as_str())
            .context("missing edge confidence")?;
        let (src_hash, src_target) = graph_endpoint_hashes(
            node_paths,
            &mut stmt_select_path,
            &e.src,
            &e.path,
            e.kind.as_str(),
        )?;
        let (dst_hash, dst_target) = graph_endpoint_hashes(
            node_paths,
            &mut stmt_select_path,
            &e.dst,
            &e.path,
            e.kind.as_str(),
        )?;
        resolved_edge_paths.push((src_target, dst_target));
        occurrence_batch.push([
            BorrowedSqlValue::Integer(owner_id),
            BorrowedSqlValue::Integer(i64::try_from(ordinal)?),
            BorrowedSqlValue::Integer(src_hash),
            BorrowedSqlValue::Integer(dst_hash),
            BorrowedSqlValue::Text(e.kind.as_str()),
            BorrowedSqlValue::Integer(i64::try_from(e.line)?),
            BorrowedSqlValue::Integer(evidence_id),
            BorrowedSqlValue::Integer(confidence_id),
        ])?;
        current_ordinal = current_ordinal
            .checked_add(1)
            .context("edge occurrence ordinal overflow")?;

        if materialize_edges {
            unique_edges.push(StoredEdge {
                src: src_hash,
                dst: dst_hash,
                kind: e.kind.as_str(),
                path_id: owner_id,
                line: e.line,
                evidence: evidence_id,
                confidence: confidence_id,
                count: 1,
                first,
            });
        }
    }
    occurrence_batch.flush()?;
    let occurrences = phase.elapsed().as_secs_f64() * 1000.;
    let phase = Instant::now();
    if materialize_edges {
        let mut edge_batch = MultiValueBatch::<8, BorrowedSqlValue<'_>>::new(
            tx,
            "INSERT INTO edges(src_hash,kind,dst_hash,path_id,line,evidence_id,confidence_id,occurrence_count) VALUES",
        )?;
        if unique_edges.len() >= 8192 {
            unique_edges
                .par_sort_unstable_by_key(|edge| (edge.src, edge.kind, edge.dst, edge.first));
        } else {
            unique_edges.sort_unstable_by_key(|edge| (edge.src, edge.kind, edge.dst, edge.first));
        }
        unique_edges.dedup_by(|later, first| {
            if (later.src, later.kind, later.dst) == (first.src, first.kind, first.dst) {
                first.count += later.count;
                true
            } else {
                false
            }
        });
        for edge in unique_edges {
            edge_batch.push([
                BorrowedSqlValue::Integer(edge.src),
                BorrowedSqlValue::Text(edge.kind),
                BorrowedSqlValue::Integer(edge.dst),
                BorrowedSqlValue::Integer(edge.path_id),
                BorrowedSqlValue::Integer(i64::try_from(edge.line)?),
                BorrowedSqlValue::Integer(edge.evidence),
                BorrowedSqlValue::Integer(edge.confidence),
                BorrowedSqlValue::Integer(edge.count),
            ])?;
        }
        edge_batch.flush()?;
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        // SAFETY: unique edge scratch storage has dropped; glibc synchronizes arenas.
        libc::malloc_trim(0);
    }
    let edges = phase.elapsed().as_secs_f64() * 1000.;
    let phase = Instant::now();
    let expression_ids =
        CoverageDictionaryCache::prefill_expressions(tx, graph, materialize_edges)?;
    let dictionaries = evidence_dictionary_ms + phase.elapsed().as_secs_f64() * 1000.;
    let phase = Instant::now();
    // Coverage intentionally has no source column. The extracted reference still
    // carries its exact AST owner, so retain only rows where that owner changes
    // the HTML file/whole-line island classification. Conflicting owners stay
    // with the file language rather than receiving a guessed JavaScript owner.
    let mut file_languages: hashbrown::HashMap<i64, &str> =
        hashbrown::HashMap::with_capacity(facts.len());
    let entry_languages: hashbrown::HashMap<&str, &str> = known_file_entries
        .into_iter()
        .flatten()
        .map(|entry| (entry.path.as_str(), entry.language.as_str()))
        .collect();
    for (path, file_facts) in facts {
        if let Some(module) = file_facts.nodes.iter().find(|node| node.kind == "module") {
            if !module.language.is_empty() {
                if let Ok(owner_id) = path_cache.get_or_insert(tx, path) {
                    file_languages.insert(owner_id, module.language.as_str());
                }
            }
        }
    }
    let mut owned_languages: hashbrown::HashMap<i64, String> = hashbrown::HashMap::new();
    let mut coverage_rows: Vec<_> = graph.coverage.iter().collect();
    if !coverage_rows
        .windows(2)
        .all(|rows| compare_coverage_rows(rows[0], rows[1]).is_le())
    {
        if coverage_rows.len() >= 8192 {
            coverage_rows.par_sort_unstable_by(|left, right| compare_coverage_rows(left, right));
        } else {
            coverage_rows.sort_unstable_by(|left, right| compare_coverage_rows(left, right));
        }
    }
    coverage_rows.dedup_by(|later, first| compare_coverage_rows(first, later).is_eq());

    let mut path_ids: hashbrown::HashMap<&str, i64> =
        hashbrown::HashMap::with_capacity(coverage_rows.len().min(facts.len()));
    let mut missing_owners = Vec::new();
    let mut previous_path = None;
    for c in &coverage_rows {
        if previous_path != Some(c.path.as_ref()) {
            let owner_id = path_cache.get_or_insert(tx, &c.path)?;
            path_ids.insert(c.path.as_ref(), owner_id);
            previous_path = Some(c.path.as_ref());
            if !file_languages.contains_key(&owner_id) {
                if let Some(language) = entry_languages
                    .get(c.path.as_ref())
                    .copied()
                    .filter(|language| !language.is_empty())
                {
                    file_languages.insert(owner_id, language);
                } else {
                    missing_owners.push(owner_id);
                }
            }
        }
    }
    if !missing_owners.is_empty() {
        let encoded = serde_json::to_string(&missing_owners)?;
        let mut statement = tx.prepare_cached(
            "SELECT p.path_id, f.language FROM files f JOIN path_dictionary p ON p.path=f.path WHERE p.path_id IN (SELECT value FROM json_each(?1))",
        )?;
        let mut rows = statement.query([encoded.as_str()])?;
        while let Some(row) = rows.next()? {
            let owner_id: i64 = row.get(0)?;
            let language: String = row.get(1)?;
            if !language.is_empty() {
                owned_languages.insert(owner_id, language);
            }
        }
    }

    let mut html_owners = hashbrown::HashMap::new();
    let mut html_islands: hashbrown::HashMap<&str, Vec<(usize, usize)>> = hashbrown::HashMap::new();
    for (path, file_facts) in facts {
        if !file_facts
            .nodes
            .iter()
            .any(|node| node.kind == "module" && node.language == "html")
        {
            continue;
        }
        let node_languages: hashbrown::HashMap<_, _> = file_facts
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node.language.as_str()))
            .collect();
        let full_islands: Vec<_> = file_facts
            .nodes
            .iter()
            .filter(|node| node.kind == "island_scope" && node.language == "javascript")
            .map(|node| (node.line, node.end_line))
            .collect();
        let mut owners = hashbrown::HashMap::new();
        for reference in &file_facts.references {
            let Some(language) = node_languages.get(reference.source.as_str()).copied() else {
                continue;
            };
            let entry = owners
                .entry((reference.line, reference.expression.as_str()))
                .or_insert(Some(language));
            if *entry != Some(language) {
                *entry = None;
            }
        }
        owners.retain(|(line, _), language| {
            let whole_line_js = full_islands
                .iter()
                .any(|(first, last)| first <= line && line <= last);
            match language {
                Some("javascript") => !whole_line_js,
                Some("html") | None => whole_line_js,
                _ => false,
            }
        });
        if !owners.is_empty() {
            html_owners.insert(path.as_str(), owners);
        }
        if !full_islands.is_empty() {
            html_islands.insert(path.as_str(), full_islands);
        }
    }
    let mut language_counts: BTreeMap<(i64, &str, &str), i64> = BTreeMap::new();
    let mut current_path = None;
    let mut current_owner_id = None;
    let mut owner_language_counts = Vec::new();
    for c in coverage_rows {
        let owner_id = if current_path == Some(c.path.as_ref()) {
            current_owner_id.context("coverage path ID cache is missing the current path")?
        } else {
            if let Some(previous_owner_id) = current_owner_id {
                flush_owner_language_counts(
                    previous_owner_id,
                    &mut owner_language_counts,
                    &mut language_counts,
                );
            }
            let owner_id = *path_ids
                .get(c.path.as_ref())
                .context("missing preloaded coverage path ID")?;
            current_path = Some(c.path.as_ref());
            current_owner_id = Some(owner_id);
            owner_id
        };
        let expression_id = *expression_ids
            .get(c.expression.as_str())
            .context("missing coverage expression")?;
        let evidence_id = *evidence_ids
            .get(c.evidence.as_ref())
            .context("missing coverage evidence")?;
        coverage_batch.push([
            BorrowedSqlValue::Integer(owner_id),
            BorrowedSqlValue::Integer(i64::try_from(c.line)?),
            BorrowedSqlValue::Integer(expression_id),
            BorrowedSqlValue::Text(c.status.as_str()),
            BorrowedSqlValue::Integer(evidence_id),
        ])?;
        let item_language = if let Some(language) = html_owners
            .get(c.path.as_ref())
            .and_then(|owners| owners.get(&(c.line, c.expression.as_str())))
        {
            let lang = language.unwrap_or("html");
            owner_language_batch.push([
                BorrowedSqlValue::Integer(owner_id),
                BorrowedSqlValue::Integer(i64::try_from(c.line)?),
                BorrowedSqlValue::Integer(expression_id),
                BorrowedSqlValue::Text(c.status.as_str()),
                BorrowedSqlValue::Integer(evidence_id),
                BorrowedSqlValue::Text(lang),
            ])?;
            lang
        } else if let Some(islands) = html_islands.get(c.path.as_ref()) {
            if islands.iter().any(|(s, e)| *s <= c.line && c.line <= *e) {
                "javascript"
            } else if let Some(lang) = file_languages.get(&owner_id) {
                lang
            } else if let Some(lang) = owned_languages.get(&owner_id) {
                lang.as_str()
            } else {
                "html"
            }
        } else if let Some(lang) = file_languages.get(&owner_id) {
            lang
        } else if let Some(lang) = owned_languages.get(&owner_id) {
            lang.as_str()
        } else {
            ""
        };
        if !item_language.is_empty() {
            if let Some((_, _, count)) =
                owner_language_counts
                    .iter_mut()
                    .find(|(language, status, _)| {
                        *language == item_language && *status == c.status.as_str()
                    })
            {
                *count += 1;
            } else {
                owner_language_counts.push((item_language, c.status.as_str(), 1));
            }
        }
    }
    if let Some(owner_id) = current_owner_id {
        flush_owner_language_counts(owner_id, &mut owner_language_counts, &mut language_counts);
    }

    coverage_batch.flush()?;
    owner_language_batch.flush()?;
    let mut language_count_batch = MultiValueBatch::<4, BorrowedSqlValue<'_>>::new(
        tx,
        "INSERT INTO coverage_language_counts(path_id,language,status,records) VALUES",
    )?;
    for ((path_id, language, status), count) in language_counts {
        language_count_batch.push([
            BorrowedSqlValue::Integer(path_id),
            BorrowedSqlValue::Text(language),
            BorrowedSqlValue::Text(status),
            BorrowedSqlValue::Integer(count),
        ])?;
    }
    language_count_batch.flush()?;
    drop(stmt_select_path);
    persist_dependencies(
        tx,
        path_cache,
        graph,
        facts,
        dependency_owners,
        &resolved_edge_paths,
    )?;
    Ok(GraphPersistenceTimings {
        occurrences,
        edges,
        dictionaries,
        coverage: phase.elapsed().as_secs_f64() * 1000.,
    })
}
