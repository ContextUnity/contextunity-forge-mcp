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

pub(super) fn digest(value: &impl Serialize) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
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

pub(super) fn require_clean_tree(root: &Path, commit: &str) -> Result<()> {
    let status = crate::core::git::git_command(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    if !status.status.success() || !status.stdout.is_empty() {
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

fn manifest_projection(bytes: &[u8]) -> Result<Vec<u8>> {
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
