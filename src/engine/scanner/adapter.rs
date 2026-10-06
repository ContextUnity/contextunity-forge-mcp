use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Default)]
struct AdapterFile {
    #[serde(default)]
    response: crate::core::response::ResponsePolicy,
    #[serde(default)]
    limits: Option<ScannerLimits>,
    adapter_version: Option<serde_json::Value>,
    roots: Option<Vec<String>>,
    ignore: Option<Vec<String>>,
    eligible_roots: Option<Vec<String>>,
    #[serde(default, alias = "doc_roots")]
    docs: Option<Vec<String>>,
    milestones: Option<Vec<String>>,
    plans: Option<Vec<String>>,
    excluded_directory_names: Option<Vec<String>>,
    excluded_file_names: Option<Vec<String>>,
    linked_workspaces: Option<Vec<LinkedWorkspaceConfig>>,
    workspaces: Option<Vec<LinkedWorkspaceConfig>>,
    owners: Option<BTreeMap<String, String>>,
    aliases: Option<BTreeMap<String, String>>,
    #[serde(default, rename = "DEBUG", alias = "debug")]
    debug: bool,
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
/// Represents adapter data.
pub struct Adapter {
    /// Optional adapter path value.
    pub adapter_path: Option<PathBuf>,
    /// The response value.
    pub response: crate::core::response::ResponsePolicy,
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
    /// Milestone directories to scan for milestones and exclude from doc/code index.
    pub milestones: Vec<String>,
    /// Plan directories to exclude from doc/code index.
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

#[cfg(test)]
mod tests {
    use super::AdapterFile;

    #[test]
    fn doc_roots_backward_compatibility_deserializes_to_docs() {
        let yaml = "roots: [src]\ndoc_roots: [docs, README.md]\n";
        let parsed: AdapterFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(parsed.docs, Some(vec!["docs".into(), "README.md".into()]));
    }
}
