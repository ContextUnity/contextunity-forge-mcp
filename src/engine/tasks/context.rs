use crate::engine::tasks::workspaces::Workspace;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, path::Path};

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
                "SELECT name, kind, path, line, details FROM nodes WHERE is_test = 0 AND kind IN ('struct', 'enum', 'trait', 'class', 'type', 'interface', 'function') AND path LIKE ?1 || '%' ORDER BY path, line LIMIT 15",
                scope_path.clone(),
            )
        } else {
            (
                "SELECT name, kind, path, line, details FROM nodes WHERE is_test = 0 AND kind IN ('struct', 'enum', 'trait', 'class', 'type', 'interface', 'function') AND path = ?1 ORDER BY line LIMIT 15",
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
                            .map(str::to_owned)
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
                    "n.path LIKE ?1 || '%'"
                } else {
                    "n.path = ?1"
                };
                let sql = format!("SELECT DISTINCT t.path,t.name FROM nodes n JOIN edges e ON e.dst_hash=n.node_hash AND e.kind IN ('calls','imports') JOIN nodes t ON t.node_hash=e.src_hash WHERE {predicate} AND t.is_test=1 ORDER BY t.path,t.name LIMIT 15");
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

    // 4. Active blackboard messages
    let blackboard_messages: Vec<Value> = {
        let registry = super::workspaces::Registry::load(root)?;
        let store = workspace.open(&registry.database)?;
        store
            .blackboard_read(task_id, None, Some(50))?
            .into_iter()
            .map(|msg| {
                json!({
                    "id": msg.id,
                    "author": msg.author,
                    "topic": msg.topic,
                    "payload": msg.payload,
                    "created_at": msg.created_at
                })
            })
            .collect()
    };

    // 5. Gate-aware workflow guidance and subtask DoD
    let stage = if details["status"] == "completed" {
        "completed"
    } else {
        details["stage"].as_str().unwrap_or("contract/v1")
    };

    let (recommended_tools, actionable_steps): (&[&str], &[&str]) = match stage {
        "contract/v1" => (
            &["code_map_overview", "get_doc", "search_docs", "code_map_inspect"],
            &[
                "Inspect task target, scope, governing ADRs, and invariants.",
                "Review existing code symbols in scope to identify extension seams.",
                "Author a sensitive failing seam test in the designated test suite (or verify existing seam directly with exit code 0 if proof_policy: direct-proof or deferred-final-test).",
                "Submit contract proof with seam test and red_exit_code.",
            ],
        ),
        "build/v1" => (
            &["ast_grep_search", "code_map_inspect", "code_map_explain", "task_manage"],
            &[
                "Implement approved contract inside the declared allowed_write_scope.",
                "Decompose non-trivial implementation into iterative subtasks satisfying Universal Subtask DoD.",
                "Turn the red seam test green; verify targeted domain test suite.",
                "Run linter (cargo clippy --all-targets --all-features -- -D warnings).",
                "Submit passing test proof with test command, exit code 0, and passed counts.",
            ],
        ),
        "review/v1" => (
            &["code_map_impact", "code_map_prove_removal", "code_map_tests"],
            &[
                "Verify reviewer is distinct from builder (different worker_id).",
                "Review candidate diff across the five contours: paths, claims, concurrency, project_isolation, administration.",
                "Confirm no invented requirements or scope leaks.",
                "Submit review proof with decision: pass.",
            ],
        ),
        "deliver/v1" => (
            &["task_submit", "milestone show"],
            &[
                "Verify delivery worker is distinct from builder.",
                "Submit delivery proof; Forge verifies proofs, writes typed receipt into milestone Markdown, and clears blackboard.",
                "Amend candidate commit with the generated milestone receipt.",
            ],
        ),
        _ => (
            &["task_manage"],
            &["Read the durable task receipt for completed work."],
        ),
    };

    let subtask_dod = [
        "Contract slice resolution: Demonstrably resolves an explicit, bounded slice of the contract without breaking boundaries.",
        "Milestone & ADR alignment: Builds upon existing architectural seams rather than ad-hoc isolated patches.",
        "Production-seam evidence: Validates through real production paths rather than synthetic stubs mocking away system complexity.",
        "Systemic non-regression: Zero warnings/lints and complete behavioral preservation of untouched invariants.",
        "Anti-looping invariant: If an approach fails after 2 iterations, halt, post blockers to task_blackboard, retain fail-closed behavior, and escalate.",
    ];

    let mut guidance = details["workflow_guidance"].clone();
    if let Some(guidance_map) = guidance.as_object_mut() {
        guidance_map.insert("stage".into(), json!(stage));
        guidance_map.insert("recommended_tools".into(), json!(recommended_tools));
        guidance_map.insert("actionable_steps".into(), json!(actionable_steps));
        guidance_map.insert("subtask_dod".into(), json!(subtask_dod));
    }

    let bundle = json!({
        "contract": {
            "task_id": task_id,
            "target": details["goal"],
            "stage": stage,
            "status": details["status"],
            "proof_policy": details["proof_policy"],
            "allowed_scope": scope,
            "subtasks": details["subtasks"],
            "invariants": details["applicable_invariants"].as_array().cloned().unwrap_or_default(),
            "contract_revision": details["contract_revision"],
            "claim_revision": details["claim_revision"],
            "worker_id": details["worker_id"],
            "worktree": details["worktree"],
        },
        "guidance": guidance,
        "adrs": adrs,
        "scope_symbols": scope_symbols,
        "covering_tests": covering_tests,
        "blackboard": blackboard_messages,
    });

    let obj = details
        .as_object_mut()
        .context("details must be an object")?;
    obj.insert("workflow_guidance".into(), guidance);
    obj.insert("context_bundle".into(), bundle);
    obj.insert("adrs".into(), json!(adrs));
    obj.insert("scope_symbols".into(), json!(scope_symbols));
    obj.insert("covering_tests".into(), json!(covering_tests));
    obj.insert("blackboard".into(), json!(blackboard_messages));

    Ok(details)
}
