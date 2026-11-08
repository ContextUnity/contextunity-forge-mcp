pub(super) use super::seam::validate_seam_reachability;
use anyhow::{bail, Context, Result};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub(super) fn validate_extended_scope_review(
    proof: &serde_json::Value,
    extended_scope: &[String],
) -> Result<()> {
    if extended_scope.is_empty() {
        return Ok(());
    }
    let approval = proof
        .get("review_proof")
        .and_then(|review| review.get("extended_scope_review"))
        .context(
            "TASK_EXTENDED_SCOPE_REVIEW_REQUIRED: review must approve the task's extended scope",
        )?;
    let approved_paths = approval
        .get("approved_paths")
        .and_then(serde_json::Value::as_array)
        .context("TASK_EXTENDED_SCOPE_REVIEW_INVALID: approved_paths must be an array")?;
    let reason_verified = approval
        .get("reason_verified")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !reason_verified {
        bail!("TASK_EXTENDED_SCOPE_REVIEW_INVALID: reason_verified must be true");
    }
    let mut approved = BTreeSet::new();
    for path in approved_paths {
        let path = path
            .as_str()
            .filter(|path| !path.trim().is_empty())
            .context(
                "TASK_EXTENDED_SCOPE_REVIEW_INVALID: approved paths must be non-empty strings",
            )?;
        if !approved.insert(path.to_owned()) {
            bail!("TASK_EXTENDED_SCOPE_REVIEW_INVALID: duplicate approved path '{path}'");
        }
    }
    let expected = extended_scope.iter().cloned().collect::<BTreeSet<_>>();
    if expected.len() != extended_scope.len() || approved != expected {
        bail!("TASK_EXTENDED_SCOPE_REVIEW_INVALID: approved_paths must exactly match the admitted extended_scope");
    }
    Ok(())
}

fn commit_tree_and_parent(worktree: &Path, commit: &str) -> Result<(String, String)> {
    let output = crate::git::git_command(worktree, &["rev-list", "--parents", "-n", "1", commit])?;
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
    let tree = crate::git::git_command(worktree, &["rev-parse", &treeish])?;
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
    let output = crate::git::git_command(worktree, &args)?;
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

pub(super) fn validate_delivery_commit(
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
    let committed_receipt = crate::git::git_command(worktree, &["show", &spec])?;
    if !committed_receipt.status.success()
        || committed_receipt.stdout != expected_receipt.as_bytes()
    {
        bail!("TASK_DELIVERY_COMMIT_INVALID: committed task receipt differs from its commitless milestone document");
    }
    Ok(())
}

pub(super) struct ProvisionalReceiptRollback {
    pub(super) path: PathBuf,
    pub(super) original: String,
    pub(super) armed: bool,
}

impl ProvisionalReceiptRollback {
    pub(super) fn restore(&mut self) -> Result<()> {
        if self.armed {
            crate::core::fs::atomic_replace(&self.path, self.original.as_bytes())?;
            self.armed = false;
        }
        Ok(())
    }

    pub(super) fn disarm(&mut self) {
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
