use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub use crate::core::fs::{canonical_root, checked_child, safe_relative};
#[cfg(all(test, target_os = "linux"))]
use crate::core::sys::available_memory_from_sources;
#[cfg(test)]
use crate::core::sys::memory_budget_from_available;
pub use crate::core::sys::{available_memory_bytes, memory_budget_bytes};
use ignore::WalkBuilder;
use rayon::prelude::*;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

fn error(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}

#[path = "adapter.rs"]
mod adapter;
pub use adapter::{load_adapter, Adapter, LinkedWorkspace, LinkedWorkspaceConfig};

/// The engine schema version value.
pub const ENGINE_SCHEMA_VERSION: &str = "15";
/// The index semantics version value.
pub const INDEX_SEMANTICS_VERSION: &str = concat!(
    "9:compact-storage-v8:",
    env!("FORGE_LANGUAGE_PROFILE_DIGEST")
);
/// The default max files value.
pub const DEFAULT_MAX_FILES: usize = 100_000;
/// The default max file bytes value.
pub const DEFAULT_MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
/// The default max total bytes value.
pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 500 * 1024 * 1024;
pub(crate) const MAX_FILE_BYTES: u64 = DEFAULT_MAX_FILE_BYTES;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
/// Represents scanner limits data.
pub struct ScannerLimits {
    #[serde(default = "default_max_files")]
    /// The max files value.
    pub max_files: usize,
    #[serde(default = "default_max_file_bytes")]
    /// The max file bytes value.
    pub max_file_bytes: u64,
    #[serde(default = "default_max_total_bytes")]
    /// The max total bytes value.
    pub max_total_bytes: u64,
    #[serde(default)]
    /// Whether allow broad root applies.
    pub allow_broad_root: bool,
    #[serde(default, skip_serializing)]
    /// Optional transient memory budget in MiB; YAML takes precedence over the environment.
    pub memory_budget_mb: Option<u64>,
}

fn default_max_files() -> usize {
    DEFAULT_MAX_FILES
}

fn default_max_file_bytes() -> u64 {
    DEFAULT_MAX_FILE_BYTES
}

fn default_max_total_bytes() -> u64 {
    DEFAULT_MAX_TOTAL_BYTES
}

impl Default for ScannerLimits {
    fn default() -> Self {
        Self {
            max_files: default_max_files(),
            max_file_bytes: default_max_file_bytes(),
            max_total_bytes: default_max_total_bytes(),
            allow_broad_root: false,
            memory_budget_mb: None,
        }
    }
}

/// Resolves the transient working-memory budget from YAML, environment, or available memory.
pub fn resolve_memory_budget_bytes(limits: &ScannerLimits) -> std::io::Result<u64> {
    if let Some(mebibytes) = limits.memory_budget_mb {
        return bytes_from_mebibytes(mebibytes);
    }
    let environment = match std::env::var("FORGE_MEMORY_BUDGET_MB") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(error("FORGE_MEMORY_BUDGET_MB must be valid UTF-8"));
        }
    };
    let configured_mb = memory_budget_mb_from_sources(None, environment.as_deref())?;
    let Some(mebibytes) = configured_mb else {
        return Ok(memory_budget_bytes());
    };
    bytes_from_mebibytes(mebibytes)
}

fn memory_budget_mb_from_sources(
    configured_mb: Option<u64>,
    environment_mb: Option<&str>,
) -> std::io::Result<Option<u64>> {
    match configured_mb {
        Some(value) => Ok(Some(value)),
        None => environment_mb
            .map(|value| {
                let mebibytes = value
                    .parse::<u64>()
                    .map_err(|_| error("FORGE_MEMORY_BUDGET_MB must be a positive integer"))?;
                if mebibytes == 0 {
                    return Err(error("FORGE_MEMORY_BUDGET_MB must be a positive integer"));
                }
                Ok(mebibytes)
            })
            .transpose(),
    }
}

fn bytes_from_mebibytes(mebibytes: u64) -> std::io::Result<u64> {
    if mebibytes == 0 {
        return Err(error("memory budget must be greater than zero MiB"));
    }
    mebibytes
        .checked_mul(1024 * 1024)
        .ok_or_else(|| error("memory budget in MiB overflows the byte range"))
}

const DEFAULT_IGNORED_NAMES: &[&str] = &[
    ".git",
    ".forge",
    ".forge-mcp",
    ".venv",
    "__pycache__",
    "target",
    "node_modules",
    "build",
    "dist",
    "coverage",
    ".cache",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".gradle",
    "vendor",
    ".cargo",
    ".idea",
    ".vscode",
    ".tox",
    ".turbo",
    "out",
    ".output",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Represents file entry data.
pub struct FileEntry {
    /// The path value.
    pub path: String,
    /// The digest value.
    pub digest: String,
    /// The bytes value.
    pub bytes: u64,
    /// The mtime ns value.
    pub mtime_ns: u128,
    #[serde(default)]
    /// The identity value.
    pub identity: [u64; 3],
    /// The language value.
    pub language: String,
    /// Whether doc applies.
    pub is_doc: bool,
}

#[derive(Debug, Clone, Serialize)]
/// Represents scan report data.
pub struct ScanReport {
    /// The files value.
    pub files: usize,
    /// The bytes value.
    pub bytes: u64,
    /// The elapsed ms value.
    pub elapsed_ms: u128,
    /// The entries value.
    pub entries: Vec<FileEntry>,
}

/// Source metadata captured by the cold-build walk before opening file content.
#[derive(Debug, Clone)]
pub(crate) struct BuildCandidate {
    pub path: String,
    pub bytes: u64,
    pub mtime_ns: u128,
    pub identity: [u64; 3],
    pub language: String,
    pub is_doc: bool,
}

impl BuildCandidate {
    pub(crate) fn matches_metadata(&self, metadata: &fs::Metadata) -> bool {
        !metadata.file_type().is_symlink()
            && metadata.is_file()
            && self.bytes == metadata.len()
            && self.mtime_ns == metadata_mtime_ns(metadata)
            && self.identity == metadata_identity(metadata)
    }

    pub(crate) fn into_entry(self, digest: String) -> FileEntry {
        FileEntry {
            path: self.path,
            digest,
            bytes: self.bytes,
            mtime_ns: self.mtime_ns,
            identity: self.identity,
            language: self.language,
            is_doc: self.is_doc,
        }
    }
}

/// Summary for the cold-build walk that sends candidates to a live consumer.
pub(crate) struct BuildStreamReport {
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
/// Represents build report data.
pub struct BuildReport {
    /// The output value.
    pub output: String,
    /// The files value.
    pub files: usize,
    /// The nodes value.
    pub nodes: usize,
    /// The doc sections value.
    pub doc_sections: usize,
    /// The elapsed ms value.
    pub elapsed_ms: u128,
    /// The engine schema version value.
    pub engine_schema_version: &'static str,
}

/// Resolves file path.
pub fn resolve_file_path(
    root: &Path,
    adapter: &Adapter,
    file_path: &str,
) -> std::io::Result<PathBuf> {
    if file_path.starts_with('[') {
        if let Some((prefix, rest)) = file_path.split_once("]/") {
            let ws_name = prefix.trim_start_matches('[');
            if let Some(lw) = adapter.linked_workspaces.iter().find(|w| w.name == ws_name) {
                return checked_child(&lw.path, Path::new(rest));
            }
        }
    }
    checked_child(root, Path::new(file_path))
}

fn ensure_bounds(files: usize, bytes: u64, limits: &ScannerLimits) -> std::io::Result<()> {
    if files > limits.max_files {
        return Err(error(format!(
            "workspace exceeds file limit ({} > max_files {})",
            files, limits.max_files
        )));
    }
    if bytes > limits.max_total_bytes {
        return Err(error(format!(
            "workspace exceeds total byte limit ({} > max_total_bytes {})",
            bytes, limits.max_total_bytes
        )));
    }
    Ok(())
}

/// Checks root scope.
pub fn check_root_scope(root: &Path, allow_broad: bool) -> std::io::Result<()> {
    if allow_broad
        || std::env::var("FORGE_ALLOW_BROAD_ROOT")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    {
        return Ok(());
    }

    // 1. Filesystem root
    if root == Path::new("/") {
        return Err(error(
            "refusing to index filesystem root '/'; specify a repository directory or set limits.allow_broad_root: true",
        ));
    }

    // 2. Direct parent is / or /home or /Users
    if let Some(parent) = root.parent() {
        if parent == Path::new("/") {
            let name = root.file_name().unwrap_or_default().to_string_lossy();
            if matches!(
                name.as_ref(),
                "home" | "Users" | "var" | "tmp" | "usr" | "etc" | "opt"
            ) {
                return Err(error(format!(
                    "refusing to index system directory '{root:?}'; specify a repository directory or set limits.allow_broad_root: true"
                )));
            }
        } else if parent == Path::new("/home") || parent == Path::new("/Users") {
            return Err(error(format!(
                "refusing to index user home directory '{root:?}'; specify a repository directory or set limits.allow_broad_root: true"
            )));
        }
    }

    // 3. Multi-repository container check:
    let is_git_repo = root.join(".git").exists();
    let has_manifest = root.join("forge-mcp.yaml").exists()
        || root.join("Cargo.toml").exists()
        || root.join("package.json").exists()
        || root.join("pyproject.toml").exists()
        || root.join("go.mod").exists();

    if !is_git_repo && !has_manifest {
        if let Ok(entries) = fs::read_dir(root) {
            let mut git_subdirs = 0;
            for entry in entries.flatten() {
                if let Ok(ft) = entry.file_type() {
                    if ft.is_dir() {
                        let p = entry.path();
                        if p.join(".git").exists() {
                            git_subdirs += 1;
                        } else if let Ok(sub_entries) = fs::read_dir(&p) {
                            for sub_entry in sub_entries.flatten() {
                                if let Ok(sft) = sub_entry.file_type() {
                                    if sft.is_dir() && sub_entry.path().join(".git").exists() {
                                        git_subdirs += 1;
                                    }
                                }
                            }
                        }
                    }
                }
                if git_subdirs >= 2 {
                    return Err(error(format!(
                        "workspace root '{root:?}' appears to be a multi-repository container ({git_subdirs}+ repos detected); specify a single repository or set limits.allow_broad_root: true"
                    )));
                }
            }
        }
    }

    Ok(())
}

pub(crate) fn bounded_batch_size(
    memory_budget: u64,
    max_item_bytes: u64,
    expansion_factor: u64,
    max_items: usize,
) -> usize {
    let estimated_item_bytes = max_item_bytes
        .max(1)
        .saturating_mul(expansion_factor.max(1));
    ((memory_budget.max(1) / estimated_item_bytes)
        .max(1)
        .min(max_items.max(1) as u64)) as usize
}

fn path_component_matches(component: &str, pattern: &str) -> bool {
    if pattern == "*" || pattern == "**" {
        return true;
    }
    if let Some((prefix, suffix)) = pattern.split_once('*') {
        component.starts_with(prefix)
            && component.ends_with(suffix)
            && component.len() >= prefix.len() + suffix.len()
    } else {
        component == pattern
    }
}

/// Checks if a relative path matches a pattern that may contain '*' components.
pub fn matches_path_pattern(rel_path: &str, pattern: &str) -> bool {
    let rel_norm = rel_path.trim_matches('/').replace('\\', "/");
    let pat_norm = pattern.trim_matches('/').replace('\\', "/");
    if pat_norm.is_empty() || rel_norm.is_empty() {
        return false;
    }
    if pat_norm.contains('*') {
        let pattern_parts: Vec<&str> = pat_norm.split('/').collect();
        let path_parts: Vec<&str> = rel_norm.split('/').collect();
        if path_parts.len() < pattern_parts.len() {
            return false;
        }
        for (i, part) in pattern_parts.iter().enumerate() {
            if !path_component_matches(path_parts[i], part) {
                return false;
            }
        }
        true
    } else {
        rel_norm == pat_norm || rel_norm.starts_with(&format!("{}/", pat_norm))
    }
}

/// Checks if a relative path belongs to any configured milestone or plan directories.
pub fn is_milestone_or_plan_path(rel_str: &str, milestones: &[String], plans: &[String]) -> bool {
    let check = |dirs: &[String]| dirs.iter().any(|d| matches_path_pattern(rel_str, d));
    check(milestones) || check(plans)
}

/// Checks whether a path is the overview README directly inside a configured milestone or plan directory.
fn is_milestone_or_plan_readme(rel_str: &str, milestones: &[String], plans: &[String]) -> bool {
    if rel_str.rsplit('/').next() != Some("README.md") {
        return false;
    }
    let Some((parent, _)) = rel_str.rsplit_once('/') else {
        return false;
    };
    let parent = parent.trim_matches('/').replace('\\', "/");
    let parent_depth = parent.split('/').count();
    let is_direct_overview = |dirs: &[String]| {
        dirs.iter().any(|dir| {
            let normalized_dir = dir.trim_matches('/').replace('\\', "/");
            normalized_dir.split('/').count() == parent_depth
                && matches_path_pattern(&parent, &normalized_dir)
        })
    };
    is_direct_overview(milestones) || is_direct_overview(plans)
}

/// Performs language.
pub fn language(path: &Path) -> Option<(&'static str, bool)> {
    if matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("md" | "mdx")
    ) {
        return Some(("markdown", true));
    }
    super::languages::for_path(path).map(|p| (p.id(), false))
}

/// Performs digest.
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Performs entry with rel.
pub fn entry_with_rel(
    path: &Path,
    relative: String,
    max_file_bytes: u64,
) -> std::io::Result<Option<FileEntry>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(None);
    }
    let Some((language, is_doc)) = language(path) else {
        return Ok(None);
    };
    if metadata.len() > max_file_bytes {
        return Err(error(format!(
            "file exceeds byte limit: {} ({} > max_file_bytes {})",
            path.display(),
            metadata.len(),
            max_file_bytes
        )));
    }
    let bytes = fs::read(path)?;
    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let mtime_ns = modified
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(Some(FileEntry {
        path: relative,
        digest: digest(&bytes),
        bytes: metadata.len(),
        mtime_ns,
        identity: metadata_identity(&metadata),
        language: language.to_owned(),
        is_doc,
    }))
}

/// Performs entry.
pub fn entry(root: &Path, path: &Path) -> std::io::Result<Option<FileEntry>> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| error("file escapes root"))?;
    let path = checked_child(root, relative)?;
    entry_with_rel(
        &path,
        relative.to_string_lossy().replace('\\', "/"),
        DEFAULT_MAX_FILE_BYTES,
    )
}

fn metadata_identity(metadata: &fs::Metadata) -> [u64; 3] {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        [
            metadata.dev(),
            metadata.ino(),
            (metadata.ctime() as u64)
                .wrapping_mul(1_000_000_000)
                .wrapping_add(metadata.ctime_nsec() as u64),
        ]
    }
    #[cfg(not(unix))]
    {
        [0; 3]
    }
}

fn metadata_mtime_ns(metadata: &fs::Metadata) -> u128 {
    metadata
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn cached_entry(
    full_path: &Path,
    rel_path: &str,
    previous: &std::collections::BTreeMap<&str, &FileEntry>,
    max_file_bytes: u64,
) -> std::io::Result<Option<FileEntry>> {
    if let Some(old) = previous.get(rel_path) {
        let metadata = fs::symlink_metadata(full_path)?;
        let modified = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        if metadata.is_file()
            && !metadata.file_type().is_symlink()
            && old.bytes == metadata.len()
            && old.mtime_ns == modified
            && old.identity == metadata_identity(&metadata)
            && old.identity != [0; 3]
        {
            return Ok(Some((*old).clone()));
        }
    }
    entry_with_rel(full_path, rel_path.to_owned(), max_file_bytes)
}

/// Performs scan with adapter.
pub fn scan_with_adapter(root: &Path, adapter: &Adapter) -> std::io::Result<ScanReport> {
    check_root_scope(root, adapter.limits.allow_broad_root)?;
    scan_reusing(root, adapter, &[])
}

/// Performs scan reusing.
pub fn scan_reusing(
    root: &Path,
    adapter: &Adapter,
    previous: &[FileEntry],
) -> std::io::Result<ScanReport> {
    let started = Instant::now();
    let (candidates, _) = collect_candidates(root, adapter)?;

    let previous: std::collections::BTreeMap<_, _> =
        previous.iter().map(|f| (f.path.as_str(), f)).collect();
    let max_file_bytes = adapter.limits.max_file_bytes;
    let scan_budget =
        resolve_memory_budget_bytes(&adapter.limits)?.min(adapter.limits.max_total_bytes.max(1));
    let batch_size = bounded_batch_size(scan_budget, max_file_bytes, 1, candidates.len().max(1));
    let mut entries = Vec::new();
    let mut bytes = 0u64;
    for batch in candidates.chunks(batch_size) {
        let batch_entries: std::io::Result<Vec<_>> = batch
            .par_iter()
            .map(|(full_path, rel_path)| {
                cached_entry(full_path, rel_path, &previous, max_file_bytes)
            })
            .collect();
        let batch_entries = batch_entries?;
        let batch_bytes = batch_entries
            .iter()
            .flatten()
            .try_fold(0u64, |total, entry| total.checked_add(entry.bytes))
            .ok_or_else(|| error("workspace byte count overflow"))?;
        bytes = bytes
            .checked_add(batch_bytes)
            .ok_or_else(|| error("workspace byte count overflow"))?;
        if bytes > adapter.limits.max_total_bytes {
            return Err(error(format!(
                "workspace exceeds total byte limit ({} > max_total_bytes {})",
                bytes, adapter.limits.max_total_bytes
            )));
        }
        entries.extend(batch_entries.into_iter().flatten());
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    ensure_bounds(entries.len(), bytes, &adapter.limits)?;
    Ok(ScanReport {
        files: entries.len(),
        bytes,
        elapsed_ms: started.elapsed().as_millis(),
        entries,
    })
}

/// Streams metadata-only cold-build candidates to the pipeline as the walker discovers them.
pub(crate) fn stream_for_build(
    root: &Path,
    adapter: &Adapter,
    mut visit: impl FnMut(BuildCandidate) -> std::io::Result<()>,
) -> std::io::Result<BuildStreamReport> {
    let mut files = 0usize;
    let mut bytes = 0u64;
    let _dependency_manifests = walk_candidate_paths(root, adapter, |full_path, relative_path| {
        let metadata = fs::symlink_metadata(&full_path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Ok(());
        }
        let Some((language, is_doc)) = language(&full_path) else {
            return Ok(());
        };
        if metadata.len() > adapter.limits.max_file_bytes {
            return Err(error(format!(
                "file exceeds byte limit: {} ({} > max_file_bytes {})",
                full_path.display(),
                metadata.len(),
                adapter.limits.max_file_bytes
            )));
        }
        bytes = bytes
            .checked_add(metadata.len())
            .ok_or_else(|| error("workspace byte count overflow"))?;
        if bytes > adapter.limits.max_total_bytes {
            return Err(error(format!(
                "workspace exceeds total byte limit ({} > max_total_bytes {})",
                bytes, adapter.limits.max_total_bytes
            )));
        }
        files += 1;
        visit(BuildCandidate {
            path: relative_path,
            bytes: metadata.len(),
            mtime_ns: metadata_mtime_ns(&metadata),
            identity: metadata_identity(&metadata),
            language: language.to_owned(),
            is_doc,
        })?;
        Ok(())
    })?;
    ensure_bounds(files, bytes, &adapter.limits)?;
    Ok(BuildStreamReport { files, bytes })
}

/// Discovers dependency manifests without retaining the cold-build candidate list.
pub(crate) fn dependency_manifests_for_build(
    root: &Path,
    adapter: &Adapter,
) -> std::io::Result<Vec<PathBuf>> {
    walk_candidate_paths(root, adapter, |_, _| Ok(()))
}

type ScanCandidates = (Vec<(PathBuf, String)>, Vec<PathBuf>);

fn collect_candidates(root: &Path, adapter: &Adapter) -> std::io::Result<ScanCandidates> {
    let mut candidates = Vec::new();
    let dependency_manifests = walk_candidate_paths(root, adapter, |full, relative| {
        candidates.push((full, relative));
        Ok(())
    })?;
    Ok((candidates, dependency_manifests))
}

fn walk_candidate_paths(
    root: &Path,
    adapter: &Adapter,
    mut visit: impl FnMut(PathBuf, String) -> std::io::Result<()>,
) -> std::io::Result<Vec<PathBuf>> {
    let mut candidate_count = 0usize;
    let mut seen_rel_paths = BTreeSet::new();
    let mut dependency_manifests = BTreeSet::new();
    let configuration_paths: BTreeSet<_> = [root.join("forge-mcp.yaml")]
        .into_iter()
        .chain(adapter.adapter_path.iter().cloned())
        .collect();

    for source_root in &adapter.roots {
        if !source_root.exists() {
            continue;
        }
        let mut builder = WalkBuilder::new(source_root);
        builder
            .hidden(false)
            .follow_links(false)
            .same_file_system(true)
            .git_ignore(true);
        let ignored_names = adapter.ignored_names.clone();
        builder.filter_entry(move |item| {
            !item.path_is_symlink()
                && !ignored_names.contains(&item.file_name().to_string_lossy().to_string())
        });
        for result in builder.build() {
            let item = result.map_err(|e| error(e.to_string()))?;
            if item.path_is_symlink()
                || adapter
                    .ignored_names
                    .contains(&item.file_name().to_string_lossy().to_string())
                || !item.file_type().is_some_and(|kind| kind.is_file())
            {
                continue;
            }
            let full = item.into_path();
            if configuration_paths.contains(&full) {
                continue;
            }
            let filename = full
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            let is_manifest =
                super::languages::dependency_registry::is_dependency_manifest_filename(filename);
            let is_source = language(&full).is_some();
            if !is_manifest && !is_source {
                continue;
            }
            let Ok(relative) = full.strip_prefix(root) else {
                continue;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            if is_milestone_or_plan_path(&relative, &adapter.milestones, &adapter.plans)
                && !is_milestone_or_plan_readme(&relative, &adapter.milestones, &adapter.plans)
            {
                continue;
            }
            if is_manifest {
                dependency_manifests.insert(full.clone());
            }
            if is_source {
                visit_candidate(
                    &mut candidate_count,
                    &mut seen_rel_paths,
                    full,
                    relative,
                    adapter.limits.max_files,
                    &mut visit,
                )?;
            }
        }
    }

    for workspace in &adapter.linked_workspaces {
        for workspace_root in &workspace.roots {
            if !workspace_root.exists() {
                continue;
            }
            let mut builder = WalkBuilder::new(workspace_root);
            builder
                .hidden(false)
                .follow_links(false)
                .same_file_system(true)
                .git_ignore(true);
            let ignored_names = workspace.ignored_names.clone();
            builder.filter_entry(move |item| {
                !item.path_is_symlink()
                    && !ignored_names.contains(&item.file_name().to_string_lossy().to_string())
            });
            for result in builder.build() {
                let item = result.map_err(|e| error(e.to_string()))?;
                if item.path_is_symlink()
                    || workspace
                        .ignored_names
                        .contains(&item.file_name().to_string_lossy().to_string())
                    || !item.file_type().is_some_and(|kind| kind.is_file())
                {
                    continue;
                }
                let full = item.into_path();
                let filename = full
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default();
                let is_manifest =
                    super::languages::dependency_registry::is_dependency_manifest_filename(
                        filename,
                    );
                let is_source = language(&full).is_some();
                if !is_manifest && !is_source {
                    continue;
                }
                let Ok(relative) = full.strip_prefix(&workspace.path) else {
                    continue;
                };
                let relative = relative.to_string_lossy().replace('\\', "/");
                if is_milestone_or_plan_path(&relative, &adapter.milestones, &adapter.plans)
                    && !is_milestone_or_plan_readme(&relative, &adapter.milestones, &adapter.plans)
                {
                    continue;
                }
                if is_manifest {
                    dependency_manifests.insert(full.clone());
                }
                if is_source {
                    visit_candidate(
                        &mut candidate_count,
                        &mut seen_rel_paths,
                        full,
                        format!("[{}]/{relative}", workspace.name),
                        adapter.limits.max_files,
                        &mut visit,
                    )?;
                }
            }
        }
        collect_source_root_manifests(
            &workspace.path,
            &workspace.roots,
            &workspace.ignored_names,
            &mut dependency_manifests,
        )?;
    }
    collect_source_root_manifests(
        root,
        &adapter.roots,
        &adapter.ignored_names,
        &mut dependency_manifests,
    )?;
    Ok(dependency_manifests.into_iter().collect())
}

fn visit_candidate(
    candidate_count: &mut usize,
    seen_rel_paths: &mut BTreeSet<String>,
    full_path: PathBuf,
    relative_path: String,
    max_files: usize,
    visit: &mut impl FnMut(PathBuf, String) -> std::io::Result<()>,
) -> std::io::Result<()> {
    if seen_rel_paths.insert(relative_path.clone()) {
        *candidate_count += 1;
        if *candidate_count > max_files {
            return Err(error(format!(
                "workspace exceeds file limit ({} > max_files {})",
                *candidate_count, max_files
            )));
        }
        visit(full_path, relative_path)?;
    }
    Ok(())
}

fn collect_source_root_manifests(
    workspace_root: &Path,
    source_roots: &[PathBuf],
    ignored_names: &BTreeSet<String>,
    output: &mut BTreeSet<PathBuf>,
) -> std::io::Result<()> {
    let mut directories = BTreeSet::new();
    for source_root in source_roots {
        let mut directory = source_root.parent();
        while let Some(path) = directory {
            if !path.starts_with(workspace_root) || path == workspace_root {
                break;
            }
            let relative = path
                .strip_prefix(workspace_root)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            if relative.components().any(|component| {
                component
                    .as_os_str()
                    .to_str()
                    .is_some_and(|name| ignored_names.contains(name))
            }) {
                break;
            }
            directories.insert(path.to_path_buf());
            directory = path.parent();
        }
    }
    for directory in directories {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let filename = entry.file_name();
            let Some(filename) = filename.to_str() else {
                continue;
            };
            if ignored_names.contains(filename)
                || !super::languages::dependency_registry::is_dependency_manifest_filename(filename)
            {
                continue;
            }
            let candidate = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&candidate) else {
                continue;
            };
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                continue;
            }
            let relative = candidate
                .strip_prefix(workspace_root)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            output.insert(checked_child(workspace_root, relative)?);
        }
    }
    Ok(())
}

/// Performs scan.
pub fn scan(root: &Path, adapter_path: Option<&Path>) -> std::io::Result<ScanReport> {
    let root = canonical_root(root)?;
    let adapter = load_adapter(&root, adapter_path)?;
    check_root_scope(&root, adapter.limits.allow_broad_root)?;
    scan_with_adapter(&root, &adapter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_batch_size_accounts_for_item_expansion_and_caps() {
        assert_eq!(
            bounded_batch_size(256 * 1024 * 1024, 5 * 1024 * 1024, 8, 25_000),
            6
        );
        assert_eq!(
            bounded_batch_size(16 * 1024 * 1024 * 1024, 5 * 1024 * 1024, 8, 25_000),
            409
        );
        assert_eq!(bounded_batch_size(1, 0, 0, 10), 1);
        assert_eq!(bounded_batch_size(1_000, 1, 1, 4), 4);
    }

    #[test]
    fn explicit_memory_budgets_validate_positive_mib_and_overflow() {
        assert_eq!(bytes_from_mebibytes(50).unwrap(), 50 * 1024 * 1024);
        assert!(bytes_from_mebibytes(0).is_err());
        assert!(bytes_from_mebibytes(u64::MAX).is_err());
    }

    #[test]
    fn build_scan_excludes_ignored_manifests_from_source_root_ancestors() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "forge_ignored_ancestor_manifests_{}_{}",
            std::process::id(),
            nonce
        ));
        let source = root.join("packages/pkg/src");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            root.join("forge-mcp.yaml"),
            "roots: [packages/pkg/src]\nignore: [package.json]\n",
        )
        .unwrap();
        fs::write(root.join("packages/pkg/package.json"), "{}").unwrap();
        fs::write(root.join("packages/package.json"), "{}").unwrap();
        fs::write(
            root.join("packages/pyproject.toml"),
            "[project]\nname='kept'\n",
        )
        .unwrap();
        fs::write(source.join("index.ts"), "export const value = 1;\n").unwrap();

        let root = root.canonicalize().unwrap();
        let adapter = load_adapter(&root, None).unwrap();
        let manifests = dependency_manifests_for_build(&root, &adapter).unwrap();

        assert!(
            manifests
                .iter()
                .any(|path| path.ends_with("packages/pyproject.toml")),
            "ancestor traversal should retain an eligible manifest: {manifests:?}"
        );
        assert!(
            manifests
                .iter()
                .all(|path| path.file_name().is_none_or(|name| name != "package.json")),
            "ignored basenames must be excluded at every source-root ancestor: {manifests:?}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn yaml_memory_budget_precedes_the_environment() {
        assert_eq!(
            memory_budget_mb_from_sources(Some(200), Some("not-a-number")).unwrap(),
            Some(200)
        );
        assert_eq!(
            memory_budget_mb_from_sources(None, Some("50")).unwrap(),
            Some(50)
        );
        assert!(memory_budget_mb_from_sources(None, Some("0")).is_err());
        assert!(memory_budget_mb_from_sources(None, Some("bad")).is_err());
        assert_eq!(memory_budget_mb_from_sources(None, None).unwrap(), None);
    }

    #[test]
    fn memory_budget_uses_a_64_mib_floor_and_16_gib_cap() {
        assert_eq!(memory_budget_from_available(0), 64 * 1024 * 1024);
        assert_eq!(
            memory_budget_from_available(768 * 1024 * 1024),
            256 * 1024 * 1024
        );
        assert_eq!(
            memory_budget_from_available(64 * 1024 * 1024 * 1024),
            16 * 1024 * 1024 * 1024
        );
    }

    #[cfg(target_os = "linux")]
    struct CgroupFixture(PathBuf);

    #[cfg(target_os = "linux")]
    impl CgroupFixture {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "forge_cgroup_memory_{}_{}",
                std::process::id(),
                nonce
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, path: &str, content: &str) {
            let path = self.root().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }

        fn root(&self) -> PathBuf {
            self.0.join("cgroup")
        }
    }

    #[cfg(target_os = "linux")]
    impl Drop for CgroupFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn nested_cgroup_v2_uses_tightest_limit_and_host_headroom() {
        let fixture = CgroupFixture::new();
        fixture.write("pod/memory.max", "400\n");
        fixture.write("pod/memory.current", "200\n");
        fixture.write("pod/container/memory.max", "max\n");
        let proc_cgroup = "0::/pod/container\n";

        assert_eq!(
            available_memory_from_sources(500, Some(proc_cgroup), &fixture.root()),
            200
        );
        assert_eq!(
            available_memory_from_sources(100, Some(proc_cgroup), &fixture.root()),
            100
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unavailable_or_unresolvable_membership_uses_safe_cgroup_root() {
        let fixture = CgroupFixture::new();
        fixture.write("memory.max", "500\n");
        fixture.write("memory.current", "200\n");

        assert_eq!(
            available_memory_from_sources(600, None, &fixture.root()),
            300
        );
        assert_eq!(
            available_memory_from_sources(
                600,
                Some("0::/namespace/path/not-mounted\n"),
                &fixture.root()
            ),
            300
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unlimited_or_over_limit_cgroups_fall_back_to_host_memory() {
        let fixture = CgroupFixture::new();
        fixture.write("memory.max", "max\n");
        fixture.write("pod/memory.max", "max\n");
        fixture.write("pod/container/memory.max", "max\n");
        let proc_cgroup = "0::/pod/container\n";
        assert_eq!(
            available_memory_from_sources(321, Some(proc_cgroup), &fixture.root()),
            321
        );

        fixture.write("memory.max", "200\n");
        fixture.write("memory.current", "100\n");
        fixture.write("pod/memory.max", "100\n");
        fixture.write("pod/memory.current", "101\n");
        fixture.write("pod/container/memory.max", "max\n");
        assert_eq!(
            available_memory_from_sources(321, Some(proc_cgroup), &fixture.root()),
            321
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn malformed_or_unsafe_cgroup_paths_fall_back_to_host_memory() {
        let fixture = CgroupFixture::new();
        fixture.write("bad/memory.max", "not-a-limit\n");
        assert_eq!(
            available_memory_from_sources(321, Some("0::/bad\n"), &fixture.root()),
            321
        );

        fixture.write("memory.max", "max\n");
        fs::write(fixture.0.join("memory.max"), "max\n").unwrap();
        let escaped = fixture.0.join("host");
        fs::create_dir_all(&escaped).unwrap();
        fs::write(escaped.join("memory.max"), "100\n").unwrap();
        fs::write(escaped.join("memory.current"), "90\n").unwrap();
        assert_eq!(
            available_memory_from_sources(321, Some("0::/../host\n"), &fixture.root()),
            321
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cgroup_v1_over_limit_falls_back_without_subtraction_panic() {
        let fixture = CgroupFixture::new();
        fixture.write("memory/memory.limit_in_bytes", "100\n");
        fixture.write("memory/memory.usage_in_bytes", "40\n");
        let proc_cgroup = "5:cpu,memory:/\n";
        assert_eq!(
            available_memory_from_sources(321, Some(proc_cgroup), &fixture.root()),
            60
        );

        // The unified path exists, but has no v2 memory-controller files.
        fs::create_dir_all(fixture.root().join("nested")).unwrap();
        fixture.write("memory/nested/memory.limit_in_bytes", "100\n");
        fixture.write("memory/nested/memory.usage_in_bytes", "40\n");
        let hybrid_cgroups = "0::/nested\n7:memory:/nested\n";
        assert_eq!(
            available_memory_from_sources(321, Some(hybrid_cgroups), &fixture.root()),
            60
        );

        fixture.write("memory/memory.usage_in_bytes", "101\n");
        assert_eq!(
            available_memory_from_sources(321, Some(proc_cgroup), &fixture.root()),
            321
        );
    }

    #[test]
    fn matches_path_pattern_supports_wildcards_and_partial_globs() {
        assert!(matches_path_pattern(
            "extensions/commerce/docs/milestones/010-test.md",
            "extensions/*/docs/milestones"
        ));
        assert!(matches_path_pattern(
            "services/auth-api/docs/plans/arch.md",
            "services/*-api/docs/plans"
        ));
        assert!(matches_path_pattern(
            "docs/milestones/020.md",
            "docs/milestones"
        ));
        assert!(!matches_path_pattern(
            "docs/other/020.md",
            "docs/milestones"
        ));
        assert!(!matches_path_pattern(
            "extensions/commerce/src/lib.rs",
            "extensions/*/docs/milestones"
        ));
    }
}
