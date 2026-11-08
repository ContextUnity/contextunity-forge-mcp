use super::*;
use crate::tasks::profile::ProofKind;
use anyhow::{bail, Context, Result};
use rusqlite::params;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(crate) fn code_map_connection(
    fallback_conn: Option<&rusqlite::Connection>,
    worktree: &Path,
) -> Result<Option<rusqlite::Connection>> {
    let index_path = worktree.join(".forge/code-map.sqlite");
    let resolved_path = if index_path.is_file() {
        Some(index_path)
    } else if let Some(parent) = fallback_conn
        .and_then(|c| c.path())
        .and_then(|p| Path::new(p).parent())
    {
        let sibling = parent.join("code-map.sqlite");
        if sibling.is_file() {
            Some(sibling)
        } else {
            None
        }
    } else {
        None
    };
    let Some(index_path) = resolved_path else {
        return Ok(None);
    };
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

fn module_export_names(nodes: &[forge_core::models::Node]) -> BTreeSet<String> {
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

pub(super) fn public_trait_methods(nodes: &[forge_core::models::Node]) -> BTreeSet<String> {
    let public_traits = nodes
        .iter()
        .filter(|node| node.kind == "trait")
        .filter(|node| {
            node.details["is_public"].as_bool() == Some(true)
                || node.details["visibility"].as_str() == Some("public")
                || node.details["signature"].as_str().is_some_and(|sig| {
                    let sig = sig.trim_start();
                    sig.starts_with("pub ") || sig.starts_with("pub(")
                })
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

pub(super) fn exported_callable(
    node: &forge_core::models::Node,
    exports: &BTreeSet<String>,
    public_trait_methods: &BTreeSet<String>,
) -> bool {
    node.details["is_public"].as_bool() == Some(true)
        || node.details["visibility"].as_str() == Some("public")
        || node.details["default_export"].as_bool() == Some(true)
        || exports.contains(&node.name)
        || public_trait_methods.contains(&node.qualname)
        || node
            .qualname
            .rsplit_once('.')
            .is_some_and(|(owner, _)| exports.contains(owner.rsplit('.').next().unwrap_or(owner)))
}

pub type AstExtractor =
    fn(path: &str, source: &str) -> Result<Option<forge_core::typed_facts::TypedFacts>>;
static AST_EXTRACTOR: std::sync::RwLock<Option<AstExtractor>> = std::sync::RwLock::new(None);

pub fn set_ast_extractor(extractor: AstExtractor) {
    if let Ok(mut lock) = AST_EXTRACTOR.write() {
        *lock = Some(extractor);
    }
}

pub(crate) fn extract_typed(
    path: &str,
    source: &str,
) -> Result<Option<forge_core::typed_facts::TypedFacts>> {
    let extractor = {
        let guard = AST_EXTRACTOR
            .read()
            .map_err(|_| anyhow::anyhow!("task AST extractor lock poisoned"))?;
        (*guard).context(
            "TASK_EXTRACTOR_NOT_INITIALIZED: production task AST extractor is not configured",
        )?
    };
    extractor(path, source)
}

pub(super) fn typed_source_nodes(
    path: &str,
    source: &str,
) -> Result<Vec<forge_core::models::Node>> {
    Ok(extract_typed(path, source)?.map_or_else(Vec::new, |facts| facts.facts.nodes))
}

pub(crate) fn public_callable_nodes(
    path: &str,
    source: &str,
) -> Result<Vec<forge_core::models::Node>> {
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

pub(super) fn accepted_boundary_callers(
    conn: &rusqlite::Connection,
    id: &str,
    profile: &crate::tasks::profile::GateProfile,
    gate_index: usize,
) -> Result<Option<BaselineBoundaryCallers>> {
    let gate_sequence = super::super::claim::gate_sequence(profile)?;
    let Some(contract_gate) = gate_sequence[..gate_index]
        .iter()
        .rev()
        .find(|cursor| cursor.gate.proof.kind() == Some(ProofKind::Contract))
    else {
        return Ok(None);
    };
    let Some(submission) = accepted_submission_for_cursor(conn, id, profile, contract_gate)? else {
        return Ok(None);
    };
    Ok(submission.boundary_callers)
}

fn accepted_submission_for_cursor(
    conn: &rusqlite::Connection,
    id: &str,
    profile: &crate::tasks::profile::GateProfile,
    cursor: &super::super::claim::GateCursor,
) -> Result<Option<Submission>> {
    let mut stmt = conn.prepare(
        "SELECT gate.gate, gate.evidence, submission.result FROM task_gates gate \
         JOIN task_submissions submission ON submission.task_id=gate.task_id AND submission.revision=gate.revision \
         WHERE gate.task_id=?1 AND gate.state='passed' \
         ORDER BY gate.revision DESC",
    )?;
    let submissions = stmt
        .query_map([id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (stage, accepted_evidence, result) in submissions {
        if super::super::claim::cursor_for_stage(profile, &stage)
            .is_ok_and(|accepted| accepted.canonical_stage_id == cursor.canonical_stage_id)
        {
            let mut submission: Submission = serde_json::from_str(&result)?;
            let evidence: Evidence = serde_json::from_str(&accepted_evidence)?;
            if evidence.stage != stage {
                bail!("TASK_GATES_INVALID: accepted evidence stage '{}' does not match persisted gate '{stage}'", evidence.stage);
            }
            submission.evidence = evidence;
            return Ok(Some(submission));
        }
    }
    Ok(None)
}

pub(crate) fn accepted_evidence_for_cursor(
    conn: &rusqlite::Connection,
    id: &str,
    profile: &crate::tasks::profile::GateProfile,
    cursor: &super::super::claim::GateCursor,
) -> Result<Option<Evidence>> {
    Ok(accepted_submission_for_cursor(conn, id, profile, cursor)?
        .map(|submission| submission.evidence))
}

fn baseline_scope_files(worktree: &Path, baseline: &str, scope: &[String]) -> Result<Vec<String>> {
    if scope.is_empty() {
        return Ok(Vec::new());
    }
    let mut args = vec!["ls-tree", "-r", "-z", "--name-only", baseline, "--"];
    args.extend(scope.iter().map(String::as_str));
    let output = crate::git::git_command(worktree, &args)?;
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
        crate::git::git_command(worktree, &["ls-files", "-co", "--exclude-standard", "-z"])?;
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
    let output = crate::git::git_command(worktree, &["show", &object])?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8(output.stdout).context(
        "TASK_CANDIDATE_BASELINE_INVALID: source is not UTF-8",
    )?))
}

fn indexed_caller_paths(
    connection: &rusqlite::Connection,
    symbol: &forge_core::models::Node,
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

pub(super) fn capture_baseline_boundary_callers(
    connection: &rusqlite::Connection,
    worktree: &Path,
    scope: &[String],
    captured_baseline: Option<&str>,
) -> Result<BaselineBoundaryCallers> {
    let baseline = match captured_baseline {
        Some(captured) => captured.to_owned(),
        None => crate::git::git_stdout(
            worktree,
            &["rev-parse", "--verify", "HEAD"],
            "TASK_CANDIDATE_BASELINE_INVALID",
        )?,
    };
    let mut entries = Vec::new();
    for path in baseline_scope_files(worktree, &baseline, scope)? {
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

pub(super) fn path_is_in_scope(path: &str, scope: &[String]) -> bool {
    scope.iter().any(|admitted| {
        if admitted.ends_with('/') {
            path.starts_with(admitted)
        } else {
            path == admitted
        }
    })
}

pub(super) fn validate_public_signature_scope(
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
        crate::git::git_stdout(
            worktree,
            &["rev-parse", "--verify", "HEAD"],
            "TASK_CANDIDATE_BASELINE_INVALID",
        )?
    };
    // Preserve accepted task proofs across history integration (rebase/amend/merge)
    // without failing closed on navigation SHA changes.
    let baseline_files = baseline_scope_files(worktree, &baseline, scope)?;
    let current_files = current_scope_files(worktree, scope)?;
    let mut baseline_symbols = Vec::new();
    for path in &baseline_files {
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
        let source_path = super::super::confined_path(worktree, path).map_err(|error| {
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
