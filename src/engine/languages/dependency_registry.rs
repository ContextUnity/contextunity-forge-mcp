use super::manifests::FrameworkManifest;
use super::registries::{
    javascript_packages::JavascriptPackageRegistry, typescript_paths::TypeScriptPathsRegistry,
};
use super::{profiles, ImportPath, LanguageFamily, LanguageProfile};
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Default)]
/// Represents dependency registry data.
pub struct DependencyRegistry {
    packages: HashMap<LanguageFamily, HashSet<String>>,
    scoped_packages: HashMap<LanguageFamily, HashMap<String, HashSet<String>>>,
    scoped_origins: HashMap<LanguageFamily, HashMap<String, HashMap<String, String>>>,
    framework_manifests: HashMap<(String, String), FrameworkManifest>,
    standard_framework_manifests: HashMap<String, FrameworkManifest>,
    javascript_packages: JavascriptPackageRegistry,
    typescript_paths: TypeScriptPathsRegistry,
    digest: String,
}

impl DependencyRegistry {
    /// Resolves local package exports and TypeScript path aliases from the dependency snapshot.
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

    /// Collects workspace dependencies and framework rules.
    pub fn collect(root: Option<&Path>) -> Self {
        let Some(root) = root else {
            return Self::default();
        };
        let adapter = crate::engine::scanner::load_adapter(root, None).ok();
        Self::collect_with_scan_config(root, adapter.as_ref())
    }

    /// Collects workspace dependencies and framework rules with scan configuration.
    pub fn collect_with_scan_config(
        root: &Path,
        adapter: Option<&crate::engine::scanner::Adapter>,
    ) -> Self {
        Self::try_collect_with_scan_config(root, adapter).unwrap_or_default()
    }

    /// Collects dependency metadata and framework manifests, returning explicit parse errors.
    ///
    /// # Errors
    ///
    /// Returns an error when an explicit `.forge/frameworks/*.{toml,yaml,yml}`
    /// file cannot be read or does not satisfy the typed framework-manifest schema.
    pub(crate) fn try_collect_with_scan_config(
        root: &Path,
        adapter: Option<&crate::engine::scanner::Adapter>,
    ) -> Result<Self> {
        Self::try_collect(root, adapter, None)
    }

    /// Collects dependency metadata from scanner-discovered manifests and direct workspace roots.
    pub(crate) fn try_collect_with_paths(
        root: &Path,
        adapter: Option<&crate::engine::scanner::Adapter>,
        discovered_manifests: &[PathBuf],
    ) -> Result<Self> {
        Self::try_collect(root, adapter, Some(discovered_manifests))
    }

    fn try_collect(
        root: &Path,
        adapter: Option<&crate::engine::scanner::Adapter>,
        discovered_manifests: Option<&[PathBuf]>,
    ) -> Result<Self> {
        let mut registry = Self::default();
        load_standard_framework_manifests(&mut registry)?;
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
            let manifest_paths = if let Some(discovered) = discovered_manifests {
                scanner_manifest_paths(&root, &ignored_names, &profiles, discovered)?
            } else {
                walked_manifest_paths(&root, ignored_names, &profiles)
            };
            for path in manifest_paths {
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                        metadata
                    }
                    _ => continue,
                };
                let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let matches = |profile: &&dyn LanguageProfile| {
                    profile
                        .manifest_filenames()
                        .iter()
                        .any(|pattern| filename_matches(pattern, filename))
                };
                if !profiles.iter().any(matches) || !visited.insert(path.clone()) {
                    continue;
                }
                // Manifest parsing is bounded independently of source-file limits.
                if metadata.len() > 4 * 1024 * 1024 {
                    continue;
                }
                let Ok(content) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let Some(directory) = path
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
                snapshots.insert(path.clone(), Sha256::digest(content.as_bytes()));
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
                        let origin = path
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
            load_framework_manifests(
                &root,
                prefix.trim_end_matches('/'),
                &mut registry,
                &mut snapshots,
            )?;
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
        Ok(registry)
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

    /// Returns a workspace-local manifest unconditionally, otherwise a bundled
    /// manifest only when the workspace declares its package.
    ///
    /// Both lookups are scoped to the workspace containing `path`.
    pub fn framework_manifest_for_path(
        &self,
        family: LanguageFamily,
        path: &str,
        framework: &str,
        package: &str,
    ) -> Option<&FrameworkManifest> {
        let (workspace, _) = super::workspace_path(path);
        if let Some(manifest) = self
            .framework_manifests
            .get(&(workspace.to_owned(), framework.to_owned()))
        {
            return Some(manifest);
        }
        self.declares_for_path(family, path, package)
            .then(|| self.standard_framework_manifests.get(framework))
            .flatten()
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

fn scanner_manifest_paths(
    root: &Path,
    ignored_names: &BTreeSet<String>,
    profiles: &[&dyn LanguageProfile],
    discovered: &[PathBuf],
) -> Result<Vec<PathBuf>> {
    let is_manifest = |filename: &str| {
        profiles.iter().any(|profile| {
            profile
                .manifest_filenames()
                .iter()
                .any(|pattern| filename_matches(pattern, filename))
        })
    };
    let mut paths = BTreeSet::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let filename = entry.file_name();
            let Some(filename) = filename.to_str() else {
                continue;
            };
            if ignored_names.contains(filename) || !is_manifest(filename) {
                continue;
            }
            let candidate = entry.path();
            let is_regular_file = fs::symlink_metadata(&candidate)
                .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink());
            if !is_regular_file {
                continue;
            }
            let relative = candidate
                .strip_prefix(root)
                .context("workspace manifest escaped its root")?;
            paths.insert(crate::engine::scanner::checked_child(root, relative)?);
        }
    }
    for discovered_path in discovered {
        let Ok(relative) = discovered_path.strip_prefix(root) else {
            continue;
        };
        let Some(filename) = discovered_path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if ignored_names.contains(filename) || !is_manifest(filename) {
            continue;
        }
        let path = crate::engine::scanner::checked_child(root, relative)?;
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_file() && !metadata.file_type().is_symlink() {
            paths.insert(path);
        }
    }
    Ok(paths.into_iter().collect())
}

fn walked_manifest_paths(
    root: &Path,
    ignored_names: BTreeSet<String>,
    profiles: &[&dyn LanguageProfile],
) -> Vec<PathBuf> {
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
                    Some(".git" | "node_modules" | "target" | ".venv" | "venv" | "__pycache__")
                )
        });
    walk.build()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .filter(|entry| {
            let Some(filename) = entry.file_name().to_str() else {
                return false;
            };
            profiles.iter().any(|profile| {
                profile
                    .manifest_filenames()
                    .iter()
                    .any(|pattern| filename_matches(pattern, filename))
            })
        })
        .map(|entry| entry.into_path())
        .collect()
}

fn load_standard_framework_manifests(registry: &mut DependencyRegistry) -> Result<()> {
    for (name, source) in [
        ("alpinejs", include_str!("manifests/alpinejs.toml")),
        ("django", include_str!("manifests/django.toml")),
        ("gridviewspec", include_str!("manifests/gridviewspec.toml")),
        ("jinja2", include_str!("manifests/jinja2.toml")),
        ("nuxt", include_str!("manifests/nuxt.toml")),
    ] {
        let manifest = FrameworkManifest::parse(name.to_owned(), "toml", source)
            .with_context(|| format!("invalid standard framework manifest {name}"))?;
        registry
            .standard_framework_manifests
            .insert(name.to_owned(), manifest);
    }
    Ok(())
}

fn load_framework_manifests(
    root: &Path,
    workspace: &str,
    registry: &mut DependencyRegistry,
    snapshots: &mut std::collections::BTreeMap<PathBuf, sha2::digest::Output<Sha256>>,
) -> Result<()> {
    let forge_directory = root.join(".forge");
    let forge_metadata = match fs::symlink_metadata(&forge_directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to inspect configuration directory {}",
                    forge_directory.display()
                )
            })
        }
    };
    if forge_metadata.file_type().is_symlink() {
        bail!(
            "configuration directory must not be a symlink: {}",
            forge_directory.display()
        );
    }
    if !forge_metadata.is_dir() {
        bail!(
            "configuration path is not a directory: {}",
            forge_directory.display()
        );
    }
    let directory = root.join(".forge/frameworks");
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to inspect framework manifest directory {}",
                    directory.display()
                )
            })
        }
    };
    if metadata.file_type().is_symlink() {
        bail!(
            "framework manifest directory must not be a symlink: {}",
            directory.display()
        );
    }
    if !metadata.is_dir() {
        bail!(
            "framework manifest path is not a directory: {}",
            directory.display()
        );
    }

    for entry in fs::read_dir(&directory).with_context(|| {
        format!(
            "failed to read framework manifest directory {}",
            directory.display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!(
                "failed to inspect entries in framework manifest directory {}",
                directory.display()
            )
        })?;
        let path = entry.path();
        let Some(extension @ ("toml" | "yaml" | "yml")) =
            path.extension().and_then(|extension| extension.to_str())
        else {
            continue;
        };
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect framework manifest {}", path.display()))?;
        if file_type.is_symlink() {
            bail!(
                "framework manifest must not be a symlink: {}",
                path.display()
            );
        }
        if !file_type.is_file() {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .context("framework manifest filename must have a UTF-8 identifier")?
            .to_owned();
        let metadata = entry
            .metadata()
            .with_context(|| format!("failed to inspect framework manifest {}", path.display()))?;
        if metadata.len() > 4 * 1024 * 1024 {
            bail!(
                "framework manifest exceeds the 4 MiB limit: {}",
                path.display()
            );
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read framework manifest {}", path.display()))?;
        let manifest = FrameworkManifest::parse(name.clone(), extension, &content)
            .with_context(|| format!("failed to load framework manifest {}", path.display()))?;
        let manifest = registry
            .standard_framework_manifests
            .get(&name)
            .map_or(manifest.clone(), |bundled| {
                manifest.with_bundled_rules(bundled)
            });
        if registry
            .framework_manifests
            .insert((workspace.to_owned(), name.clone()), manifest)
            .is_some()
        {
            bail!("duplicate framework manifest {name} in workspace {workspace}");
        }
        snapshots.insert(path, Sha256::digest(content.as_bytes()));
    }
    Ok(())
}

fn preferred_manifest(candidate: &str, recorded: &str) -> bool {
    let rank = |path: &str| match path.rsplit('/').next().unwrap_or(path) {
        "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" => 0,
        _ => 1,
    };
    (rank(candidate), candidate) < (rank(recorded), recorded)
}

fn filename_matches(pattern: &str, filename: &str) -> bool {
    match pattern.split_once('*') {
        Some((prefix, suffix)) => filename.starts_with(prefix) && filename.ends_with(suffix),
        None => pattern == filename,
    }
}

/// Reports whether a filename matches a language profile's external dependency manifest.
pub fn is_dependency_manifest_filename(filename: &str) -> bool {
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

    #[cfg(feature = "lang-python")]
    #[test]
    fn project_framework_packs_merge_replace_and_activate() {
        use super::super::manifests::FrameworkManifestValue;
        use std::collections::BTreeMap;

        struct TestRoot(PathBuf);
        impl Drop for TestRoot {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temporary = TestRoot(std::env::temp_dir().join(format!(
            "forge-framework-packs-{}-{nonce}",
            std::process::id()
        )));
        fs::create_dir(&temporary.0).unwrap();
        let family = LanguageFamily("python");
        let mut failures = Vec::new();
        for extension in ["toml", "yaml", "yml"] {
            for (mode, replace) in [("default", false), ("extend", false), ("replace", true)] {
                for declared in [false, true] {
                    let workspace = temporary.0.join(format!("{extension}-{mode}-{declared}"));
                    let root = workspace.as_path();
                    let directory = root.join(".forge/frameworks");
                    fs::create_dir_all(&directory).unwrap();
                    if declared {
                        fs::write(root.join("requirements.txt"), "django>=5\n").unwrap();
                    }
                    let directive = match (extension, mode) {
                        (_, "default") => "",
                        ("toml", "extend") => "extend = true\n",
                        ("toml", "replace") => "replace = true\n",
                        (_, "extend") => "extend: true\n",
                        (_, "replace") => "replace: true\n",
                        _ => unreachable!(),
                    };
                    let rules = if extension == "toml" {
                        r#"receivers = ["template_context=project.render", { metadata = { arity = 2 }, enabled = true }]
builtins = ["template.tag.project", "template.tag.if"]
filters = ["template.filter.project", "template.filter.upper"]
routes = ["template.tag.trans=project.i18n", { methods = ["GET", "POST"], priority = 7 }]
"#
                    } else {
                        r#"receivers:
  - template_context=project.render
  - metadata: { arity: 2 }
    enabled: true
builtins: [template.tag.project, template.tag.if]
filters: [template.filter.project, template.filter.upper]
routes:
  - template.tag.trans=project.i18n
  - methods: [GET, POST]
    priority: 7
"#
                    };
                    let project_path = directory.join(format!("django.{extension}"));
                    fs::write(&project_path, format!("{directive}{rules}")).unwrap();
                    let label = format!("{extension}/{mode}/declared={declared}");
                    let registry = match DependencyRegistry::try_collect_with_paths(root, None, &[])
                    {
                        Ok(registry) => registry,
                        Err(error) => {
                            failures.push(format!(
                                "{label}: collection rejected valid pack: {error:#}"
                            ));
                            continue;
                        }
                    };
                    let Some(manifest) = registry.framework_manifest_for_path(
                        family,
                        "src/views.py",
                        "django",
                        "django",
                    ) else {
                        failures.push(format!("{label}: explicit project pack did not activate"));
                        continue;
                    };
                    let opaque_receiver = FrameworkManifestValue::Table(BTreeMap::from([
                        ("enabled".to_owned(), FrameworkManifestValue::Boolean(true)),
                        (
                            "metadata".to_owned(),
                            FrameworkManifestValue::Table(BTreeMap::from([(
                                "arity".to_owned(),
                                FrameworkManifestValue::Integer(2),
                            )])),
                        ),
                    ]));
                    let opaque_route = FrameworkManifestValue::Table(BTreeMap::from([
                        (
                            "methods".to_owned(),
                            FrameworkManifestValue::Array(vec![
                                FrameworkManifestValue::String("GET".to_owned()),
                                FrameworkManifestValue::String("POST".to_owned()),
                            ]),
                        ),
                        ("priority".to_owned(), FrameworkManifestValue::Integer(7)),
                    ]));
                    for (claim, satisfied) in [
                        (
                            "project receiver takes precedence",
                            manifest.receiver_target("template_context") == Some("project.render"),
                        ),
                        (
                            "project route takes precedence",
                            manifest.route_target("template.tag.trans") == Some("project.i18n"),
                        ),
                        ("project builtin remains", manifest.has_builtin("template.tag.project")),
                        ("project filter remains", manifest.has_filter("template.filter.project")),
                        (
                            "bundled builtin follows merge mode",
                            manifest.has_builtin("template.tag.block") != replace,
                        ),
                        (
                            "bundled filter follows merge mode",
                            manifest.has_filter("template.filter.lower") != replace,
                        ),
                        (
                            "bundled route follows merge mode",
                            manifest.route_target("template.tag.static") == (!replace).then_some("static"),
                        ),
                        (
                            "receiver table remains opaque and project first",
                            manifest.receivers.get(1) == Some(&opaque_receiver),
                        ),
                        (
                            "route table remains opaque and project first",
                            manifest.routes.get(1) == Some(&opaque_route),
                        ),
                        (
                            "builtin exact-string identity is unique",
                            manifest.builtins.iter().filter(|rule| matches!(rule, FrameworkManifestValue::String(value) if value == "template.tag.if")).count() == 1,
                        ),
                        (
                            "filter exact-string identity is unique",
                            manifest.filters.iter().filter(|rule| matches!(rule, FrameworkManifestValue::String(value) if value == "template.filter.upper")).count() == 1,
                        ),
                        (
                            "receiver source identity is unique",
                            manifest.receivers.iter().filter(|rule| matches!(rule, FrameworkManifestValue::String(value) if value.starts_with("template_context="))).count() == 1,
                        ),
                        (
                            "route source identity is unique",
                            manifest.routes.iter().filter(|rule| matches!(rule, FrameworkManifestValue::String(value) if value.starts_with("template.tag.trans="))).count() == 1,
                        ),
                        (
                            "project pack stays in its workspace",
                            registry.framework_manifest_for_path(family, "[other]/src/views.py", "django", "django").is_none(),
                        ),
                    ] {
                        if !satisfied {
                            failures.push(format!("{label}: {claim}"));
                        }
                    }
                    let again =
                        DependencyRegistry::try_collect_with_paths(root, None, &[]).unwrap();
                    if registry.digest() != again.digest()
                        || again.framework_manifest_for_path(
                            family,
                            "src/views.py",
                            "django",
                            "django",
                        ) != Some(manifest)
                    {
                        failures.push(format!("{label}: re-collection is not deterministic"));
                    }
                    for (invalid, content) in [
                        ("missing section", "receivers = []\nbuiltins = []\nfilters = []\n"),
                        ("unknown section", "receivers = []\nbuiltins = []\nfilters = []\nroutes = []\nunknown = []\n"),
                        ("wrong section shape", "receivers = {}\nbuiltins = []\nfilters = []\nroutes = []\n"),
                        ("conflicting modes", "extend = true\nreplace = true\nreceivers = []\nbuiltins = []\nfilters = []\nroutes = []\n"),
                        ("wrong mode shape", "extend = \"true\"\nreceivers = []\nbuiltins = []\nfilters = []\nroutes = []\n"),
                    ] {
                        fs::write(directory.join("invalid.toml"), content).unwrap();
                        if DependencyRegistry::try_collect_with_paths(root, None, &[]).is_ok() {
                            failures.push(format!("{label}: accepted {invalid}"));
                        }
                    }
                }
            }
        }

        let workspace = temporary.0.join("dependency-only");
        fs::create_dir(&workspace).unwrap();
        let root = workspace.as_path();
        let undeclared = DependencyRegistry::try_collect_with_paths(root, None, &[]).unwrap();
        if undeclared
            .framework_manifest_for_path(family, "src/app.py", "django", "django")
            .is_some()
        {
            failures
                .push("undeclared bundled framework activated without a project pack".to_owned());
        }
        if undeclared
            .framework_manifest_for_path(family, "src/app.py", "gridviewspec", "grid_view_spec")
            .is_some()
        {
            failures
                .push("undeclared GridViewSpec bundle activated without a project pack".to_owned());
        }
        fs::write(
            root.join("pyproject.toml"),
            "[project]\nname = 'consumer'\ndependencies = ['grid-view-spec>=0.1']\n",
        )
        .unwrap();
        let registry = DependencyRegistry::try_collect_with_paths(root, None, &[]).unwrap();
        match registry.framework_manifest_for_path(family, "src/app.py", "gridviewspec", "grid_view_spec") {
            Some(manifest) if manifest.receiver_target("GridViewSpec") == Some("grid_view_spec.types.spec.GridViewSpec") => {}
            _ => failures.push("declared grid-view-spec did not activate its verified GridViewSpec bundle receiver".to_owned()),
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

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
