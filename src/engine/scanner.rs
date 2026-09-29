use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use ignore::WalkBuilder;
use rayon::prelude::*;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const ENGINE_SCHEMA_VERSION: &str = "3";
pub const INDEX_SEMANTICS_VERSION: &str = concat!("6:", env!("FORGE_LANGUAGE_PROFILE_DIGEST"));
const MAX_FILES: usize = 100_000;
pub(crate) const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 500 * 1024 * 1024;
const DEFAULT_IGNORED_NAMES: &[&str] = &[
    ".git",
    ".forge",
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
];

#[derive(Debug, Clone, Deserialize, Default, Serialize, PartialEq, Eq)]
pub struct LinkedWorkspaceConfig {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub roots: Option<Vec<String>>,
    #[serde(default)]
    pub doc_roots: Option<Vec<String>>,
    #[serde(default)]
    pub ignore: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkedWorkspace {
    pub name: String,
    pub path: PathBuf,
    pub roots: Vec<PathBuf>,
    pub ignored_names: BTreeSet<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct AdapterFile {
    #[serde(default)]
    response: crate::core::response::ResponsePolicy,
    adapter_version: Option<serde_json::Value>,
    roots: Option<Vec<String>>,
    ignore: Option<Vec<String>>,
    eligible_roots: Option<Vec<String>>,
    doc_roots: Option<Vec<String>>,
    excluded_directory_names: Option<Vec<String>>,
    excluded_file_names: Option<Vec<String>>,
    linked_workspaces: Option<Vec<LinkedWorkspaceConfig>>,
    workspaces: Option<Vec<LinkedWorkspaceConfig>>,
}

#[derive(Debug, Clone)]
pub struct Adapter {
    pub adapter_path: Option<PathBuf>,
    pub response: crate::core::response::ResponsePolicy,
    pub roots: Vec<PathBuf>,
    pub ignored_names: BTreeSet<String>,
    pub adapter_version: Option<String>,
    pub digest: String,
    pub linked_workspaces: Vec<LinkedWorkspace>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub digest: String,
    pub bytes: u64,
    pub mtime_ns: u128,
    #[serde(default)]
    pub identity: [u64; 3],
    pub language: String,
    pub is_doc: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanReport {
    pub files: usize,
    pub bytes: u64,
    pub elapsed_ms: u128,
    pub entries: Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildReport {
    pub output: String,
    pub files: usize,
    pub nodes: usize,
    pub doc_sections: usize,
    pub elapsed_ms: u128,
    pub engine_schema_version: &'static str,
}

fn error(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}

pub fn canonical_root(path: &Path) -> std::io::Result<PathBuf> {
    let root = path.canonicalize()?;
    if !root.is_dir() {
        return Err(error("workspace root must be a directory"));
    }
    Ok(root)
}

pub fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

/// Resolve a relative path without traversing a symlink component.
pub fn checked_child(root: &Path, relative: &Path) -> std::io::Result<PathBuf> {
    if !safe_relative(relative) {
        return Err(error("path must be a relative plain path"));
    }
    let target = root.join(relative);
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(error("path must be relative"));
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(error("path must not traverse a symlink"));
            }
            Ok(_) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                // Remaining nonexistent components cannot contain symlinks; recorded deletions retain admission.
                return Ok(target);
            }
            Err(source) => return Err(source),
        }
    }
    if target.exists() && !target.canonicalize()?.starts_with(root) {
        return Err(error("path escapes workspace root"));
    }
    Ok(target)
}

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

fn ensure_bounds(files: usize, bytes: u64) -> std::io::Result<()> {
    if files > MAX_FILES {
        return Err(error("workspace exceeds file limit"));
    }
    if bytes > MAX_TOTAL_BYTES {
        return Err(error("workspace exceeds total byte limit"));
    }
    Ok(())
}

pub fn load_adapter(root: &Path, adapter_path: Option<&Path>) -> std::io::Result<Adapter> {
    let default_path = root.join("forge-mcp.yaml");
    let default_path = default_path.exists().then_some(default_path);
    let selected = adapter_path.or(default_path.as_deref());
    let mut explicit_adapter_path = None;
    let (raw, digest, adapter_version) = match selected {
        None => (AdapterFile::default(), String::new(), None),
        Some(path) => {
            let metadata = fs::symlink_metadata(path)?;
            if metadata.file_type().is_symlink() {
                return Err(error("adapter must not be a symlink"));
            }
            let canonical = path.canonicalize()?;
            if !canonical.starts_with(root) {
                return Err(error("adapter must be inside workspace root"));
            }
            if adapter_path.is_some() {
                explicit_adapter_path = Some(canonical.clone());
            }
            let content = fs::read_to_string(&canonical)?;
            let mut identity: serde_yaml::Value =
                serde_yaml::from_str(&content).map_err(|e| error(e.to_string()))?;
            if let Some(mapping) = identity.as_mapping_mut() {
                mapping.remove(serde_yaml::Value::String("response".into()));
            }
            let hash = if identity
                .as_mapping()
                .is_some_and(|mapping| mapping.is_empty())
            {
                String::new()
            } else {
                let identity =
                    serde_yaml::to_string(&identity).map_err(|e| error(e.to_string()))?;
                format!("{:x}", Sha256::digest(identity.as_bytes()))
            };
            let parsed: AdapterFile =
                serde_yaml::from_str(&content).map_err(|e| error(e.to_string()))?;
            let version_str = parsed.adapter_version.as_ref().map(|v| match v {
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            });
            (parsed, hash, version_str)
        }
    };
    raw.response.validate().map_err(|e| error(e.to_string()))?;
    let roots = raw
        .roots
        .or(raw.eligible_roots)
        .unwrap_or_else(|| vec![".".into()]);
    let roots = roots.into_iter().try_fold(Vec::new(), |mut values, item| {
        if item == "." {
            values.push(root.to_path_buf());
            return Ok(values);
        }
        let relative = Path::new(&item);
        if !safe_relative(relative) {
            return Err(error("adapter roots must be relative plain paths"));
        }
        let candidate = checked_child(root, relative)?;
        if candidate.exists() {
            let metadata = fs::symlink_metadata(&candidate)?;
            if metadata.file_type().is_symlink() {
                return Err(error("adapter root must not be a symlink"));
            }
            values.push(candidate);
        }
        Ok(values)
    })?;
    let mut roots = roots;
    for item in raw.doc_roots.unwrap_or_default() {
        if item.contains('*') || item.contains('?') {
            continue;
        }
        let relative = Path::new(&item);
        if !safe_relative(relative) {
            return Err(error("adapter document roots must be relative plain paths"));
        }
        let candidate = checked_child(root, relative)?;
        if candidate.exists() {
            roots.push(candidate);
        }
    }
    roots.sort();
    roots.dedup();
    let mut linked_workspaces = Vec::new();
    let configs = raw.linked_workspaces.or(raw.workspaces).unwrap_or_default();
    for lw_cfg in configs {
        if !lw_cfg.enabled.unwrap_or(true) {
            continue;
        }
        let target_path = if Path::new(&lw_cfg.path).is_absolute() {
            PathBuf::from(&lw_cfg.path)
        } else {
            root.join(&lw_cfg.path)
        };

        if !target_path.exists() || !target_path.is_dir() {
            eprintln!(
                "[forge-mcp] linked workspace '{}' is unavailable at {:?}; skipping",
                lw_cfg.name, target_path
            );
            continue;
        }

        let canonical_ws = match target_path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                eprintln!(
                    "[forge-mcp] failed to canonicalize linked workspace '{}': {}; skipping",
                    lw_cfg.name, e
                );
                continue;
            }
        };

        let raw_roots = lw_cfg.roots.unwrap_or_else(|| vec![".".into()]);
        let mut ws_roots = Vec::new();
        for item in raw_roots {
            if item == "." {
                ws_roots.push(canonical_ws.clone());
                continue;
            }
            let cand = canonical_ws.join(Path::new(&item));
            if cand.exists() {
                if let Ok(c) = cand.canonicalize() {
                    if c.starts_with(&canonical_ws) {
                        ws_roots.push(c);
                    }
                }
            }
        }
        for item in lw_cfg.doc_roots.unwrap_or_default() {
            if item.contains('*') || item.contains('?') {
                continue;
            }
            let cand = canonical_ws.join(Path::new(&item));
            if cand.exists() {
                if let Ok(c) = cand.canonicalize() {
                    if c.starts_with(&canonical_ws) {
                        ws_roots.push(c);
                    }
                }
            }
        }
        ws_roots.sort();
        ws_roots.dedup();

        let mut ignored: BTreeSet<String> = DEFAULT_IGNORED_NAMES
            .iter()
            .copied()
            .map(str::to_owned)
            .collect();
        if let Some(user_ignores) = lw_cfg.ignore {
            ignored.extend(user_ignores);
        }

        linked_workspaces.push(LinkedWorkspace {
            name: lw_cfg.name,
            path: canonical_ws,
            roots: ws_roots,
            ignored_names: ignored,
        });
    }

    Ok(Adapter {
        adapter_path: explicit_adapter_path,
        response: raw.response,
        roots,
        ignored_names: raw
            .ignore
            .unwrap_or_default()
            .into_iter()
            .chain(raw.excluded_directory_names.unwrap_or_default())
            .chain(raw.excluded_file_names.unwrap_or_default())
            .chain(DEFAULT_IGNORED_NAMES.iter().copied().map(str::to_owned))
            .collect(),
        adapter_version,
        digest,
        linked_workspaces,
    })
}

pub fn language(path: &Path) -> Option<(&'static str, bool)> {
    if matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("md" | "mdx")
    ) {
        return Some(("markdown", true));
    }
    super::languages::for_path(path).map(|p| (p.id(), false))
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn entry_with_rel(path: &Path, relative: String) -> std::io::Result<Option<FileEntry>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(None);
    }
    let Some((language, is_doc)) = language(path) else {
        return Ok(None);
    };
    if metadata.len() > MAX_FILE_BYTES {
        return Err(error(format!(
            "file exceeds byte limit: {}",
            path.display()
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

pub fn entry(root: &Path, path: &Path) -> std::io::Result<Option<FileEntry>> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| error("file escapes root"))?;
    let path = checked_child(root, relative)?;
    entry_with_rel(&path, relative.to_string_lossy().replace('\\', "/"))
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

fn cached_entry(
    full_path: &Path,
    rel_path: &str,
    previous: &std::collections::BTreeMap<&str, &FileEntry>,
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
    entry_with_rel(full_path, rel_path.to_owned())
}

pub fn scan_with_adapter(root: &Path, adapter: &Adapter) -> std::io::Result<ScanReport> {
    scan_reusing(root, adapter, &[])
}

pub fn scan_reusing(
    root: &Path,
    adapter: &Adapter,
    previous: &[FileEntry],
) -> std::io::Result<ScanReport> {
    let started = Instant::now();
    let mut candidates = Vec::new();
    let mut seen_rel_paths = BTreeSet::new();
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
            {
                continue;
            }
            if item.file_type().is_some_and(|kind| kind.is_file()) {
                let full = item.into_path();
                if configuration_paths.contains(&full) {
                    continue;
                }
                if let Ok(rel) = full.strip_prefix(root) {
                    let rel_str = rel.to_string_lossy().replace('\\', "/");
                    if seen_rel_paths.insert(rel_str.clone()) {
                        candidates.push((full, rel_str));
                    }
                }
            }
        }
    }

    for lw in &adapter.linked_workspaces {
        for ws_root in &lw.roots {
            if !ws_root.exists() {
                continue;
            }
            let mut builder = WalkBuilder::new(ws_root);
            builder
                .hidden(false)
                .follow_links(false)
                .same_file_system(true)
                .git_ignore(true);
            let ignored_names = lw.ignored_names.clone();
            builder.filter_entry(move |item| {
                !item.path_is_symlink()
                    && !ignored_names.contains(&item.file_name().to_string_lossy().to_string())
            });
            for result in builder.build() {
                let item = result.map_err(|e| error(e.to_string()))?;
                if item.path_is_symlink()
                    || lw
                        .ignored_names
                        .contains(&item.file_name().to_string_lossy().to_string())
                {
                    continue;
                }
                if item.file_type().is_some_and(|kind| kind.is_file()) {
                    let full = item.into_path();
                    if let Ok(rel) = full.strip_prefix(&lw.path) {
                        let rel_str =
                            format!("[{}]/{}", lw.name, rel.to_string_lossy().replace('\\', "/"));
                        if seen_rel_paths.insert(rel_str.clone()) {
                            candidates.push((full, rel_str));
                        }
                    }
                }
            }
        }
    }

    let previous: std::collections::BTreeMap<_, _> =
        previous.iter().map(|f| (f.path.as_str(), f)).collect();
    let entries: std::io::Result<Vec<_>> = candidates
        .par_iter()
        .map(|(full_path, rel_path)| cached_entry(full_path, rel_path, &previous))
        .collect();
    let mut entries: Vec<_> = entries?.into_iter().flatten().collect();
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    let bytes = entries.iter().map(|item| item.bytes).sum();
    ensure_bounds(entries.len(), bytes)?;
    Ok(ScanReport {
        files: entries.len(),
        bytes,
        elapsed_ms: started.elapsed().as_millis(),
        entries,
    })
}

pub fn scan(root: &Path, adapter_path: Option<&Path>) -> std::io::Result<ScanReport> {
    let root = canonical_root(root)?;
    let adapter = load_adapter(&root, adapter_path)?;
    scan_with_adapter(&root, &adapter)
}
