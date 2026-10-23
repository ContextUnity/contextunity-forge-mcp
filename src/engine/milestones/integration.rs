//! Content binding for externally reviewed milestone integration evidence.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// Stable content and authority identities inspected by the integration reviewer.
pub struct HandoffBinding {
    /// Canonical projection version.
    pub version: u32,
    /// Repository task namespace.
    pub repository: String,
    /// Project task namespace.
    pub project: String,
    /// Milestone identity.
    pub milestone: String,
    /// Current contracts and immutable accepted task proofs.
    pub authority_digest: String,
    /// Final tracked paths, modes and contents, excluding generated milestone metadata.
    pub implementation_digest: String,
    /// Explicitly selected integration base under the same projection.
    pub target_base_digest: String,
    /// Accepted task builder identities, sorted and deduplicated.
    pub builders: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Independent review of the combined implementation against its target base.
pub struct IntegrationReview {
    /// Reviewer identity, distinct from every accepted task builder.
    pub worker_id: String,
    /// Passing integration review decision.
    pub decision: String,
    /// Review coverage and conclusion.
    pub summary: String,
    /// Unresolved changes outside admitted task authority.
    pub scope_findings: Vec<String>,
    /// Unresolved contract or invariant findings.
    pub contract_findings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Verification performed externally against the bound implementation.
pub struct IntegrationVerification {
    /// Exact repository verification command.
    pub command: String,
    /// Process exit code.
    pub exit_code: i32,
    /// Passing test count.
    pub tests_passed: u64,
    /// Failing test count.
    pub tests_failed: u64,
    /// Optional registered command ID explicitly selected as the project linter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lint_command: Option<String>,
    /// Registered project verification command, required for source polish.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// External integration review and verification submitted for milestone handoff.
pub struct IntegrationEvidence {
    /// Exact content and task authority reviewed and verified.
    pub binding: HandoffBinding,
    /// Git selector resolving the explicit target base; its content is checked independently.
    pub target_base: String,
    /// Independent review outcome.
    pub review: IntegrationReview,
    /// Repository verification outcome.
    pub verification: IntegrationVerification,
}

impl IntegrationEvidence {
    pub(super) fn validate(&self, current: &HandoffBinding) -> Result<()> {
        if &self.binding != current {
            bail!("HANDOFF_EVIDENCE_STALE: final content, target base, or admitted task proofs changed; obtain fresh integration review and verification");
        }
        if self.review.worker_id.trim().is_empty()
            || current.builders.contains(&self.review.worker_id)
        {
            bail!("HANDOFF_REVIEW_NOT_INDEPENDENT: integration reviewer must differ from every milestone builder");
        }
        if self.review.decision != "pass"
            || self.review.summary.trim().is_empty()
            || !self.review.scope_findings.is_empty()
            || !self.review.contract_findings.is_empty()
        {
            bail!("HANDOFF_REVIEW_REJECTED: passing integration review with no unresolved scope or contract findings is required");
        }
        if self.verification.command.trim().is_empty()
            || self.verification.exit_code != 0
            || self.verification.tests_failed != 0
        {
            bail!("HANDOFF_VERIFICATION_FAILED: successful repository verification is required");
        }
        Ok(())
    }
}

pub(super) fn digest(value: &(impl Serialize + ?Sized)) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
}

#[derive(Debug, Serialize)]
pub(super) struct CommandObservation {
    selection: String,
    command: String,
    exit_code: i32,
    stdout_digest: String,
    stderr_digest: String,
    implementation_digest: String,
}

/// Execute only an explicitly selected, registered project command.
pub(super) fn observe_command(
    root: &Path,
    selection: &str,
    binding: &HandoffBinding,
) -> Result<CommandObservation> {
    let profile = crate::core::tasks::profile::load_for_workspace(root)?;
    let command = profile
        .commands
        .get(selection)
        .with_context(|| {
            format!(
                "HANDOFF_COMMAND_UNREGISTERED: '{selection}' is not a registered project command"
            )
        })?
        .command();
    let output = std::process::Command::new("sh")
        .args(["-c", command])
        .current_dir(root)
        .output()?;
    let observation = CommandObservation {
        selection: selection.to_owned(),
        command: command.to_owned(),
        exit_code: output.status.code().unwrap_or(-1),
        stdout_digest: hex::encode(Sha256::digest(&output.stdout)),
        stderr_digest: hex::encode(Sha256::digest(&output.stderr)),
        implementation_digest: binding.implementation_digest.clone(),
    };
    if !output.status.success() {
        bail!(
            "HANDOFF_VERIFICATION_FAILED: selected command '{}' exited {}: {}",
            selection,
            observation.exit_code,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(observation)
}

pub(super) fn read_passport(
    root: &Path,
    commit: &str,
    identity: &str,
) -> Result<Option<serde_json::Value>> {
    let output = crate::core::git::git_command(root, &["notes", "--ref=forge", "show", commit])?;
    if !output.status.success() {
        if String::from_utf8_lossy(&output.stderr).contains("no note found") {
            return Ok(None);
        }
        bail!(
            "HANDOFF_PASSPORT_UNAVAILABLE: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let note: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(note) => note,
        Err(_) => return Ok(None),
    };
    let passport = note
        .get("milestone_passports")
        .and_then(|passports| passports.get(identity))
        .cloned();
    if let Some(passport) = &passport {
        let mut payload = passport.clone();
        let token = payload
            .as_object_mut()
            .context("HANDOFF_PASSPORT_INVALID: passport must be a mapping")?
            .remove("verified_handoff_proof")
            .context("HANDOFF_PASSPORT_INVALID: missing proof token")?;
        if token.as_str() != Some(digest(&payload)?.as_str())
            || payload["identity"] != identity
            || payload["commit"] != commit
        {
            bail!(
                "HANDOFF_PASSPORT_INVALID: content, identity or commit does not match proof token"
            );
        }
    }
    Ok(passport)
}

pub(super) fn write_passport(
    root: &Path,
    commit: &str,
    identity: &str,
    passport: &serde_json::Value,
) -> Result<()> {
    write_note_entry(root, commit, "milestone_passports", identity, passport)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HandoffProofOwner {
    pub identity: String,
    pub worktree: std::path::PathBuf,
    pub proof_shas: std::collections::BTreeSet<String>,
}

pub(super) fn write_handoff_owner(
    root: &Path,
    commit: &str,
    owner: &HandoffProofOwner,
) -> Result<()> {
    if !owner.proof_shas.contains(commit) || owner.proof_shas.len() > 2 {
        bail!("HANDOFF_PROOF_OWNERSHIP_INVALID: preparation must bind its head and target");
    }
    let key = digest(&(&owner.identity, &owner.worktree))?;
    write_note_entry(
        root,
        &preparation_anchor(root)?,
        "handoff_proof_owners",
        &key,
        &serde_json::to_value(owner)?,
    )
}

pub(super) fn handoff_owners(root: &Path) -> Result<Vec<HandoffProofOwner>> {
    let output = crate::core::git::git_command(
        root,
        &["notes", "--ref=forge", "show", &preparation_anchor(root)?],
    )?;
    if !output.status.success() {
        if String::from_utf8_lossy(&output.stderr).contains("no note found") {
            return Ok(Vec::new());
        }
        bail!("HANDOFF_REF_CLEANUP_FAILED: cannot read preparation ownership note");
    }
    let mut owners = Vec::new();
    let note: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    if let Some(entries) = note.get("handoff_proof_owners") {
        let entries = entries
            .as_object()
            .context("HANDOFF_REF_CLEANUP_FAILED: invalid proof ownership mapping")?;
        for (key, entry) in entries {
            let owner: HandoffProofOwner = serde_json::from_value(entry.clone())?;
            if owner.proof_shas.is_empty()
                || owner.proof_shas.len() > 2
                || owner.proof_shas.iter().any(|sha| {
                    !matches!(sha.len(), 40 | 64)
                        || !sha.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
                || *key != digest(&(&owner.identity, &owner.worktree))?
            {
                bail!("HANDOFF_PROOF_OWNERSHIP_INVALID: invalid preparation owner");
            }
            owners.push(owner);
        }
    }
    Ok(owners)
}

// A stable note anchor makes preparation state one replaceable entry per canonical
// milestone/worktree, independent of mutable HEADs or the selected target base.
fn preparation_anchor(root: &Path) -> Result<String> {
    crate::core::git::git_stdout(
        root,
        &["hash-object", "-w", "--stdin"],
        "HANDOFF_PROOF_OWNERSHIP_INVALID",
    )
}

pub(super) fn clear_handoff_owners(root: &Path, identity: &str) -> Result<()> {
    for owner in handoff_owners(root)? {
        if owner.identity == identity {
            write_note_entry(
                root,
                &preparation_anchor(root)?,
                "handoff_proof_owners",
                &digest(&(&owner.identity, &owner.worktree))?,
                &serde_json::Value::Null,
            )?;
        }
    }
    Ok(())
}

fn write_note_entry(
    root: &Path,
    commit: &str,
    collection: &str,
    identity: &str,
    passport: &serde_json::Value,
) -> Result<()> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let temporary = format!("refs/notes/forge-passport-{}-{nonce}", std::process::id());
    // Build a note commit on a private ref, then compare-and-swap the shared ref.
    // A competing milestone must not lose its entry in the same commit note.
    let result = (|| {
        for _ in 0..3 {
            let old = note_ref(root)?;
            if let Some(old) = &old {
                git_write(
                    root,
                    &["update-ref", &temporary, old],
                    "HANDOFF_PASSPORT_WRITE_FAILED",
                )?;
            } else {
                git_write(
                    root,
                    &["update-ref", "-d", &temporary],
                    "HANDOFF_PASSPORT_WRITE_FAILED",
                )?;
            }
            let existing = crate::core::git::git_command(
                root,
                &["notes", &format!("--ref={temporary}"), "show", commit],
            )?;
            let mut note = if existing.status.success() {
                match serde_json::from_slice::<serde_json::Value>(&existing.stdout) {
                    Ok(value) if value.is_object() => value,
                    _ => {
                        serde_json::json!({"external_note":String::from_utf8_lossy(&existing.stdout)})
                    }
                }
            } else if String::from_utf8_lossy(&existing.stderr).contains("no note found") {
                serde_json::json!({})
            } else {
                bail!("HANDOFF_PASSPORT_UNAVAILABLE: cannot read existing Git note");
            };
            if note.get(collection).is_some_and(|value| !value.is_object()) {
                bail!(
                    "HANDOFF_PASSPORT_INVALID: existing milestone_passports note is not a mapping"
                );
            }
            if note.get(collection).is_none() {
                note[collection] = serde_json::json!({});
            }
            if passport.is_null() {
                note[collection]
                    .as_object_mut()
                    .context("note mapping missing")?
                    .remove(identity);
            } else {
                note[collection][identity] = passport.clone();
            }
            let note_json = serde_json::to_string(&note)?;
            if collection == "milestone_passports" && !passport.is_null() && note_json.len() >= 2048
            {
                bail!("HANDOFF_PASSPORT_TOO_LARGE: Git note must be smaller than 2048 bytes");
            }
            let note_path = std::env::temp_dir()
                .join(format!("forge-note-{}-{nonce}.json", std::process::id()));
            std::fs::write(&note_path, note_json)?;
            let add_result = git_write(
                root,
                &[
                    "notes",
                    &format!("--ref={temporary}"),
                    "add",
                    "-f",
                    "-F",
                    note_path.to_str().context("invalid temp note path")?,
                    commit,
                ],
                "HANDOFF_PASSPORT_WRITE_FAILED",
            );
            let _ = std::fs::remove_file(&note_path);
            add_result?;
            let new = crate::core::git::git_stdout(
                root,
                &["rev-parse", "--verify", &temporary],
                "HANDOFF_PASSPORT_WRITE_FAILED",
            )?;
            let zero = "0".repeat(new.len());
            let updated = crate::core::git::git_command(
                root,
                &[
                    "update-ref",
                    "refs/notes/forge",
                    &new,
                    old.as_deref().unwrap_or(&zero),
                ],
            )?;
            if updated.status.success() {
                return Ok(());
            }
            if note_ref(root)? == old {
                bail!(
                    "HANDOFF_PASSPORT_WRITE_FAILED: {}",
                    String::from_utf8_lossy(&updated.stderr)
                );
            }
        }
        bail!("HANDOFF_PASSPORT_WRITE_FAILED: concurrent note updates; retry handoff");
    })();
    let cleanup = git_write(
        root,
        &["update-ref", "-d", &temporary],
        "HANDOFF_PASSPORT_WRITE_FAILED",
    );
    result?;
    cleanup
}

fn note_ref(root: &Path) -> Result<Option<String>> {
    let output = crate::core::git::git_command(
        root,
        &["rev-parse", "--verify", "--quiet", "refs/notes/forge"],
    )?;
    if output.status.success() {
        return Ok(Some(String::from_utf8(output.stdout)?.trim().to_owned()));
    }
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    bail!("HANDOFF_PASSPORT_UNAVAILABLE: cannot resolve Git notes ref");
}

pub(super) fn git_write(root: &Path, args: &[&str], code: &str) -> Result<()> {
    let output = crate::core::git::git_command(root, args)?;
    if !output.status.success() {
        bail!("{code}: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

pub(super) fn resolve_commit(root: &Path, selector: &str) -> Result<String> {
    crate::core::git::git_stdout(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{selector}^{{commit}}"),
        ],
        "HANDOFF_COMMIT_INVALID",
    )
}

pub(super) fn require_clean_tree(root: &Path, commit: &str, generated: &[String]) -> Result<()> {
    let status = crate::core::git::git_command(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let dirty = status
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .any(|entry| {
            !entry.starts_with(b"?? ")
                || !generated
                    .iter()
                    .any(|path| entry.get(3..) == Some(path.as_bytes()))
        });
    if !status.status.success() || dirty {
        bail!("HANDOFF_WORKTREE_DIRTY: commit implementation changes and untracked deliverables before preparing integration evidence: {}", String::from_utf8_lossy(&status.stdout).replace('\0', "; "));
    }
    let head = resolve_commit(root, "HEAD")?;
    if content_tree(root, commit)? != content_tree(root, &head)? {
        bail!("HANDOFF_COMMIT_MISMATCH: selected handoff content differs from the current worktree HEAD");
    }
    Ok(())
}

fn content_tree(root: &Path, commit: &str) -> Result<String> {
    crate::core::git::git_stdout(
        root,
        &["rev-parse", &format!("{commit}^{{tree}}")],
        "HANDOFF_COMMIT_INVALID",
    )
}

// Preserve raw contract and prose bytes. Remove only renderer-owned YAML fields;
// unfamiliar YAML spellings remain bound rather than being silently excluded.
fn generated_fields_removed(text: &str, fields: &[&str], subtasks: bool) -> String {
    let mut output = String::new();
    let mut skipped_indent = None;
    let mut in_subtasks = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let indent = trimmed.len() - trimmed.trim_start_matches(' ').len();
        if let Some(parent) = skipped_indent {
            if trimmed.trim().is_empty() || indent > parent {
                continue;
            }
            skipped_indent = None;
        }
        if indent == 0 && !trimmed.trim().is_empty() && !trimmed.starts_with('-') {
            in_subtasks = trimmed.starts_with("subtasks:");
        }
        let root_field = indent == 0
            && fields
                .iter()
                .any(|field| trimmed.starts_with(&format!("{field}:")));
        let subtask_field = subtasks
            && in_subtasks
            && indent == 2
            && ["status:", "evidence:"]
                .iter()
                .any(|field| trimmed.trim_start().starts_with(field));
        if root_field || subtask_field {
            skipped_indent = Some(indent);
        } else {
            output.push_str(line);
        }
    }
    output
}

pub(super) fn manifest_projection(bytes: &[u8]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(bytes)?;
    let lines: Vec<_> = text.split_inclusive('\n').collect();
    let Some(end) = lines
        .iter()
        .skip(1)
        .position(|line| line.trim() == "---")
        .map(|i| i + 1)
    else {
        bail!("HANDOFF_MANIFEST_INVALID: unterminated frontmatter");
    };
    let mut result = lines[0].to_owned();
    result.push_str(&generated_fields_removed(
        &lines[1..end].concat(),
        &["status", "started_at", "handoff"],
        false,
    ));
    result.push_str(lines[end]);
    let mut cursor = end + 1;
    while cursor < lines.len() {
        let line = lines[cursor];
        result.push_str(line);
        cursor += 1;
        if line.trim() != "```yaml" {
            continue;
        }
        let start = cursor;
        while cursor < lines.len() && lines[cursor].trim() != "```" {
            cursor += 1;
        }
        let yaml = lines[start..cursor].concat();
        let value: serde_yaml::Value = serde_yaml::from_str(&yaml)?;
        if value.get("task_ref").is_some() {
            result.push_str(&generated_fields_removed(
                &yaml,
                &["status", "receipt"],
                true,
            ));
        } else {
            result.push_str(&yaml);
        }
    }
    Ok(result.into_bytes())
}

pub(super) fn content_digest(root: &Path, commit: &str, milestone_path: &str) -> Result<String> {
    let tree =
        crate::core::git::git_command(root, &["ls-tree", "-r", "-z", "--full-tree", commit])?;
    if !tree.status.success() {
        bail!("HANDOFF_CONTENT_UNAVAILABLE: cannot enumerate committed content");
    }
    let mut hash = Sha256::new();
    hash.update(b"forge-handoff-content-v1\0");
    for record in tree
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .context("malformed Git tree entry")?;
        let header = std::str::from_utf8(&record[..tab])?;
        let path = &record[tab + 1..];
        let fields: Vec<_> = header.split_whitespace().collect();
        if fields.len() != 3 {
            bail!("malformed Git tree header");
        }
        hash.update((path.len() as u64).to_be_bytes());
        hash.update(path);
        hash.update(fields[0].as_bytes());
        if path == milestone_path.as_bytes() {
            let blob = crate::core::git::git_command(root, &["cat-file", "blob", fields[2]])?;
            if !blob.status.success() {
                bail!("HANDOFF_CONTENT_UNAVAILABLE: milestone content is unavailable");
            }
            hash.update(Sha256::digest(manifest_projection(&blob.stdout)?));
        } else {
            hash.update(Sha256::digest(fields[2].as_bytes()));
        }
    }
    Ok(hex::encode(hash.finalize()))
}
