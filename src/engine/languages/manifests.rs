use super::{profiles, ImportPath, LanguageFamily, LanguageProfile};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, collections::HashSet, path::Path};

#[path = "manifests/javascript_packages.rs"]
mod javascript_packages;
#[path = "manifests/typescript_paths.rs"]
mod typescript_paths;

#[derive(Default)]
/// Represents dependency registry data.
pub struct DependencyRegistry {
    packages: HashMap<LanguageFamily, HashSet<String>>,
    scoped_packages: HashMap<LanguageFamily, HashMap<String, HashSet<String>>>,
    scoped_origins: HashMap<LanguageFamily, HashMap<String, HashMap<String, String>>>,
    javascript_packages: javascript_packages::Registry,
    typescript_paths: typescript_paths::Registry,
    digest: String,
}

impl DependencyRegistry {
    /// Resolves local package exports and TypeScript path aliases from the manifest snapshot.
    pub(crate) fn normalize_javascript_import(
        &self,
        profile_id: &str,
        owner: &str,
        module: &str,
        indexed_modules: &HashSet<&str>,
    ) -> Option<ImportPath> {
        if !matches!(profile_id, "javascript" | "typescript")
            || module.starts_with('.')
            || module.starts_with('/')
        {
            return None;
        }
        if profile_id == "typescript" {
            if let Some(path) = self
                .typescript_paths
                .resolve_import(owner, module, indexed_modules)
            {
                return Some(path);
            }
        }
        self.javascript_packages.normalize_import(owner, module)
    }

    /// Performs collect.
    pub fn collect(root: Option<&Path>) -> Self {
        let Some(root) = root else {
            return Self::default();
        };
        let adapter = crate::engine::scanner::load_adapter(root, None).ok();
        Self::collect_with_adapter(root, adapter.as_ref())
    }

    /// Performs collect with adapter.
    pub fn collect_with_adapter(
        root: &Path,
        adapter: Option<&crate::engine::scanner::Adapter>,
    ) -> Self {
        let mut registry = Self::default();
        let mut roots = vec![(
            root.to_path_buf(),
            String::new(),
            adapter
                .map(|adapter| adapter.ignored_names.clone())
                .unwrap_or_default(),
        )];
        if let Some(adapter) = adapter {
            roots.extend(adapter.linked_workspaces.iter().map(|workspace| {
                (
                    workspace.path.clone(),
                    format!("[{}]/", workspace.name),
                    workspace.ignored_names.clone(),
                )
            }));
        }
        let profiles: Vec<_> = profiles()
            .filter(|profile| !profile.manifest_filenames().is_empty())
            .collect();
        let mut visited = HashSet::new();
        let mut snapshots = std::collections::BTreeMap::new();
        let mut parsed_families = Vec::with_capacity(profiles.len());
        for (root, prefix, ignored_names) in roots {
            let ignored_names: HashSet<std::ffi::OsString> =
                ignored_names.into_iter().map(Into::into).collect();
            let mut walk = ignore::WalkBuilder::new(&root);
            walk.hidden(false)
                .follow_links(false)
                .git_ignore(true)
                .filter_entry(move |entry| {
                    !entry.path_is_symlink()
                        && !ignored_names.contains(entry.file_name())
                        && !matches!(
                            entry.file_name().to_str(),
                            Some(
                                ".git"
                                    | "node_modules"
                                    | "target"
                                    | ".venv"
                                    | "venv"
                                    | "__pycache__"
                            )
                        )
                });
            for entry in walk.build().filter_map(Result::ok) {
                if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                    continue;
                }
                let Some(filename) = entry.file_name().to_str() else {
                    continue;
                };
                let matches = |profile: &&dyn LanguageProfile| {
                    profile
                        .manifest_filenames()
                        .iter()
                        .any(|pattern| filename_matches(pattern, filename))
                };
                if !profiles.iter().any(matches) || !visited.insert(entry.path().to_path_buf()) {
                    continue;
                }
                // Manifest parsing is bounded independently of source-file limits.
                if entry
                    .metadata()
                    .map_or(true, |metadata| metadata.len() > 4 * 1024 * 1024)
                {
                    continue;
                }
                let Ok(content) = std::fs::read_to_string(entry.path()) else {
                    continue;
                };
                let Some(directory) = entry
                    .path()
                    .parent()
                    .and_then(|parent| parent.strip_prefix(&root).ok())
                else {
                    continue;
                };
                let mut scope = prefix.clone();
                scope.push_str(&directory.to_string_lossy().replace('\\', "/"));
                let scope = scope.trim_end_matches('/').to_owned();
                if filename == "package.json" {
                    registry
                        .javascript_packages
                        .record_manifest(&scope, &content);
                } else if filename == "pnpm-workspace.yaml" {
                    registry
                        .javascript_packages
                        .record_pnpm_workspace(&scope, &content);
                } else if filename.starts_with("tsconfig") && filename.ends_with(".json") {
                    registry
                        .typescript_paths
                        .record_config(&scope, filename, &content);
                }
                snapshots.insert(
                    entry.path().to_path_buf(),
                    Sha256::digest(content.as_bytes()),
                );
                if filename == "pnpm-workspace.yaml"
                    || (filename.starts_with("tsconfig") && filename.ends_with(".json"))
                {
                    continue;
                }
                parsed_families.clear();
                for profile in profiles.iter().copied().filter(|profile| matches(profile)) {
                    let family = profile.family();
                    if parsed_families.contains(&family) {
                        continue;
                    }
                    parsed_families.push(family);
                    let dependencies = profile.extract_manifest_dependencies(filename, &content);
                    {
                        let origin = entry
                            .path()
                            .strip_prefix(&root)
                            .map(|relative| {
                                format!(
                                    "{}{}",
                                    prefix,
                                    relative.to_string_lossy().replace('\\', "/")
                                )
                            })
                            .unwrap_or_else(|_| filename.to_owned());
                        let origins = registry
                            .scoped_origins
                            .entry(family)
                            .or_default()
                            .entry(scope.clone())
                            .or_default();
                        for dependency in &dependencies {
                            let recorded = origins.entry(dependency.clone()).or_default();
                            if recorded.is_empty() || preferred_manifest(&origin, recorded) {
                                *recorded = origin.clone();
                            }
                        }
                        registry
                            .scoped_packages
                            .entry(family)
                            .or_default()
                            .entry(scope.clone())
                            .or_default()
                            .extend(dependencies.iter().cloned());
                    }
                    registry
                        .packages
                        .entry(family)
                        .or_default()
                        .extend(dependencies);
                }
            }
        }
        registry.javascript_packages.apply_pnpm_workspaces();
        registry.typescript_paths.resolve_all();
        let mut hasher = Sha256::new();
        for (path, digest) in snapshots {
            let path = path.to_string_lossy();
            hasher.update((path.len() as u64).to_be_bytes());
            hasher.update(path.as_bytes());
            hasher.update(digest);
        }
        registry.digest = hex::encode(hasher.finalize());
        registry
    }

    /// Performs digest.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub(crate) fn declares_for_path(
        &self,
        family: LanguageFamily,
        path: &str,
        package: &str,
    ) -> bool {
        let Some(scope) = self.nearest_manifest_scope_for_path(family, path) else {
            return false;
        };
        self.scoped_packages
            .get(&family)
            .and_then(|scopes| scopes.get(scope))
            .is_some_and(|packages| packages.contains(package))
    }

    pub(crate) fn nuxt_package_scope_for_path(&self, path: &str) -> Option<&str> {
        self.javascript_packages.nuxt_package_scope_for_path(path)
    }

    pub(crate) fn nearest_manifest_scope_for_path(
        &self,
        family: LanguageFamily,
        path: &str,
    ) -> Option<&str> {
        let scopes = self.scoped_packages.get(&family)?;
        let workspace = super::workspace_path(path).0;
        let mut directory = path.rsplit_once('/').map_or("", |(directory, _)| directory);
        loop {
            if let Some((scope, _)) = scopes.get_key_value(directory) {
                return Some(scope.as_str());
            }
            if directory.is_empty() || (!workspace.is_empty() && directory == workspace) {
                return None;
            }
            directory = directory.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }

    /// Performs classification.
    pub fn classification(
        &self,
        profile: &dyn LanguageProfile,
        module: &str,
    ) -> Option<&'static str> {
        if module.is_empty() {
            return None;
        }
        if profile.is_stdlib(module) {
            return Some("standard library");
        }
        let declared = self
            .packages
            .get(&profile.family())
            .is_some_and(|packages| {
                if packages.contains(module) {
                    return true;
                }
                // Borrowed prefixes preserve scoped npm and Go import-path identities.
                module.char_indices().any(|(index, character)| {
                    matches!(character, '.' | '/' | ':') && packages.contains(&module[..index])
                })
            });
        Some(if declared {
            "external dependency (manifest)"
        } else {
            "external dependency"
        })
    }

    /// Classifies an import using the nearest declaring manifest in its workspace.
    pub fn classification_for_path(
        &self,
        profile: &dyn LanguageProfile,
        module: &str,
        path: &str,
    ) -> Option<String> {
        let classification = self.classification(profile, module)?;
        if classification != "external dependency (manifest)" {
            return Some(classification.to_owned());
        }
        let workspace = super::workspace_path(path).0;
        let mut directory = path.rsplit_once('/').map_or("", |(directory, _)| directory);
        loop {
            if let Some(origins) = self
                .scoped_origins
                .get(&profile.family())
                .and_then(|scopes| scopes.get(directory))
            {
                let origin = origins.get(module).or_else(|| {
                    module.char_indices().rev().find_map(|(index, character)| {
                        matches!(character, '.' | '/' | ':')
                            .then(|| origins.get(&module[..index]))
                            .flatten()
                    })
                });
                if let Some(origin) = origin {
                    return Some(format!("external dependency (manifest: {origin})"));
                }
            }
            if directory.is_empty() || (!workspace.is_empty() && directory == workspace) {
                return Some("external dependency".to_owned());
            }
            directory = directory.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }
}

fn preferred_manifest(candidate: &str, recorded: &str) -> bool {
    let rank = |path: &str| match path.rsplit('/').next().unwrap_or(path) {
        "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" => 0,
        _ => 1,
    };
    (rank(candidate), candidate) < (rank(recorded), recorded)
}

fn match_mapping<'a>(pattern: &str, value: &'a str) -> Option<&'a str> {
    match pattern.split_once('*') {
        Some((prefix, suffix)) if !suffix.contains('*') => {
            value.strip_prefix(prefix)?.strip_suffix(suffix)
        }
        None if pattern == value => Some(""),
        _ => None,
    }
}

fn joined_scope(scope: &str, target: &str, floor: usize) -> Option<String> {
    if target.starts_with('/') || target.contains('\\') || target.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = scope.split('/').filter(|part| !part.is_empty()).collect();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.len() <= floor {
                    return None;
                }
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

fn javascript_namespace(path: &str) -> String {
    let (_, local_path) = super::workspace_path(path);
    super::module_stem(local_path)
        .trim_end_matches("/index")
        .replace('/', ".")
}

fn filename_matches(pattern: &str, filename: &str) -> bool {
    match pattern.split_once('*') {
        Some((prefix, suffix)) => filename.starts_with(prefix) && filename.ends_with(suffix),
        None => pattern == filename,
    }
}

/// Reports whether manifest filename applies.
pub fn is_manifest_filename(filename: &str) -> bool {
    profiles().any(|profile| {
        profile
            .manifest_filenames()
            .iter()
            .any(|pattern| filename_matches(pattern, filename))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requirement_patterns_are_filename_scoped() {
        assert!(filename_matches(
            "requirements*.txt",
            "requirements-dev.txt"
        ));
        assert!(!filename_matches(
            "requirements*.txt",
            "requirements-dev.yaml"
        ));
        assert!(filename_matches("Cargo.toml", "Cargo.toml"));
    }
}
