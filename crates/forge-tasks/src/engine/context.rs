use crate::engine::workspaces::Workspace;
use crate::store::Task;
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
    let sql = "WITH scope_nodes(hash) AS (SELECT DISTINCT target.node_hash FROM nodes target JOIN path_dictionary tp ON tp.path_id=target.path_id WHERE EXISTS(SELECT 1 FROM json_each(?1) s WHERE tp.path=trim(s.value,'/') OR substr(tp.path,1,length(trim(s.value,'/'))+1)=trim(s.value,'/')||'/')), governed_paths(path,direct_doc_id) AS (SELECT DISTINCT d.path,d.doc_id FROM edges e JOIN nodes n ON n.node_hash=e.src_hash JOIN doc_sections d ON ((e.kind='governs' AND d.doc_id=json_extract(n.details,'$.document_id')) OR (e.kind='documents' AND d.doc_id=n.id)) JOIN scope_nodes s ON s.hash=e.dst_hash WHERE e.kind IN ('governs','documents') AND n.kind IN ('document','invariant') AND d.doc_type != 'contract' AND d.path NOT LIKE 'archive/%' AND d.path NOT LIKE '%/archive/%' AND d.path NOT LIKE 'docs/milestones/%' AND d.path NOT LIKE '%/docs/milestones/%' AND d.path NOT LIKE 'docs/contracts/%' AND d.path NOT LIKE '%/docs/contracts/%') SELECT d.path,d.section_title,d.content,n.details,EXISTS(SELECT 1 FROM governed_paths gp WHERE gp.path=d.path AND gp.direct_doc_id=d.doc_id) AS is_direct_section FROM (SELECT DISTINCT path FROM governed_paths) g JOIN doc_sections d ON d.path=g.path JOIN nodes n ON n.id=d.doc_id ORDER BY d.path,d.rowid LIMIT 200";
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
            row.get::<_, bool>(4)?,
        ))
    }) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };
    let mut documents = BTreeMap::<String, Value>::new();
    let mut navigation_counts = BTreeMap::<String, usize>::new();
    let mut section_counts = BTreeMap::<String, usize>::new();
    for row in rows.flatten() {
        let (path, title, content, details, is_direct_section) = row;
        let navigation_count = navigation_counts.entry(path.clone()).or_default();
        let section_count = section_counts.entry(path.clone()).or_default();
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
        let mut kept_in_section = false;
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
            if keep {
                kept_in_section = true;
                if !summary.iter().any(|item| item.as_str() == Some(line)) {
                    summary.push(json!(line));
                    if !decision && !decision_section && navigation_rule {
                        *navigation_count += 1;
                    }
                }
            }
        }
        if is_direct_section && !kept_in_section && !title.trim().is_empty() && *section_count < 5 {
            let section = json!(format!("Section: {}", title.trim()));
            if !summary.contains(&section) {
                summary.push(section);
                *section_count += 1;
            }
        }
    }
    documents.into_values().take(10).collect()
}

fn governing_invariants_for_scope(conn: &rusqlite::Connection, scope: &[String]) -> Result<Value> {
    const DIRECT_LIMIT: usize = 5;
    const TRANSITIVE_LIMIT: usize = 5;
    if scope.is_empty() {
        return Ok(empty_governing_invariants());
    }
    let scope_json = serde_json::to_string(scope)?;
    let scope_nodes = "WITH scope_nodes(hash) AS (SELECT DISTINCT n.node_hash FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE EXISTS(SELECT 1 FROM json_each(?1) s WHERE p.path=trim(s.value,'/') OR substr(p.path,1,length(trim(s.value,'/'))+1)=trim(s.value,'/')||'/'))";
    let mut stmt = conn.prepare(&format!("{scope_nodes} SELECT DISTINCT n.name, json_extract(n.details,'$.rule'), p.path, n.line FROM edges e JOIN scope_nodes s ON s.hash=e.dst_hash JOIN nodes n ON n.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=n.path_id WHERE e.kind='governs' ORDER BY n.name, p.path, n.line LIMIT ?2"))?;
    let direct_rows = stmt
        .query_map(
            rusqlite::params![&scope_json, &(DIRECT_LIMIT as i64)],
            |row| {
                let name: String = row.get(0)?;
                let rule: Option<String> = row.get(1)?;
                let path: String = row.get(2)?;
                let line: i64 = row.get(3)?;
                Ok(json!({
                    "id": name,
                    "rule": rule,
                    "source": { "path": path, "line": line }
                }))
            },
        )?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    let neighbors = "neighbors(hash) AS (SELECT e.src_hash FROM edges e JOIN scope_nodes s ON s.hash=e.dst_hash WHERE e.kind IN('calls','possible_calls') UNION SELECT e.dst_hash FROM edges e JOIN scope_nodes s ON s.hash=e.src_hash WHERE e.kind IN('calls','possible_calls'))";
    let mut stmt = conn.prepare(&format!("{scope_nodes}, {neighbors}, direct AS (SELECT DISTINCT e.src_hash FROM edges e JOIN scope_nodes s ON s.hash=e.dst_hash WHERE e.kind='governs') SELECT DISTINCT n.name, json_extract(n.details,'$.rule'), p.path, n.line FROM edges e JOIN neighbors g ON g.hash=e.dst_hash JOIN nodes n ON n.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=n.path_id WHERE e.kind='governs' AND NOT EXISTS(SELECT 1 FROM direct d WHERE d.src_hash=e.src_hash) ORDER BY n.name, p.path, n.line LIMIT ?2"))?;
    let transitive_rows = stmt
        .query_map(
            rusqlite::params![&scope_json, &(TRANSITIVE_LIMIT as i64)],
            |row| {
                let name: String = row.get(0)?;
                let _rule: Option<String> = row.get(1)?;
                let path: String = row.get(2)?;
                let line: i64 = row.get(3)?;
                Ok((
                    name.clone(),
                    json!({ "id": name, "source": { "path": path, "line": line } }),
                ))
            },
        )?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    let transitive_ids = transitive_rows
        .iter()
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let omitted_sources = transitive_rows
        .iter()
        .map(|(_, src)| src.clone())
        .collect::<Vec<_>>();

    let count_nodes = format!("{scope_nodes}, direct AS (SELECT DISTINCT e.src_hash FROM edges e JOIN scope_nodes s ON s.hash=e.dst_hash WHERE e.kind='governs') SELECT count(*) FROM direct");
    let mut count_stmt = conn.prepare(&count_nodes)?;
    let direct_total: usize = count_stmt
        .query_row(rusqlite::params![&scope_json], |row| row.get(0))
        .unwrap_or(direct_rows.len());

    let count_transitive = format!("{scope_nodes}, {neighbors}, direct AS (SELECT DISTINCT e.src_hash FROM edges e JOIN scope_nodes s ON s.hash=e.dst_hash WHERE e.kind='governs'), transitive AS (SELECT DISTINCT e.src_hash FROM edges e JOIN neighbors n ON n.hash=e.dst_hash WHERE e.kind='governs' AND NOT EXISTS(SELECT 1 FROM direct d WHERE d.src_hash=e.src_hash)) SELECT count(*) FROM transitive");
    let mut count_stmt2 = conn.prepare(&count_transitive)?;
    let transitive_total: usize = count_stmt2
        .query_row(rusqlite::params![&scope_json], |row| row.get(0))
        .unwrap_or(transitive_rows.len());

    Ok(json!({
        "direct": direct_rows,
        "omitted_direct": direct_total.saturating_sub(direct_rows.len()),
        "transitive_ids": transitive_ids,
        "omitted_transitive": transitive_total.saturating_sub(transitive_rows.len()),
        "omitted_sources": omitted_sources,
    }))
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
    let governing = governing_invariants_for_scope(&conn, scope)
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

    let mut documentation = conn.prepare("SELECT DISTINCT d.path || '#' || d.section_title FROM edges e JOIN nodes target ON target.node_hash=e.dst_hash JOIN path_dictionary tp ON tp.path_id=target.path_id JOIN nodes doc_node ON doc_node.node_hash=e.src_hash JOIN doc_sections d ON ((e.kind='documents' AND d.doc_id=doc_node.id) OR (e.kind='governs' AND d.doc_id=json_extract(doc_node.details,'$.document_id'))) WHERE e.kind IN ('documents','governs') AND doc_node.kind IN ('document','invariant') AND d.doc_type != 'contract' AND d.path NOT LIKE 'archive/%' AND d.path NOT LIKE '%/archive/%' AND d.path NOT LIKE 'docs/milestones/%' AND d.path NOT LIKE '%/docs/milestones/%' AND tp.path=?1 AND target.name=?2 AND target.line=?3 ORDER BY d.path,d.rowid LIMIT 5").ok();
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
                let docs = documentation
                    .as_mut()
                    .and_then(|statement| {
                        statement
                            .query_map(rusqlite::params![&path, &name, line], |row| {
                                row.get::<_, String>(0)
                            })
                            .ok()
                            .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
                    })
                    .unwrap_or_default();
                let mut symbol = json!({
                    "name": name,
                    "kind": kind,
                    "path": path,
                    "line": line,
                    "signature": signature,
                });
                if !docs.is_empty() {
                    symbol["documented_by"] = json!(docs);
                }
                Ok(symbol)
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

fn scoped_commands(
    profile: &crate::tasks::profile::GateProfile,
    seam: Option<&Value>,
    covering_tests: &[Value],
) -> Vec<Value> {
    let selected_commands = profile
        .commands
        .iter()
        .map(|(name, definition)| (name.as_str(), definition))
        .collect::<Vec<_>>();
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
            selected_commands
                .iter()
                .filter_map(move |(name, definition)| {
                    let command = definition.command_for_path(&path)?;
                    let scope_target = profile
                        .scope_args_for_command_path(definition, &path)
                        .unwrap_or_else(|| path.clone());
                    let command = if command.contains("{scope_target}") {
                        command.replace("{scope_target}", &scope_target)
                    } else {
                        format!("{command} {scope_target}")
                    };
                    Some(json!({
                        "name": name,
                        "source": source,
                        "test_path": path.clone(),
                        "command": command,
                    }))
                })
        })
        .take(30)
        .collect()
}

fn linked_milestone_context(
    root: &Path,
    details: &Value,
    dependencies: &[String],
) -> Result<Vec<Value>> {
    let authority = details.get("authority_snapshot");
    let mut references = authority
        .and_then(|snapshot| snapshot.get("milestone_linked_to"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    for dependency in dependencies {
        if let Some((parent, _)) = dependency.rsplit_once(':') {
            if let Some(milestone) = parent.rsplit('/').next() {
                if milestone.starts_with("m-") {
                    references.insert(milestone.to_owned());
                }
            }
        }
    }
    if references.is_empty() {
        return Ok(Vec::new());
    }

    let registry = super::workspaces::Registry::load(root)?;
    let mut linked = Vec::new();
    for reference in references {
        let (workspace, manifest_ref) = match registry.resolve_milestone(Some("all"), &reference) {
            Ok(resolved) => resolved,
            Err(error) => {
                linked.push(json!({
                    "reference": reference,
                    "available": false,
                    "error": truncate_chars(&error.to_string(), 240),
                }));
                continue;
            }
        };
        let manifest_path = crate::tasks::confined_path(&workspace.root, &manifest_ref)?;
        let manifest_text = std::fs::read_to_string(&manifest_path)?;
        let archived = manifest_path
            .components()
            .any(|component| component.as_os_str() == "archive");
        let status = crate::tasks::Milestone::status_from_frontmatter(&manifest_text, archived)?;
        let milestone_id = manifest_text
            .strip_prefix("---\n")
            .and_then(|frontmatter| frontmatter.split_once("\n---\n"))
            .and_then(|(frontmatter, _)| {
                serde_yaml::from_str::<serde_yaml::Value>(frontmatter).ok()
            })
            .and_then(|frontmatter| {
                frontmatter
                    .get("id")
                    .and_then(serde_yaml::Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| reference.clone());
        let store = registry.open(workspace)?;
        let tasks = store.list(Some(&manifest_ref), "all", None)?;
        let task_states = tasks
            .iter()
            .take(100)
            .map(|task| {
                json!({
                    "task_id": task.task_id,
                    "task_ref": task.spec.task_ref,
                    "status": task.status,
                    "stage": task.stage,
                })
            })
            .collect::<Vec<_>>();
        let dependency_states = dependencies
            .iter()
            .filter_map(|dependency| {
                let requested_ref = dependency.rsplit_once(':').map(|(_, task_ref)| task_ref)?;
                let task = tasks.iter().find(|task| {
                    task.task_id == *dependency || task.spec.task_ref == requested_ref
                })?;
                Some(json!({
                    "dependency": dependency,
                    "task_id": task.task_id,
                    "task_ref": task.spec.task_ref,
                    "status": task.status,
                    "stage": task.stage,
                    "blocking": task.status != "completed",
                }))
            })
            .collect::<Vec<_>>();
        linked.push(json!({
            "reference": reference,
            "milestone_id": milestone_id,
            "status": status,
            "workspace": workspace.name,
            "manifest_ref": manifest_ref,
            "available": true,
            "task_count": tasks.len(),
            "tasks": task_states,
            "dependency_states": dependency_states,
        }));
    }
    Ok(linked)
}

fn validate_gate_requirements(
    profile: &crate::tasks::profile::GateProfile,
    active_gate: Option<&crate::tasks::profile::GateDef>,
    modules: &crate::tasks::catalog::SubsystemRegistry,
) -> Result<()> {
    for required in &profile.modules.required {
        if !modules.is_enabled(required) {
            anyhow::bail!(
                "FEATURE_REQUIRED_BY_PROFILE: the active task profile requires disabled Forge subsystem '{required}'"
            );
        }
    }
    if let Some(gate) = active_gate {
        for tool in &gate.tools {
            if let Some(subsystem) =
                crate::tasks::catalog::SubsystemRegistry::subsystem_for_tool(tool)
            {
                if !modules.is_enabled(subsystem) {
                    anyhow::bail!(
                        "FEATURE_REQUIRED_BY_PROFILE: active gate '{}' requires tool '{}' from disabled Forge subsystem '{subsystem}'",
                        gate.id,
                        tool
                    );
                }
            }
        }
    }
    Ok(())
}

/// Reject a claim whose active profile gate depends on a disabled workspace subsystem.
pub(super) fn validate_claim_gate_requirements(workspace: &Workspace, task: &Task) -> Result<()> {
    let details = serde_json::to_value(task)?;
    let profile = workspace.profile_for_details(&details)?;
    let adapter = crate::config::load_adapter(&workspace.root, None)
        .context("failed to resolve workspace subsystem configuration")?;
    let stage = if task.status == "completed" {
        "completed"
    } else {
        task.stage.as_str()
    };
    let active_gate = if stage == "completed" {
        None
    } else {
        Some(super::claim::cursor_for_stage(&profile, stage)?.gate)
    };
    validate_gate_requirements(&profile, active_gate.as_ref(), &adapter.modules)
}

fn is_rfc_message(message: &Value) -> bool {
    message
        .get("payload")
        .and_then(Value::as_str)
        .is_some_and(|payload| payload.trim_start().starts_with("RFC:"))
}

fn prune_blackboard_guidance(guidance: &mut Value) {
    let Some(guidance_map) = guidance.as_object_mut() else {
        return;
    };
    for field in ["steps", "rules", "subtask_dod"] {
        if let Some(items) = guidance_map.get_mut(field).and_then(Value::as_array_mut) {
            items.retain(|item| {
                !item
                    .as_str()
                    .is_some_and(|instruction| instruction.contains("task_blackboard"))
            });
        }
    }
}

/// Remove blackboard instructions from task details when the owning workspace disables it.
pub(super) fn adapt_workspace_guidance(workspace: &Workspace, mut details: Value) -> Result<Value> {
    let adapter = crate::config::load_adapter(&workspace.root, None)
        .context("failed to resolve workspace subsystem configuration")?;
    if !adapter.modules.is_enabled("blackboard") {
        if let Some(guidance) = details.get_mut("workflow_guidance") {
            prune_blackboard_guidance(guidance);
        }
    }
    Ok(details)
}

pub(super) fn context_bundle(
    root: &Path,
    workspace: &Workspace,
    task_id: &str,
    mut details: Value,
) -> Result<Value> {
    let profile = workspace.profile_for_details(&details)?;
    let adapter = crate::config::load_adapter(&workspace.root, None)
        .context("failed to resolve workspace subsystem configuration")?;
    let modules = adapter.modules;
    let stage = if details.get("status").and_then(Value::as_str) == Some("completed") {
        "completed"
    } else {
        details
            .get("stage")
            .and_then(Value::as_str)
            .context("task stage missing")?
    };
    let gate_sequence = super::claim::gate_sequence(&profile)?;
    let active_cursor = if stage == "completed" {
        None
    } else {
        Some(super::claim::cursor_for_stage(&profile, stage)?)
    };
    let active_index = active_cursor
        .as_ref()
        .map(|cursor| cursor.flat_index)
        .unwrap_or(gate_sequence.len());
    let active_gate = active_cursor.as_ref().map(|cursor| &cursor.gate);
    validate_gate_requirements(&profile, active_gate, &modules)?;
    let codemap_enabled = modules.is_enabled("codemap");
    let blackboard_enabled = modules.is_enabled("blackboard");
    let proof_kind = active_gate.and_then(|gate| gate.proof.kind());
    let gate_history = details
        .get("gates")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let evidence_for_cursor =
        |cursor: &super::claim::GateCursor, expected_state: &str| -> Option<Value> {
            gate_history.iter().rev().find_map(|entry| {
                if entry.get("state").and_then(Value::as_str) != Some(expected_state) {
                    return None;
                }
                let stage = entry.get("stage").and_then(Value::as_str)?;
                let historical_cursor = super::claim::cursor_for_stage(&profile, stage).ok()?;
                (historical_cursor.canonical_stage_id == cursor.canonical_stage_id)
                    .then(|| entry.get("evidence").and_then(Value::as_str))
                    .flatten()
                    .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            })
        };
    let snapshot_for = |before_index: usize| -> Option<Value> {
        gate_sequence
            .iter()
            .take(before_index)
            .rev()
            .find_map(|cursor| {
                let gate = &cursor.gate;
                if !gate.sha_snapshot {
                    return None;
                }
                let evidence = evidence_for_cursor(cursor, "passed")?;
                let commit = evidence.get("commit")?.as_str()?;
                Some(json!({
                    "stage": cursor.canonical_stage_id,
                    "commit": commit,
                    "candidate_baseline_head": evidence.get("candidate_baseline_head"),
                    "inspect_cmd": crate::tasks::snapshot_inspect_cmd(commit),
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
    let (adrs, governing_invariants) = if codemap_enabled {
        query_indexed_context(&workspace.root, root, &scope)
    } else {
        (Vec::new(), empty_governing_invariants())
    };

    // 2. Scope symbols skeleton from code map
    let scope_symbols = if codemap_enabled {
        query_scope_symbols(&workspace.root, root, &scope)
    } else {
        Vec::new()
    };

    // 3. Covering test seams
    let (covering_tests, external_callers) = if codemap_enabled {
        (
            find_covering_tests(&workspace.root, root, &scope),
            find_external_callers(&workspace.root, root, &scope),
        )
    } else {
        (Vec::new(), Vec::new())
    };

    // Select proof context by configured gate taxonomy and references.
    let contract_seam_test = gate_sequence
        .iter()
        .take(active_index)
        .rev()
        .find_map(|cursor| {
            let gate = &cursor.gate;
            if gate.proof.kind() != Some(crate::tasks::profile::ProofKind::Contract) {
                return None;
            }
            let evidence = evidence_for_cursor(cursor, "passed")?;
            evidence
                .get("proof")
                .and_then(|proof| proof.get("contract_proof"))
                .or_else(|| evidence.get("contract_proof"))
                .and_then(|proof| proof.get("seam_test_ref"))
                .cloned()
        });

    let unresolved_review_findings = active_cursor.as_ref().and_then(|active_cursor| {
        gate_history.iter().rev().find_map(|rejected| {
            if rejected.get("state").and_then(Value::as_str) != Some("rejected") {
                return None;
            }
            let rejected_stage = rejected.get("stage").and_then(Value::as_str)?;
            let rejected_cursor = super::claim::cursor_for_stage(&profile, rejected_stage).ok()?;
            let gate = &rejected_cursor.gate;
            let revision = rejected.get("claim_revision").and_then(Value::as_u64)?;
            let raw_findings = details
                .get("findings")?
                .as_array()?
                .iter()
                .rev()
                .find(|finding| {
                    finding.get("claim_revision").and_then(Value::as_u64) == Some(revision)
                })
                .and_then(|finding| finding.get("findings"));
            let parsed_findings = raw_findings.and_then(|value| {
                value
                    .as_str()
                    .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                    .or_else(|| Some(value.clone()))
            });
            let reject_target = super::gates::resolve_rejection_target(
                gate,
                parsed_findings.as_ref(),
                rejected_stage,
            );
            let target_cursor = super::claim::cursor_for_stage(&profile, reject_target).ok()?;
            if target_cursor.canonical_stage_id != active_cursor.canonical_stage_id
                || !matches!(
                    gate.proof.kind(),
                    Some(
                        crate::tasks::profile::ProofKind::Review
                            | crate::tasks::profile::ProofKind::Delivery
                    )
                )
            {
                return None;
            }
            parsed_findings
        })
    });

    let candidate_snapshot = snapshot_for(active_index);
    let latest_delivery_snapshot = snapshot_for(gate_sequence.len());

    let milestone_ref = details.get("milestone_ref").cloned();
    let depends_on = details
        .get("spec")
        .and_then(|s| s.get("depends_on"))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let dependency_refs = depends_on
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let linked_milestones = linked_milestone_context(root, &details, &dependency_refs)?;
    let receipt = details.get("receipt").cloned();

    // 4. Active blackboard messages (bounded to 15 recent messages, with max 500-char payload and source tags)
    let blackboard_messages: Option<Vec<Value>> = if blackboard_enabled {
        Some({
            let registry = super::workspaces::Registry::load(root)?;
            let store = workspace.open(&registry.database)?;
            let task_namespace = task_id
                .rsplit_once(':')
                .map(|(namespace, _)| format!("{namespace}:"));
            let milestone_ref_str = details
                .get("milestone_ref")
                .and_then(|v| v.as_str())
                .and_then(|m| store.milestone_scope_ref(m).ok());
            let mut stmt = store.connection.prepare(
            "SELECT id, task_id, recipient_task_id, subtask_ref, gate, author, topic, payload, created_at FROM task_blackboard \
             WHERE status='active' AND (task_id = ?1 OR recipient_task_id = ?1 OR \
               (?2 IS NOT NULL AND milestone_ref = ?2 AND task_id IS NULL) OR \
               (?3 IS NOT NULL AND substr(task_id,1,length(?3)) = ?3)) \
              AND (recipient_task_id IS NULL OR recipient_task_id = ?1) \
             AND (gate IS NULL OR gate = ?4 OR topic IN ('decisions', 'findings', 'blockers')) \
             ORDER BY created_at DESC, id DESC LIMIT 15",
        )?;
            let mut msgs = stmt
                .query_map(
                    rusqlite::params![task_id, milestone_ref_str, task_namespace, stage],
                    |row| {
                        let id = row.get::<_, u64>(0)?;
                        let msg_task_id = row.get::<_, Option<String>>(1)?;
                        let recipient_task_id = row.get::<_, Option<String>>(2)?;
                        let msg_subtask_ref = row.get::<_, Option<String>>(3)?;
                        let gate = row.get::<_, Option<String>>(4)?;
                        let author = row.get::<_, String>(5)?;
                        let topic = row.get::<_, String>(6)?;
                        let raw_payload: String = row.get(7)?;
                        let created_at = row.get::<_, i64>(8)?;
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
                            "recipient_task_id": recipient_task_id,
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
        })
    } else {
        None
    };

    // 5. Gate-aware workflow guidance and subtask DoD
    let mut requested_tools = active_gate
        .map(|gate| gate.tools.clone())
        .unwrap_or_else(|| profile.completed.tools.clone());
    if requested_tools.is_empty()
        && matches!(
            proof_kind,
            Some(
                crate::tasks::profile::ProofKind::Contract
                    | crate::tasks::profile::ProofKind::Command
                    | crate::tasks::profile::ProofKind::Review
            )
        )
    {
        requested_tools = [
            "code_map_overview",
            "code_map_search",
            "code_map_inspect",
            "forge_read",
            "search_docs",
            "get_doc",
            "task_blackboard",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
    }
    let recommended_tools = crate::tasks::catalog::filter_subsystem_tools(
        requested_tools.iter().map(String::as_str),
        &modules,
    )
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    let guidance_bindings: BTreeMap<String, String> = details
        .get("authority_snapshot")
        .and_then(|snapshot| snapshot.get("milestone_guidance"))
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    let guidance_slot_warnings = profile.validate_guidance_bindings(&guidance_bindings)?;
    let resolved_guidance = active_gate
        .map(|gate| profile.resolve_gate_guidance(gate, &guidance_bindings))
        .unwrap_or_default();
    let mut guidance = details["workflow_guidance"].clone();
    if !blackboard_enabled {
        prune_blackboard_guidance(&mut guidance);
    }
    if let Some(guidance_map) = guidance.as_object_mut() {
        let steps = guidance_map
            .get("steps")
            .cloned()
            .unwrap_or_else(|| json!([]));
        guidance_map.insert("stage".into(), json!(stage));
        if let Some((role_id, role)) = active_gate
            .and_then(|gate| gate.role.as_deref())
            .and_then(|id| profile.roles.get(id).map(|role| (id, role)))
            .filter(|(_, role)| role.runner.is_some() || role.prompt_template.is_some())
        {
            let model = role.models.first();
            let bindings = [
                ("task_id", task_id),
                ("stage", stage),
                ("role", role_id),
                ("worktree", details["worktree"].as_str().unwrap_or_default()),
                (
                    "model",
                    model.map(|model| model.model.as_str()).unwrap_or_default(),
                ),
                (
                    "reasoning",
                    model
                        .and_then(|model| model.reasoning.as_deref())
                        .unwrap_or_default(),
                ),
                (
                    "role_guidance",
                    guidance_map
                        .get("role_guide")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                ),
                ("goal", details["goal"].as_str().unwrap_or_default()),
            ];
            let render = |template: &str| {
                // Expand only original template slots; inserted values remain literal.
                let mut rendered = String::new();
                let mut remaining = template;
                while let Some(start) = remaining.find('{') {
                    rendered.push_str(&remaining[..start]);
                    remaining = &remaining[start..];
                    let Some(end) = remaining.find('}') else {
                        break;
                    };
                    if let Some((_, value)) =
                        bindings.iter().find(|(key, _)| *key == &remaining[1..end])
                    {
                        rendered.push_str(value);
                    } else {
                        rendered.push_str(&remaining[..=end]);
                    }
                    remaining = &remaining[end + 1..];
                }
                rendered.push_str(remaining);
                rendered
            };
            if role.runner.is_some() || role.prompt_template.is_some() {
                let runner = role.runner.as_deref().map(render);
                let rendered_prompt = render(role.prompt_template.as_deref().unwrap_or(
                    "Execute role {role} for task {task_id} at stage {stage} in {worktree}. Inspect the claimed gate context, verify scope and invariants, and submit evidence via task_submit. Goal: {goal}. {role_guidance}",
                ));
                guidance_map.insert(
                    "role_runner".into(),
                    json!({
                        "role": role_id,
                        "mode": role.effective_mode(),
                        "runner": runner,
                        "rendered_prompt": rendered_prompt,
                    }),
                );
            }
        }
        guidance_map.insert("recommended_tools".into(), json!(recommended_tools));
        guidance_map.insert("actionable_steps".into(), steps);
        let subtask_dod = workspace
            .gates
            .subtask_dod_criteria()
            .into_iter()
            .filter(|criterion| blackboard_enabled || !criterion.contains("task_blackboard"))
            .collect::<Vec<_>>();
        guidance_map.insert("subtask_dod".into(), json!(subtask_dod));
        let targeted_messages = blackboard_messages.as_ref().map(|messages| {
            messages
                .iter()
                .filter(|message| {
                    message.get("gate").and_then(Value::as_str) == Some(stage)
                        || message.get("recipient_task_id").and_then(Value::as_str) == Some(task_id)
                        || is_rfc_message(message)
                })
                .cloned()
                .collect::<Vec<_>>()
        });
        guidance_map.insert("blackboard_messages".into(), json!(targeted_messages));
        guidance_map.insert(
            "breadcrumb".into(),
            details.get("breadcrumb").cloned().unwrap_or(Value::Null),
        );
        guidance_map.insert(
            "gate_cursor".into(),
            json!({
                "gate_path": details.get("active_gate_path"),
                "stage_id": details.get("active_stage_id"),
                "subagent_role": details.get("subagent_role"),
            }),
        );
        guidance_map.insert(
            "scoped_commands".into(),
            json!(if codemap_enabled {
                scoped_commands(&profile, contract_seam_test.as_ref(), &covering_tests)
            } else {
                Vec::new()
            }),
        );
        guidance_map.insert("resolved_guidance".into(), json!(resolved_guidance));
        guidance_map.insert(
            "guidance_slot_warnings".into(),
            json!(guidance_slot_warnings),
        );
        guidance_map.insert("linked_milestones".into(), json!(linked_milestones));
        let proof_template = match proof_kind {
            Some(crate::tasks::profile::ProofKind::Contract) => json!({
                "contract_proof": {
                    "seam_test_ref": "<indexed test file or symbol>",
                    "red_exit_code": 101,
                }
            }),
            Some(crate::tasks::profile::ProofKind::Command) => json!({
                "command_proof": {
                    "command": "<registered command with scoped test target>",
                    "exit_code": 0,
                    "tests_passed": 1,
                    "tests_failed": 0,
                }
            }),
            Some(crate::tasks::profile::ProofKind::Review) => json!({
                "review_proof": {"decision": "pass", "contours": "<one result per configured contour>"}
            }),
            Some(crate::tasks::profile::ProofKind::Delivery) => {
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
    let blackboard_info = blackboard_messages.as_ref().map(|messages| {
        let mut topic_counts = serde_json::Map::new();
        let mut targeted_count = 0usize;
        for message in messages {
            if message.get("gate").and_then(Value::as_str) == Some(stage)
                || message.get("recipient_task_id").and_then(Value::as_str) == Some(task_id)
            {
                targeted_count += 1;
            }
            if let Some(topic) = message.get("topic").and_then(Value::as_str) {
                let count = topic_counts
                    .entry(topic.to_owned())
                    .or_insert_with(|| json!(0));
                *count = json!(count.as_u64().unwrap_or(0) + 1);
            }
        }
        json!({
            "active_gate": stage,
            "message_count": messages.len(),
            "targeted_count": targeted_count,
            "topic_counts": topic_counts,
            "message_ids": messages.iter().filter_map(|message| message.get("id").cloned()).collect::<Vec<_>>(),
        })
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

    if let Some(crate::tasks::profile::ProofDef::Scheme { scheme }) =
        active_gate.map(|gate| &gate.proof)
    {
        bundle_map.insert("proof_schema".into(), serde_json::to_value(scheme)?);
    }

    if codemap_enabled
        && matches!(
            proof_kind,
            Some(
                crate::tasks::profile::ProofKind::Contract
                    | crate::tasks::profile::ProofKind::Command
                    | crate::tasks::profile::ProofKind::Review
            )
        )
    {
        bundle_map.insert("adrs".into(), json!(adrs));
        bundle_map.insert("governing_invariants".into(), governing_invariants);
    }
    if codemap_enabled
        && matches!(
            proof_kind,
            Some(
                crate::tasks::profile::ProofKind::Contract
                    | crate::tasks::profile::ProofKind::Command
            )
        )
    {
        bundle_map.insert("scope_symbols".into(), json!(scope_symbols));
        bundle_map.insert("covering_tests".into(), json!(covering_tests));
        bundle_map.insert("external_callers".into(), json!(external_callers));
    }
    if proof_kind == Some(crate::tasks::profile::ProofKind::Command) {
        if let Some(seam) = contract_seam_test {
            bundle_map.insert("contract_seam_test".into(), seam);
        }
        if let Some(findings) = unresolved_review_findings {
            bundle_map.insert("unresolved_review_findings".into(), findings);
        }
    }
    if proof_kind == Some(crate::tasks::profile::ProofKind::Review) {
        if let Some(snapshot) = candidate_snapshot.as_ref() {
            bundle_map.insert("candidate_snapshot".into(), snapshot.clone());
        }
    }
    if proof_kind == Some(crate::tasks::profile::ProofKind::Delivery)
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
    bundle_map.insert(
        "blackboard_info".into(),
        blackboard_info.clone().unwrap_or(Value::Null),
    );
    bundle_map.insert("linked_milestones".into(), json!(linked_milestones));

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
    obj.remove("authority_snapshot");

    // Retain bounded blackboard and contract essentials at root
    obj.insert("depends_on".into(), depends_on);
    obj.insert("blackboard".into(), json!(blackboard_messages));
    obj.insert(
        "blackboard_info".into(),
        blackboard_info.unwrap_or(Value::Null),
    );
    obj.insert("linked_milestones".into(), json!(linked_milestones));
    obj.insert("workflow_guidance".into(), guidance.clone());
    obj.insert("context_bundle".into(), bundle);

    Ok(details)
}
