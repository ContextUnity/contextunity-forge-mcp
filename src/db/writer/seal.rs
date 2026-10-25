use super::{CompactGraph, FileEntry, TypedFacts};
use crate::core::{
    commitments::{ALGORITHM, DOMAINS},
    models::{compact_graph::Coverage, compact_graph::Edge, is_test, stable_hash64, Node},
};
use anyhow::{ensure, Context, Result};
use hashbrown::HashMap;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::Instant;

type OwnerHashers<'a> = HashMap<&'a str, Sha256>;
type Leaves = BTreeMap<String, String>;
type NodePathMap<'a> = HashMap<&'a str, (&'a str, i64)>;
type NodeLookupMaps<'a> = (NodePathMap<'a>, HashMap<i64, &'a str>);

pub(super) struct NodeDomain<'a> {
    hashers: OwnerHashers<'a>,
}

impl<'a> NodeDomain<'a> {
    pub(super) fn new() -> Self {
        Self {
            hashers: OwnerHashers::new(),
        }
    }

    pub(super) fn push(&mut self, owner: &'a str, node: &Node, details: &[u8]) -> Result<()> {
        let line = i64::try_from(node.line).context("node line exceeds SQLite range")?;
        let end_line =
            i64::try_from(node.end_line).context("node end line exceeds SQLite range")?;
        let hasher = row_hasher(&mut self.hashers, "nodes", owner);
        encode_text(hasher, &node.id);
        encode_text(hasher, &node.kind);
        encode_text(hasher, &node.name);
        encode_text(hasher, &node.qualname);
        encode_text(hasher, &node.path);
        encode_integer(hasher, line);
        encode_integer(hasher, end_line);
        encode_integer(hasher, i64::from(node.is_test));
        encode_text(hasher, &node.language);
        encode_integer(hasher, i64::from(node.generated));
        encode_text_bytes(hasher, details);
        encode_integer(hasher, stable_hash64(&node.id));
        Ok(())
    }

    pub(super) fn finish(self) -> Result<BTreeMap<String, String>> {
        finish_hashers("nodes", self.hashers)
    }
}

#[derive(Default)]
pub(super) struct ProjectionTimings {
    pub(super) file_domains_ms: f64,
    pub(super) node_domain_ms: f64,
    pub(super) graph_edges_ms: f64,
    pub(super) graph_occurrences_ms: f64,
    pub(super) graph_unique_edges_ms: f64,
    pub(super) graph_shared_owners_ms: f64,
    pub(super) graph_coverage_ms: f64,
    pub(super) graph_dependencies_ms: f64,
    pub(super) graph_domains_ms: f64,
}

struct ProjectedNode<'a> {
    owner: &'a str,
    node: &'a Node,
    node_id: i64,
    node_hash: i64,
}

struct UniqueEdge<'a> {
    edge: &'a Edge,
    src: &'a str,
    dst: &'a str,
    count: i64,
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
struct Dependency<'a> {
    owner: &'a str,
    target_hash: Option<i64>,
    kind: &'a str,
    symbol: &'a str,
    resolution: &'a str,
}

struct DependencyLeafRow<'a> {
    target: Option<&'a str>,
    kind: &'a str,
    symbol: &'a str,
    resolution: &'a str,
}

struct GraphProjection<'a> {
    entries: &'a [FileEntry],
    facts: &'a BTreeMap<String, TypedFacts>,
    graph: &'a CompactGraph,
    projected_nodes: &'a [ProjectedNode<'a>],
    node_by_id: &'a NodePathMap<'a>,
    file_by_hash: &'a HashMap<i64, &'a str>,
}

struct OwnerLanguageLeafRow<'a> {
    line: i64,
    expression: &'a str,
    status: &'a str,
    evidence: &'a str,
    language: &'a str,
}

pub(super) fn local_facts_leaf(
    path: &str,
    digest: &str,
    language: &str,
    is_test: bool,
    facts_blob: &[u8],
) -> Result<(String, String)> {
    let mut hasher = new_leaf_hasher("local_facts", path);
    hasher.update([0xff]);
    encode_text(&mut hasher, path);
    encode_text(&mut hasher, digest);
    encode_text(&mut hasher, language);
    encode_integer(&mut hasher, i64::from(is_test));
    encode_integer(&mut hasher, 0);
    encode_blob(&mut hasher, facts_blob);
    let key = leaf_key("local_facts", path)?;
    let digest = hex::encode(hasher.finalize());
    Ok((key, digest))
}

pub(super) fn file_domain_leaves(
    entries: &[FileEntry],
    facts: &BTreeMap<String, TypedFacts>,
    reference_keys: &[String],
) -> Result<BTreeMap<&'static str, Leaves>> {
    let mut domains = BTreeMap::new();
    let projected_nodes = project_file_domains(entries, facts, &mut domains, true)?;
    domains.insert(
        "shared_owners",
        project_shared_owner_domain(facts, &projected_nodes, Some(reference_keys))?,
    );
    domains.retain(|domain, _| matches!(*domain, "doc_sections" | "shared_owners"));
    Ok(domains)
}

/// Builds in-memory logical commitments that do not require database reads.
/// The `nodes` domain is produced while its exact serialized details are persisted.
pub(super) fn logical_domain_leaves(
    entries: &[FileEntry],
    facts: &BTreeMap<String, TypedFacts>,
    graph: &CompactGraph,
    timings: &mut ProjectionTimings,
    include_restored_file_domains: bool,
) -> Result<BTreeMap<&'static str, Leaves>> {
    let mut domains = BTreeMap::new();
    let started = Instant::now();
    let projected_nodes =
        project_file_domains(entries, facts, &mut domains, include_restored_file_domains)?;
    timings.file_domains_ms = started.elapsed().as_secs_f64() * 1000.;
    let started = Instant::now();
    let (node_by_id, file_by_hash) = node_lookup_maps(entries, facts, &projected_nodes)?;
    timings.node_domain_ms = started.elapsed().as_secs_f64() * 1000.;
    let started = Instant::now();
    project_graph_domains(
        GraphProjection {
            entries,
            facts,
            graph,
            projected_nodes: &projected_nodes,
            node_by_id: &node_by_id,
            file_by_hash: &file_by_hash,
        },
        &mut domains,
        timings,
        include_restored_file_domains,
    )?;
    timings.graph_domains_ms = started.elapsed().as_secs_f64() * 1000.;
    for &domain in DOMAINS {
        if domain == "local_facts"
            || domain == "nodes"
            || (!include_restored_file_domains
                && matches!(domain, "doc_sections" | "shared_owners"))
        {
            continue;
        }
        ensure!(
            domains.contains_key(domain),
            "in-memory projection did not produce domain {domain}"
        );
    }
    Ok(domains)
}

fn new_leaf_hasher(table: &str, owner: &str) -> Sha256 {
    let mut hasher = Sha256::new();
    hasher.update(ALGORITHM);
    hasher.update(table);
    hasher.update((owner.len() as u64).to_le_bytes());
    hasher.update(owner.as_bytes());
    hasher
}

fn leaf_key(table: &str, owner: &str) -> Result<String> {
    Ok(format!("leaf:{}", serde_json::to_string(&(table, owner))?))
}

fn row_hasher<'map, 'owner>(
    hashers: &'map mut OwnerHashers<'owner>,
    table: &str,
    owner: &'owner str,
) -> &'map mut Sha256 {
    let hasher = hashers
        .entry(owner)
        .or_insert_with(|| new_leaf_hasher(table, owner));
    hasher.update([0xff]);
    hasher
}

fn encode_text(hasher: &mut Sha256, value: &str) {
    encode_text_bytes(hasher, value.as_bytes());
}

fn encode_text_bytes(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(b"t");
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}

fn encode_integer(hasher: &mut Sha256, value: i64) {
    hasher.update(b"i");
    hasher.update(value.to_le_bytes());
}

fn encode_real(hasher: &mut Sha256, value: f64) {
    hasher.update(b"r");
    hasher.update(value.to_bits().to_le_bytes());
}

fn encode_blob(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(b"b");
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}

fn finish_hashers(table: &'static str, hashers: OwnerHashers<'_>) -> Result<Leaves> {
    hashers
        .into_iter()
        .map(|(owner, hasher)| Ok((leaf_key(table, owner)?, hex::encode(hasher.finalize()))))
        .collect()
}

fn project_file_domains<'a>(
    entries: &'a [FileEntry],
    facts: &'a BTreeMap<String, TypedFacts>,
    domains: &mut BTreeMap<&'static str, Leaves>,
    include_documents: bool,
) -> Result<Vec<ProjectedNode<'a>>> {
    let mut inventory = OwnerHashers::new();
    let mut files = OwnerHashers::new();
    let mut errors = OwnerHashers::new();
    let mut docs = OwnerHashers::new();
    let mut projected_nodes = Vec::new();
    let mut node_id = 0i64;

    for file in entries {
        let file_facts = facts
            .get(&file.path)
            .with_context(|| format!("missing extracted facts for {}", file.path))?;
        let size = i64::try_from(file.bytes).context("file size exceeds SQLite integer range")?;
        let test = i64::from(is_test(&file.path));
        let path_hash = stable_hash64(&file.path);
        {
            let hasher = row_hasher(&mut inventory, "source_inventory", &file.path);
            encode_text(hasher, &file.path);
            encode_text(hasher, "indexed");
            encode_text(hasher, &file.digest);
            encode_integer(hasher, size);
        }
        {
            let hasher = row_hasher(&mut files, "files", &file.path);
            encode_text(hasher, &file.path);
            encode_text(hasher, "indexed");
            encode_text(hasher, &file.digest);
            encode_integer(hasher, size);
            encode_text(hasher, &file.language);
            encode_integer(hasher, test);
            encode_integer(hasher, 0);
            encode_integer(hasher, path_hash);
        }

        let mut file_errors: Vec<_> = file_facts.errors.iter().collect();
        file_errors.sort_unstable_by(|left, right| {
            left.line
                .cmp(&right.line)
                .then_with(|| left.message.as_bytes().cmp(right.message.as_bytes()))
        });
        for error in file_errors {
            let line = i64::try_from(error.line).context("diagnostic line exceeds SQLite range")?;
            let hasher = row_hasher(&mut errors, "errors", &error.path);
            encode_text(hasher, &error.path);
            encode_integer(hasher, line);
            encode_text(hasher, &error.message);
        }

        let mut file_docs: Vec<_> = file_facts
            .docs
            .iter()
            .filter(|_| include_documents)
            .collect();
        file_docs.sort_unstable_by(|left, right| left.doc_id.cmp(&right.doc_id));
        for doc in file_docs {
            let size = i64::try_from(doc.size).context("document size exceeds SQLite range")?;
            let invariants = serde_json::to_string(&doc.invariants)?;
            let referenced = serde_json::to_string(&doc.referenced_symbols)?;
            let hasher = row_hasher(&mut docs, "doc_sections", &doc.path);
            encode_text(hasher, &doc.doc_id);
            encode_text(hasher, &doc.path);
            encode_text(hasher, &doc.section_title);
            encode_text(hasher, &doc.doc_type);
            encode_text(hasher, &doc.content);
            encode_text(hasher, &invariants);
            encode_text(hasher, &referenced);
            encode_real(hasher, doc.mtime);
            encode_integer(hasher, size);
            encode_integer(hasher, i64::from(doc.is_invariant));
        }

        for node in &file_facts.nodes {
            node_id = node_id.checked_add(1).context("node ID overflow")?;
            projected_nodes.push(ProjectedNode {
                owner: &file.path,
                node,
                node_id,
                node_hash: stable_hash64(&node.id),
            });
        }
    }

    domains.insert(
        "source_inventory",
        finish_hashers("source_inventory", inventory)?,
    );
    domains.insert("files", finish_hashers("files", files)?);
    domains.insert("errors", finish_hashers("errors", errors)?);
    if include_documents {
        domains.insert("doc_sections", finish_hashers("doc_sections", docs)?);
    }
    Ok(projected_nodes)
}

fn node_lookup_maps<'a>(
    entries: &'a [FileEntry],
    facts: &BTreeMap<String, TypedFacts>,
    projected_nodes: &[ProjectedNode<'a>],
) -> Result<NodeLookupMaps<'a>> {
    let node_by_id = projected_nodes
        .iter()
        .map(|projected| {
            (
                projected.node.id.as_str(),
                (projected.node.path.as_str(), projected.node_hash),
            )
        })
        .collect();
    let mut file_by_hash = HashMap::with_capacity(entries.len());
    for file in entries {
        file_by_hash.insert(stable_hash64(&file.path), file.path.as_str());
        ensure!(
            facts.contains_key(&file.path),
            "missing facts for indexed path {}",
            file.path
        );
    }
    Ok((node_by_id, file_by_hash))
}

fn project_graph_domains<'a>(
    projection: GraphProjection<'a>,
    domains: &mut BTreeMap<&'static str, Leaves>,
    timings: &mut ProjectionTimings,
    include_shared_owners: bool,
) -> Result<()> {
    let GraphProjection {
        entries,
        facts,
        graph,
        projected_nodes,
        node_by_id,
        file_by_hash,
    } = projection;
    let mut ordinals = HashMap::<&str, i64>::new();
    let mut unique_edge_indexes = HashMap::<(i64, &str, i64), usize>::new();
    let mut unique_edges: Vec<UniqueEdge<'_>> = Vec::with_capacity(graph.edges.len());
    let mut occurrence_hashers = OwnerHashers::new();
    let mut dependencies = Vec::with_capacity(graph.edges.len().saturating_mul(2));

    let edges_started = Instant::now();
    for edge in &graph.edges {
        let owner = edge.path.as_str();
        let ordinal = ordinals.entry(owner).or_default();
        let (src_path, src_hash) = node_by_id.get(edge.src.as_str()).with_context(|| {
            format!(
                "edge {} has missing source endpoint {}",
                edge.kind.as_str(),
                edge.src
            )
        })?;
        let (dst_path, dst_hash) = node_by_id.get(edge.dst.as_str()).with_context(|| {
            format!(
                "edge {} has missing destination endpoint {}",
                edge.kind.as_str(),
                edge.dst
            )
        })?;
        let src = edge.src.as_str();
        let dst = edge.dst.as_str();
        let edge_key = (*src_hash, edge.kind.as_str(), *dst_hash);
        let occurrence_hasher = occurrence_hashers
            .entry(owner)
            .or_insert_with(|| new_leaf_hasher("edge_occurrences", owner));
        occurrence_hasher.update([0xff]);
        encode_text(occurrence_hasher, owner);
        encode_integer(occurrence_hasher, *ordinal);
        encode_text(occurrence_hasher, src);
        encode_text(occurrence_hasher, dst);
        encode_text(occurrence_hasher, edge.kind.as_str());
        encode_text(occurrence_hasher, owner);
        encode_integer(
            occurrence_hasher,
            i64::try_from(edge.line).context("edge line exceeds SQLite range")?,
        );
        encode_text(occurrence_hasher, &edge.evidence);
        encode_text(occurrence_hasher, edge.confidence.as_str());
        *ordinal = (*ordinal)
            .checked_add(1)
            .context("edge occurrence ordinal overflow")?;
        if let Some(index) = unique_edge_indexes.get(&edge_key).copied() {
            let existing = &mut unique_edges[index];
            existing.count = existing
                .count
                .checked_add(1)
                .context("edge occurrence count overflow")?;
        } else {
            unique_edge_indexes.insert(edge_key, unique_edges.len());
            unique_edges.push(UniqueEdge {
                edge,
                src,
                dst,
                count: 1,
            });
        }

        let mut first_target_hash = None;
        for target in [*src_path, *dst_path] {
            if target != edge.path {
                let target_hash = stable_hash64(target);
                if first_target_hash != Some(target_hash) {
                    dependencies.push(Dependency {
                        owner: &edge.path,
                        target_hash: Some(target_hash),
                        kind: edge.kind.as_str(),
                        symbol: &edge.evidence,
                        resolution: "resolved",
                    });
                    first_target_hash = Some(target_hash);
                }
            }
        }
    }
    drop(unique_edge_indexes);
    drop(ordinals);
    timings.graph_edges_ms = edges_started.elapsed().as_secs_f64() * 1000.;

    let occurrences_finished = Instant::now();
    domains.insert(
        "edge_occurrences",
        finish_hashers("edge_occurrences", occurrence_hashers)?,
    );
    timings.graph_occurrences_ms = occurrences_finished.elapsed().as_secs_f64() * 1000.;

    let unique_edges_started = Instant::now();
    unique_edges.sort_unstable_by(|left, right| {
        left.edge
            .path
            .cmp(&right.edge.path)
            .then_with(|| left.src.cmp(right.src))
            .then_with(|| left.dst.cmp(right.dst))
            .then_with(|| left.edge.kind.as_str().cmp(right.edge.kind.as_str()))
    });
    let mut edge_hashers = OwnerHashers::new();
    for row in unique_edges {
        let owner = row.edge.path.as_str();
        let hasher = edge_hashers
            .entry(owner)
            .or_insert_with(|| new_leaf_hasher("edges", owner));
        let edge = row.edge;
        hasher.update([0xff]);
        encode_text(hasher, row.src);
        encode_text(hasher, row.dst);
        encode_text(hasher, edge.kind.as_str());
        encode_text(hasher, owner);
        encode_integer(
            hasher,
            i64::try_from(edge.line).context("edge line exceeds SQLite range")?,
        );
        encode_text(hasher, &edge.evidence);
        encode_text(hasher, edge.confidence.as_str());
        encode_integer(hasher, row.count);
    }
    domains.insert("edges", finish_hashers("edges", edge_hashers)?);
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::malloc_trim(0);
    }
    timings.graph_unique_edges_ms = unique_edges_started.elapsed().as_secs_f64() * 1000.;

    let shared_started = Instant::now();
    if include_shared_owners {
        let shared_owners = project_shared_owner_domain(facts, projected_nodes, None)?;
        domains.insert("shared_owners", shared_owners);
    }
    timings.graph_shared_owners_ms = shared_started.elapsed().as_secs_f64() * 1000.;
    let coverage_started = Instant::now();
    let (coverage_domains, unresolved) = project_coverage_domains(entries, facts, graph)?;
    domains.insert("resolution_coverage", coverage_domains.0);
    domains.insert("coverage_owner_language", coverage_domains.1);
    domains.insert("coverage_language_counts", coverage_domains.2);
    timings.graph_coverage_ms = coverage_started.elapsed().as_secs_f64() * 1000.;
    let dependency_started = Instant::now();
    dependencies.extend(unresolved);
    let dependency_leaves = project_dependency_domain(facts, file_by_hash, dependencies)?;
    domains.insert("dependencies", dependency_leaves);
    timings.graph_dependencies_ms = dependency_started.elapsed().as_secs_f64() * 1000.;
    Ok(())
}

fn project_shared_owner_domain<'a>(
    facts: &'a BTreeMap<String, TypedFacts>,
    projected_nodes: &[ProjectedNode<'a>],
    prepared_references: Option<&'a [String]>,
) -> Result<Leaves> {
    let mut key_by_hash = HashMap::<i64, &str>::new();
    let mut owner_rows = Vec::<(&str, i64, i64, i64)>::new();
    let mut id_by_node_id = HashMap::<i64, &str>::new();
    let mut nodes_by_owner = BTreeMap::<&str, Vec<&ProjectedNode<'a>>>::new();
    for node in projected_nodes {
        nodes_by_owner.entry(node.owner).or_default().push(node);
        id_by_node_id.insert(node.node_id, node.node.id.as_str());
    }

    for (owner, file_facts) in facts {
        if let Some(owner_nodes) = nodes_by_owner.get(owner.as_str()) {
            for projected in owner_nodes {
                let key_hash = register_shared_key(&mut key_by_hash, &projected.node.qualname)?;
                owner_rows.push((owner.as_str(), 0, key_hash, projected.node_id));
            }
        }
        let keys = prepared_references.map_or_else(
            || file_facts.reference_keys(),
            |keys| keys.iter().map(String::as_str).collect(),
        );
        for key in keys {
            let key_hash = register_shared_key(&mut key_by_hash, key)?;
            owner_rows.push((owner.as_str(), 1, key_hash, 0));
        }
        if let Some(owner_nodes) = nodes_by_owner.get(owner.as_str()) {
            for projected in owner_nodes {
                if projected.node.details["default_export"] == true {
                    let key_hash = register_shared_key(&mut key_by_hash, &projected.node.id)?;
                    owner_rows.push((owner.as_str(), 2, key_hash, 0));
                }
            }
        }
    }

    owner_rows.sort_unstable();
    owner_rows.dedup();

    struct SharedRow<'a> {
        kind: &'static str,
        key: &'a str,
        ordinal: i64,
        node_id: Option<&'a str>,
    }
    let mut by_owner = BTreeMap::<&str, Vec<SharedRow<'_>>>::new();
    for (owner, kind_id, key_hash, ordinal) in owner_rows {
        let kind = match kind_id {
            0 => "symbol",
            1 => "reference",
            2 => "default_export",
            _ => anyhow::bail!("invalid projected shared-owner kind {kind_id}"),
        };
        let key = key_by_hash
            .get(&key_hash)
            .with_context(|| format!("missing projected shared key {key_hash}"))?;
        let key = *key;
        let node_id = if kind_id == 0 {
            Some(
                *id_by_node_id
                    .get(&ordinal)
                    .with_context(|| format!("missing projected shared symbol node {ordinal}"))?,
            )
        } else {
            None
        };
        by_owner.entry(owner).or_default().push(SharedRow {
            kind,
            key,
            ordinal,
            node_id,
        });
    }

    let mut leaves = Leaves::new();
    for (owner, mut rows) in by_owner {
        rows.sort_unstable_by(|left, right| {
            left.kind
                .cmp(right.kind)
                .then_with(|| left.key.cmp(right.key))
                .then_with(|| left.node_id.cmp(&right.node_id))
                .then_with(|| left.ordinal.cmp(&right.ordinal))
        });
        let mut hasher = new_leaf_hasher("shared_owners", owner);
        for row in rows {
            hasher.update([0xff]);
            encode_text(&mut hasher, row.kind);
            encode_text(&mut hasher, row.key);
            encode_text(&mut hasher, owner);
            if let Some(node_id) = row.node_id {
                encode_text(&mut hasher, node_id);
            } else {
                encode_integer(&mut hasher, row.ordinal);
            }
        }
        leaves.insert(
            leaf_key("shared_owners", owner)?,
            hex::encode(hasher.finalize()),
        );
    }
    Ok(leaves)
}

fn register_shared_key<'a>(keys: &mut HashMap<i64, &'a str>, key: &'a str) -> Result<i64> {
    let hash = stable_hash64(key);
    if let Some(existing) = keys.get(&hash) {
        ensure!(*existing == key, "shared key hash collision detected");
    } else {
        keys.insert(hash, key);
    }
    Ok(hash)
}

type CoverageDomains = (Leaves, Leaves, Leaves);
type HtmlOwners = HashMap<String, HashMap<usize, HashMap<String, Option<String>>>>;

fn project_coverage_domains<'a>(
    entries: &[FileEntry],
    facts: &BTreeMap<String, TypedFacts>,
    graph: &'a CompactGraph,
) -> Result<(CoverageDomains, Vec<Dependency<'a>>)> {
    let mut coverage_rows: Vec<_> = graph.coverage.iter().collect();
    coverage_rows.sort_unstable_by(|left, right| compare_coverage(left, right));
    coverage_rows.dedup_by(|right, left| compare_coverage(left, right).is_eq());

    let mut file_languages = HashMap::<String, String>::new();
    let mut html_owners = HtmlOwners::new();
    let mut html_islands = HashMap::<String, Vec<(usize, usize)>>::new();
    for (path, file_facts) in facts {
        if let Some(module) = file_facts.nodes.iter().find(|node| node.kind == "module") {
            if !module.language.is_empty() {
                file_languages.insert(path.clone(), module.language.clone());
            }
        }
        if !file_facts
            .nodes
            .iter()
            .any(|node| node.kind == "module" && node.language == "html")
        {
            continue;
        }
        let node_languages: HashMap<_, _> = file_facts
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node.language.as_str()))
            .collect();
        let islands: Vec<_> = file_facts
            .nodes
            .iter()
            .filter(|node| node.kind == "island_scope" && node.language == "javascript")
            .map(|node| (node.line, node.end_line))
            .collect();
        let mut owners = HashMap::<usize, HashMap<String, Option<String>>>::new();
        for reference in &file_facts.references {
            let Some(language) = node_languages.get(reference.source.as_str()).copied() else {
                continue;
            };
            let entry = owners
                .entry(reference.line)
                .or_default()
                .entry(reference.expression.clone())
                .or_insert_with(|| Some(language.to_owned()));
            if entry.as_deref() != Some(language) {
                *entry = None;
            }
        }
        owners.retain(|line, expressions| {
            let whole_line_js = islands
                .iter()
                .any(|(first, last)| first <= line && line <= last);
            expressions.retain(|_, language| match language.as_deref() {
                Some("javascript") => !whole_line_js,
                Some("html") | None => whole_line_js,
                _ => false,
            });
            !expressions.is_empty()
        });
        if !owners.is_empty() {
            html_owners.insert(path.clone(), owners);
        }
        if !islands.is_empty() {
            html_islands.insert(path.clone(), islands);
        }
    }
    let entry_languages: HashMap<_, _> = entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry.language.as_str()))
        .collect();

    let mut coverage_hashers = OwnerHashers::new();
    let mut owner_language_rows = BTreeMap::<String, Vec<OwnerLanguageLeafRow<'_>>>::new();
    let mut language_counts = BTreeMap::<String, BTreeMap<(String, String), i64>>::new();
    let mut unresolved_dependencies = Vec::new();
    for coverage in &coverage_rows {
        let owner = coverage.path.as_str();
        let line = i64::try_from(coverage.line).context("coverage line exceeds SQLite range")?;
        let expression = coverage.expression.as_str();
        let status = coverage.status.as_str();
        let evidence = coverage.evidence.as_ref();
        let hasher = row_hasher(&mut coverage_hashers, "resolution_coverage", owner);
        encode_text(hasher, owner);
        encode_integer(hasher, line);
        encode_text(hasher, expression);
        encode_text(hasher, status);
        encode_text(hasher, evidence);

        let html_language = html_owners
            .get(owner)
            .and_then(|owners| owners.get(&coverage.line))
            .and_then(|expressions| expressions.get(expression))
            .map(|language| language.as_deref().unwrap_or("html"));
        if let Some(language) = html_language {
            owner_language_rows
                .entry(owner.to_owned())
                .or_default()
                .push(OwnerLanguageLeafRow {
                    line,
                    expression,
                    status,
                    evidence,
                    language,
                });
        }

        let base_language = file_languages
            .get(owner)
            .map(String::as_str)
            .or_else(|| entry_languages.get(owner).copied())
            .filter(|language| !language.is_empty());
        let language = if let Some(language) = html_language {
            Some(language)
        } else if let Some(islands) = html_islands.get(owner) {
            if islands
                .iter()
                .any(|(first, last)| *first <= coverage.line && coverage.line <= *last)
            {
                Some("javascript")
            } else {
                base_language.or(Some("html"))
            }
        } else {
            base_language
        };
        if let Some(language) = language.filter(|language| !language.is_empty()) {
            let count = language_counts
                .entry(owner.to_owned())
                .or_default()
                .entry((language.to_owned(), status.to_owned()))
                .or_default();
            *count = count.checked_add(1).context("coverage count overflow")?;
        }
        if status != "resolved" {
            unresolved_dependencies.push(Dependency {
                owner,
                target_hash: None,
                kind: "unresolved",
                symbol: expression,
                resolution: status,
            });
        }
    }

    let mut owner_language = Leaves::new();
    for (owner, mut rows) in owner_language_rows {
        rows.sort_unstable_by(|left, right| {
            left.line
                .cmp(&right.line)
                .then_with(|| left.expression.as_bytes().cmp(right.expression.as_bytes()))
                .then_with(|| left.status.as_bytes().cmp(right.status.as_bytes()))
                .then_with(|| left.evidence.as_bytes().cmp(right.evidence.as_bytes()))
                .then_with(|| left.language.as_bytes().cmp(right.language.as_bytes()))
        });
        let mut hasher = new_leaf_hasher("coverage_owner_language", &owner);
        for row in rows {
            hasher.update([0xff]);
            encode_text(&mut hasher, &owner);
            encode_integer(&mut hasher, row.line);
            encode_text(&mut hasher, row.expression);
            encode_text(&mut hasher, row.status);
            encode_text(&mut hasher, row.evidence);
            encode_text(&mut hasher, row.language);
        }
        owner_language.insert(
            leaf_key("coverage_owner_language", &owner)?,
            hex::encode(hasher.finalize()),
        );
    }

    let mut language_count_leaves = Leaves::new();
    for (owner, counts) in language_counts {
        let mut hasher = new_leaf_hasher("coverage_language_counts", &owner);
        for ((language, status), count) in counts {
            hasher.update([0xff]);
            encode_text(&mut hasher, &owner);
            encode_text(&mut hasher, &language);
            encode_text(&mut hasher, &status);
            encode_integer(&mut hasher, count);
        }
        language_count_leaves.insert(
            leaf_key("coverage_language_counts", &owner)?,
            hex::encode(hasher.finalize()),
        );
    }

    Ok((
        (
            finish_hashers("resolution_coverage", coverage_hashers)?,
            owner_language,
            language_count_leaves,
        ),
        unresolved_dependencies,
    ))
}

fn compare_coverage(left: &Coverage, right: &Coverage) -> std::cmp::Ordering {
    left.path
        .as_bytes()
        .cmp(right.path.as_bytes())
        .then_with(|| left.line.cmp(&right.line))
        .then_with(|| left.expression.as_bytes().cmp(right.expression.as_bytes()))
        .then_with(|| left.status.as_str().cmp(right.status.as_str()))
        .then_with(|| left.evidence.as_ref().cmp(right.evidence.as_ref()))
}

fn project_dependency_domain<'a>(
    facts: &'a BTreeMap<String, TypedFacts>,
    file_by_hash: &'a HashMap<i64, &'a str>,
    mut dependencies: Vec<Dependency<'a>>,
) -> Result<Leaves> {
    #[cfg(feature = "lang-html")]
    for (provider_path, file_facts) in facts {
        for node in file_facts.nodes.iter().filter(|node| {
            node.language == "python"
                && matches!(node.kind.as_str(), "module" | "function" | "method")
        }) {
            let Some(flow) = file_facts.flows.get(node) else {
                continue;
            };
            for context in flow.template_contexts() {
                let Some(target) =
                    crate::core::config::local_template_target(provider_path, &context.target)
                else {
                    continue;
                };
                if crate::core::config::workspace_path(provider_path).0
                    != crate::core::config::workspace_path(&target).0
                {
                    continue;
                }
                dependencies.push(Dependency {
                    owner: provider_path,
                    target_hash: Some(stable_hash64(&target)),
                    kind: "renders",
                    symbol: &context.target,
                    resolution: "resolved",
                });
            }
        }
    }
    #[cfg(not(feature = "lang-html"))]
    let _ = facts;

    dependencies.sort_unstable();
    dependencies.dedup();

    let mut by_owner = BTreeMap::<&str, Vec<DependencyLeafRow<'_>>>::new();
    for dependency in dependencies {
        let target = dependency
            .target_hash
            .and_then(|hash| file_by_hash.get(&hash).copied());
        by_owner
            .entry(dependency.owner)
            .or_default()
            .push(DependencyLeafRow {
                target,
                kind: dependency.kind,
                symbol: dependency.symbol,
                resolution: dependency.resolution,
            });
    }

    let mut leaves = Leaves::new();
    for (owner, mut rows) in by_owner {
        rows.sort_unstable_by(|left, right| {
            left.target
                .cmp(&right.target)
                .then_with(|| left.kind.cmp(right.kind))
                .then_with(|| left.symbol.cmp(right.symbol))
                .then_with(|| left.resolution.cmp(right.resolution))
        });
        let mut hasher = new_leaf_hasher("dependencies", owner);
        for row in rows {
            hasher.update([0xff]);
            encode_text(&mut hasher, owner);
            if let Some(target) = row.target {
                encode_text(&mut hasher, target);
            } else {
                hasher.update(b"n");
            }
            encode_text(&mut hasher, row.kind);
            encode_text(&mut hasher, row.symbol);
            encode_text(&mut hasher, row.resolution);
        }
        leaves.insert(
            leaf_key("dependencies", owner)?,
            hex::encode(hasher.finalize()),
        );
    }
    Ok(leaves)
}
