use crate::engine::tasks::workspaces::Workspace;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// Truncate string by Unicode scalar count without slicing invalid byte boundaries.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let prefix: String = s.chars().take(max_chars).collect();
        format!("{prefix}... [truncated]")
    }
}

fn empty_governing_invariants() -> Value {
    json!({
        "direct": [],
        "omitted_direct": 0,
        "transitive_ids": [],
        "omitted_transitive": 0,
        "omitted_sources": [],
    })
}

fn open_code_map(workspace_root: &Path, root: &Path) -> Option<rusqlite::Connection> {
    let db_path = [
        workspace_root.join(".forge/code-map.sqlite"),
        root.join(".forge/code-map.sqlite"),
    ]
    .into_iter()
    .find(|path| path.is_file())?;
    rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()
}

fn query_indexed_adrs(conn: &rusqlite::Connection, scope: &[String]) -> Vec<Value> {
    if scope.is_empty() {
        return Vec::new();
    }
    let scope = match serde_json::to_string(scope) {
        Ok(scope) => scope,
        Err(_) => return Vec::new(),
    };
    let sql = "WITH scope_nodes(hash) AS (SELECT DISTINCT target.node_hash FROM nodes target JOIN path_dictionary tp ON tp.path_id=target.path_id WHERE EXISTS(SELECT 1 FROM json_each(?1) s WHERE tp.path=trim(s.value,'/') OR substr(tp.path,1,length(trim(s.value,'/'))+1)=trim(s.value,'/')||'/')), governed_paths(path) AS (SELECT DISTINCT d.path FROM edges e JOIN nodes n ON n.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=n.path_id JOIN doc_sections d ON d.doc_id=json_extract(n.details,'$.document_id') JOIN scope_nodes s ON s.hash=e.dst_hash WHERE e.kind='governs' AND (p.path LIKE 'docs/adr/%' OR p.path LIKE 'docs/architecture/%')) SELECT d.path,d.section_title,d.content,n.details FROM governed_paths g JOIN doc_sections d ON d.path=g.path JOIN nodes n ON n.id=d.doc_id ORDER BY d.path,d.rowid LIMIT 200";
    let mut statement = match conn.prepare(sql) {
        Ok(statement) => statement,
        Err(_) => return Vec::new(),
    };
    let rows = match statement.query_map([scope], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    }) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };
    let mut documents = BTreeMap::<String, Value>::new();
    let mut navigation_counts = BTreeMap::<String, usize>::new();
    for row in rows.flatten() {
        let (path, title, content, details) = row;
        let navigation_count = navigation_counts.entry(path.clone()).or_default();
        let details = serde_json::from_str::<Value>(&details).unwrap_or(Value::Null);
        let document = documents.entry(path.clone()).or_insert_with(|| {
            json!({
                "path":path,
                "title":title,
                "status":details.get("status").cloned().unwrap_or(Value::Null),
                "relevance":"direct",
                "invariants_summary":[],
            })
        });
        let Some(summary) = document["invariants_summary"].as_array_mut() else {
            continue;
        };
        for line in content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            let decision = line
                .trim_start_matches(['-', '*'])
                .trim_start()
                .starts_with("Decision:");
            let decision_section = title.trim().eq_ignore_ascii_case("decision");
            let navigation_rule = line.contains("INV-") || line.contains("Invariant:");
            let keep = decision || decision_section || (navigation_rule && *navigation_count < 5);
            if keep && !summary.iter().any(|item| item.as_str() == Some(line)) {
                summary.push(json!(line));
                if !decision && !decision_section && navigation_rule {
                    *navigation_count += 1;
                }
            }
        }
    }
    documents.into_values().take(10).collect()
}

fn query_indexed_context(
    workspace_root: &Path,
    root: &Path,
    scope: &[String],
) -> (Vec<Value>, Value) {
    let Some(conn) = open_code_map(workspace_root, root) else {
        return (Vec::new(), empty_governing_invariants());
    };
    let adrs = query_indexed_adrs(&conn, scope);
    let governing = crate::db::reader::governing_invariants_for_scope(&conn, scope)
        .unwrap_or_else(|_| empty_governing_invariants());
    (adrs, governing)
}

/// Query SQLite code map index for high-value symbols within the task scope.
fn query_scope_symbols(workspace_root: &Path, root: &Path, scope: &[String]) -> Vec<Value> {
    let candidate_dbs = [
        workspace_root.join(".forge/code-map.sqlite"),
        root.join(".forge/code-map.sqlite"),
    ];

    let db_path = candidate_dbs.into_iter().find(|p| p.is_file());
    let db_path = match db_path {
        Some(path) => path,
        None => return Vec::new(),
    };

    let conn = match rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut symbols = Vec::new();
    for scope_path in scope {
        let (sql, param) = if scope_path.ends_with('/') {
            (
                "SELECT n.name,n.kind,p.path,n.line,n.details FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.is_test=0 AND n.kind IN ('struct','enum','trait','class','type','interface','function') AND p.path LIKE ?1 || '%' ORDER BY p.path,n.line LIMIT 15",
                scope_path.clone(),
            )
        } else {
            (
                "SELECT n.name,n.kind,p.path,n.line,n.details FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.is_test=0 AND n.kind IN ('struct','enum','trait','class','type','interface','function') AND p.path=?1 ORDER BY n.line LIMIT 15",
                scope_path.clone(),
            )
        };

        if let Ok(mut stmt) = conn.prepare(sql) {
            let rows = stmt.query_map([&param], |row| {
                let name: String = row.get(0)?;
                let kind: String = row.get(1)?;
                let path: String = row.get(2)?;
                let line: i64 = row.get(3)?;
                let details_str: String = row.get(4)?;
                let signature = serde_json::from_str::<Value>(&details_str)
                    .ok()
                    .and_then(|v| {
                        v.get("signature")
                            .and_then(|s| s.as_str())
                            .map(|s| truncate_chars(s, 200))
                    });
                Ok(json!({
                    "name": name,
                    "kind": kind,
                    "path": path,
                    "line": line,
                    "signature": signature,
                }))
            });
            if let Ok(iter) = rows {
                for item in iter.flatten() {
                    symbols.push(item);
                }
            }
        }
    }

    symbols.truncate(30);
    symbols
}

/// Discover tests covering symbols or modules within the task scope.
fn find_covering_tests(workspace_root: &Path, root: &Path, scope: &[String]) -> Vec<Value> {
    let mut covering = Vec::new();
    let mut seen = BTreeSet::new();

    // 1. Direct test files in declared scope
    for path in scope {
        if (path.starts_with("tests/") || path.contains("test")) && !seen.contains(path) {
            seen.insert(path.clone());
            covering.push(json!({
                "path": path,
                "kind": "scoped_test",
                "match_reason": "declared_in_scope"
            }));
        }
    }

    // Indexed calls and imports prove a dependency from a test to a scoped symbol.
    let db_path = [
        workspace_root.join(".forge/code-map.sqlite"),
        root.join(".forge/code-map.sqlite"),
    ]
    .into_iter()
    .find(|path| path.is_file());
    if let Some(db_path) = db_path {
        if let Ok(conn) = rusqlite::Connection::open_with_flags(
            db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ) {
            for scope_path in scope.iter().filter(|path| !path.starts_with("tests/")) {
                let predicate = if scope_path.ends_with('/') {
                    "np.path LIKE ?1 || '%'"
                } else {
                    "np.path = ?1"
                };
                let sql = format!("SELECT DISTINCT tp.path,t.name FROM nodes n JOIN path_dictionary np ON np.path_id=n.path_id JOIN edges e ON e.dst_hash=n.node_hash AND e.kind IN ('calls','imports','possible_calls','implements','overrides') JOIN nodes t ON t.node_hash=e.src_hash JOIN path_dictionary tp ON tp.path_id=t.path_id WHERE {predicate} AND t.is_test=1 ORDER BY tp.path,t.name LIMIT 15");
                if let Ok(mut stmt) = conn.prepare(&sql) {
                    if let Ok(rows) = stmt.query_map([scope_path], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    }) {
                        for row in rows.flatten() {
                            if seen.insert(row.0.clone()) {
                                covering.push(json!({"path":row.0,"name":row.1,"kind":"test_symbol","match_reason":"indexed_dependency"}));
                            }
                        }
                    }
                }

                let indirect_sql = format!(
                    "SELECT DISTINCT tp.path,t.name FROM nodes target \
                     JOIN path_dictionary np ON np.path_id=target.path_id \
                     JOIN edges second ON second.dst_hash=target.node_hash AND second.kind IN ('calls','imports','possible_calls','implements','overrides') \
                     JOIN nodes intermediate ON intermediate.node_hash=second.src_hash \
                     JOIN edges first ON first.dst_hash=intermediate.node_hash AND first.kind IN ('calls','imports','possible_calls','implements','overrides') \
                     JOIN nodes t ON t.node_hash=first.src_hash AND t.is_test=1 \
                     JOIN path_dictionary tp ON tp.path_id=t.path_id \
                     WHERE {predicate} ORDER BY tp.path,t.name LIMIT 15"
                );
                if let Ok(mut stmt) = conn.prepare(&indirect_sql) {
                    if let Ok(rows) = stmt.query_map([scope_path], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    }) {
                        for row in rows.flatten() {
                            if seen.insert(row.0.clone()) {
                                covering.push(json!({"path":row.0,"name":row.1,"kind":"test_symbol","match_reason":"two_hop_indexed_dependency"}));
                            }
                        }
                    }
                }
            }
        }
    }

    covering.truncate(15);
    covering
}

/// Return indexed one-hop callers outside the task's admitted paths.
fn find_external_callers(workspace_root: &Path, root: &Path, scope: &[String]) -> Vec<Value> {
    let db_path = [
        workspace_root.join(".forge/code-map.sqlite"),
        root.join(".forge/code-map.sqlite"),
    ]
    .into_iter()
    .find(|path| path.is_file());
    let Some(db_path) = db_path else {
        return Vec::new();
    };
    let Ok(conn) =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return Vec::new();
    };

    let mut callers = Vec::new();
    let mut seen = BTreeSet::new();
    for scope_path in scope.iter().filter(|path| !path.starts_with("tests/")) {
        let predicate = if scope_path.ends_with('/') {
            "np.path LIKE ?1 || '%'"
        } else {
            "np.path = ?1"
        };
        let sql = format!(
            "SELECT DISTINCT cp.path,c.name,target.name,target.kind,c.details \
             FROM nodes target JOIN path_dictionary np ON np.path_id=target.path_id \
             JOIN edges e ON e.dst_hash=target.node_hash AND e.kind IN ('calls','imports','possible_calls','implements','overrides') \
             JOIN nodes c ON c.node_hash=e.src_hash AND c.is_test=0 \
             JOIN path_dictionary cp ON cp.path_id=c.path_id \
             WHERE {predicate} ORDER BY cp.path,c.name,target.name LIMIT 100"
        );
        let Ok(mut stmt) = conn.prepare(&sql) else {
            continue;
        };
        let Ok(rows) = stmt.query_map([scope_path], |row| {
            let caller_details: String = row.get(4)?;
            let signature = serde_json::from_str::<Value>(&caller_details)
                .ok()
                .and_then(|details| details.get("signature").cloned());
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                signature,
            ))
        }) else {
            continue;
        };
        for row in rows.flatten() {
            let (path, caller, target, kind, signature) = row;
            let is_in_scope = scope.iter().any(|admitted| {
                if admitted.ends_with('/') {
                    path.starts_with(admitted)
                } else {
                    path.as_str() == admitted.as_str()
                }
            });
            if is_in_scope || !seen.insert((path.clone(), caller.clone(), target.clone())) {
                continue;
            }
            callers.push(json!({
                "path": path,
                "caller": caller,
                "target": target,
                "kind": kind,
                "signature": signature,
            }));
        }
    }
    callers.truncate(25);
    callers
}

fn reference_path(reference: &Value) -> Option<String> {
    let reference = reference.as_str()?.trim();
    let path = reference.split("::").next().unwrap_or(reference);
    let path = path.split('#').next().unwrap_or(path);
    (!path.is_empty()).then(|| path.to_owned())
}

fn cargo_test_target(root: &Path, reference: &Value) -> Option<(String, String)> {
    let path = reference_path(reference)?;
    let test_path = Path::new(&path);
    if test_path
        .extension()
        .and_then(|extension| extension.to_str())
        != Some("rs")
        || !root.join(test_path).is_file()
    {
        return None;
    }
    if path.starts_with("src/") {
        return Some((path, "--lib".to_owned()));
    }

    let tests_root = test_path
        .ancestors()
        .find(|ancestor| ancestor.file_name().and_then(|name| name.to_str()) == Some("tests"))?;
    let mut directory = test_path.parent()?;
    while directory != tests_root {
        let target_root = directory.with_extension("rs");
        if root.join(&target_root).is_file() {
            let target = target_root.file_stem()?.to_str()?;
            return Some((path, format!("--test {target}")));
        }
        directory = directory.parent()?;
    }
    if test_path.parent()? == tests_root {
        return Some((
            path.clone(),
            format!("--test {}", test_path.file_stem()?.to_str()?),
        ));
    }
    None
}

fn scoped_commands(
    root: &Path,
    commands: &std::collections::BTreeMap<String, crate::core::tasks::profile::CommandDef>,
    seam: Option<&Value>,
    covering_tests: &[Value],
) -> Vec<Value> {
    let selected_commands = commands
        .get("rust-check-and-test")
        .map(|definition| vec![("rust-check-and-test", definition)])
        .unwrap_or_else(|| {
            commands
                .iter()
                .map(|(name, definition)| (name.as_str(), definition))
                .collect()
        });
    let mut targets = BTreeSet::new();
    if let Some(seam) = seam.and_then(reference_path) {
        targets.insert((seam, "contract_seam_test"));
    }
    for test in covering_tests {
        if let Some(path) = test.get("path").and_then(Value::as_str) {
            targets.insert((path.to_owned(), "covering_test"));
        }
    }

    targets
        .into_iter()
        .flat_map(|(path, source)| {
            selected_commands.iter().map(move |(name, definition)| {
                let command = definition.command();
                let suffix = if command.contains("cargo") {
                    cargo_test_target(root, &json!(path))
                        .map(|(_, target)| target)
                        .unwrap_or_else(|| path.clone())
                } else {
                    path.clone()
                };
                json!({
                    "name": name,
                    "source": source,
                    "test_path": path.clone(),
                    "command": format!("{command} {suffix}"),
                })
            })
        })
        .take(30)
        .collect()
}

/// Builds the comprehensive, zero-shot task context bundle.
pub(super) fn context_bundle(
    root: &Path,
    workspace: &Workspace,
    task_id: &str,
    mut details: Value,
) -> Result<Value> {
    let profile = workspace.profile_for_details(&details)?;
    let stage = if details.get("status").and_then(Value::as_str) == Some("completed") {
        "completed"
    } else {
        details
            .get("stage")
            .and_then(Value::as_str)
            .context("task stage missing")?
    };
    let active_index = if stage == "completed" {
        profile.gates.len()
    } else {
        profile.index_of(stage).with_context(|| {
            format!(
                "TASK_STAGE_UNKNOWN: '{stage}' is not in the active profile; choose one of: {}",
                profile
                    .gates
                    .iter()
                    .map(|gate| gate.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?
    };
    let active_gate = profile.gates.get(active_index);
    let proof_kind = active_gate.and_then(|gate| gate.proof.kind());
    let gate_history = details
        .get("gates")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let evidence_for = |gate_id: &str, expected_state: &str| -> Option<Value> {
        gate_history.iter().rev().find_map(|entry| {
            (entry.get("stage").and_then(Value::as_str) == Some(gate_id)
                && entry.get("state").and_then(Value::as_str) == Some(expected_state))
            .then(|| entry.get("evidence").and_then(Value::as_str))
            .flatten()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        })
    };
    let snapshot_for = |before_index: usize| -> Option<Value> {
        profile
            .gates
            .iter()
            .take(before_index)
            .rev()
            .find_map(|gate| {
                if !gate.sha_snapshot {
                    return None;
                }
                let evidence = evidence_for(&gate.id, "passed")?;
                let commit = evidence.get("commit")?.as_str()?;
                Some(json!({
                    "stage": gate.id,
                    "commit": commit,
                    "candidate_baseline_head": evidence.get("candidate_baseline_head"),
                    "inspect_cmd": crate::core::tasks::snapshot_inspect_cmd(commit),
                }))
            })
    };
    let scope: Vec<String> = details["allowed_write_scope"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    // 1. Scope-linked document context is grounded in current indexed graph edges.
    let (adrs, governing_invariants) = query_indexed_context(&workspace.root, root, &scope);

    // 2. Scope symbols skeleton from code map
    let scope_symbols = query_scope_symbols(&workspace.root, root, &scope);

    // 3. Covering test seams
    let covering_tests = find_covering_tests(&workspace.root, root, &scope);
    let external_callers = find_external_callers(&workspace.root, root, &scope);

    // Select proof context by configured gate taxonomy and references.
    let contract_seam_test = profile
        .gates
        .iter()
        .take(active_index)
        .rev()
        .find_map(|gate| {
            if gate.proof.kind() != Some(crate::core::tasks::profile::ProofKind::Contract) {
                return None;
            }
            let evidence = evidence_for(&gate.id, "passed")?;
            evidence
                .get("proof")
                .and_then(|proof| proof.get("contract_proof"))
                .or_else(|| evidence.get("contract_proof"))
                .and_then(|proof| proof.get("seam_test_ref"))
                .cloned()
        });

    let unresolved_review_findings = active_gate.and_then(|_| {
        let active_id = stage;
        gate_history.iter().rev().find_map(|rejected| {
            if rejected.get("state").and_then(Value::as_str) != Some("rejected") {
                return None;
            }
            let rejected_stage = rejected.get("stage").and_then(Value::as_str)?;
            let gate = profile.by_stage(rejected_stage)?;
            if gate.reject_to.as_deref() != Some(active_id)
                || !matches!(
                    gate.proof.kind(),
                    Some(
                        crate::core::tasks::profile::ProofKind::Review
                            | crate::core::tasks::profile::ProofKind::Delivery
                    )
                )
            {
                return None;
            }
            let revision = rejected.get("claim_revision").and_then(Value::as_u64)?;
            details
                .get("findings")?
                .as_array()?
                .iter()
                .rev()
                .find(|finding| {
                    finding.get("claim_revision").and_then(Value::as_u64) == Some(revision)
                })
                .and_then(|finding| finding.get("findings"))
                .and_then(|value| {
                    value
                        .as_str()
                        .and_then(|raw| {
                            serde_json::from_str::<Value>(raw)
                                .ok()
                                .or_else(|| Some(json!(raw)))
                        })
                        .or_else(|| Some(value.clone()))
                })
        })
    });

    let candidate_snapshot = snapshot_for(active_index);
    let latest_delivery_snapshot = snapshot_for(profile.gates.len());

    let milestone_ref = details.get("milestone_ref").cloned();
    let depends_on = details
        .get("spec")
        .and_then(|s| s.get("depends_on"))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let receipt = details.get("receipt").cloned();

    // 4. Active blackboard messages (bounded to 15 recent messages, with max 500-char payload and source tags)
    let blackboard_messages: Vec<Value> = {
        let registry = super::workspaces::Registry::load(root)?;
        let store = workspace.open(&registry.database)?;
        let milestone_ref_str = details
            .get("milestone_ref")
            .and_then(|v| v.as_str())
            .and_then(|m| store.milestone_scope_ref(m).ok());
        let milestone_prefix = task_id.split_once(':').map(|(p, _)| format!("{p}:"));
        let mut stmt = store.connection.prepare(
            "SELECT id, task_id, subtask_ref, gate, author, topic, payload, created_at FROM task_blackboard \
             WHERE (task_id = ?1 \
                OR (?2 IS NOT NULL AND milestone_ref = ?2 AND task_id IS NULL) \
                OR (?3 IS NOT NULL AND substr(task_id, 1, length(?3)) = ?3)) \
             AND (gate IS NULL OR gate = ?4) \
             ORDER BY created_at DESC, id DESC LIMIT 15",
        )?;
        let mut msgs = stmt
            .query_map(
                rusqlite::params![task_id, milestone_ref_str, milestone_prefix, stage],
                |row| {
                    let id = row.get::<_, u64>(0)?;
                    let msg_task_id = row.get::<_, Option<String>>(1)?;
                    let msg_subtask_ref = row.get::<_, Option<String>>(2)?;
                    let gate = row.get::<_, Option<String>>(3)?;
                    let author = row.get::<_, String>(4)?;
                    let topic = row.get::<_, String>(5)?;
                    let raw_payload: String = row.get(6)?;
                    let created_at = row.get::<_, i64>(7)?;
                    let payload = truncate_chars(&raw_payload, 500);

                    let scope = match msg_task_id.as_deref() {
                        None => "milestone",
                        Some(t) if t == task_id => {
                            if msg_subtask_ref.is_some() {
                                "subtask"
                            } else {
                                "task"
                            }
                        }
                        Some(_) => "sibling",
                    };

                    Ok(json!({
                        "id": id,
                        "scope": scope,
                        "task_id": msg_task_id,
                        "subtask_ref": msg_subtask_ref,
                        "gate": gate,
                        "author": author,
                        "topic": topic,
                        "payload": payload,
                        "created_at": created_at,
                    }))
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        msgs.reverse();
        msgs
    };

    // 5. Gate-aware workflow guidance and subtask DoD
    let recommended_tools = profile
        .by_stage(stage)
        .map(|gate| gate.tools.clone())
        .unwrap_or_else(|| profile.completed.tools.clone());
    let mut guidance = details["workflow_guidance"].clone();
    if let Some(guidance_map) = guidance.as_object_mut() {
        let steps = guidance_map
            .get("steps")
            .cloned()
            .unwrap_or_else(|| json!([]));
        guidance_map.insert("stage".into(), json!(stage));
        guidance_map.insert("recommended_tools".into(), json!(recommended_tools));
        guidance_map.insert("actionable_steps".into(), steps);
        guidance_map.insert(
            "subtask_dod".into(),
            json!(workspace.gates.subtask_dod.clone()),
        );
        let targeted_messages = blackboard_messages
            .iter()
            .filter(|message| message.get("gate").and_then(Value::as_str) == Some(stage))
            .cloned()
            .collect::<Vec<_>>();
        guidance_map.insert("blackboard_messages".into(), json!(targeted_messages));
        guidance_map.insert(
            "scoped_commands".into(),
            json!(scoped_commands(
                details
                    .get("worktree")
                    .and_then(Value::as_str)
                    .map(Path::new)
                    .unwrap_or(&workspace.root),
                &profile.commands,
                contract_seam_test.as_ref(),
                &covering_tests,
            )),
        );
        let proof_template = match proof_kind {
            Some(crate::core::tasks::profile::ProofKind::Contract) => json!({
                "contract_proof": {
                    "seam_test_ref": "<indexed test file or symbol>",
                    "red_exit_code": 101,
                }
            }),
            Some(crate::core::tasks::profile::ProofKind::Command) => json!({
                "command_proof": {
                    "command": "<registered command with scoped test target>",
                    "exit_code": 0,
                    "tests_passed": 1,
                    "tests_failed": 0,
                }
            }),
            Some(crate::core::tasks::profile::ProofKind::Review) => json!({
                "review_proof": {"decision": "pass", "contours": "<one result per configured contour>"}
            }),
            Some(crate::core::tasks::profile::ProofKind::Delivery) => {
                json!({"delivery_proof": {"delivered": true}})
            }
            _ => json!({}),
        };
        guidance_map.insert(
            "expected_evidence_template".into(),
            json!({
                "task_id": task_id,
                "stage": stage,
                "claim_revision": details["claim_revision"],
                "contract_revision": details["contract_revision"],
                "worker_id": details["worker_id"],
                "worktree": details["worktree"],
                "proof": proof_template,
            }),
        );
    }
    let mut topic_counts = serde_json::Map::new();
    let mut targeted_count = 0usize;
    for message in &blackboard_messages {
        if message.get("gate").and_then(Value::as_str) == Some(stage) {
            targeted_count += 1;
        }
        if let Some(topic) = message.get("topic").and_then(Value::as_str) {
            let count = topic_counts
                .entry(topic.to_owned())
                .or_insert_with(|| json!(0));
            *count = json!(count.as_u64().unwrap_or(0) + 1);
        }
    }
    let blackboard_info = json!({
        "active_gate": stage,
        "message_count": blackboard_messages.len(),
        "targeted_count": targeted_count,
        "topic_counts": topic_counts,
        "message_ids": blackboard_messages.iter().filter_map(|message| message.get("id").cloned()).collect::<Vec<_>>(),
    });
    let mut bundle_map = serde_json::Map::new();
    bundle_map.insert(
        "contract".into(),
        json!({
            "task_id": task_id,
            "target": details["goal"],
            "stage": stage,
            "status": details["status"],
            "proof_policy": details["proof_policy"],
            "allowed_scope": scope,
            "subtasks": details["subtasks"],
            "depends_on": depends_on.clone(),
            "invariants": details["applicable_invariants"].as_array().cloned().unwrap_or_default(),
            "contract_revision": details["contract_revision"],
            "claim_revision": details["claim_revision"],
            "worker_id": details["worker_id"],
            "worktree": details["worktree"],
        }),
    );
    bundle_map.insert("guidance".into(), guidance.clone());

    if let Some(crate::core::tasks::profile::ProofDef::Scheme { scheme }) =
        active_gate.map(|gate| &gate.proof)
    {
        bundle_map.insert("proof_schema".into(), serde_json::to_value(scheme)?);
    }

    if matches!(
        proof_kind,
        Some(
            crate::core::tasks::profile::ProofKind::Contract
                | crate::core::tasks::profile::ProofKind::Command
                | crate::core::tasks::profile::ProofKind::Review
        )
    ) {
        bundle_map.insert("adrs".into(), json!(adrs));
        bundle_map.insert("governing_invariants".into(), governing_invariants);
    }
    if matches!(
        proof_kind,
        Some(
            crate::core::tasks::profile::ProofKind::Contract
                | crate::core::tasks::profile::ProofKind::Command
        )
    ) {
        bundle_map.insert("scope_symbols".into(), json!(scope_symbols));
        bundle_map.insert("covering_tests".into(), json!(covering_tests));
        bundle_map.insert("external_callers".into(), json!(external_callers));
    }
    if proof_kind == Some(crate::core::tasks::profile::ProofKind::Command) {
        if let Some(seam) = contract_seam_test {
            bundle_map.insert("contract_seam_test".into(), seam);
        }
        if let Some(findings) = unresolved_review_findings {
            bundle_map.insert("unresolved_review_findings".into(), findings);
        }
    }
    if proof_kind == Some(crate::core::tasks::profile::ProofKind::Review) {
        if let Some(snapshot) = candidate_snapshot.as_ref() {
            bundle_map.insert("candidate_snapshot".into(), snapshot.clone());
        }
    }
    if proof_kind == Some(crate::core::tasks::profile::ProofKind::Delivery)
        || details.get("status").and_then(|s| s.as_str()) == Some("completed")
    {
        if let Some(snapshot) = latest_delivery_snapshot.as_ref() {
            bundle_map.insert("latest_snapshot".into(), snapshot.clone());
        }
        if let Some(m_ref) = milestone_ref.as_ref() {
            bundle_map.insert("milestone_ref".into(), m_ref.clone());
        }
        if details.get("status").and_then(|s| s.as_str()) == Some("completed") {
            if let Some(r) = receipt.as_ref() {
                bundle_map.insert("receipt".into(), r.clone());
            }
        }
    }
    bundle_map.insert("blackboard".into(), json!(blackboard_messages));
    bundle_map.insert("blackboard_info".into(), blackboard_info.clone());

    let bundle = Value::Object(bundle_map);

    let obj = details
        .as_object_mut()
        .context("details must be an object")?;

    // Prune raw historical archives and table dumps from the response
    obj.remove("gates");
    obj.remove("attempts");
    obj.remove("findings");
    obj.remove("spec");
    obj.remove("receipt");
    obj.remove("adrs");
    obj.remove("scope_symbols");
    obj.remove("covering_tests");

    // Retain bounded blackboard and contract essentials at root
    obj.insert("depends_on".into(), depends_on);
    obj.insert("blackboard".into(), json!(blackboard_messages));
    obj.insert("blackboard_info".into(), blackboard_info.clone());
    obj.insert("workflow_guidance".into(), guidance.clone());
    obj.insert("context_bundle".into(), bundle);

    Ok(details)
}
