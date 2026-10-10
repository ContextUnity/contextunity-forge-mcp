use crate::engine::tasks::workspaces::Workspace;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, path::Path};

/// Truncate string by Unicode scalar count without slicing invalid byte boundaries.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let prefix: String = s.chars().take(max_chars).collect();
        format!("{prefix}... [truncated]")
    }
}

/// Extract alphanumeric tokens of length >= 3 from paths, ignoring common folder/file extensions.
fn extract_scope_tokens(scope: &[String]) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();
    let ignored = [
        "src", "rs", "py", "ts", "js", "go", "java", "mod", "lib", "main", "core", "test", "tests",
    ];
    for path in scope {
        for segment in path.split(['/', '\\', '.', '_', '-']) {
            let lower = segment.to_ascii_lowercase();
            if lower.len() >= 3 && !ignored.contains(&lower.as_str()) {
                tokens.insert(lower);
            }
        }
    }
    tokens
}

/// Scan docs/adr/ and docs/architecture/ for architectural documents governing the task scope.
fn scan_adrs(workspace_root: &Path, root: &Path, tokens: &BTreeSet<String>) -> Vec<Value> {
    let mut results = Vec::new();
    let mut seen_paths = BTreeSet::new();

    let candidate_dirs = [
        (workspace_root.join("docs/adr"), workspace_root),
        (workspace_root.join("docs/architecture"), workspace_root),
        (root.join("docs/adr"), root),
        (root.join("docs/architecture"), root),
    ];

    for (dir, base) in candidate_dirs {
        if !dir.is_dir() {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            if path.file_name().and_then(|s| s.to_str()) == Some("README.md") {
                continue;
            }
            let rel_path = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            if seen_paths.contains(&rel_path) {
                continue;
            }
            seen_paths.insert(rel_path.clone());

            let text = fs::read_to_string(&path).unwrap_or_default();
            let text = text.replace("\r\n", "\n");
            let mut title = String::new();
            let mut status = "accepted".to_string();

            // Extract frontmatter or heading
            if let Some(stripped) = text.strip_prefix("---\n") {
                if let Some((frontmatter, _)) = stripped.split_once("\n---\n") {
                    for line in frontmatter.lines() {
                        if let Some(t) = line.strip_prefix("title:") {
                            title = t.trim().trim_matches('"').trim_matches('\'').to_string();
                        } else if let Some(s) = line.strip_prefix("status:") {
                            status = s.trim().trim_matches('"').trim_matches('\'').to_string();
                        }
                    }
                }
            }
            if title.is_empty() {
                for line in text.lines() {
                    if let Some(h) = line.strip_prefix("# ") {
                        title = h.trim().to_string();
                        break;
                    }
                }
            }
            if title.is_empty() {
                title = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
            }

            let text_lower = format!(
                "{} {}",
                rel_path.to_ascii_lowercase(),
                text.to_ascii_lowercase()
            );
            let matches_token = tokens.iter().any(|tok| text_lower.contains(tok));

            if !matches_token {
                continue;
            }

            results.push(json!({
                "path": rel_path,
                "title": title,
                "status": status,
                "relevance": "direct",
            }));
        }
    }

    results.sort_by(|a, b| {
        let a_rel = a["relevance"].as_str() == Some("direct");
        let b_rel = b["relevance"].as_str() == Some("direct");
        b_rel
            .cmp(&a_rel)
            .then_with(|| a["path"].as_str().cmp(&b["path"].as_str()))
    });
    results.truncate(10);
    results
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
                let sql = format!("SELECT DISTINCT tp.path,t.name FROM nodes n JOIN path_dictionary np ON np.path_id=n.path_id JOIN edges e ON e.dst_hash=n.node_hash AND e.kind IN ('calls','imports') JOIN nodes t ON t.node_hash=e.src_hash JOIN path_dictionary tp ON tp.path_id=t.path_id WHERE {predicate} AND t.is_test=1 ORDER BY tp.path,t.name LIMIT 15");
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
            }
        }
    }

    covering.truncate(15);
    covering
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
        details.get("stage").and_then(Value::as_str).context("task stage missing")?
    };
    let active_index = if stage == "completed" {
        profile.gates.len()
    } else {
        profile.index_of(stage).with_context(|| format!(
            "TASK_STAGE_UNKNOWN: '{stage}' is not in the active profile; choose one of: {}",
            profile.gates.iter().map(|gate| gate.id.as_str()).collect::<Vec<_>>().join(", ")
        ))?
    };
    let active_gate = profile.gates.get(active_index);
    let proof_kind = active_gate.and_then(|gate| gate.proof.kind());
    let gate_history = details.get("gates").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
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
        profile.gates.iter().take(before_index).rev().find_map(|gate| {
            if !gate.sha_snapshot { return None; }
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

    let tokens = extract_scope_tokens(&scope);

    // 1. Scope-to-ADR mapping
    let adrs = scan_adrs(&workspace.root, root, &tokens);

    // 2. Scope symbols skeleton from code map
    let scope_symbols = query_scope_symbols(&workspace.root, root, &scope);

    // 3. Covering test seams
    let covering_tests = find_covering_tests(&workspace.root, root, &scope);

    // Select proof context by configured gate taxonomy and references.
    let contract_seam_test = profile.gates.iter().take(active_index).rev().find_map(|gate| {
        if gate.proof.kind() != Some(crate::core::tasks::profile::ProofKind::Contract) { return None; }
        let evidence = evidence_for(&gate.id, "passed")?;
        evidence.get("proof")
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
                || !matches!(gate.proof.kind(), Some(crate::core::tasks::profile::ProofKind::Review | crate::core::tasks::profile::ProofKind::Delivery))
            {
                return None;
            }
            let revision = rejected.get("claim_revision").and_then(Value::as_u64)?;
            details.get("findings")?.as_array()?.iter().rev().find(|finding| {
                finding.get("claim_revision").and_then(Value::as_u64) == Some(revision)
            }).and_then(|finding| finding.get("findings")).and_then(|value| {
                value.as_str().and_then(|raw| serde_json::from_str::<Value>(raw).ok().or_else(|| Some(json!(raw))))
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
            "SELECT id, task_id, subtask_ref, author, topic, payload, created_at FROM task_blackboard \
             WHERE task_id = ?1 \
                OR (?2 IS NOT NULL AND milestone_ref = ?2 AND task_id IS NULL) \
                OR (?3 IS NOT NULL AND substr(task_id, 1, length(?3)) = ?3) \
             ORDER BY created_at DESC, id DESC LIMIT 15",
        )?;
        let mut msgs = stmt
            .query_map(
                rusqlite::params![task_id, milestone_ref_str, milestone_prefix],
                |row| {
                    let id = row.get::<_, u64>(0)?;
                    let msg_task_id = row.get::<_, Option<String>>(1)?;
                    let msg_subtask_ref = row.get::<_, Option<String>>(2)?;
                    let author = row.get::<_, String>(3)?;
                    let topic = row.get::<_, String>(4)?;
                    let raw_payload: String = row.get(5)?;
                    let created_at = row.get::<_, i64>(6)?;
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
    }
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
    bundle_map.insert("guidance".into(), guidance);

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
    obj.insert("context_bundle".into(), bundle);

    Ok(details)
}
