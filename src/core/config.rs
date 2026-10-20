//! Workspace adapter configuration, scanner limits, and path resolution.

pub use crate::core::fs::{checked_child, safe_relative};
use crate::core::response::ResponsePolicy;
use crate::core::sys::memory_budget_bytes;
use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Default maximum number of files admitted for indexing.
pub const DEFAULT_MAX_FILES: usize = 100_000;
/// Default maximum size in bytes for a single indexable source file (5 MiB).
pub const DEFAULT_MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
/// Default maximum total size in bytes for all indexable files (500 MiB).
pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 500 * 1024 * 1024;
/// Alias for maximum allowed file bytes.
pub const MAX_FILE_BYTES: u64 = DEFAULT_MAX_FILE_BYTES;

/// Standard directory and file names ignored during scanning.
pub const DEFAULT_IGNORED_NAMES: &[&str] = &[
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

fn error(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}

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

/// Parses and validates transient memory budget from configuration or environment.
pub fn memory_budget_mb_from_sources(
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

/// Converts mebibytes to bytes with overflow validation.
pub fn bytes_from_mebibytes(mebibytes: u64) -> std::io::Result<u64> {
    if mebibytes == 0 {
        return Err(error("memory budget must be greater than zero MiB"));
    }
    mebibytes
        .checked_mul(1024 * 1024)
        .ok_or_else(|| error("memory budget in MiB overflows the byte range"))
}

#[derive(Debug, Clone, Deserialize, Default)]
/// Represents adapter file YAML representation.
pub struct AdapterFile {
    #[serde(default)]
    /// The response policy.
    pub response: ResponsePolicy,
    #[serde(default)]
    /// Optional scanner limits.
    pub limits: Option<ScannerLimits>,
    /// Optional adapter version.
    pub adapter_version: Option<serde_json::Value>,
    /// Optional roots list.
    pub roots: Option<Vec<String>>,
    /// Optional ignore list.
    pub ignore: Option<Vec<String>>,
    /// Optional eligible roots list.
    pub eligible_roots: Option<Vec<String>>,
    #[serde(default, alias = "doc_roots")]
    /// Optional documentation roots.
    pub docs: Option<Vec<String>>,
    /// Optional milestones directories.
    pub milestones: Option<Vec<String>>,
    /// Optional plans directories.
    pub plans: Option<Vec<String>>,
    /// Optional excluded directory names.
    pub excluded_directory_names: Option<Vec<String>>,
    /// Optional excluded file names.
    pub excluded_file_names: Option<Vec<String>>,
    /// Optional linked workspaces configuration.
    pub linked_workspaces: Option<Vec<LinkedWorkspaceConfig>>,
    /// Optional workspaces alias.
    pub workspaces: Option<Vec<LinkedWorkspaceConfig>>,
    /// Optional owners map.
    pub owners: Option<BTreeMap<String, String>>,
    /// Optional aliases map.
    pub aliases: Option<BTreeMap<String, String>>,
    #[serde(default, rename = "DEBUG", alias = "debug")]
    /// Debug mode flag.
    pub debug: bool,
}

#[derive(Debug, Clone, Deserialize, Default, Serialize, PartialEq, Eq)]
/// Configures linked workspace operations.
pub struct LinkedWorkspaceConfig {
    /// The name value.
    pub name: String,
    /// The path value.
    pub path: String,
    #[serde(default)]
    /// Optional enabled value.
    pub enabled: Option<bool>,
    #[serde(default)]
    /// Optional roots value.
    pub roots: Option<Vec<String>>,
    #[serde(default, alias = "doc_roots")]
    /// Optional docs value.
    pub docs: Option<Vec<String>>,
    #[serde(default)]
    /// Optional ignore value.
    pub ignore: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Represents linked workspace data.
pub struct LinkedWorkspace {
    /// The name value.
    pub name: String,
    /// The path value.
    pub path: PathBuf,
    /// The roots value.
    pub roots: Vec<PathBuf>,
    /// The ignored names value.
    pub ignored_names: BTreeSet<String>,
}

#[derive(Debug, Clone)]
/// Represents workspace adapter configuration.
pub struct Adapter {
    /// Optional adapter path value.
    pub adapter_path: Option<PathBuf>,
    /// The response value.
    pub response: ResponsePolicy,
    /// The limits value.
    pub limits: ScannerLimits,
    /// The roots value.
    pub roots: Vec<PathBuf>,
    /// The ignored names value.
    pub ignored_names: BTreeSet<String>,
    /// Optional adapter version value.
    pub adapter_version: Option<String>,
    /// The digest value.
    pub digest: String,
    /// The linked workspaces value.
    pub linked_workspaces: Vec<LinkedWorkspace>,
    /// The owners value.
    pub owners: BTreeMap<String, String>,
    /// The aliases value.
    pub aliases: BTreeMap<String, String>,
    /// Milestone directories for task discovery; Markdown documents remain indexed.
    pub milestones: Vec<String>,
    /// Plan directories for task discovery; Markdown documents remain indexed.
    pub plans: Vec<String>,
    /// Whether debug applies.
    pub debug: bool,
}

/// Loads and normalizes workspace adapter configuration.
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
                mapping.remove(serde_yaml::Value::String("DEBUG".into()));
                mapping.remove(serde_yaml::Value::String("debug".into()));
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
    for item in raw.docs.unwrap_or_default() {
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
        for item in lw_cfg.docs.unwrap_or_default() {
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
        limits: raw.limits.unwrap_or_default(),
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
        owners: raw.owners.unwrap_or_default(),
        aliases: raw.aliases.unwrap_or_default(),
        milestones: raw
            .milestones
            .unwrap_or_else(|| vec!["docs/milestones".into()]),
        plans: raw.plans.unwrap_or_else(|| vec!["docs/plans".into()]),
        debug: raw.debug
            || std::env::var("FORGE_DEBUG")
                .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            || std::env::var("DEBUG").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true")),
    })
}

/// Resolves a workspace-relative file path against workspace root and linked workspaces.
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

/// Checks root scope against filesystem roots, system directories, user home directories, and multi-repository containers.
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

/// Formats the adapter configuration for serialization into SQLite database metadata.
pub fn policy(root: &Path, adapter: &Adapter) -> Value {
    json!({
        "adapter_path": adapter.adapter_path.as_ref().map(|path| path.strip_prefix(root).unwrap_or(path).to_string_lossy()),
        "roots": adapter.roots.iter().map(|p| p.strip_prefix(root).unwrap_or(p).to_string_lossy()).collect::<Vec<_>>(),
        "limits": &adapter.limits,
        "ignored_names": adapter.ignored_names,
        "adapter_version": adapter.adapter_version,
        "digest": adapter.digest,
        "linked_workspaces": adapter.linked_workspaces,
        "owners": adapter.owners,
        "aliases": adapter.aliases,
        "milestones": adapter.milestones,
        "plans": adapter.plans,
    })
}

/// Resolves the persisted adapter path recorded in SQLite database metadata.
pub fn persisted_adapter_path(root: &Path, value: &Value) -> Result<Option<PathBuf>> {
    match value.get("adapter_path") {
        None | Some(Value::Null) => Ok(None),
        Some(path) => Ok(Some(checked_child(
            root,
            Path::new(path.as_str().context("invalid persisted adapter path")?),
        )?)),
    }
}

/// Reads and reconstructs an Adapter from the database metadata table.
pub fn read_policy(root: &Path, conn: &Connection) -> Result<Adapter> {
    let raw: String =
        conn.query_row("SELECT value FROM metadata WHERE key='adapter'", [], |r| {
            r.get(0)
        })?;
    let value: Value = serde_json::from_str(&raw)?;
    let adapter_path = persisted_adapter_path(root, &value)?;
    let roots = value["roots"]
        .as_array()
        .context("invalid persisted roots")?
        .iter()
        .map(|v| {
            let p = v.as_str().context("invalid root")?;
            if p.is_empty() {
                Ok(root.to_path_buf())
            } else {
                Ok(checked_child(root, Path::new(p))?)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let limits: ScannerLimits = serde_json::from_value(value["limits"].clone())
        .context("invalid persisted limits")?;
    let ignored_names: BTreeSet<String> = serde_json::from_value(value["ignored_names"].clone())
        .context("invalid persisted ignored names")?;
    let adapter_version = value["adapter_version"].as_str().map(ToOwned::to_owned);
    let digest = value["digest"].as_str().unwrap_or_default().to_owned();
    let linked_workspaces: Vec<LinkedWorkspace> =
        serde_json::from_value(value["linked_workspaces"].clone())
            .unwrap_or_default();
    let owners: BTreeMap<String, String> =
        serde_json::from_value(value["owners"].clone()).unwrap_or_default();
    let aliases: BTreeMap<String, String> =
        serde_json::from_value(value["aliases"].clone()).unwrap_or_default();
    let milestones: Vec<String> =
        serde_json::from_value(value["milestones"].clone()).unwrap_or_default();
    let plans: Vec<String> = serde_json::from_value(value["plans"].clone()).unwrap_or_default();
    Ok(Adapter {
        adapter_path,
        response: ResponsePolicy::default(),
        limits,
        roots,
        ignored_names,
        adapter_version,
        digest,
        linked_workspaces,
        owners,
        aliases,
        milestones,
        plans,
        debug: false,
    })
}

/// Current engine schema version stored in database metadata.
pub const ENGINE_SCHEMA_VERSION: &str = "15";

/// Current index semantics version stored in database metadata.
pub const INDEX_SEMANTICS_VERSION: &str = concat!(
    "9:compact-storage-v8:",
    env!("FORGE_LANGUAGE_PROFILE_DIGEST")
);

/// Separates a leading workspace bracket from a relative path, e.g. "[workspace]/foo/bar" -> ("[workspace]", "foo/bar").
pub fn workspace_path(path: &str) -> (&str, &str) {
    match path.split_once('/') {
        Some((head, tail)) if head.starts_with('[') && head.ends_with(']') => (head, tail),
        _ => ("", path),
    }
}

/// Normalizes and resolves a relative template target against its enclosing owner template.
pub fn local_template_target(owner: &str, target: &str) -> Option<String> {
    let target = target.strip_prefix("./").unwrap_or(target);
    if target.is_empty()
        || target.contains([':', '\\'])
        || target
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    let template_root = if owner.starts_with("templates/") {
        "templates/"
    } else if let Some(index) = owner.rfind("/templates/") {
        &owner[..index + "/templates/".len()]
    } else {
        owner.rsplit_once('/').map_or("", |(parent, _)| parent)
    };
    if template_root.is_empty() {
        Some(target.to_owned())
    } else if template_root.ends_with('/') {
        Some(format!("{template_root}{target}"))
    } else {
        Some(format!("{template_root}/{target}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_roots_backward_compatibility_deserializes_to_docs() {
        let yaml = "roots: [src]\ndoc_roots: [docs, README.md]\n";
        let parsed: AdapterFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(parsed.docs, Some(vec!["docs".into(), "README.md".into()]));
    }
}
