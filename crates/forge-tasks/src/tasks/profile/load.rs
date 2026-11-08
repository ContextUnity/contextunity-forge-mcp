use super::*;
use std::path::{Component, PathBuf};

pub fn parse_profile_reference(
    reference: &str,
    require_pin: bool,
) -> Result<(PathBuf, Option<String>)> {
    let (path, pin) = match reference.rsplit_once(':') {
        Some((path, pin)) if !path.is_empty() && !pin.is_empty() => {
            (path, Some(pin.to_ascii_lowercase()))
        }
        _ => (reference, None),
    };
    if require_pin && pin.is_none() {
        bail!("ACDD_PROFILE_PIN_REQUIRED: use '<path>:<sha256_prefix>'");
    }
    if let Some(pin) = &pin {
        if pin.len() < 7 || !pin.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("ACDD_PROFILE_PIN_INVALID: SHA-256 prefix must contain at least 7 hexadecimal characters");
        }
    }
    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("ACDD_PROFILE_PATH_INVALID: profile path must be relative and confined to the workspace");
    }
    Ok((path.to_path_buf(), pin))
}

pub(super) fn load_profile_path(
    root: &Path,
    reference: &str,
    require_pin: bool,
) -> Result<ParsedProfile> {
    let (relative, pin) = parse_profile_reference(reference, require_pin)?;
    let path = crate::tasks::confined_path(root, &relative.to_string_lossy())?;
    let contents = std::fs::read(&path)
        .with_context(|| format!("ACDD_PROFILE_NOT_FOUND: {}", path.display()))?;
    if let Some(prefix) = pin {
        use sha2::{Digest, Sha256};
        let digest = format!("{:x}", Sha256::digest(&contents));
        if !digest.starts_with(&prefix) {
            bail!("TASK_PROFILE_TAMPERED: profile SHA-256 does not match pinned prefix '{prefix}'");
        }
    }
    let text = std::str::from_utf8(&contents)
        .context("ACDD_PROFILE_SCHEMA_INVALID: profile must be UTF-8 YAML")?;
    parse_profile_document(text).with_context(|| format!("invalid profile in {}", path.display()))
}

/// Load one pinned task or milestone override above the workspace base profile.
pub fn load_pinned(root: &Path, reference: &str) -> Result<Arc<GateProfile>> {
    let base = load_for_workspace(root)?;
    load_pinned_over(root, reference, &base)
}

/// Load one pinned task or milestone override above an already-selected workspace base profile.
pub fn load_pinned_over(
    root: &Path,
    reference: &str,
    base_profile: &GateProfile,
) -> Result<Arc<GateProfile>> {
    let mut base = base_profile.clone();
    merge_profile_document(&mut base, load_profile_path(root, reference, true)?);
    base.validate()?;
    if !base.gates.iter().any(|g| {
        gate_is_delivery(g)
            || g.gates.iter().any(gate_is_delivery)
            || g.tasks
                .as_ref()
                .is_some_and(|t| t.gates.iter().any(gate_is_delivery))
    }) {
        bail!("ACDD_PROFILE_INVALID: a merged workspace profile must contain a terminal delivery gate");
    }
    Ok(Arc::new(base))
}

/// Load a task profile override, then milestone override, then workspace profile.
pub fn load_for_task(
    root: &Path,
    task_profile: Option<&str>,
    milestone_profile: Option<&str>,
) -> Result<Arc<GateProfile>> {
    match task_profile.or(milestone_profile) {
        Some(reference) => load_pinned(root, reference),
        None => load_for_workspace(root),
    }
}
