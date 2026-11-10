use anyhow::{bail, ensure, Context, Result};
use forge_tasks::tasks::profile::{self, GateProfile};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Immutable path choices for one in-process CLI invocation.
pub(super) struct ResolvedPaths {
    pub config: Option<PathBuf>,
    pub profile: Arc<GateProfile>,
    pub explicit_profile: bool,
    pub plans: Vec<String>,
}

impl ResolvedPaths {
    pub fn resolve(root: &Path, config: Option<&Path>, profile: Option<&Path>) -> Result<Self> {
        let explicit_config = config.is_some();
        let config = match config {
            Some(path) => Some(confined_file(root, path)?),
            None => root
                .join("forge-mcp.yaml")
                .is_file()
                .then(|| root.join("forge-mcp.yaml")),
        };
        let value = config
            .as_ref()
            .map(|path| -> Result<serde_yaml::Value> {
                Ok(serde_yaml::from_str(&fs::read_to_string(path)?)?)
            })
            .transpose()?;
        let configured = value.as_ref().and_then(|value| value.get("acdd_profile"));
        let selected = match profile {
            Some(path) => Some(if path.is_absolute() {
                path.strip_prefix(root)
                    .context("CLI_PATH_INVALID: profile escapes root")?
                    .to_string_lossy()
                    .into_owned()
            } else {
                path.to_string_lossy().into_owned()
            }),
            None => configured
                .map(|reference| {
                    reference
                        .as_str()
                        .map(str::to_owned)
                        .context("acdd_profile must be a path string")
                })
                .transpose()?,
        };
        let loaded = match selected {
            Some(reference) => {
                let (relative, pin) = profile::parse_profile_reference(&reference, false)?;
                let path = confined_file(root, &relative)?;
                if pin.is_some() {
                    profile::load_pinned_over(root, &reference, &profile::compiled())?
                } else {
                    Arc::new(profile::validate_profile_document(&fs::read_to_string(
                        path,
                    )?)?)
                }
            }
            None if explicit_config => {
                let repository = root.join(".forge/acdd/profile.yaml");
                if repository.exists() {
                    Arc::new(profile::validate_profile_document(&fs::read_to_string(
                        confined_file(root, &repository)?,
                    )?)?)
                } else {
                    profile::compiled()
                }
            }
            None => profile::load_for_workspace(root)?,
        };
        let plans = value
            .as_ref()
            .and_then(|value| value.get("plans"))
            .map(|value| serde_yaml::from_value::<Vec<String>>(value.clone()))
            .transpose()?
            .unwrap_or_else(|| vec!["docs/plans".into()]);
        Ok(Self {
            config,
            profile: loaded,
            explicit_profile: profile.is_some(),
            plans,
        })
    }
}

pub(super) fn confined_file(root: &Path, path: &Path) -> Result<PathBuf> {
    let selected = if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    };
    let selected = selected
        .canonicalize()
        .with_context(|| format!("CLI_PATH_MISSING: {}", selected.display()))?;
    ensure!(
        selected.starts_with(root.canonicalize()?) && selected.is_file(),
        "CLI_PATH_INVALID: expected a file confined to the selected root"
    );
    Ok(selected)
}

/// Verifies that a relative path does not escape `root` and that none of its ancestor
/// directories or destination components traverse a symlink (ADR 0005).
pub(super) fn verify_no_symlink_traversal(
    root: &Path,
    relative: &Path,
    error_prefix: &str,
) -> Result<PathBuf> {
    ensure!(
        !relative.is_absolute(),
        "{error_prefix}: path must be relative: {}",
        relative.display()
    );
    let canonical_root = root.canonicalize().with_context(|| {
        format!(
            "{error_prefix}: cannot canonicalize workspace root: {}",
            root.display()
        )
    })?;
    let mut cursor = root.to_path_buf();
    for comp in relative.components() {
        match comp {
            std::path::Component::CurDir => continue,
            std::path::Component::Normal(name) => cursor.push(name),
            _ => bail!(
                "{error_prefix}: path contains invalid component: {}",
                relative.display()
            ),
        }
        match fs::symlink_metadata(&cursor) {
            Ok(meta) => {
                ensure!(
                    !meta.file_type().is_symlink(),
                    "{error_prefix}: symlink in path: {}",
                    cursor.display()
                );
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                break;
            }
            Err(err) => return Err(err.into()),
        }
    }
    let target = root.join(relative);
    if target.exists() {
        let canonical_target = target.canonicalize()?;
        ensure!(
            canonical_target.starts_with(&canonical_root),
            "{error_prefix}: path escapes workspace root: {}",
            target.display()
        );
    }
    Ok(target)
}
