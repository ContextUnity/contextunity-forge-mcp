use super::*;

pub(super) fn persist_files(
    tx: &Connection,
    path_cache: &mut PathDictionaryCache,
    entries: &[FileEntry],
    facts: &BTreeMap<String, TypedFacts>,
    encoded_facts: Option<&hashbrown::HashMap<String, Vec<u8>>>,
    bulk_search: bool,
) -> Result<(i64, usize, f64)> {
    let mut node_id: i64 = tx.query_row("SELECT coalesce(max(node_id),0)FROM nodes", [], |r| {
        r.get(0)
    })?;
    let mut docs_count = 0;

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
            tx.prepare(
                "INSERT INTO nodes VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            )
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
    let mut local_facts_batch = bulk_search
        .then(|| {
            MultiValueBatch::<6, BorrowedSqlValue<'_>>::new(tx, "INSERT INTO local_facts VALUES")
        })
        .transpose()?;
    let mut doc_sections_batch = bulk_search
        .then(|| {
            MultiValueBatch::<10, BorrowedSqlValue<'_>>::new(tx, "INSERT INTO doc_sections VALUES")
        })
        .transpose()?;
    let mut file_batch = Vec::<[SqlValue; 8]>::new();
    let mut node_batch = Vec::<[BorrowedSqlValue<'_>; 15]>::new();
    let mut reusable_detail_buffers = Vec::<Vec<u8>>::new();
    let mut owned_search_batch = Vec::<SearchRow>::new();
    let mut fts_insert_ms = 0.0;
    let mut reusable_search_buffers = Vec::<String>::new();
    let mut incremental_search_buffer = String::new();
    let mut shared_owner_rows = Vec::<[i64; 4]>::new();
    let (file_batch_rows, node_batch_rows, owned_search_batch_rows) = if bulk_search {
        (
            Some(multi_value_batch_rows::<8>(tx)?),
            Some(multi_value_batch_rows::<15>(tx)?),
            Some(multi_value_batch_rows::<2>(tx)?),
        )
    } else {
        (None, None, None)
    };

    for file in entries {
        let f = facts.get(&file.path).context("missing extracted facts")?;
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
        if let Some(batch) = local_facts_batch.as_mut() {
            let facts_blob = match encoded_facts.and_then(|m| m.get(&file.path)) {
                Some(blob) => BorrowedSqlValue::Blob(blob),
                None => BorrowedSqlValue::OwnedBlob(encode_facts(f)?),
            };
            batch.push([
                BorrowedSqlValue::Text(&file.path),
                BorrowedSqlValue::Text(&file.digest),
                BorrowedSqlValue::Text(&file.language),
                BorrowedSqlValue::Integer(i64::from(is_test(&file.path))),
                BorrowedSqlValue::Integer(0),
                facts_blob,
            ])?;
        } else {
            let facts_blob = match encoded_facts.and_then(|m| m.get(&file.path)) {
                Some(blob) => std::borrow::Cow::Borrowed(blob.as_slice()),
                None => std::borrow::Cow::Owned(encode_facts(f)?),
            };
            stmt_local_facts
                .as_mut()
                .context("missing incremental local facts insert")?
                .execute(params![
                    file.path,
                    file.digest,
                    file.language,
                    is_test(&file.path),
                    facts_blob.as_ref()
                ])?;
        }
        for n in &f.nodes {
            node_id += 1;
            let node_path_id = if n.path == file.path {
                owner_id
            } else {
                path_cache.get_or_insert(tx, &n.path)?
            };
            if bulk_search {
                let mut details = reusable_detail_buffers.pop().unwrap_or_default();
                details.clear();
                serde_json::to_writer(&mut details, &n.navigation_details())?;
                node_batch.push([
                    BorrowedSqlValue::Integer(node_id),
                    BorrowedSqlValue::Text(&n.id),
                    BorrowedSqlValue::Text(&n.kind),
                    BorrowedSqlValue::Text(&n.name),
                    BorrowedSqlValue::Text(&n.qualname),
                    BorrowedSqlValue::Integer(node_path_id),
                    BorrowedSqlValue::Text(&n.path),
                    BorrowedSqlValue::Integer(i64::try_from(n.line)?),
                    BorrowedSqlValue::Integer(i64::try_from(n.end_line)?),
                    BorrowedSqlValue::Integer(i64::from(n.is_test)),
                    BorrowedSqlValue::Text(&n.language),
                    BorrowedSqlValue::Integer(i64::from(n.generated)),
                    BorrowedSqlValue::OwnedJsonText(details),
                    BorrowedSqlValue::Integer(stable_hash64(&n.id)),
                    BorrowedSqlValue::Integer(owner_id),
                ]);
                if node_batch.len() == node_batch_rows.context("missing cold node batch size")? {
                    flush_node_batch(tx, &mut node_batch, &mut reusable_detail_buffers)?;
                }
            } else {
                let details = serde_json::to_string(&n.navigation_details())?;
                stmt_nodes
                    .as_mut()
                    .context("missing incremental node insert")?
                    .execute(params![
                        node_id,
                        n.id,
                        n.kind,
                        n.name,
                        n.qualname,
                        node_path_id,
                        n.path,
                        n.line,
                        n.end_line,
                        n.is_test,
                        n.language,
                        n.generated,
                        details,
                        stable_hash64(&n.id),
                        owner_id
                    ])?;
            }
            if bulk_search {
                let mut search = reusable_search_buffers.pop().unwrap_or_default();
                write_node_search_text(&mut search, n, f);
                owned_search_batch.push(SearchRow {
                    node_id,
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
                    stmt_node_search.execute(params![node_id, &incremental_search_buffer])?;
                }
                incremental_search_buffer.clear();
            }
            let key_hash = shared_key_cache.get_or_insert_node(&n.qualname, node_id)?;
            shared_owner_rows.push([0, key_hash, owner_id, node_id]);
        }
        for key in reference_keys(f) {
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
                    BorrowedSqlValue::Text(&d.doc_id),
                    BorrowedSqlValue::Text(&d.path),
                    BorrowedSqlValue::Text(&d.section_title),
                    BorrowedSqlValue::Text(&d.doc_type),
                    BorrowedSqlValue::Text(&d.content),
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

    insert_multi_value_batch(tx, "INSERT INTO files VALUES", &file_batch)?;
    flush_node_batch(tx, &mut node_batch, &mut reusable_detail_buffers)?;
    fts_insert_ms +=
        flush_owned_search_batch(tx, &mut owned_search_batch, &mut reusable_search_buffers)?;
    if let Some(batch) = source_inventory_batch.as_mut() {
        batch.flush()?;
    }
    if let Some(batch) = local_facts_batch.as_mut() {
        batch.flush()?;
    }
    if let Some(batch) = doc_sections_batch.as_mut() {
        batch.flush()?;
    }

    shared_key_cache.flush()?;
    shared_owner_batch.flush()?;
    Ok((node_id, docs_count, fts_insert_ms))
}

fn flush_node_batch(
    tx: &Connection,
    batch: &mut Vec<[BorrowedSqlValue<'_>; 15]>,
    reusable_buffers: &mut Vec<Vec<u8>>,
) -> Result<()> {
    insert_multi_value_batch(tx, "INSERT INTO nodes VALUES", batch)?;
    for row in batch.drain(..) {
        for value in row {
            if let BorrowedSqlValue::OwnedJsonText(buffer) = value {
                reusable_buffers.push(buffer);
            }
        }
    }
    Ok(())
}

fn write_node_search_text(search: &mut String, node: &Node, facts: &Facts) {
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
    node_id: i64,
    text: String,
}

fn flush_owned_search_batch(
    tx: &Connection,
    batch: &mut Vec<SearchRow>,
    reusable_buffers: &mut Vec<String>,
) -> Result<f64> {
    let rows: Vec<_> = batch
        .iter()
        .map(|row| {
            [
                BorrowedSqlValue::Integer(row.node_id),
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
    path: String,
    node_hash: i64,
    path_hash: i64,
}

impl CachedNodePath {
    pub(super) fn new(id: &str, path: String) -> Self {
        Self {
            node_hash: stable_hash64(id),
            path_hash: stable_hash64(&path),
            path,
        }
    }
}

pub(super) fn graph_endpoint_hashes(
    node_paths: &mut hashbrown::HashMap<String, Option<CachedNodePath>>,
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
            (node.path != owner).then_some(node.path_hash),
        ));
    }
    let path: Option<String> = stmt_select_path
        .query_row([endpoint], |row| row.get(0))
        .optional()?;
    let node = node_paths
        .entry(endpoint.to_owned())
        .or_insert_with(|| path.map(|path| CachedNodePath::new(endpoint, path)));
    let node = node
        .as_ref()
        .with_context(|| format!("edge {kind} has missing endpoint {endpoint}"))?;
    Ok((
        node.node_hash,
        (node.path != owner).then_some(node.path_hash),
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

pub(super) struct GraphPersistenceTimings {
    pub(super) occurrences: f64,
    pub(super) edges: f64,
    pub(super) dictionaries: f64,
    pub(super) coverage: f64,
}

pub(super) fn persist_graph(
    tx: &Connection,
    path_cache: &mut PathDictionaryCache,
    graph: &CompactGraph,
    node_paths: &mut hashbrown::HashMap<String, Option<CachedNodePath>>,
    materialize_edges: bool,
) -> Result<GraphPersistenceTimings> {
    let phase = Instant::now();
    let evidence_ids = CoverageDictionaryCache::prefill_evidence(tx, graph)?;
    let evidence_dictionary_ms = phase.elapsed().as_secs_f64() * 1000.;
    let phase = Instant::now();
    let mut ordinals: hashbrown::HashMap<&str, usize> = hashbrown::HashMap::new();
    let mut occurrence_batch =
        MultiValueBatch::<8, BorrowedSqlValue<'_>>::new(tx, "INSERT INTO edge_occurrences VALUES")?;
    let mut unique_edges = Vec::<StoredEdge<'_>>::with_capacity(if materialize_edges {
        graph.edges.len()
    } else {
        0
    });
    let mut dependency_batch =
        MultiValueBatch::<6, BorrowedSqlValue<'_>>::new(tx, "INSERT INTO dependencies VALUES")?;
    let mut stmt_select_path = tx.prepare_cached(
        "SELECT p.path FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.id=?1",
    )?;
    let mut coverage_batch = MultiValueBatch::<5, BorrowedSqlValue<'_>>::new(tx,
        "INSERT OR IGNORE INTO resolution_coverage(path_id,line,expression_id,status,evidence_id) VALUES",
    )?;
    let mut dependencies = Vec::<StoredDependency<'_>>::with_capacity(
        graph.edges.len().saturating_add(graph.coverage.len()),
    );

    for (first, e) in graph.edges.iter().enumerate() {
        let ordinal = ordinals.entry(&e.path).or_default();
        let evidence_id = *evidence_ids
            .get(e.evidence.as_str())
            .context("missing edge evidence")?;
        let confidence_id = *evidence_ids
            .get(e.confidence.as_str())
            .context("missing edge confidence")?;
        let owner_id = path_cache.get_or_insert(tx, &e.path)?;
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
        occurrence_batch.push([
            BorrowedSqlValue::Integer(owner_id),
            BorrowedSqlValue::Integer(i64::try_from(*ordinal)?),
            BorrowedSqlValue::Integer(src_hash),
            BorrowedSqlValue::Integer(dst_hash),
            BorrowedSqlValue::Text(e.kind.as_str()),
            BorrowedSqlValue::Integer(i64::try_from(e.line)?),
            BorrowedSqlValue::Integer(evidence_id),
            BorrowedSqlValue::Integer(confidence_id),
        ])?;
        *ordinal = ordinal
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
        let mut target_hashes = [None, None];
        let mut targets_len = 0;
        for target_hash in [src_target, dst_target].into_iter().flatten() {
            if !target_hashes[..targets_len].contains(&Some(target_hash)) {
                target_hashes[targets_len] = Some(target_hash);
                targets_len += 1;
            }
        }
        for target_hash in target_hashes[..targets_len].iter().flatten() {
            dependencies.push(StoredDependency {
                owner: owner_id,
                target: Some(*target_hash),
                kind: e.kind.as_str(),
                symbol: e.evidence.as_str(),
                resolution: "resolved",
            });
        }
    }
    occurrence_batch.flush()?;
    let occurrences = phase.elapsed().as_secs_f64() * 1000.;
    let phase = Instant::now();
    if materialize_edges {
        let mut edge_batch = MultiValueBatch::<8, BorrowedSqlValue<'_>>::new(tx, "INSERT INTO edges(src_hash,kind,dst_hash,path_id,line,evidence_id,confidence_id,occurrence_count) VALUES")?;
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
    let edges = phase.elapsed().as_secs_f64() * 1000.;
    let phase = Instant::now();
    let expression_ids =
        CoverageDictionaryCache::prefill_expressions(tx, graph, materialize_edges)?;
    let dictionaries = evidence_dictionary_ms + phase.elapsed().as_secs_f64() * 1000.;
    let phase = Instant::now();
    for c in &graph.coverage {
        let owner_id = path_cache.get_or_insert(tx, &c.path)?;
        let expression_id = *expression_ids
            .get(c.expression.as_str())
            .context("missing coverage expression")?;
        let evidence_id = *evidence_ids
            .get(c.evidence.as_str())
            .context("missing coverage evidence")?;
        coverage_batch.push([
            BorrowedSqlValue::Integer(owner_id),
            BorrowedSqlValue::Integer(i64::try_from(c.line)?),
            BorrowedSqlValue::Integer(expression_id),
            BorrowedSqlValue::Text(c.status.as_str()),
            BorrowedSqlValue::Integer(evidence_id),
        ])?;
        if c.status != "resolved" {
            dependencies.push(StoredDependency {
                owner: owner_id,
                target: None,
                kind: "unresolved",
                symbol: c.expression.as_str(),
                resolution: c.status.as_str(),
            });
        }
    }

    coverage_batch.flush()?;
    if dependencies.len() >= 8192 {
        dependencies.par_sort_unstable();
    } else {
        dependencies.sort_unstable();
    }
    dependencies.dedup();
    let mut owner = None;
    let mut ordinal = 0i64;
    for dependency in dependencies {
        if owner != Some(dependency.owner) {
            owner = Some(dependency.owner);
            ordinal = 0;
        }
        dependency_batch.push([
            BorrowedSqlValue::Integer(dependency.owner),
            BorrowedSqlValue::Integer(ordinal),
            dependency
                .target
                .map_or(BorrowedSqlValue::Null, BorrowedSqlValue::Integer),
            BorrowedSqlValue::Text(dependency.kind),
            BorrowedSqlValue::Text(dependency.symbol),
            BorrowedSqlValue::Text(dependency.resolution),
        ])?;
        ordinal = ordinal
            .checked_add(1)
            .context("dependency ordinal overflow")?;
    }
    dependency_batch.flush()?;
    Ok(GraphPersistenceTimings {
        occurrences,
        edges,
        dictionaries,
        coverage: phase.elapsed().as_secs_f64() * 1000.,
    })
}
