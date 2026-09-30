use super::{profiles, LanguageFamily, LanguageProfile};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, collections::HashSet, path::Path};

#[derive(Default)]
pub struct DependencyRegistry {
    packages: HashMap<LanguageFamily, HashSet<String>>,
    digest: String,
}

impl DependencyRegistry {
    pub fn collect(root: Option<&Path>) -> Self {
        let Some(root) = root else {
            return Self::default();
        };
        let adapter = crate::engine::scanner::load_adapter(root, None).ok();
        Self::collect_with_adapter(root, adapter.as_ref())
    }

    pub fn collect_with_adapter(
        root: &Path,
        adapter: Option<&crate::engine::scanner::Adapter>,
    ) -> Self {
        let mut registry = Self::default();
        let mut roots = vec![(
            root.to_path_buf(),
            adapter
                .map(|adapter| adapter.ignored_names.clone())
                .unwrap_or_default(),
        )];
        if let Some(adapter) = adapter {
            roots.extend(
                adapter
                    .linked_workspaces
                    .iter()
                    .map(|workspace| (workspace.path.clone(), workspace.ignored_names.clone())),
            );
        }
        let profiles: Vec<_> = profiles()
            .filter(|profile| !profile.manifest_filenames().is_empty())
            .collect();
        let mut visited = HashSet::new();
        let mut snapshots = std::collections::BTreeMap::new();
        let mut parsed_families = Vec::with_capacity(profiles.len());
        for (root, ignored_names) in roots {
            let ignored_names: HashSet<std::ffi::OsString> =
                ignored_names.into_iter().map(Into::into).collect();
            let mut walk = ignore::WalkBuilder::new(root);
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
                snapshots.insert(
                    entry.path().to_path_buf(),
                    Sha256::digest(content.as_bytes()),
                );
                parsed_families.clear();
                for profile in profiles.iter().copied().filter(|profile| matches(profile)) {
                    let family = profile.family();
                    if parsed_families.contains(&family) {
                        continue;
                    }
                    parsed_families.push(family);
                    registry
                        .packages
                        .entry(family)
                        .or_default()
                        .extend(profile.extract_manifest_dependencies(filename, &content));
                }
            }
        }
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

    pub fn digest(&self) -> &str {
        &self.digest
    }

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
}

fn filename_matches(pattern: &str, filename: &str) -> bool {
    match pattern.split_once('*') {
        Some((prefix, suffix)) => filename.starts_with(prefix) && filename.ends_with(suffix),
        None => pattern == filename,
    }
}

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
