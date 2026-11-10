use super::*;
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};

pub(in crate::milestones) fn proof_shas(text: &str) -> Result<BTreeSet<String>> {
    let normalized = text.replace("\r\n", "\n");
    let metadata = normalized
        .strip_prefix("---\n")
        .and_then(|body| body.split_once("\n---\n"))
        .and_then(|(yaml, _)| serde_yaml::from_str::<serde_yaml::Value>(yaml).ok());
    if let Some(values) = metadata.as_ref().and_then(|meta| {
        meta.get("handoff")
            .and_then(|handoff| handoff.get("proof_shas"))
    }) {
        return values
            .as_sequence()
            .context("HANDOFF_PROOF_SHAS_INVALID: expected a list")?
            .iter()
            .map(|value| {
                let value = value
                    .as_str()
                    .filter(|value| {
                        value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
                    .context("HANDOFF_PROOF_SHAS_INVALID: expected full 40-hex member")?;
                Ok(value.to_owned())
            })
            .collect();
    }
    Ok(text
        .split(|ch: char| !ch.is_ascii_hexdigit())
        .filter(|value| value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(str::to_owned)
        .collect())
}

#[derive(Default)]
pub(super) struct ActiveProofOwners {
    proofs: BTreeSet<String>,
    identities: BTreeSet<String>,
    outgoing_proofs: BTreeSet<String>,
}

// Capture durable task/prepared-handoff owners before deciding whether SQLite was pruned.
pub(super) fn active_proof_owners(
    root: &Path,
    store: &TasksStore,
    outgoing: &str,
) -> Result<ActiveProofOwners> {
    let archived = store
        .connection
        .prepare("SELECT milestone_id FROM milestone_tombstones")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<BTreeSet<_>>>()?;
    let output = crate::core::git::git_command(root, &["worktree", "list", "--porcelain", "-z"])?;
    if !output.status.success() {
        bail!("HANDOFF_REF_CLEANUP_FAILED: cannot enumerate linked worktrees");
    }
    let mut active = BTreeMap::new();
    let mut terminal = BTreeSet::new();
    for field in output.stdout.split(|byte| *byte == 0) {
        let Some(path) = field.strip_prefix(b"worktree ") else {
            continue;
        };
        let tree = std::path::PathBuf::from(std::str::from_utf8(path)?).canonicalize()?;
        let settings = tasks::TaskSettings::load(&tree)?;
        let mut identities = BTreeSet::new();
        for path in crate::milestones::milestone_paths(&tree)? {
            if crate::milestones::is_archived_path(&path) {
                continue;
            }
            let item = crate::milestones::contract::snapshot(&tree, path, &settings)?;
            let identity = format!("{}/{}/{}", item.repository, item.project, item.id);
            if matches!(item.status.as_str(), "active" | "planned")
                && identity != outgoing
                && !archived.contains(&identity)
            {
                if !item.tasks.is_empty()
                    && item.tasks.iter().all(|task| {
                        matches!(task["status"].as_str(), Some("completed" | "cancelled"))
                    })
                {
                    terminal.insert((tree.clone(), identity.clone()));
                }
                identities.insert(identity);
            }
        }
        active.insert(tree, identities);
    }
    let mut owners = ActiveProofOwners {
        identities: active.values().flatten().cloned().collect(),
        ..Default::default()
    };
    for owner in integration::handoff_owners(root)? {
        if owner.identity == outgoing {
            owners.outgoing_proofs.extend(owner.proof_shas);
            continue;
        }
        if active
            .get(&owner.worktree)
            .is_some_and(|identities| identities.contains(&owner.identity))
        {
            terminal.remove(&(owner.worktree.clone(), owner.identity.clone()));
            owners.proofs.extend(owner.proof_shas);
        }
    }
    if !terminal.is_empty() {
        bail!("HANDOFF_PROOF_OWNERSHIP_UNAVAILABLE: terminal foreign milestones may have unowned legacy handoff preparations: {terminal:?}. Run handoff-context in those worktrees with their actual target bases before retrying cleanup.");
    }
    let descriptors: Vec<String> = store
        .connection
        .prepare("SELECT descriptor FROM tasks")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for descriptor in descriptors {
        let task: Task = serde_json::from_str(&descriptor)?;
        if crate::milestones::is_archived_path(Path::new(&task.milestone_ref)) {
            continue;
        }
        let Some(authority) = &task.authority_snapshot else {
            continue;
        };
        let identity = format!(
            "{}/{}/{}",
            authority.repository, authority.project, authority.milestone_id
        );
        if identity == outgoing {
            continue;
        }
        owners.identities.insert(identity);
        owners.proofs.extend(proof_shas(&descriptor)?);
        let evidence: Vec<String> = store
            .connection
            .prepare("SELECT evidence FROM task_gates WHERE task_id=?1 AND evidence IS NOT NULL")?
            .query_map([&task.task_id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for proof in evidence {
            owners.proofs.extend(proof_shas(&proof)?);
        }
    }
    Ok(owners)
}

pub(in crate::milestones) fn cleanup_recovered_refs(
    root: &Path,
    store: &TasksStore,
    archives: &[(String, String, String, String)],
) -> Result<()> {
    // Capture all ownership before cleanup. Recovery calls this only after
    // committed active state has been verified and its keepers published.
    let owners = archives
        .iter()
        .map(|(repository, project, id, _)| {
            active_proof_owners(root, store, &format!("{repository}/{project}/{id}"))
        })
        .collect::<Result<Vec<_>>>()?;
    for ((repository, project, id, text), owners) in archives.iter().zip(owners) {
        cleanup_handoff_refs(root, repository, project, id, text, &owners)?;
    }
    Ok(())
}

pub(in crate::milestones) fn retire_unowned_evidence(
    root: &Path,
    store: &TasksStore,
) -> Result<()> {
    let owners = active_proof_owners(root, store, "")?;
    let refs = crate::core::git::git_command(
        root,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/forge/evidence/",
        ],
    )?;
    if !refs.status.success() {
        bail!("HANDOFF_REF_CLEANUP_FAILED: cannot enumerate legacy pins");
    }
    for reference in String::from_utf8(refs.stdout)?.lines() {
        if let Some(sha) = reference
            .strip_prefix("refs/forge/evidence/")
            .filter(|sha| full_git_sha(sha))
        {
            if !owners.proofs.contains(sha) {
                integration::git_write(
                    root,
                    &["update-ref", "-d", reference],
                    "HANDOFF_REF_CLEANUP_FAILED",
                )?;
            }
        }
    }
    Ok(())
}

pub(super) fn cleanup_handoff_refs(
    root: &Path,
    repository: &str,
    project: &str,
    id: &str,
    text: &str,
    owners: &ActiveProofOwners,
) -> Result<()> {
    let mut proofs = proof_shas(text)?;
    proofs.extend(owners.outgoing_proofs.iter().cloned());
    let mut refs_to_delete = Vec::new();
    for sha in proofs {
        if owners.proofs.contains(&sha) {
            continue;
        }
        refs_to_delete.push(format!("refs/forge/evidence/{sha}"));
    }
    // Delete only the outgoing namespace; active foreign owners remain protected.
    for prefix in [
        format!("refs/forge/state/{repository}/{project}/{id}/"),
        format!("refs/forge/state/{project}/{id}/"),
        format!("refs/forge/state/{id}/"),
    ] {
        let shared = if prefix == format!("refs/forge/state/{id}/") {
            owners
                .identities
                .iter()
                .any(|identity| identity.rsplit('/').next() == Some(id))
        } else if prefix == format!("refs/forge/state/{project}/{id}/") {
            owners
                .identities
                .iter()
                .any(|identity| identity.ends_with(&format!("/{project}/{id}")))
        } else {
            false
        };
        if shared {
            continue;
        }
        let output =
            crate::core::git::git_command(root, &["for-each-ref", "--format=%(refname)", &prefix])?;
        if !output.status.success() {
            bail!("HANDOFF_REF_CLEANUP_FAILED: cannot enumerate keeper refs");
        }
        let refs = String::from_utf8(output.stdout)?;
        refs_to_delete.extend(
            refs.lines()
                .filter(|reference| reference.starts_with(&prefix))
                .map(str::to_owned),
        );
    }
    integration::delete_refs(root, &refs_to_delete)?;
    prune_snapshot_refs(root, &format!("{project}/{id}:"));
    integration::clear_handoff_owners(root, &format!("{repository}/{project}/{id}"))?;
    Ok(())
}

pub(super) fn already_handed_off(id: &str) -> anyhow::Error {
    anyhow::anyhow!("MILESTONE_ALREADY_HANDED_OFF: Milestone '{id}' has already been handed off and pruned in SQLite by another worktree. Merge or pull the handoff commit into this branch.")
}

pub type SymbolFactsExtractor =
    fn(path: &str, source: &str) -> Result<Vec<forge_core::models::Node>>;
static SYMBOL_FACTS_EXTRACTOR: std::sync::RwLock<Option<SymbolFactsExtractor>> =
    std::sync::RwLock::new(None);

pub fn set_symbol_facts_extractor(extractor: SymbolFactsExtractor) {
    if let Ok(mut lock) = SYMBOL_FACTS_EXTRACTOR.write() {
        *lock = Some(extractor);
    }
}

pub(super) fn symbol_facts(
    root: &Path,
    revision: &str,
    path: &str,
) -> Result<BTreeMap<String, String>> {
    let rel = Path::new(path);
    if !forge_core::fs::safe_relative(rel)
        || rel.components().any(|c| {
            c.as_os_str()
                .to_str()
                .is_some_and(|name| forge_core::config::DEFAULT_IGNORED_NAMES.contains(&name))
        })
    {
        return Ok(BTreeMap::new());
    }
    let extractor = match SYMBOL_FACTS_EXTRACTOR.read() {
        Ok(lock) => *lock,
        Err(_) => None,
    };
    let Some(extractor) = extractor else {
        return Ok(BTreeMap::new());
    };
    let blob = crate::core::git::git_command(root, &["show", &format!("{revision}:{path}")])?;
    if !blob.status.success() {
        let exists = crate::core::git::git_command(
            root,
            &["cat-file", "-e", &format!("{revision}:{path}")],
        )?;
        if !exists.status.success() {
            return Ok(BTreeMap::new());
        }
        bail!("HANDOFF_TELEMETRY_FAILED: cannot read AST input '{path}'");
    }
    let source = String::from_utf8(blob.stdout)?;
    let nodes = extractor(path, &source)?;
    let lines: Vec<_> = source.lines().collect();
    nodes
        .iter()
        .filter(|node| node.kind != "module")
        .map(|node| {
            let first = node.line.saturating_sub(1).min(lines.len());
            let end = node.end_line.min(lines.len()).max(first);
            Ok((
                format!("{path}:{}:{}", node.kind, node.qualname),
                integration::digest(&lines[first..end])?,
            ))
        })
        .collect()
}

fn observe_command(
    root: &Path,
    selection: &str,
    kind: &str,
    implementation_digest: &str,
) -> Result<Option<integration::CommandObservation>> {
    let profile = crate::tasks::profile::load_for_workspace(root)?;
    let command = profile.commands.get(selection).with_context(|| {
        let available: Vec<_> = profile.commands.keys().map(String::as_str).collect();
        format!("HANDOFF_COMMAND_UNREGISTERED: '{selection}' is not a registered project command in profile (available: {available:?})")
    })?;
    let commands = command.commands_for_root(root);
    let mut last_output = None;
    for cmd in &commands {
        let output = std::process::Command::new("sh")
            .args(["-c", cmd])
            .current_dir(root)
            .output()?;
        if !output.status.success() {
            bail!(
                "HANDOFF_VERIFICATION_FAILED: selected {kind} command '{selection}' ('{cmd}') exited {}:\n--- STDOUT ---\n{}\n--- STDERR ---\n{}",
                output.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        last_output = Some(output);
    }
    Ok(last_output.map(|output| integration::CommandObservation {
        selection: selection.to_owned(),
        command: commands.join(" && "),
        exit_code: 0,
        stdout_digest: Some(hex::encode(Sha256::digest(&output.stdout))),
        stderr_digest: Some(hex::encode(Sha256::digest(&output.stderr))),
        reused_verification: None,
        implementation_digest: implementation_digest.to_owned(),
    }))
}

fn observe_or_reuse_command(
    root: &Path,
    store: &TasksStore,
    identity: &str,
    selection: &str,
    kind: &str,
    implementation_digest: &str,
) -> Result<Option<integration::CommandObservation>> {
    let recorded: Option<(String, String)> = store
        .connection
        .query_row(
            "SELECT t.gate_path, t.proof_payload
             FROM gate_transitions t
             WHERE t.milestone_id = ?1
               AND t.subject = 'milestone'
               AND t.proof_kind = 'command'
               AND t.action = 'pass'
             ORDER BY t.id DESC LIMIT 1",
            rusqlite::params![identity],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    if let Some((gate_path, payload_str)) = recorded {
        if let Ok(payload) = serde_json::from_str::<Value>(&payload_str) {
            let digest_matches = payload["implementation_digest"]
                .as_str()
                .is_some_and(|d| d == implementation_digest);
            let cmd_matches = payload["command_id"].as_str() == Some(selection);
            let exit_zero = payload["exit_code"].as_i64() == Some(0);
            let no_failed = payload["tests_failed"].as_u64() == Some(0);
            let attempt_id = payload["attempt_id"].as_i64().unwrap_or(1);

            if digest_matches && cmd_matches && exit_zero && no_failed {
                let trace: Option<(Option<String>, String)> = store
                    .connection
                    .query_row(
                        "SELECT stdout_sha256, command_str
                         FROM gate_execution_traces
                         WHERE milestone_id = ?1
                           AND subject = 'milestone'
                           AND gate_path = ?2
                           AND command_id = ?3
                           AND attempt_id = ?4
                           AND status = 'completed'
                           AND exit_code = 0
                         ORDER BY id DESC LIMIT 1",
                        rusqlite::params![identity, gate_path, selection, attempt_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?;

                if let Some((stdout_sha, cmd_str)) = trace {
                    return Ok(Some(integration::CommandObservation {
                        selection: selection.to_owned(),
                        command: cmd_str,
                        exit_code: 0,
                        stdout_digest: stdout_sha
                            .or_else(|| payload["stdout_sha256"].as_str().map(str::to_owned)),
                        stderr_digest: None,
                        reused_verification: Some(true),
                        implementation_digest: implementation_digest.to_owned(),
                    }));
                }
            }
        }
    }

    observe_command(root, selection, kind, implementation_digest)
}

pub(super) fn telemetry_passport(
    root: &Path,
    item: &Snapshot,
    store: &TasksStore,
    relevant: &[Task],
    commits: (&str, &str),
    evidence: &IntegrationEvidence,
    times: (chrono::DateTime<Utc>, chrono::DateTime<Utc>),
) -> Result<(Value, BTreeSet<String>)> {
    let (sha, target) = commits;
    let (started, completed) = times;
    let mut proofs = BTreeSet::from([sha.to_owned(), target.to_owned()]);
    let mut per_task = Vec::new();
    for task in relevant {
        let claimed_at: Option<i64> = store.connection.query_row(
            "SELECT min(claimed_at) FROM task_claims WHERE task_id=?1",
            [&task.task_id],
            |row| row.get(0),
        )?;
        let profile = store.profile_for_task(task)?;
        let flattened_profile =
            crate::engine::tasks::claim::flattened_profile(&profile, Some(task))?;
        let review_gates: BTreeSet<_> = flattened_profile
            .gates
            .iter()
            .filter(|gate| {
                gate.proof.kind() == Some(crate::core::tasks::profile::ProofKind::Review)
            })
            .map(|gate| gate.id.as_str())
            .collect();
        let claims: Vec<(String, String)> = store
            .connection
            .prepare("SELECT gate,state FROM task_gates WHERE task_id=?1 ORDER BY revision")?
            .query_map([&task.task_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let gates: Vec<String> = store
            .connection
            .prepare("SELECT evidence FROM task_gates WHERE task_id=?1 AND evidence IS NOT NULL")?
            .query_map([&task.task_id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for gate in gates {
            proofs.extend(proof_shas(&gate)?);
        }
        if let Some(receipt) = &task.receipt {
            proofs.extend(proof_shas(&serde_json::to_string(receipt)?)?);
        }
        per_task.push(json!({"task_id":task.task_id,"status":task.status,"first_claimed_at":claimed_at,"completed_at":task.completed_at,
            "execution_wall_seconds":claimed_at.zip(task.completed_at).and_then(|(start,end)| end.checked_sub(start).filter(|duration| *duration >= 0)),
            "review_cycles":claims.iter().filter(|(gate,_)| review_gates.contains(gate.as_str())).count(),
            "rejected_reviews":claims.iter().filter(|(gate,state)| review_gates.contains(gate.as_str()) && state == "rejected").count()}));
    }
    let diff = crate::core::git::git_command(
        root,
        &["diff", "--numstat", "--no-renames", "-z", target, sha],
    )?;
    if !diff.status.success() {
        bail!("HANDOFF_TELEMETRY_FAILED: cannot inspect integration diff");
    }
    let mut breakdown: BTreeMap<&str, (u64, u64, u64)> = BTreeMap::from([
        ("src", (0, 0, 0)),
        ("tests", (0, 0, 0)),
        ("docs", (0, 0, 0)),
        ("other", (0, 0, 0)),
    ]);
    let mut before = BTreeMap::new();
    let mut after = BTreeMap::new();
    for row in diff
        .stdout
        .split(|byte| *byte == 0)
        .filter(|row| !row.is_empty())
    {
        let row = std::str::from_utf8(row)?;
        let mut fields = row.splitn(3, '\t');
        let added = fields.next().context("invalid numstat")?;
        let removed = fields.next().context("invalid numstat")?;
        let path = fields.next().context("invalid numstat path")?;
        let category = if path.starts_with("src/") || path.starts_with("crates/") {
            "src"
        } else if path.starts_with("tests/") {
            "tests"
        } else if path.starts_with("docs/") {
            "docs"
        } else {
            "other"
        };
        let summary = breakdown
            .get_mut(category)
            .context("diff category missing")?;
        summary.0 += 1;
        summary.1 += added.parse::<u64>().unwrap_or(0);
        summary.2 += removed.parse::<u64>().unwrap_or(0);
        // Zero removed lines can also mean an insertion into an existing code file;
        // keep its baseline facts so unchanged symbols are not reported as added.
        before.extend(symbol_facts(root, target, path)?);
        after.extend(symbol_facts(root, sha, path)?);
    }
    let added_total = after
        .keys()
        .filter(|key| !before.contains_key(*key))
        .count();
    let removed_total = before
        .keys()
        .filter(|key| !after.contains_key(*key))
        .count();
    let modified_total = after
        .iter()
        .filter(|(key, value)| before.get(*key).is_some_and(|old| old != *value))
        .count();
    let ast_symbol_deltas = json!({
        "added": added_total,
        "removed": removed_total,
        "modified": modified_total,
    });
    let breakdown = breakdown
        .into_iter()
        .map(|(category, (files, added, removed))| {
            (
                category,
                json!({"files":files,"added":added,"removed":removed}),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let settings = tasks::TaskSettings::load(root)?;
    let canonical = |dep: &str, project: &str| -> Result<String> {
        // Only an explicitly qualified owner is raw alias input; the default
        // project has already been canonicalized by the manifest parser.
        let (owner, id) = match dep.split_once('/') {
            Some((owner, id)) => (
                crate::core::tasks::canonical_project(owner, &settings.aliases)?,
                id,
            ),
            None => (project.to_owned(), dep),
        };
        Ok(format!("{}/{}/{}", item.repository, owner, id))
    };
    let identity = format!("{}/{}/{}", item.repository, item.project, item.id);
    let upstream: BTreeSet<_> = item
        .depends_on
        .iter()
        .map(|dep| canonical(dep, &item.project))
        .collect::<Result<_>>()?;
    let mut downstream = BTreeSet::new();
    let mut known = BTreeSet::new();
    for path in crate::milestones::milestone_paths(root)? {
        let text = fs::read_to_string(&path)?;
        let Ok(parsed) = crate::core::tasks::parse_milestone_manifest(
            &text,
            path.strip_prefix(root)?,
            &settings.task_repository,
            settings
                .task_project
                .as_deref()
                .unwrap_or(&settings.task_repository),
            &settings.owners,
            &settings.aliases,
        ) else {
            continue;
        };
        let owner = format!("{}/{}/{}", parsed.repository, parsed.project, parsed.id);
        known.insert(owner.clone());
        if parsed
            .depends_on
            .iter()
            .map(|dep| canonical(dep, &parsed.project))
            .collect::<Result<Vec<_>>>()?
            .contains(&identity)
        {
            downstream.insert(owner);
        }
    }
    let lint = if let Some(selection) = &evidence.verification.lint_command {
        match observe_or_reuse_command(
            root,
            store,
            &identity,
            selection,
            "lint",
            &evidence.binding.implementation_digest,
        )? {
            Some(observation) => json!({"status":"clean","observation":observation}),
            None => json!({"status":"unobserved"}),
        }
    } else {
        json!({"status":"unobserved"})
    };
    let verification = if let Some(selection) = &evidence.verification.project_command {
        observe_or_reuse_command(
            root,
            store,
            &identity,
            selection,
            "project",
            &evidence.binding.implementation_digest,
        )?
    } else {
        None
    };
    let mut passport = json!({"version":1,"identity":identity,"binding":evidence.binding,"commit":sha,"target_base":target,
        "active_authority_digest":integration::digest(&integration::manifest_projection(item.text.as_bytes())?)?,
        "started_at":started.to_rfc3339(),"completed_at":completed.to_rfc3339(),"duration_seconds":(completed-started).num_seconds(),
        "tasks_count":relevant.len(),"tasks":per_task,"diff":breakdown,"ast_symbol_deltas":ast_symbol_deltas,
        "test_suite_results":evidence.verification,"automatic_project_verification":verification,"linter":lint,
        "topology":{"upstream":upstream,"downstream":downstream,"unresolved_upstream":upstream.difference(&known).collect::<Vec<_>>()}});
    let token = integration::digest(&passport)?;
    passport["verified_handoff_proof"] = token.into();
    Ok((passport, proofs))
}

pub(super) fn prune_snapshot_refs(root: &Path, prefix: &str) {
    let clean = prefix
        .strip_suffix(':')
        .unwrap_or(prefix)
        .replace(['\\', ' '], "_");
    let target_prefix = format!("refs/forge/snapshots/{clean}/");
    if let Ok(output) = crate::core::git::git_command(
        root,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/forge/snapshots/",
        ],
    ) {
        let refs: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|reference| reference.starts_with(&target_prefix))
            .map(str::to_owned)
            .collect();
        let _ = integration::delete_refs(root, &refs);
    }
}
