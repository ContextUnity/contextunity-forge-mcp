use crate::core::tasks::{
    canonical_commit_from_prefix,
    gates::{
        review_rollup_summary, validate_durable_receipt_with_profile, validate_gate_proof, Evidence,
    },
    profile::ProofKind,
    Receipt, ReceiptRollup, SubtaskSpec,
};
use crate::db::tasks_store::{
    check_stage, clear_blackboard, end_claim, load, now, save, Task, TasksStore,
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

fn code_map_connection(
    _store: &TasksStore,
    worktree: &Path,
) -> Result<Option<rusqlite::Connection>> {
    let index_path = worktree.join(".forge/code-map.sqlite");
    if !index_path.is_file() {
        return Ok(None);
    }
    let connection = rusqlite::Connection::open_with_flags(
        &index_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .context("TASK_CODE_MAP_INVALID: indexed task boundary data is unavailable")?;
    let has_path_dict: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='path_dictionary')",
            [],
            |row| row.get(0),
        )
        .context("TASK_CODE_MAP_INVALID: unable to inspect indexed task boundary tables")?;
    if !has_path_dict {
        anyhow::bail!(
            "TASK_CODE_MAP_INVALID: table 'path_dictionary' is missing from indexed code map"
        );
    }
    Ok(Some(connection))
}

fn normalized_signature(signature: &str) -> String {
    signature.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_public_signature(signature: &str) -> bool {
    let signature = signature.trim_start();
    signature.starts_with("pub ") || signature.starts_with("pub(")
}

fn language_for_source_path(path: &str) -> Option<&'static str> {
    match Path::new(path).extension()?.to_str()? {
        "rs" => Some("rust"),
        "py" => Some("python"),
        "js" | "jsx" | "mjs" | "cjs" => Some("javascript"),
        "ts" | "tsx" => Some("typescript"),
        _ => None,
    }
}

fn module_export_names(nodes: &[crate::core::models::Node]) -> BTreeSet<String> {
    nodes
        .iter()
        .filter(|node| node.kind == "module")
        .filter_map(|node| node.details["exports"].as_array())
        .flatten()
        .filter(|export| {
            export["module"].is_null() && export["type_only"] != serde_json::Value::Bool(true)
        })
        .filter_map(|export| export["local"].as_str().map(str::to_owned))
        .collect()
}

fn public_trait_methods(nodes: &[crate::core::models::Node]) -> BTreeSet<String> {
    let public_traits = nodes
        .iter()
        .filter(|node| node.kind == "trait")
        .filter(|node| {
            node.details["signature"]
                .as_str()
                .is_some_and(is_public_signature)
        })
        .map(|node| node.qualname.as_str())
        .collect::<BTreeSet<_>>();
    nodes
        .iter()
        .filter(|node| node.kind == "method")
        .filter(|node| {
            node.qualname
                .rsplit_once('.')
                .is_some_and(|(owner, _)| public_traits.contains(owner))
        })
        .map(|node| node.qualname.clone())
        .collect()
}

fn exported_callable(
    node: &crate::core::models::Node,
    exports: &BTreeSet<String>,
    public_trait_methods: &BTreeSet<String>,
) -> bool {
    match node.language.as_str() {
        "rust" => {
            node.details["signature"]
                .as_str()
                .is_some_and(is_public_signature)
                || public_trait_methods.contains(&node.qualname)
        }
        "python" => {
            !node.name.starts_with('_')
                || exports.contains(&node.name)
                || node.qualname.rsplit_once('.').is_some_and(|(owner, _)| {
                    exports.contains(owner.rsplit('.').next().unwrap_or(owner))
                })
        }
        "javascript" | "typescript" => {
            node.details["default_export"] == serde_json::Value::Bool(true)
                || exports.contains(&node.name)
                || (node.kind == "method"
                    && node.qualname.rsplit_once('.').is_some_and(|(owner, _)| {
                        exports.contains(owner.rsplit('.').next().unwrap_or(owner))
                    }))
        }
        _ => false,
    }
}

fn typed_source_nodes(path: &str, source: &str) -> Result<Vec<crate::core::models::Node>> {
    let language = language_for_source_path(path)
        .context("TASK_CODE_MAP_INVALID: source language is unavailable")?;
    Ok(crate::engine::ast::extract_typed(path, language, source)?
        .facts
        .nodes)
}

fn public_callable_nodes(path: &str, source: &str) -> Result<Vec<crate::core::models::Node>> {
    let nodes = typed_source_nodes(path, source)?;
    let exports = module_export_names(&nodes);
    let public_trait_methods = public_trait_methods(&nodes);
    Ok(nodes
        .into_iter()
        .filter(|node| {
            matches!(node.kind.as_str(), "function" | "method")
                && !node.is_test
                && exported_callable(node, &exports, &public_trait_methods)
        })
        .collect())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BaselineBoundaryCallers {
    baseline_commit: String,
    entries: Vec<BaselineCallerEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BaselineCallerEntry {
    path: String,
    kind: String,
    name: String,
    qualname: String,
    caller_paths: BTreeSet<String>,
    #[serde(default)]
    target_present: bool,
}

fn accepted_boundary_callers(
    conn: &rusqlite::Connection,
    id: &str,
    profile: &crate::core::tasks::profile::GateProfile,
    gate_index: usize,
) -> Result<Option<BaselineBoundaryCallers>> {
    let Some(contract_gate) = profile.gates[..gate_index]
        .iter()
        .rev()
        .find(|gate| gate.proof.kind() == Some(ProofKind::Contract))
    else {
        return Ok(None);
    };
    let submission: Option<String> = conn
        .query_row(
            "SELECT submission.result FROM task_gates gate \
             JOIN task_submissions submission ON submission.task_id=gate.task_id AND submission.revision=gate.revision \
             WHERE gate.task_id=?1 AND gate.gate=?2 AND gate.state='passed' \
             ORDER BY gate.revision DESC LIMIT 1",
            params![id, contract_gate.id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(submission) = submission else {
        return Ok(None);
    };
    Ok(serde_json::from_str::<Submission>(&submission)?.boundary_callers)
}

fn baseline_scope_files(worktree: &Path, baseline: &str, scope: &[String]) -> Result<Vec<String>> {
    if scope.is_empty() {
        return Ok(Vec::new());
    }
    let mut args = vec!["ls-tree", "-r", "-z", "--name-only", baseline, "--"];
    args.extend(scope.iter().map(String::as_str));
    let output = crate::core::git::git_command(worktree, &args)?;
    if !output.status.success() {
        bail!("TASK_CANDIDATE_BASELINE_INVALID: could not enumerate baseline source files");
    }
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8(path.to_vec()).context("baseline path is not UTF-8"))
        .collect()
}

fn current_scope_files(worktree: &Path, scope: &[String]) -> Result<Vec<String>> {
    let output =
        crate::core::git::git_command(worktree, &["ls-files", "-co", "--exclude-standard", "-z"])?;
    if !output.status.success() {
        bail!("TASK_SCOPE_INCOMPLETE: could not enumerate current scoped source files");
    }
    let mut files = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8(path.to_vec()).context("current path is not UTF-8"))
        .collect::<Result<Vec<_>>>()?;
    files.retain(|path| path_is_in_scope(path, scope));
    files.sort();
    files.dedup();
    Ok(files)
}

fn baseline_source(worktree: &Path, baseline: &str, path: &str) -> Result<Option<String>> {
    let object = format!("{baseline}:{path}");
    let output = crate::core::git::git_command(worktree, &["show", &object])?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8(output.stdout).context(
        "TASK_CANDIDATE_BASELINE_INVALID: source is not UTF-8",
    )?))
}

fn indexed_caller_paths(
    connection: &rusqlite::Connection,
    symbol: &crate::core::models::Node,
) -> Result<Option<BTreeSet<String>>> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT caller_path.path FROM nodes target \
         JOIN path_dictionary target_path ON target_path.path_id=target.path_id \
         LEFT JOIN edges ON edges.dst_hash=target.node_hash \
           AND edges.kind IN ('calls','imports','possible_calls','implements','overrides') \
         LEFT JOIN nodes caller ON caller.node_hash=edges.src_hash AND caller.is_test=0 \
         LEFT JOIN path_dictionary caller_path ON caller_path.path_id=caller.path_id \
         WHERE target_path.path=?1 AND target.kind=?2 AND target.name=?3 AND target.qualname=?4 \
         ORDER BY caller_path.path",
    )?;
    let rows = statement.query_map(
        params![symbol.path, symbol.kind, symbol.name, symbol.qualname],
        |row| row.get::<_, Option<String>>(0),
    )?;
    let mut callers = None;
    for row in rows {
        let paths = callers.get_or_insert_with(BTreeSet::new);
        if let Some(path) = row? {
            paths.insert(path);
        }
    }
    Ok(callers)
}

fn capture_baseline_boundary_callers(
    connection: &rusqlite::Connection,
    worktree: &Path,
    scope: &[String],
    captured_baseline: Option<&str>,
) -> Result<BaselineBoundaryCallers> {
    let baseline = match captured_baseline {
        Some(captured) => captured.to_owned(),
        None => crate::core::git::git_stdout(
            worktree,
            &["rev-parse", "--verify", "HEAD"],
            "TASK_CANDIDATE_BASELINE_INVALID",
        )?,
    };
    let mut entries = Vec::new();
    for path in baseline_scope_files(worktree, &baseline, scope)? {
        if language_for_source_path(&path).is_none() {
            continue;
        }
        let Some(source) = baseline_source(worktree, &baseline, &path)? else {
            continue;
        };
        for symbol in public_callable_nodes(&path, &source).map_err(|error| {
            anyhow::anyhow!(
                "TASK_CANDIDATE_BASELINE_INVALID: could not parse '{path}' at {baseline}: {error}"
            )
        })? {
            entries.push(BaselineCallerEntry {
                path: symbol.path.clone(),
                kind: symbol.kind.clone(),
                name: symbol.name.clone(),
                qualname: symbol.qualname.clone(),
                caller_paths: indexed_caller_paths(connection, &symbol)?.with_context(|| {
                    format!("TASK_SCOPE_INCOMPLETE: baseline caller evidence unavailable for '{}' in '{}'. Do: restore the baseline symbol and refresh the index before accepting the contract.", symbol.name, symbol.path)
                })?,
                target_present: true,
            });
        }
    }
    Ok(BaselineBoundaryCallers {
        baseline_commit: baseline,
        entries,
    })
}

fn path_is_in_scope(path: &str, scope: &[String]) -> bool {
    scope.iter().any(|admitted| {
        if admitted.ends_with('/') {
            path.starts_with(admitted)
        } else {
            path == admitted
        }
    })
}

#[derive(Debug)]
struct SeamReference {
    path: String,
    symbol: Option<String>,
    line: Option<usize>,
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

fn seam_symbol_matches(node: &crate::core::models::Node, symbol: &str) -> bool {
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
    nodes: &[crate::core::models::Node],
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

impl From<&crate::core::models::Node> for SeamNodeIdentity {
    fn from(node: &crate::core::models::Node) -> Self {
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

fn reference_source_identities(
    facts: &crate::core::typed_facts::TypedFacts,
) -> BTreeMap<String, SeamNodeIdentity> {
    facts
        .facts
        .nodes
        .iter()
        .map(|node| (node.id.clone(), SeamNodeIdentity::from(node)))
        .collect()
}

fn seam_reference_key(
    reference: &crate::core::models::Reference,
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

fn import_binding_name(reference: &crate::core::models::Reference) -> Option<String> {
    reference.alias.clone().or_else(|| {
        reference
            .expression
            .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .rfind(|token| !token.is_empty())
            .map(str::to_owned)
    })
}

fn seam_references_for_node(
    facts: &crate::core::typed_facts::TypedFacts,
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
    facts: &crate::core::typed_facts::TypedFacts,
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
    facts: &crate::core::typed_facts::TypedFacts,
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
) -> Result<Option<crate::core::typed_facts::TypedFacts>> {
    let blob = connection
        .query_row(
            "SELECT facts_blob FROM local_facts WHERE path=?1",
            [path],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    blob.map(|blob| crate::core::typed_facts::decode_facts(&blob))
        .transpose()
}

fn current_local_facts(
    worktree: &Path,
    path: &str,
) -> Result<Option<crate::core::typed_facts::TypedFacts>> {
    let Ok(path) = super::confined_path(worktree, path) else {
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
    let Ok(language) = language_for_source_path(&relative_path)
        .context("TASK_CODE_MAP_INVALID: source language is unavailable")
    else {
        return Ok(None);
    };
    Ok(crate::engine::ast::extract_typed(&relative_path, language, &source).ok())
}

fn file_has_seam_node(
    facts: &crate::core::typed_facts::TypedFacts,
    identity: &SeamNodeIdentity,
) -> bool {
    facts
        .facts
        .nodes
        .iter()
        .any(|node| SeamNodeIdentity::from(node) == *identity)
}

fn edge_is_named_by_current_facts(
    facts: &crate::core::typed_facts::TypedFacts,
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
    let target_token = |reference: &crate::core::models::Reference| {
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
        "SELECT edge.dst_hash, edge.kind, source_path.path, source.kind, source.name, source.qualname, source.is_test, \
                target_path.path, target.kind, target.name, target.qualname, target.is_test \
         FROM edges edge \
         JOIN nodes source ON source.node_hash=edge.src_hash \
         JOIN path_dictionary source_path ON source_path.path_id=source.path_id \
         JOIN nodes target ON target.node_hash=edge.dst_hash \
         JOIN path_dictionary target_path ON target_path.path_id=target.path_id \
         WHERE edge.src_hash=?1 AND edge.kind IN ('calls','imports','possible_calls','implements','overrides') \
         ORDER BY edge.kind, target_path.path, target.qualname",
    )?;
    let edges = statement
        .query_map([source_hash], |row| {
            Ok(IndexedSeamEdge {
                dst_hash: row.get(0)?,
                kind: row.get(1)?,
                src_path: row.get(2)?,
                src: SeamNodeIdentity {
                    kind: row.get(3)?,
                    name: row.get(4)?,
                    qualname: row.get(5)?,
                    is_test: row.get(6)?,
                },
                dst_path: row.get(7)?,
                dst: SeamNodeIdentity {
                    kind: row.get(8)?,
                    name: row.get(9)?,
                    qualname: row.get(10)?,
                    is_test: row.get(11)?,
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
    Ok(edge_is_named_by_current_facts(
        &current_source,
        &edge.src,
        &edge.dst,
        &edge.kind,
    ))
}

fn missing_scope_files(worktree: &Path, scope: &[String]) -> Vec<String> {
    scope
        .iter()
        .filter(|relative| !worktree.join(relative).exists())
        .cloned()
        .collect()
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

fn validate_seam_reachability(
    connection: &rusqlite::Connection,
    task_id: &str,
    worktree: &Path,
    scope: &[String],
    reference: &str,
) -> Result<()> {
    let Some(reference) = parse_seam_reference(reference) else {
        bail!("TASK_SEAM_DISCONNECTED: {task_id}. seam_test_ref '{reference}' is not a confined indexed test path. Do: submit an indexed test file or symbol connected to the admitted production scope.");
    };
    let source_path = super::confined_path(worktree, &reference.path).map_err(|_| {
        anyhow::anyhow!("TASK_SEAM_DISCONNECTED: {task_id}. seam_test_ref '{}' is not present in the claimed worktree.", reference.path)
    })?;
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
    let local_index_path = worktree.join(".forge/code-map.sqlite");
    let local_connection = if local_index_path.is_file() {
        Some(
            rusqlite::Connection::open_with_flags(
                &local_index_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .context("TASK_CODE_MAP_INVALID: claimed-worktree code index is unreadable")?,
        )
    } else {
        None
    };
    let connection = local_connection.as_ref().unwrap_or(connection);
    let test_hashes = indexed_test_hashes(connection, &reference, &test_nodes)?;
    if test_hashes.is_empty() {
        bail!("TASK_SEAM_DISCONNECTED: {task_id}. seam_test_ref '{}' has no exact indexed test symbol. Do: refresh the index and submit the current test file or symbol.", reference.path);
    }

    let mut connected = false;
    for hash in test_hashes {
        for first in indexed_seam_edges(connection, hash)? {
            if !indexed_edge_is_current(connection, worktree, &first)? {
                continue;
            }
            if !first.dst.is_test && path_is_in_scope(&first.dst_path, scope) {
                connected = true;
                break;
            }
            for second in indexed_seam_edges(connection, first.dst_hash)? {
                if second.src != first.dst || second.src_path != first.dst_path {
                    continue;
                }
                if !indexed_edge_is_current(connection, worktree, &second)? {
                    continue;
                }
                if !second.dst.is_test && path_is_in_scope(&second.dst_path, scope) {
                    connected = true;
                    break;
                }
            }
            if connected {
                break;
            }
        }
        if connected {
            break;
        }
    }
    if !connected {
        let missing_scope_files = missing_scope_files(worktree, scope);
        if !missing_scope_files.is_empty() {
            bail!(
                "TASK_SEAM_DISCONNECTED: {task_id}. seam_test_ref '{}' has no current indexed one- or two-hop path into admitted production scope; target scope files do not exist yet on disk ({}). Do: for greenfield/new modules in task.scope, create minimal file scaffolding with compiling production stubs and module/import declarations so AST imports resolve, refresh the index (`contextunity-forge-mcp build`), and resubmit contract proof.",
                reference.path,
                missing_scope_files.join(", ")
            );
        }
        bail!("TASK_SEAM_DISCONNECTED: {task_id}. seam_test_ref '{}' has no current indexed one- or two-hop path into admitted production scope. Do: refresh the claimed-worktree code map, then choose a connected test seam or admit its production dependency.", reference.path);
    }
    Ok(())
}

#[cfg(test)]
#[test]
fn public_trait_methods_are_public_signature_boundaries() {
    for source in [
        "pub trait Router { fn route(&self) -> usize { 1 } }",
        "pub trait Router { fn route(&self) -> usize; }",
        "pub(crate) trait Router { fn route(&self) -> usize; }",
        "pub(in crate::routing) trait Router { fn route(&self) -> usize { 1 } }",
    ] {
        let callables = public_callable_nodes("src/routing.rs", source).unwrap();
        assert!(
            callables
                .iter()
                .any(|node| node.kind == "method" && node.name == "route"),
            "public trait method must participate in signature boundary checks: {source}"
        );
    }
    for source in [
        "trait Router { fn route(&self) -> usize; }",
        "struct Router; impl Router { fn route(&self) -> usize { 1 } }",
        "struct Router; impl Router { pub fn route(&self) -> usize { 1 } }",
        "pub trait Router { fn route(&self) -> usize; } struct Local; impl Router for Local { fn route(&self) -> usize { 1 } }",
    ] {
        let callables = public_callable_nodes("src/routing.rs", source).unwrap();
        assert!(
            !callables.iter().any(|node| {
                node.kind == "method"
                    && node.name == "route"
                    && !node.qualname.contains("Router.route")
            }),
            "only methods owned by a public trait are implicit public boundaries: {source}"
        );
        if source.starts_with("trait Router") {
            assert!(callables.iter().all(|node| node.name != "route"));
        }
    }
}

fn validate_public_signature_scope(
    connection: &rusqlite::Connection,
    task_id: &str,
    worktree: &Path,
    scope: &[String],
    captured_baseline: Option<&str>,
    captured_callers: Option<&BaselineBoundaryCallers>,
) -> Result<()> {
    let baseline = if let Some(captured) = captured_baseline {
        captured.to_owned()
    } else {
        crate::core::git::git_stdout(
            worktree,
            &["rev-parse", "--verify", "HEAD"],
            "TASK_CANDIDATE_BASELINE_INVALID",
        )?
    };
    if captured_callers.is_some_and(|snapshot| snapshot.baseline_commit != baseline) {
        bail!("TASK_CANDIDATE_BASELINE_MISMATCH: accepted contract caller evidence is tied to a different Git baseline");
    }
    let baseline_files = baseline_scope_files(worktree, &baseline, scope)?;
    let current_files = current_scope_files(worktree, scope)?;
    let mut baseline_symbols = Vec::new();
    for path in &baseline_files {
        if language_for_source_path(path).is_none() {
            continue;
        }
        let Some(source) = baseline_source(worktree, &baseline, path)? else {
            continue;
        };
        let symbols = public_callable_nodes(path, &source).map_err(|error| {
            anyhow::anyhow!(
                "TASK_CANDIDATE_BASELINE_INVALID: could not parse '{path}' at {baseline}: {error}"
            )
        })?;
        baseline_symbols.extend(symbols);
    }

    let mut current_by_identity: BTreeMap<(String, String, String), Vec<String>> = BTreeMap::new();
    for path in &current_files {
        if language_for_source_path(path).is_none() {
            continue;
        }
        let source_path = super::confined_path(worktree, path).map_err(|error| {
            anyhow::anyhow!("TASK_SCOPE_INCOMPLETE: {task_id}. current source '{path}' is not confined: {error}")
        })?;
        let source = match std::fs::read_to_string(source_path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => bail!("TASK_SCOPE_INCOMPLETE: {task_id}. current source '{path}' cannot be read for signature validation: {error}"),
        };
        let symbols = public_callable_nodes(path, &source).map_err(|error| {
            anyhow::anyhow!("TASK_SCOPE_INCOMPLETE: {task_id}. could not parse current source '{path}' for signature validation: {error}")
        })?;
        for node in symbols {
            current_by_identity
                .entry((node.path, node.kind, node.qualname))
                .or_default()
                .push(normalized_signature(
                    node.details["signature"].as_str().unwrap_or_default(),
                ));
        }
    }

    for baseline_node in baseline_symbols {
        let signature = baseline_node.details["signature"]
            .as_str()
            .context("TASK_CANDIDATE_BASELINE_INVALID: public symbol has no typed signature")?;
        let key = (
            baseline_node.path.clone(),
            baseline_node.kind.clone(),
            baseline_node.qualname.clone(),
        );
        let signature = normalized_signature(signature);
        if current_by_identity
            .get(&key)
            .is_some_and(|signatures| signatures.iter().any(|current| current == &signature))
        {
            continue;
        }
        let captured = captured_callers
            .into_iter()
            .flat_map(|snapshot| snapshot.entries.iter())
            .find(|entry| {
                entry.path == baseline_node.path
                    && entry.kind == baseline_node.kind
                    && entry.name == baseline_node.name
                    && entry.qualname == baseline_node.qualname
            });
        let indexed = indexed_caller_paths(connection, &baseline_node)?;
        let captured_presence =
            captured.is_some_and(|entry| entry.target_present || !entry.caller_paths.is_empty());
        if !captured_presence && indexed.is_none() {
            bail!("TASK_SCOPE_INCOMPLETE: {task_id}. baseline caller evidence unavailable for '{}' in '{}'. Do: restore the baseline symbol and refresh the index before accepting the contract.", baseline_node.name, baseline_node.path);
        }
        let mut callers = captured
            .map(|entry| entry.caller_paths.clone())
            .unwrap_or_default();
        callers.extend(indexed.into_iter().flatten());
        let external = callers
            .into_iter()
            .filter(|caller| !path_is_in_scope(caller, scope))
            .collect::<BTreeSet<_>>();
        if !external.is_empty() {
            bail!("TASK_SCOPE_INCOMPLETE: {task_id}. public signature for '{}' in '{}' changed with indexed non-test callers outside admitted scope: {}. Do: admit every caller path or preserve the public signature.", baseline_node.name, baseline_node.path, external.into_iter().collect::<Vec<_>>().join(", "));
        }
    }
    Ok(())
}

#[cfg(test)]
#[test]
fn deleted_source_retires_public_symbols_without_read_errors() {
    let root = std::env::temp_dir().join(format!(
        "forge-retire-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("src")).unwrap();
    let git = |args: &[&str]| {
        let output = crate::core::git::git_command(&root, args).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--quiet"]);
    git(&["config", "user.name", "ACDD test"]);
    git(&["config", "user.email", "acdd-test@example.invalid"]);
    git(&["config", "commit.gpgsign", "false"]);
    std::fs::write(root.join("forge-mcp.yaml"), "roots: [src]\n").unwrap();
    std::fs::write(root.join("src/api.rs"), "pub fn retired() -> usize { 1 }\n").unwrap();
    git(&["add", "forge-mcp.yaml", "src"]);
    git(&["commit", "-m", "baseline"]);
    let index = root.join(".forge/code-map.sqlite");
    crate::db::writer::build(&root, &index, None).unwrap();
    let connection = rusqlite::Connection::open(index).unwrap();
    std::fs::remove_file(root.join("src/api.rs")).unwrap();
    let result = validate_public_signature_scope(
        &connection,
        "retirement",
        &root,
        &["src/api.rs".into()],
        None,
        None,
    );
    drop(connection);
    std::fs::remove_dir_all(root).unwrap();
    result.expect(
        "deleted tracked source has retired symbols, rather than unreadable current source",
    );
}

fn commit_tree_and_parent(worktree: &Path, commit: &str) -> Result<(String, String)> {
    let output =
        crate::core::git::git_command(worktree, &["rev-list", "--parents", "-n", "1", commit])?;
    if !output.status.success() {
        bail!("TASK_RECEIPT_INVALID: Git commit is unavailable");
    }
    let fields = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if fields.len() != 2 {
        bail!("TASK_RECEIPT_INVALID: candidate and delivery commits must have one baseline parent");
    }
    let treeish = format!("{commit}^{{tree}}");
    let tree = crate::core::git::git_command(worktree, &["rev-parse", &treeish])?;
    if !tree.status.success() {
        bail!("TASK_RECEIPT_INVALID: Git commit tree is unavailable");
    }
    Ok((
        String::from_utf8_lossy(&tree.stdout).trim().to_owned(),
        fields[1].clone(),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeliveryTreeEntry {
    mode: String,
    object: String,
}

fn delivery_tree_entries(
    worktree: &Path,
    treeish: &str,
    scope: Option<&[String]>,
) -> Result<std::collections::BTreeMap<Vec<u8>, DeliveryTreeEntry>> {
    let mut args = vec!["ls-tree", "-r", "-z", "--full-tree", treeish];
    if let Some(scope) = scope.filter(|scope| !scope.is_empty()) {
        args.push("--");
        args.extend(scope.iter().map(String::as_str));
    }
    let output = crate::core::git::git_command(worktree, &args)?;
    if !output.status.success() {
        bail!("TASK_DELIVERY_COMMIT_INVALID: could not read Git tree entries");
    }
    let mut entries = std::collections::BTreeMap::new();
    for record in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let separator = record
            .iter()
            .position(|byte| *byte == b'\t')
            .context("TASK_DELIVERY_COMMIT_INVALID: malformed Git tree entry")?;
        let (header, path_with_separator) = record.split_at(separator);
        let path = &path_with_separator[1..];
        let fields = header.split(|byte| *byte == b' ').collect::<Vec<_>>();
        if fields.len() != 3 {
            bail!("TASK_DELIVERY_COMMIT_INVALID: malformed Git tree entry header");
        }
        let entry = DeliveryTreeEntry {
            mode: String::from_utf8(fields[0].to_vec())?,
            object: String::from_utf8(fields[2].to_vec())?,
        };
        if entries.insert(path.to_vec(), entry).is_some() {
            bail!("TASK_DELIVERY_COMMIT_INVALID: duplicate Git tree path");
        }
    }
    Ok(entries)
}

fn validate_delivery_commit(
    worktree: &Path,
    candidate: &str,
    candidate_baseline: &str,
    scope: &[String],
    landed: &str,
    receipt_path: &Path,
    expected_receipt: &str,
) -> Result<()> {
    let landed_sha = landed;
    let candidate_entries = delivery_tree_entries(worktree, candidate, None)
        .context("TASK_CANDIDATE_INVALID: candidate snapshot tree is unavailable")?;
    let candidate_scoped = delivery_tree_entries(worktree, candidate, Some(scope))
        .context("TASK_CANDIDATE_INVALID: candidate scope entries are unavailable")?;
    if candidate_entries != candidate_scoped {
        bail!(
            "TASK_CANDIDATE_INVALID: candidate snapshot contains a path outside its admitted scope"
        );
    }
    let baseline_entries = delivery_tree_entries(worktree, candidate_baseline, None)
        .context("TASK_CANDIDATE_BASELINE_INVALID: baseline tree is unavailable")?;
    let baseline_scoped = delivery_tree_entries(worktree, candidate_baseline, Some(scope))
        .context("TASK_CANDIDATE_BASELINE_INVALID: baseline scope entries are unavailable")?;
    let landed = commit_tree_and_parent(worktree, landed_sha)
        .context("TASK_DELIVERY_COMMIT_INVALID: landed commit must have a baseline parent")?;
    if candidate_baseline != landed.1 {
        bail!("TASK_DELIVERY_COMMIT_INVALID: delivery parent must match the reviewed candidate baseline");
    }
    let receipt_path = receipt_path.to_string_lossy().replace('\\', "/");
    let mut expected_entries = baseline_entries;
    for path in baseline_scoped.keys() {
        expected_entries.remove(path);
    }
    expected_entries.extend(candidate_entries);
    let delivered_entries = delivery_tree_entries(worktree, landed_sha, None)
        .context("TASK_DELIVERY_COMMIT_INVALID: delivery tree is unavailable")?;
    let receipt_entry = delivered_entries
        .get(receipt_path.as_bytes())
        .cloned()
        .context("TASK_DELIVERY_COMMIT_INVALID: delivery tree is missing its milestone receipt")?;
    expected_entries.insert(receipt_path.as_bytes().to_vec(), receipt_entry);
    if expected_entries != delivered_entries {
        bail!("TASK_DELIVERY_COMMIT_INVALID: delivery tree must preserve the baseline, apply only candidate scope entries, and add the completed task receipt");
    }
    let spec = format!("{landed_sha}:{}", receipt_path);
    let committed_receipt = crate::core::git::git_command(worktree, &["show", &spec])?;
    if !committed_receipt.status.success()
        || committed_receipt.stdout != expected_receipt.as_bytes()
    {
        bail!("TASK_DELIVERY_COMMIT_INVALID: committed task receipt differs from its commitless milestone document");
    }
    Ok(())
}

struct ProvisionalReceiptRollback {
    path: PathBuf,
    original: String,
    armed: bool,
}

impl ProvisionalReceiptRollback {
    fn restore(&mut self) -> Result<()> {
        if self.armed {
            crate::core::fs::atomic_replace(&self.path, self.original.as_bytes())?;
            self.armed = false;
        }
        Ok(())
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ProvisionalReceiptRollback {
    fn drop(&mut self) {
        if self.armed {
            let _ = crate::core::fs::atomic_replace(&self.path, self.original.as_bytes());
        }
    }
}

impl TasksStore {
    /// Performs submit.
    pub fn submit(
        &mut self,
        id: &str,
        stage: &str,
        evidence: &Evidence,
        action: &str,
        findings: Option<&serde_json::Value>,
    ) -> Result<Task> {
        let mut evidence = evidence.clone();
        self.submit_with_pre_persist(id, stage, &mut evidence, action, findings, |_| Ok(None))
    }

    /// Performs submit after running a caller-owned operation once gate evidence is valid.
    /// The hook runs while the gate transaction is held and before its state is persisted.
    pub fn submit_with_pre_persist(
        &mut self,
        id: &str,
        stage: &str,
        evidence: &mut Evidence,
        action: &str,
        findings: Option<&serde_json::Value>,
        before_persist: impl FnOnce(&Task) -> Result<Option<String>>,
    ) -> Result<Task> {
        self.assert_id(id)?;
        if evidence.stage != stage {
            bail!("TASK_STAGE_INVALID: task '{id}'. evidence.stage '{}' != submitted stage '{stage}'. Do: set evidence.stage to '{stage}'.", evidence.stage);
        }
        if !matches!(action, "pass" | "reject") {
            bail!("invalid submission action");
        }
        let observed_task = self.inspect(id)?;
        let profile = self.profile_for_task(&observed_task)?;
        let code_map = if action == "pass"
            && profile.by_stage(stage).is_some_and(|gate| {
                gate.proof
                    .kind()
                    .is_some_and(|kind| matches!(kind, ProofKind::Contract | ProofKind::Command))
            }) {
            observed_task
                .worktree
                .as_deref()
                .map(|worktree| code_map_connection(self, Path::new(worktree)))
                .transpose()?
                .flatten()
        } else {
            None
        };
        let checkpoint = self.checkpoint_context()?;
        let mut tx =
            crate::db::tasks_store::recovery::transaction(&mut self.connection, checkpoint)?;
        tx.enroll(&[id.to_owned()], false)?;
        let mut task =
            load(&tx, id)?.context(crate::db::tasks_store::task_not_found_message(id))?;
        if task.profile_pin != observed_task.profile_pin || task.stage != observed_task.stage {
            bail!("TASK_STALE_SUBMISSION: task '{id}'. stage or profile_pin changed concurrently. Do: retry submission.");
        }
        let mut document_update = None;
        let mut auto_delivery: Option<(String, String, std::path::PathBuf)> = None;
        let retry: Option<String> = tx
            .query_row(
                "SELECT result FROM task_submissions WHERE task_id=?1 AND revision=?2",
                params![id, evidence.claim_revision],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(retry) = retry {
            let accepted: Submission = serde_json::from_str(&retry)?;
            if accepted.evidence == *evidence
                && accepted.action == action
                && accepted.findings.as_ref() == findings
                && accepted.task.contract_revision == task.contract_revision
            {
                return Ok(accepted.task);
            }
            bail!("TASK_STALE_SUBMISSION: task '{id}'. claim_revision {} already submitted with different evidence. Do: resubmit the accepted evidence for claim_revision {}.", evidence.claim_revision, evidence.claim_revision);
        }
        let gate_index =
            profile.index_of(&task.stage).with_context(|| {
                format!(
            "TASK_STAGE_UNKNOWN: task '{}' is stored at unknown stage '{}'; active gates: {}",
            task.task_id,
            task.stage,
            profile.gates.iter().map(|gate| gate.id.as_str()).collect::<Vec<_>>().join(", ")
        )
            })?;
        task.gate = gate_index;
        let gate_def = profile
            .gates
            .get(gate_index)
            .cloned()
            .context("TASK_STAGE_UNKNOWN")?;
        check_stage(id, &gate_def.id, stage)?;
        if task.status != "in_progress" {
            bail!("TASK_STALE_SUBMISSION: task '{id}'. status is '{}'. Do: claim the task before submitting.", task.status);
        }
        let stale_field = if task.claim_revision != evidence.claim_revision {
            Some((
                "claim_revision",
                evidence.claim_revision.to_string(),
                task.claim_revision.to_string(),
            ))
        } else if task.contract_revision != evidence.contract_revision {
            Some((
                "contract_revision",
                evidence.contract_revision.to_string(),
                task.contract_revision.to_string(),
            ))
        } else if task.task_id != evidence.task_id {
            Some(("task_id", evidence.task_id.clone(), task.task_id.clone()))
        } else if task.worker_id.as_deref() != Some(&evidence.worker_id) {
            Some((
                "worker_id",
                evidence.worker_id.clone(),
                task.worker_id.clone().unwrap_or_default(),
            ))
        } else if task.worktree.as_deref() != Some(&evidence.worktree) {
            Some((
                "worktree",
                evidence.worktree.clone(),
                task.worktree.clone().unwrap_or_default(),
            ))
        } else {
            None
        };
        if let Some((field, evidence_value, active_value)) = stale_field {
            bail!("TASK_STALE_SUBMISSION: task '{id}'. evidence {field} {evidence_value} != active {active_value}. Do: resubmit with active {field} {active_value}.");
        }
        if evidence.proof.is_null() {
            bail!("TASK_EVIDENCE_INVALID: task '{id}' for stage '{stage}'. proof is null. Do: provide non-empty proof for stage '{stage}'.");
        }
        if evidence
            .commit
            .as_ref()
            .is_some_and(|commit| commit.trim().is_empty())
        {
            let snapshot = candidate_snapshot(&tx, id, &profile, gate_index)?
                .unwrap_or_else(|| "unavailable".into());
            bail!("TASK_EVIDENCE_INVALID: task '{id}' for stage '{stage}'. commit field is empty. Do: set commit to candidate snapshot {snapshot}.");
        }
        validate_gate_proof(
            &evidence.proof,
            &gate_def,
            &profile,
            action == "pass",
            &task.spec.proof_policy,
        )?;
        let mut boundary_callers_snapshot = None;
        if action == "pass"
            && matches!(
                gate_def.proof.kind(),
                Some(ProofKind::Contract | ProofKind::Command)
            )
        {
            let worktree = Path::new(&evidence.worktree);
            if let Some(index) = code_map.as_ref() {
                if gate_def.proof.kind() == Some(ProofKind::Contract) {
                    let seam_test_ref = evidence
                        .proof
                        .get("contract_proof")
                        .and_then(|proof| proof.get("seam_test_ref"))
                        .and_then(serde_json::Value::as_str)
                        .context("TASK_EVIDENCE_INVALID: seam test reference required")?;
                    validate_seam_reachability(
                        index,
                        id,
                        worktree,
                        &task.spec.scope,
                        seam_test_ref,
                    )?;
                }
                let captured_callers = if gate_def.proof.kind() == Some(ProofKind::Command) {
                    accepted_boundary_callers(&tx, id, &profile, gate_index)?
                } else {
                    None
                };
                validate_public_signature_scope(
                    index,
                    id,
                    worktree,
                    &task.spec.scope,
                    evidence.candidate_baseline_head.as_deref(),
                    captured_callers.as_ref(),
                )?;
                if gate_def.proof.kind() == Some(ProofKind::Contract) {
                    boundary_callers_snapshot = Some(capture_baseline_boundary_callers(
                        index,
                        worktree,
                        &task.spec.scope,
                        evidence.candidate_baseline_head.as_deref(),
                    )?);
                }
            }
        }
        if let Some(prior) = gate_def.independent_from.as_deref() {
            let builder: String = tx.query_row(
                "SELECT json_extract(evidence,'$.worker_id') FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='passed' ORDER BY revision DESC LIMIT 1",
                params![id, prior],
                |r| r.get(0),
            )?;
            if builder == evidence.worker_id {
                bail!("TASK_REVIEW_NOT_INDEPENDENT: task '{id}'. reviewer worker '{}' matches builder '{builder}'. Do: submit review with a worker_id other than '{builder}'.", evidence.worker_id);
            }
            let build = accepted(&tx, id, prior)?;
            if gate_def.proof.kind() != Some(ProofKind::Delivery) {
                let canonical = evidence.commit.as_deref().and_then(|supplied| {
                    build
                        .commit
                        .as_deref()
                        .and_then(|snapshot| canonical_commit_from_prefix(supplied, snapshot))
                });
                if let Some(canonical) = canonical {
                    evidence.commit = Some(canonical);
                }
                if build.commit != evidence.commit {
                    let supplied = evidence.commit.as_deref().unwrap_or("missing");
                    let snapshot = build.commit.as_deref().unwrap_or("missing");
                    bail!("TASK_CANDIDATE_MISMATCH: task '{id}'. supplied commit {supplied} != candidate snapshot {snapshot}. Do: resubmit with commit {snapshot}.");
                }
            }
        }
        if gate_def.proof.kind() == Some(ProofKind::Delivery) && action == "pass" {
            let snapshot_gate = profile.gates[..gate_index]
                .iter()
                .rev()
                .find(|gate| gate.sha_snapshot)
                .context("TASK_GATES_INVALID: delivery requires a prior sha_snapshot gate")?;
            let build = accepted(&tx, id, &snapshot_gate.id)?;
            let candidate_snapshot = build.commit.as_deref().unwrap_or("missing");
            if !gate_def.auto_commit {
                let canonical = evidence.commit.as_deref().and_then(|supplied| {
                    canonical_commit_from_prefix(supplied, candidate_snapshot)
                });
                if let Some(canonical) = canonical {
                    evidence.commit = Some(canonical);
                }
            }
            let receipt_commit = evidence
                .commit
                .as_deref()
                .context(format!("TASK_EVIDENCE_INVALID: task '{id}' for stage 'deliver'. delivery evidence missing required commit. Do: set commit to candidate snapshot {candidate_snapshot}."))?;
            let candidate_baseline = build.candidate_baseline_head.as_deref().context(
                "TASK_CANDIDATE_BASELINE_INVALID: accepted candidate has no captured baseline",
            )?;
            if evidence.candidate_baseline_head.as_deref() != Some(candidate_baseline) {
                bail!("TASK_CANDIDATE_BASELINE_MISMATCH: delivery baseline differs from the accepted candidate");
            }
            if !gate_def.auto_commit && build.commit.as_deref() != Some(receipt_commit) {
                bail!("TASK_CANDIDATE_MISMATCH: task '{id}'. supplied commit {receipt_commit} != candidate snapshot {candidate_snapshot}. Do: resubmit with commit {candidate_snapshot}.");
            }
            let mut review_proofs = Vec::with_capacity(gate_def.review_sources.len());
            for review_name in &gate_def.review_sources {
                let review = accepted(&tx, id, review_name)?;
                if review.commit != build.commit {
                    bail!("TASK_CANDIDATE_MISMATCH: task '{id}'. review source '{review_name}' commit != candidate snapshot {candidate_snapshot}. Do: resubmit review evidence with commit {candidate_snapshot}.");
                }
                let review_gate = profile
                    .by_stage(review_name)
                    .context("TASK_GATES_INVALID")?;
                validate_gate_proof(
                    &review.proof,
                    review_gate,
                    &profile,
                    true,
                    &task.spec.proof_policy,
                )?;
                review_proofs.push((review_name.clone(), review.proof.clone()));
            }
            let claim_worktree: String = tx.query_row(
                "SELECT worktree FROM task_claims WHERE task_id=?1 AND revision=?2 AND ended=0",
                params![id, task.claim_revision],
                |row| row.get(0),
            )?;
            let path = super::confined_path(Path::new(&claim_worktree), &task.milestone_ref)?;
            if gate_def.auto_commit {
                auto_delivery = Some((
                    build
                        .commit
                        .as_deref()
                        .context("TASK_SNAPSHOT_NOT_FOUND")?
                        .to_owned(),
                    candidate_baseline.to_owned(),
                    path.clone(),
                ));
            }
            let text = std::fs::read_to_string(&path)?;
            let milestone = super::workspaces::parse_milestone_manifest(
                &text,
                Path::new(&task.milestone_ref),
                &self.repository,
                &self.default_project,
                &self.project_owners,
                &self.project_aliases,
            )?;
            let spec = milestone
                .tasks
                .iter()
                .find(|s| milestone.task_id(s) == id)
                .context("TASK_RECEIPT_INVALID: task not found")?;
            if milestone.digest(spec)? != task.digest {
                bail!("TASK_RECEIPT_INVALID: specification mismatch");
            }
            let review_value = if review_proofs.len() == 1 {
                review_proofs[0].1.clone()
            } else {
                serde_json::json!({"review_sources": review_proofs.iter().cloned().collect::<std::collections::BTreeMap<_, _>>()})
            };
            let outcomes = tx
                .prepare(
                    "SELECT topic,payload FROM task_blackboard \
                 WHERE task_id=?1 AND topic IN ('decisions','deferred') \
                 ORDER BY created_at,id",
                )?
                .query_map([id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .map(|(topic, payload)| format!("{topic}: {payload}"))
                .collect::<Vec<_>>();
            let rollup = ReceiptRollup {
                verified_invariants: task.applicable_invariants.clone(),
                architectural_notes: outcomes,
                review_summary: review_rollup_summary(&review_proofs)?,
            };
            let document_commit = if gate_def.auto_commit {
                None
            } else {
                Some(receipt_commit.to_owned())
            };
            let generated = Receipt {
                commit: document_commit.clone(),
                contract_revision: task.contract_revision,
                passed_at: chrono::Utc::now().to_rfc3339(),
                evidence: build.proof.clone(),
                review: review_value.clone(),
                decision: "pass".into(),
                rollup: Some(rollup),
            };
            let receipt = match (spec.status.as_deref(), spec.receipt.as_ref()) {
                (Some("completed"), Some(existing)) => {
                    // The document may have been replaced immediately before a process crash.
                    // Reuse only the exact accepted outcome, retaining its original timestamp.
                    validate_receipt(
                        existing,
                        &task,
                        &build,
                        &review_value,
                        document_commit.as_deref(),
                        &profile,
                    )?;
                    if existing.rollup != generated.rollup {
                        bail!("TASK_RECEIPT_INVALID: rollup differs from accepted task context");
                    }
                    existing.clone()
                }
                (Some("completed"), None) | (_, Some(_)) => {
                    bail!("TASK_RECEIPT_INVALID: incomplete terminal receipt")
                }
                _ => generated,
            };
            validate_receipt(
                &receipt,
                &task,
                &build,
                &review_value,
                document_commit.as_deref(),
                &profile,
            )?;
            let (_, task_ref) = id.rsplit_once(':').context("invalid task identity")?;
            let mut stmt = tx.prepare(
                "SELECT subtask_ref, title, status, evidence FROM task_subtasks WHERE task_id=?1 ORDER BY rowid",
            )?;
            let current_subtasks = stmt
                .query_map([id], |r| {
                    Ok(SubtaskSpec {
                        subtask_ref: r.get(0)?,
                        title: r.get(1)?,
                        status: r.get(2)?,
                        evidence: r.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let final_subtasks = if !current_subtasks.is_empty() {
                current_subtasks
            } else {
                task.spec.subtasks.clone()
            };
            let written = crate::engine::milestones::render_task_receipt(
                &text,
                task_ref,
                &receipt,
                &final_subtasks,
            )?;
            task.spec.subtasks = final_subtasks;
            document_update = Some((path, text, written));
            task.receipt = Some(receipt);
            task.completed_at = Some(now());
            task.status = "completed".into();
        } else if action == "pass" {
            task.gate = gate_index + 1;
            task.stage = profile.gate_id(task.gate).to_owned();
            task.status = "ready".into();
        } else {
            let findings = findings.context("reject requires findings")?;
            tx.execute(
                "INSERT INTO task_findings VALUES(?1,?2,?3)",
                params![id, task.claim_revision, serde_json::to_string(findings)?],
            )?;
            if let Some(target) = gate_def.reject_to.as_deref() {
                task.gate = profile.index_of(target).with_context(|| {
                    format!("TASK_STAGE_INVALID: task '{id}'. reject target stage '{target}' not defined in profile. Do: set reject_to to a stage that exists in the profile.")
                })?;
                task.stage = target.to_owned();
            }
            task.status = "ready".into();
        }
        let mut provisional_rollback = None;
        let pre_persist = (|| -> Result<()> {
            if let Some((path, original, written)) = &document_update {
                provisional_rollback = Some(ProvisionalReceiptRollback {
                    path: path.clone(),
                    original: original.clone(),
                    armed: true,
                });
                crate::core::fs::atomic_replace(path, written.as_bytes())?;
            }
            let created_delivery = before_persist(&task)?;
            match (auto_delivery.as_ref(), created_delivery) {
                (Some((candidate, candidate_baseline, path)), Some(landed)) => {
                    let worktree = Path::new(&evidence.worktree);
                    let relative = path.strip_prefix(worktree).context(
                        "TASK_DELIVERY_COMMIT_INVALID: receipt path is outside the worktree",
                    )?;
                    let written = document_update
                        .as_ref()
                        .map(|(_, _, written)| written.as_str())
                        .context(
                            "TASK_DELIVERY_COMMIT_INVALID: provisional receipt document is missing",
                        )?;
                    validate_delivery_commit(
                        worktree,
                        candidate,
                        candidate_baseline,
                        &task.spec.scope,
                        &landed,
                        relative,
                        written,
                    )?;
                    evidence.commit = Some(landed.clone());
                    let receipt = task
                        .receipt
                        .as_mut()
                        .context("TASK_RECEIPT_INVALID: delivery receipt is missing")?;
                    receipt.commit = Some(landed);
                    validate_durable_receipt_with_profile(
                        receipt,
                        task.contract_revision,
                        &profile,
                    )?;
                }
                (Some(_), None) => {
                    bail!("TASK_DELIVERY_COMMIT_REQUIRED: auto_commit delivery must create a hooked commit");
                }
                (None, Some(_)) => {
                    bail!("TASK_DELIVERY_COMMIT_INVALID: unexpected commit result for this gate");
                }
                (None, None) => {}
            }
            Ok(())
        })();
        if let Err(error) = pre_persist {
            if let Some(rollback) = provisional_rollback.as_mut() {
                rollback
                    .restore()
                    .context("failed to restore milestone document after delivery failure")?;
            }
            return Err(error);
        }
        tx.execute(
            "INSERT INTO task_gates VALUES(?1,?2,?3,?4,?5)",
            params![
                id,
                evidence.stage,
                task.claim_revision,
                if action == "pass" {
                    "passed"
                } else {
                    "rejected"
                },
                serde_json::to_string(evidence)?
            ],
        )?;
        end_claim(&tx, id)?;
        task.worker_id = None;
        task.worktree = None;
        save(&tx, &task)?;
        let submission = Submission {
            evidence: evidence.clone(),
            action: action.into(),
            findings: findings.cloned(),
            task: task.clone(),
            boundary_callers: boundary_callers_snapshot.clone(),
        };
        tx.execute(
            "INSERT INTO task_submissions VALUES(?1,?2,?3)",
            params![
                id,
                evidence.claim_revision,
                serde_json::to_string(&submission)?
            ],
        )?;
        if task.status == "completed" {
            clear_blackboard(&tx, id)?;
        }
        if let Some((path, _, written)) = &document_update {
            crate::core::fs::atomic_replace(path, written.as_bytes())?;
        }
        if let Err(error) = tx.commit() {
            if crate::db::tasks_store::recovery::is_pending(&error) {
                if let Some(rollback) = provisional_rollback.as_mut() {
                    rollback.disarm();
                }
                return Err(error);
            }
            if let Some(rollback) = provisional_rollback.as_mut() {
                rollback
                    .restore()
                    .context("failed to restore milestone document after SQLite commit error")?;
            } else if let Some((path, original, _)) = &document_update {
                crate::core::fs::atomic_replace(path, original.as_bytes())
                    .context("failed to restore milestone document after SQLite commit error")?;
            }
            return Err(error);
        }
        if let Some(rollback) = provisional_rollback.as_mut() {
            rollback.disarm();
        }
        Ok(task)
    }
}

#[derive(Serialize, Deserialize)]
struct Submission {
    evidence: Evidence,
    action: String,
    findings: Option<serde_json::Value>,
    task: Task,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    boundary_callers: Option<BaselineBoundaryCallers>,
}

fn accepted(conn: &rusqlite::Connection, id: &str, gate: &str) -> Result<Evidence> {
    let json: String = conn.query_row("SELECT evidence FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='passed' ORDER BY revision DESC LIMIT 1",params![id,gate],|r|r.get(0))?;
    Ok(serde_json::from_str(&json)?)
}

fn candidate_snapshot(
    conn: &rusqlite::Connection,
    id: &str,
    profile: &crate::core::tasks::profile::GateProfile,
    gate_index: usize,
) -> Result<Option<String>> {
    let Some(snapshot_gate) = profile.gates[..gate_index]
        .iter()
        .rev()
        .find(|gate| gate.sha_snapshot)
    else {
        return Ok(None);
    };
    let serialized: Option<String> = conn
        .query_row(
            "SELECT evidence FROM task_gates WHERE task_id=?1 AND gate=?2 AND state='passed' ORDER BY revision DESC LIMIT 1",
            params![id, snapshot_gate.id],
            |row| row.get(0),
        )
        .optional()?;
    serialized
        .map(|json| {
            let evidence: Evidence = serde_json::from_str(&json)?;
            Ok(evidence.commit)
        })
        .transpose()
        .map(Option::flatten)
}

fn validate_receipt(
    receipt: &Receipt,
    task: &Task,
    build: &Evidence,
    review: &serde_json::Value,
    expected_commit: Option<&str>,
    profile: &crate::core::tasks::profile::GateProfile,
) -> Result<()> {
    validate_durable_receipt_with_profile(receipt, task.contract_revision, profile)?;
    if receipt.commit.as_deref() != expected_commit
        || receipt.contract_revision != task.contract_revision
        || receipt.evidence != build.proof
        || &receipt.review != review
        || receipt.decision != "pass"
        || chrono::DateTime::parse_from_rfc3339(&receipt.passed_at).is_err()
        || receipt
            .rollup
            .as_ref()
            .is_some_and(|rollup| rollup.verified_invariants != task.applicable_invariants)
    {
        bail!("TASK_RECEIPT_INVALID: accepted proof mismatch");
    }
    Ok(())
}
