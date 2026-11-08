use super::scope::{path_is_in_scope, typed_source_nodes};
use super::*;
use anyhow::{bail, Result};
use rusqlite::{params, OptionalExtension};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::Path,
};

#[derive(Debug)]
pub(crate) struct SeamReference {
    pub(crate) path: String,
    pub(crate) symbol: Option<String>,
    pub(crate) line: Option<usize>,
}

fn parse_seam_reference(reference: &str) -> Option<SeamReference> {
    let reference = reference.trim();
    let (without_line, line) = match reference.split_once('#') {
        Some((reference, suffix)) => {
            let line = suffix.strip_prefix('L').unwrap_or(suffix).parse().ok()?;
            (reference, Some(line))
        }
        None => (reference, None),
    };
    let (path, symbol) = match without_line.split_once("::") {
        Some((path, symbol)) if !symbol.trim().is_empty() => (path, Some(symbol.trim().to_owned())),
        Some(_) => return None,
        None => (without_line, None),
    };
    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return None;
    }
    Some(SeamReference {
        path: path.to_string_lossy().replace('\\', "/"),
        symbol,
        line,
    })
}

fn seam_symbol_matches(node: &forge_core::models::Node, symbol: &str) -> bool {
    let normalized = symbol.replace("::", ".");
    if symbol.contains("::") || symbol.contains('.') {
        node.qualname == normalized
    } else {
        node.name == symbol
    }
}

fn indexed_test_hashes(
    connection: &rusqlite::Connection,
    reference: &SeamReference,
    nodes: &[forge_core::models::Node],
) -> Result<Vec<i64>> {
    let mut hashes = BTreeSet::new();
    let mut statement = connection.prepare(
        "SELECT node.node_hash FROM nodes node \
         JOIN path_dictionary path ON path.path_id=node.path_id \
         WHERE path.path=?1 AND node.kind=?2 AND node.name=?3 \
           AND node.qualname=?4 AND node.is_test=1",
    )?;
    for node in nodes {
        let hash = statement
            .query_row(
                params![reference.path, node.kind, node.name, node.qualname],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if let Some(hash) = hash {
            hashes.insert(hash);
        }
    }
    Ok(hashes.into_iter().collect())
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SeamNodeIdentity {
    kind: String,
    name: String,
    qualname: String,
    is_test: bool,
}

impl From<&forge_core::models::Node> for SeamNodeIdentity {
    fn from(node: &forge_core::models::Node) -> Self {
        Self {
            kind: node.kind.clone(),
            name: node.name.clone(),
            qualname: node.qualname.clone(),
            is_test: node.is_test,
        }
    }
}

#[derive(Clone, Debug)]
struct IndexedSeamEdge {
    dst_hash: i64,
    kind: String,
    line: usize,
    evidence: String,
    src_path: String,
    src: SeamNodeIdentity,
    dst_path: String,
    dst: SeamNodeIdentity,
}

const SEAM_EDGE_KINDS: [&str; 5] = [
    "calls",
    "imports",
    "possible_calls",
    "implements",
    "overrides",
];
const MAX_SEAM_HOPS: usize = 8;

fn reference_source_identities(
    facts: &forge_core::typed_facts::TypedFacts,
) -> BTreeMap<String, SeamNodeIdentity> {
    facts
        .facts
        .nodes
        .iter()
        .map(|node| (node.id.clone(), SeamNodeIdentity::from(node)))
        .collect()
}

fn seam_reference_key(
    reference: &forge_core::models::Reference,
    source: &SeamNodeIdentity,
) -> Option<String> {
    let mut value = serde_json::to_value(reference).ok()?;
    let object = value.as_object_mut()?;
    object.insert(
        "source".into(),
        serde_json::json!([source.kind, source.name, source.qualname, source.is_test]),
    );
    object.remove("line");
    object.remove("column");
    Some(value.to_string())
}

fn seam_reference_tokens(expression: &str) -> BTreeSet<String> {
    expression
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

fn import_binding_name(reference: &forge_core::models::Reference) -> Option<String> {
    reference.alias.clone().or_else(|| {
        reference
            .expression
            .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .rfind(|token| !token.is_empty())
            .map(str::to_owned)
    })
}

fn seam_references_for_node(
    facts: &forge_core::typed_facts::TypedFacts,
    identity: &SeamNodeIdentity,
) -> BTreeSet<String> {
    let sources = reference_source_identities(facts);
    facts
        .facts
        .references
        .iter()
        .filter(|reference| SEAM_EDGE_KINDS.contains(&reference.kind.as_str()))
        .filter_map(|reference| {
            let source = sources.get(&reference.source)?;
            (source == identity)
                .then(|| seam_reference_key(reference, source))
                .flatten()
        })
        .collect()
}

fn seam_import_references_for_bindings(
    facts: &forge_core::typed_facts::TypedFacts,
    bindings: &BTreeSet<String>,
) -> BTreeSet<String> {
    if bindings.is_empty() {
        return BTreeSet::new();
    }
    let sources = reference_source_identities(facts);
    facts
        .facts
        .references
        .iter()
        .filter(|reference| reference.kind == "imports")
        .filter(|reference| {
            import_binding_name(reference).is_some_and(|name| bindings.contains(&name))
        })
        .filter_map(|reference| seam_reference_key(reference, sources.get(&reference.source)?))
        .collect()
}

fn seam_bindings_for_node(
    facts: &forge_core::typed_facts::TypedFacts,
    identity: &SeamNodeIdentity,
    target_name: &str,
) -> BTreeSet<String> {
    let mut bindings = BTreeSet::from([target_name.to_owned()]);
    let sources = reference_source_identities(facts);
    for reference in facts
        .facts
        .references
        .iter()
        .filter(|reference| SEAM_EDGE_KINDS.contains(&reference.kind.as_str()))
        .filter(|reference| sources.get(&reference.source) == Some(identity))
    {
        bindings.extend(seam_reference_tokens(&reference.expression));
    }
    bindings
}

fn indexed_local_facts(
    connection: &rusqlite::Connection,
    path: &str,
) -> Result<Option<forge_core::typed_facts::TypedFacts>> {
    let blob = connection
        .query_row(
            "SELECT facts_blob FROM local_facts WHERE path=?1",
            [path],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    blob.map(|blob| forge_core::typed_facts::decode_facts(&blob))
        .transpose()
}

pub(crate) fn current_local_facts(
    worktree: &Path,
    path: &str,
) -> Result<Option<forge_core::typed_facts::TypedFacts>> {
    let Ok(path) = super::super::confined_path(worktree, path) else {
        return Ok(None);
    };
    let Ok(source) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let relative_path = path
        .strip_prefix(worktree)
        .ok()
        .and_then(Path::to_str)
        .map(|path| path.replace('\\', "/"));
    let Some(relative_path) = relative_path else {
        return Ok(None);
    };
    Ok(extract_typed(&relative_path, &source).ok().flatten())
}

fn file_has_seam_node(
    facts: &forge_core::typed_facts::TypedFacts,
    identity: &SeamNodeIdentity,
) -> bool {
    facts
        .facts
        .nodes
        .iter()
        .any(|node| SeamNodeIdentity::from(node) == *identity)
}

fn edge_is_named_by_current_facts(
    facts: &forge_core::typed_facts::TypedFacts,
    source: &SeamNodeIdentity,
    target: &SeamNodeIdentity,
    kind: &str,
) -> bool {
    let identities = reference_source_identities(facts);
    if facts.facts.edges.iter().any(|edge| {
        identities.get(&edge.src) == Some(source)
            && identities.get(&edge.dst) == Some(target)
            && edge.kind == kind
    }) {
        return true;
    }

    let source_references = facts
        .facts
        .references
        .iter()
        .filter(|reference| identities.get(&reference.source) == Some(source));
    let target_name = &target.name;
    let target_token = |reference: &forge_core::models::Reference| {
        seam_reference_tokens(&reference.expression).contains(target_name)
    };
    match kind {
        "calls" | "possible_calls" => source_references.into_iter().any(|reference| {
            matches!(reference.kind.as_str(), "calls" | "possible_calls") && target_token(reference)
        }),
        "imports" => source_references
            .into_iter()
            .any(|reference| reference.kind == "imports" && target_token(reference)),
        "implements" | "overrides" => source_references
            .into_iter()
            .any(|reference| reference.kind == kind && target_token(reference)),
        _ => false,
    }
}

fn indexed_seam_edges(
    connection: &rusqlite::Connection,
    source_hash: i64,
) -> Result<Vec<IndexedSeamEdge>> {
    let mut statement = connection.prepare(
        "SELECT edge.dst_hash, edge.kind, occurrence.line, evidence.evidence, source_path.path, source.kind, source.name, source.qualname, source.is_test, \
                target_path.path, target.kind, target.name, target.qualname, target.is_test \
         FROM edges edge \
         JOIN edge_occurrences occurrence ON occurrence.src_hash=edge.src_hash \
           AND occurrence.dst_hash=edge.dst_hash AND occurrence.kind=edge.kind \
         JOIN coverage_evidence evidence ON evidence.evidence_id=occurrence.evidence_id \
         JOIN nodes source ON source.node_hash=edge.src_hash \
         JOIN path_dictionary source_path ON source_path.path_id=source.path_id \
         JOIN nodes target ON target.node_hash=edge.dst_hash \
         JOIN path_dictionary target_path ON target_path.path_id=target.path_id \
         WHERE edge.src_hash=?1 AND edge.kind IN ('calls','imports') \
         ORDER BY edge.kind, target_path.path, target.qualname, occurrence.line, evidence.evidence",
    )?;
    let edges = statement
        .query_map([source_hash], |row| {
            Ok(IndexedSeamEdge {
                dst_hash: row.get(0)?,
                kind: row.get(1)?,
                line: row.get(2)?,
                evidence: row.get(3)?,
                src_path: row.get(4)?,
                src: SeamNodeIdentity {
                    kind: row.get(5)?,
                    name: row.get(6)?,
                    qualname: row.get(7)?,
                    is_test: row.get(8)?,
                },
                dst_path: row.get(9)?,
                dst: SeamNodeIdentity {
                    kind: row.get(10)?,
                    name: row.get(11)?,
                    qualname: row.get(12)?,
                    is_test: row.get(13)?,
                },
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(anyhow::Error::from)?;
    Ok(edges)
}

fn indexed_edge_is_current(
    connection: &rusqlite::Connection,
    worktree: &Path,
    edge: &IndexedSeamEdge,
) -> Result<bool> {
    let Some(indexed_source) = indexed_local_facts(connection, &edge.src_path)? else {
        return Ok(false);
    };
    let Some(current_source) = current_local_facts(worktree, &edge.src_path)? else {
        return Ok(false);
    };
    let Some(current_target) = current_local_facts(worktree, &edge.dst_path)? else {
        return Ok(false);
    };
    if !file_has_seam_node(&indexed_source, &edge.src)
        || !file_has_seam_node(&current_source, &edge.src)
        || !file_has_seam_node(&current_target, &edge.dst)
    {
        return Ok(false);
    }

    if seam_references_for_node(&indexed_source, &edge.src)
        != seam_references_for_node(&current_source, &edge.src)
    {
        return Ok(false);
    }
    let mut bindings = seam_bindings_for_node(&indexed_source, &edge.src, &edge.dst.name);
    bindings.extend(seam_bindings_for_node(
        &current_source,
        &edge.src,
        &edge.dst.name,
    ));
    if seam_import_references_for_bindings(&indexed_source, &bindings)
        != seam_import_references_for_bindings(&current_source, &bindings)
    {
        return Ok(false);
    }
    if edge.kind == "imports" && edge.dst.kind == "module" {
        let module = current_source
            .facts
            .nodes
            .iter()
            .find(|node| node.kind == "module" && SeamNodeIdentity::from(*node) == edge.src);
        let Some(module) = module else {
            return Ok(false);
        };
        return Ok(current_source.facts.references.iter().any(|reference| {
            reference.source == module.id
                && reference.kind == "imports"
                && reference.line == edge.line
                && reference.expression == edge.evidence
        }));
    }
    Ok(edge_is_named_by_current_facts(
        &current_source,
        &edge.src,
        &edge.dst,
        &edge.kind,
    ))
}

fn module_identity(facts: &forge_core::typed_facts::TypedFacts) -> Option<SeamNodeIdentity> {
    facts
        .facts
        .nodes
        .iter()
        .find(|node| node.kind == "module")
        .map(SeamNodeIdentity::from)
}

fn module_import_references<'a>(
    facts: &'a forge_core::typed_facts::TypedFacts,
    module: &SeamNodeIdentity,
    binding: &str,
) -> Vec<&'a forge_core::models::Reference> {
    let sources = reference_source_identities(facts);
    facts
        .facts
        .references
        .iter()
        .filter(|reference| reference.kind == "imports")
        .filter(|reference| sources.get(&reference.source) == Some(module))
        .filter(|reference| import_binding_name(reference).as_deref() == Some(binding))
        .collect()
}

fn indexed_module_import_edge(
    connection: &rusqlite::Connection,
    module_path: &str,
    reference: &forge_core::models::Reference,
) -> Result<Option<IndexedSeamEdge>> {
    let module_hash: Option<i64> = connection
        .query_row(
            "SELECT node.node_hash FROM nodes node \
             JOIN path_dictionary path ON path.path_id=node.path_id \
             WHERE path.path=?1 AND node.kind='module' ORDER BY node.qualname LIMIT 1",
            [module_path],
            |row| row.get(0),
        )
        .optional()?;
    let Some(module_hash) = module_hash else {
        return Ok(None);
    };
    let mut matches = indexed_seam_edges(connection, module_hash)?
        .into_iter()
        .filter(|edge| {
            edge.kind == "imports"
                && edge.dst.kind == "module"
                && edge.line == reference.line
                && edge.evidence == reference.expression
        })
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        Ok(matches.pop())
    } else {
        Ok(None)
    }
}

fn call_binding_names(
    facts: &forge_core::typed_facts::TypedFacts,
    function: &SeamNodeIdentity,
) -> BTreeSet<String> {
    let sources = reference_source_identities(facts);
    facts
        .facts
        .references
        .iter()
        .filter(|reference| matches!(reference.kind.as_str(), "calls" | "unresolved"))
        .filter(|reference| sources.get(&reference.source) == Some(function))
        .filter_map(|reference| {
            let expression = reference.expression.trim();
            (!expression.is_empty()
                && expression
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_'))
            .then(|| expression.to_owned())
        })
        .collect()
}

fn imported_symbol_name(reference: &forge_core::models::Reference) -> Option<String> {
    reference
        .expression
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .rfind(|token| !token.is_empty())
        .map(str::to_owned)
}

fn unrestricted_public_export(
    target_module_facts: &forge_core::typed_facts::TypedFacts,
    imported_name: &str,
    reference: &forge_core::models::Reference,
) -> bool {
    if target_module_facts.facts.nodes.iter().any(|node| {
        node.line == reference.line
            && (node.details["is_public"].as_bool() == Some(true)
                || node.details["visibility"].as_str() == Some("public")
                || matches!(node.kind.as_str(), "export" | "reexport"))
    }) {
        return true;
    }
    if target_module_facts
        .facts
        .references
        .iter()
        .any(|r| r.line == reference.line && r.kind == "exports")
    {
        return true;
    }
    let is_exported = target_module_facts
        .facts
        .nodes
        .iter()
        .filter(|node| node.kind == "module")
        .any(|node| {
            node.details["exports"].as_array().is_some_and(|exports| {
                exports.iter().any(|export| {
                    (export["name"].as_str() == Some(imported_name)
                        || export["local"].as_str() == Some(imported_name))
                        && export["type_only"] != serde_json::Value::Bool(true)
                })
            })
        });
    if !is_exported {
        return false;
    }
    let is_restricted = target_module_facts
        .facts
        .nodes
        .iter()
        .filter(|node| node.kind == "module")
        .any(|node| {
            node.details["rust_restricted_exports"]
                .as_array()
                .is_some_and(|restricted| {
                    restricted
                        .iter()
                        .any(|item| item["name"].as_str() == Some(imported_name))
                })
        });
    !is_restricted
}

fn current_public_callable(
    worktree: &Path,
    path: &str,
    name: &str,
) -> Result<Option<SeamNodeIdentity>> {
    let Some(facts) = current_local_facts(worktree, path)? else {
        return Ok(None);
    };
    let candidates = facts
        .facts
        .nodes
        .iter()
        .filter(|node| {
            matches!(node.kind.as_str(), "function" | "method")
                && !node.is_test
                && node.name == name
                && (node.details["is_public"].as_bool() == Some(true)
                    || node.details["visibility"].as_str() == Some("public"))
        })
        .map(SeamNodeIdentity::from)
        .collect::<Vec<_>>();
    if candidates.len() == 1 {
        Ok(candidates.into_iter().next())
    } else {
        Ok(None)
    }
}

fn current_import_call_path(
    connection: &rusqlite::Connection,
    worktree: &Path,
    source_path: &str,
    function: &SeamNodeIdentity,
    max_hops: usize,
    scope: &[String],
) -> Result<Option<Vec<IndexedSeamEdge>>> {
    let Some(indexed_source) = indexed_local_facts(connection, source_path)? else {
        return Ok(None);
    };
    let Some(current_source) = current_local_facts(worktree, source_path)? else {
        return Ok(None);
    };
    if !file_has_seam_node(&indexed_source, function)
        || !file_has_seam_node(&current_source, function)
    {
        return Ok(None);
    }
    let indexed_calls = call_binding_names(&indexed_source, function);
    let current_calls = call_binding_names(&current_source, function);
    let Some(module) = module_identity(&current_source) else {
        return Ok(None);
    };
    let Some(indexed_module) = module_identity(&indexed_source) else {
        return Ok(None);
    };
    if module != indexed_module {
        return Ok(None);
    }
    let mut routes = Vec::new();
    for binding in current_calls.intersection(&indexed_calls) {
        let current_imports = module_import_references(&current_source, &module, binding);
        let indexed_imports = module_import_references(&indexed_source, &indexed_module, binding);
        if current_imports.len() != 1
            || indexed_imports.len() != 1
            || seam_reference_key(current_imports[0], &module)
                != seam_reference_key(indexed_imports[0], &indexed_module)
        {
            continue;
        }
        let Some(mut edge) =
            indexed_module_import_edge(connection, source_path, current_imports[0])?
        else {
            continue;
        };
        if !indexed_edge_is_current(connection, worktree, &edge)? {
            continue;
        }
        if edge.dst.is_test {
            continue;
        }
        let Some(mut imported_name) = imported_symbol_name(current_imports[0]) else {
            continue;
        };
        let mut path = Vec::new();
        let mut visited = BTreeSet::new();
        for hop in 0..max_hops {
            if !visited.insert((edge.dst_path.clone(), imported_name.clone())) {
                break;
            }
            let target_path = edge.dst_path.clone();
            let target_module_facts = current_local_facts(worktree, &target_path)?;
            let Some(target_module_facts) = target_module_facts else {
                break;
            };
            if !target_module_facts
                .facts
                .nodes
                .iter()
                .any(|node| node.kind == "module")
            {
                break;
            }
            let indexed_target_facts = indexed_local_facts(connection, &target_path)?;
            let Some(indexed_target_facts) = indexed_target_facts else {
                break;
            };
            if module_identity(&target_module_facts) != module_identity(&indexed_target_facts) {
                break;
            }
            path.push(edge.clone());

            if path_is_in_scope(&target_path, scope) {
                if let Some(target) =
                    current_public_callable(worktree, &target_path, &imported_name)?
                {
                    if indexed_target_facts
                        .facts
                        .nodes
                        .iter()
                        .any(|node| SeamNodeIdentity::from(node) == target && !node.is_test)
                    {
                        routes.push(path);
                        break;
                    }
                }
            }

            if hop + 1 >= max_hops {
                break;
            }
            let Some(target_module) = module_identity(&target_module_facts) else {
                break;
            };
            let current_reexports =
                module_import_references(&target_module_facts, &target_module, &imported_name);
            let indexed_reexports =
                module_import_references(&indexed_target_facts, &target_module, &imported_name);
            if current_reexports.len() != 1
                || indexed_reexports.len() != 1
                || seam_reference_key(current_reexports[0], &target_module)
                    != seam_reference_key(indexed_reexports[0], &target_module)
                || !unrestricted_public_export(
                    &target_module_facts,
                    &imported_name,
                    current_reexports[0],
                )
            {
                break;
            }
            let Some(next_edge) =
                indexed_module_import_edge(connection, &target_path, current_reexports[0])?
            else {
                break;
            };
            if !indexed_edge_is_current(connection, worktree, &next_edge)? || next_edge.dst.is_test
            {
                break;
            }
            imported_name = imported_symbol_name(current_reexports[0]).unwrap_or_default();
            if imported_name.is_empty() {
                break;
            }
            edge = next_edge;
        }
    }
    if routes.len() == 1 {
        Ok(routes.pop())
    } else {
        Ok(None)
    }
}

fn missing_scope_files(worktree: &Path, scope: &[String]) -> Vec<String> {
    scope
        .iter()
        .filter(|relative| !worktree.join(relative).exists())
        .cloned()
        .collect()
}

fn seam_node_label(path: &str, node: &SeamNodeIdentity) -> String {
    format!("{path}::{}", node.qualname)
}

fn seam_path_label(edges: &[IndexedSeamEdge]) -> String {
    edges
        .iter()
        .map(|edge| {
            format!(
                "{} -{}-> {}",
                seam_node_label(&edge.src_path, &edge.src),
                edge.kind,
                seam_node_label(&edge.dst_path, &edge.dst)
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn indexed_seam_node(
    connection: &rusqlite::Connection,
    node_hash: i64,
) -> Result<Option<(String, SeamNodeIdentity)>> {
    connection
        .query_row(
            "SELECT path.path, node.kind, node.name, node.qualname, node.is_test \
             FROM nodes node JOIN path_dictionary path ON path.path_id=node.path_id \
             WHERE node.node_hash=?1",
            [node_hash],
            |row| {
                Ok((
                    row.get(0)?,
                    SeamNodeIdentity {
                        kind: row.get(1)?,
                        name: row.get(2)?,
                        qualname: row.get(3)?,
                        is_test: row.get(4)?,
                    },
                ))
            },
        )
        .optional()
        .map_err(anyhow::Error::from)
}

fn indexed_path_to_scope(
    connection: &rusqlite::Connection,
    source_hash: i64,
    max_hops: usize,
    scope: &[String],
) -> Result<Option<Vec<IndexedSeamEdge>>> {
    let mut queue = VecDeque::from([(source_hash, Vec::<IndexedSeamEdge>::new())]);
    let mut visited = BTreeSet::from([source_hash]);
    while let Some((node_hash, path)) = queue.pop_front() {
        if path.len() >= max_hops {
            continue;
        }
        for edge in indexed_seam_edges(connection, node_hash)? {
            let mut next_path = path.clone();
            next_path.push(edge.clone());
            if !edge.dst.is_test && path_is_in_scope(&edge.dst_path, scope) {
                return Ok(Some(next_path));
            }
            if visited.insert(edge.dst_hash) {
                queue.push_back((edge.dst_hash, next_path));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
#[test]
fn missing_scope_files_contract() {
    let root = std::env::temp_dir().join(format!(
        "forge-missing-scope-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("present.rs"), "").unwrap();
    let scope = vec![
        "present.rs".into(),
        "new.rs".into(),
        "nested/module.rs".into(),
    ];
    let missing = missing_scope_files(&root, &scope);
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(missing, ["new.rs", "nested/module.rs"]);
}

pub(crate) fn validate_seam_file_and_symbol(
    task_id: &str,
    worktree: &Path,
    reference_str: &str,
) -> Result<(SeamReference, Vec<forge_core::models::Node>)> {
    let Some(reference) = parse_seam_reference(reference_str) else {
        bail!("TASK_SEAM_DISCONNECTED: {task_id}. seam_test_ref '{reference_str}' is not a confined indexed test path. Do: submit an indexed test file or symbol connected to the admitted production scope.");
    };
    let source_path = super::super::confined_path(worktree, &reference.path)
        .map_err(|_| anyhow::anyhow!("TASK_SEAM_DISCONNECTED: {task_id}. seam_test_ref '{}' is not present in the claimed worktree.", reference.path))?;
    if !source_path.is_file() {
        bail!("TASK_SEAM_DISCONNECTED: {task_id}. seam_test_ref '{}' is not present in the claimed worktree.", reference.path);
    }
    let source = std::fs::read_to_string(&source_path).map_err(|_| {
        anyhow::anyhow!("TASK_SEAM_DISCONNECTED: {task_id}. seam_test_ref '{}' is not readable in the claimed worktree.", reference.path)
    })?;
    let nodes = typed_source_nodes(&reference.path, &source).map_err(|_| {
        anyhow::anyhow!("TASK_SEAM_DISCONNECTED: {task_id}. seam_test_ref '{}' cannot be parsed as a supported test file.", reference.path)
    })?;
    let test_nodes = nodes
        .into_iter()
        .filter(|node| {
            node.is_test
                && matches!(node.kind.as_str(), "function" | "method")
                && reference.line.is_none_or(|line| node.line == line)
                && reference
                    .symbol
                    .as_deref()
                    .is_none_or(|symbol| seam_symbol_matches(node, symbol))
        })
        .collect::<Vec<_>>();
    if test_nodes.is_empty() {
        bail!("TASK_SEAM_DISCONNECTED: {task_id}. seam_test_ref '{}' does not identify a current test symbol in the claimed worktree. Do: submit a current indexed test file or exact test symbol.", reference.path);
    }
    if reference.symbol.is_some() && test_nodes.len() != 1 {
        bail!("TASK_SEAM_DISCONNECTED: {task_id}. seam_test_ref '{}' does not identify one unambiguous current test symbol in the claimed worktree. Do: use its exact qualified symbol or line.", reference.path);
    }
    Ok((reference, test_nodes))
}

pub(crate) fn validate_seam_reachability(
    connection: &rusqlite::Connection,
    task_id: &str,
    worktree: &Path,
    scope: &[String],
    reference_str: &str,
) -> Result<()> {
    let (reference, test_nodes) = validate_seam_file_and_symbol(task_id, worktree, reference_str)?;
    let local_connection = super::code_map_connection(Some(connection), worktree)?;
    let connection = local_connection.as_ref().unwrap_or(connection);
    let test_hashes = indexed_test_hashes(connection, &reference, &test_nodes)?;
    if test_hashes.is_empty() {
        let parsed_tests = test_nodes
            .iter()
            .map(|node| seam_node_label(&reference.path, &SeamNodeIdentity::from(node)))
            .collect::<Vec<_>>();
        bail!("TASK_SEAM_DISCONNECTED: {task_id}. seam_test_ref '{}' has no exact indexed test symbol. Calls discovered: none. Coverage statuses: parsed test symbols={}, indexed test symbols=0. Reached targets: [{}]. Stop reason: the index has no node matching the current test source. Do: run `contextunity-forge-mcp scan`, then submit the current test file or exact symbol.", reference.path, parsed_tests.len(), parsed_tests.join(", "));
    }

    let mut queue = VecDeque::new();
    let mut visited = BTreeSet::new();
    let mut reached = BTreeSet::new();
    let mut calls = BTreeSet::new();
    let mut stale_paths = BTreeSet::new();
    let mut current_edges = 0usize;
    let mut stale_edges = 0usize;
    for hash in test_hashes {
        for node in &test_nodes {
            reached.insert(seam_node_label(
                &reference.path,
                &SeamNodeIdentity::from(node),
            ));
        }
        visited.insert((hash, 0usize));
        queue.push_back((hash, 0usize, Vec::<IndexedSeamEdge>::new()));
    }

    while let Some((source_hash, depth, prefix)) = queue.pop_front() {
        if depth >= MAX_SEAM_HOPS {
            continue;
        }
        if let Some((source_path, source)) = indexed_seam_node(connection, source_hash)? {
            if matches!(source.kind.as_str(), "function" | "method") {
                let remaining = MAX_SEAM_HOPS.saturating_sub(depth);
                if let Some(import_path) = current_import_call_path(
                    connection,
                    worktree,
                    &source_path,
                    &source,
                    remaining,
                    scope,
                )? {
                    reached.extend(
                        import_path
                            .iter()
                            .map(|edge| seam_node_label(&edge.dst_path, &edge.dst)),
                    );
                    calls.insert(format!(
                        "{} -call/import chain-> {}",
                        seam_node_label(&source_path, &source),
                        seam_path_label(&import_path)
                    ));
                    return Ok(());
                }
            }
        }
        for edge in indexed_seam_edges(connection, source_hash)? {
            let edge_label = format!(
                "{} -{}-> {}",
                seam_node_label(&edge.src_path, &edge.src),
                edge.kind,
                seam_node_label(&edge.dst_path, &edge.dst)
            );
            calls.insert(edge_label);
            let mut path = prefix.clone();
            path.push(edge.clone());
            if !indexed_edge_is_current(connection, worktree, &edge)? {
                stale_edges += 1;
                let remaining = MAX_SEAM_HOPS.saturating_sub(depth + 1);
                let suffix = if !edge.dst.is_test && path_is_in_scope(&edge.dst_path, scope) {
                    Some(Vec::new())
                } else {
                    indexed_path_to_scope(connection, edge.dst_hash, remaining, scope)?
                };
                if let Some(suffix) = suffix {
                    let mut stale_path = path;
                    stale_path.extend(suffix);
                    stale_paths.insert(seam_path_label(&stale_path));
                }
                continue;
            }

            current_edges += 1;
            reached.insert(seam_node_label(&edge.dst_path, &edge.dst));
            if !edge.dst.is_test && path_is_in_scope(&edge.dst_path, scope) {
                return Ok(());
            }
            let next_depth = depth + 1;
            if visited.insert((edge.dst_hash, next_depth)) {
                queue.push_back((edge.dst_hash, next_depth, path));
            }
        }
    }

    let calls = if calls.is_empty() {
        "none".to_owned()
    } else {
        calls.into_iter().collect::<Vec<_>>().join("; ")
    };
    let reached = if reached.is_empty() {
        "none".to_owned()
    } else {
        reached.into_iter().collect::<Vec<_>>().join(", ")
    };
    let coverage = format!(
        "current indexed edges={current_edges}, stale indexed edges={stale_edges}, max hops={MAX_SEAM_HOPS}"
    );
    if !stale_paths.is_empty() {
        bail!("TASK_SEAM_STALE: {task_id}. indexed seam evidence for '{}' was invalidated by current source drift. Calls discovered: [{calls}]. Coverage statuses: {coverage}. Reached targets: [{reached}]. Stale indexed path(s): [{}]. Stop reason: no other current proven path reaches admitted production scope. Do: run `contextunity-forge-mcp scan` to refresh the code map, then resubmit contract proof.", reference.path, stale_paths.into_iter().collect::<Vec<_>>().join("; "));
    }

    let missing_scope = missing_scope_files(worktree, scope);
    let (stop_reason, action) = if missing_scope.is_empty() {
        (
            format!("no current proven call/import path reaches admitted production scope within {MAX_SEAM_HOPS} hops"),
            "refresh the claimed-worktree code map with `contextunity-forge-mcp scan`, then connect a current indexed test path to an admitted production target".to_owned(),
        )
    } else {
        (
            format!(
                "target scope files do not exist yet on disk ({})",
                missing_scope.join(", ")
            ),
            format!(
                "create minimal file scaffolding for {}; run `contextunity-forge-mcp build` to compile the seam",
                missing_scope.join(", ")
            ),
        )
    };
    bail!("TASK_SEAM_DISCONNECTED: {task_id}. seam_test_ref '{}' has no current proven call/import path into admitted production scope. Calls discovered: [{calls}]. Coverage statuses: {coverage}. Reached targets: [{reached}]. Stop reason: {stop_reason}. Do: {action}.", reference.path)
}
