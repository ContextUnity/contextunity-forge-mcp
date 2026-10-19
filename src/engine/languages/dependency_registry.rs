use super::manifests::{
    named_kind, FrameworkManifest, RuleCoverageCode, RuleCoverageDiagnostic, RuleDomain,
    RuleIRHandle, RuleIdentity,
};
use super::registries::{
    javascript_packages::JavascriptPackageRegistry, typescript_paths::TypeScriptPathsRegistry,
};
use super::{profiles, ImportPath, LanguageFamily, LanguageProfile};
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

type LanguageContextCacheKey = (String, String, String);

thread_local! {
    static LANGUAGE_CONTEXT_CACHE: RefCell<HashMap<LanguageContextCacheKey, Arc<super::LanguageContext>>> =
        RefCell::new(HashMap::new());
}

#[derive(Default)]
/// Represents dependency registry data.
pub struct DependencyRegistry {
    packages: HashMap<LanguageFamily, HashSet<String>>,
    scoped_packages: HashMap<LanguageFamily, HashMap<String, HashSet<String>>>,
    scoped_origins: HashMap<LanguageFamily, HashMap<String, HashMap<String, String>>>,
    framework_manifests: HashMap<(String, String), FrameworkManifest>,
    standard_framework_manifests: HashMap<String, FrameworkManifest>,
    rule_ir: RuleIRHandle,
    javascript_packages: JavascriptPackageRegistry,
    typescript_paths: TypeScriptPathsRegistry,
    digest: String,
}

impl DependencyRegistry {
    /// Loads the built-in framework rules without requiring dependency manifests.
    pub(crate) fn bundled_framework_rules() -> Self {
        let mut registry = Self::default();
        load_standard_framework_manifests(&mut registry)
            .expect("bundled framework manifests are validated at compile time");
        registry
    }

    /// Reuses merged extraction rules for files with the same workspace and package scope.
    pub(crate) fn language_context_for_path(&self, path: &str) -> Arc<super::LanguageContext> {
        let (workspace, _) = super::workspace_path(path);
        let family = LanguageFamily("python");
        let manifest_scope = self
            .nearest_manifest_scope_for_path(family, path)
            .unwrap_or(workspace);
        let key = (
            self.digest.clone(),
            workspace.to_owned(),
            manifest_scope.to_owned(),
        );
        if let Some(context) =
            LANGUAGE_CONTEXT_CACHE.with(|cache| cache.borrow().get(&key).cloned())
        {
            return context;
        }

        let mut names: BTreeSet<&str> = self
            .standard_framework_manifests
            .keys()
            .map(String::as_str)
            .collect();
        names.extend(
            self.framework_manifests
                .keys()
                .filter(|(scope, _)| scope == workspace)
                .map(|(_, name)| name.as_str()),
        );

        let mut context = super::LanguageContext {
            rule_ir: self.rule_ir.for_workspace(workspace),
            ..super::LanguageContext::default()
        };
        let declared_packages = self
            .scoped_packages
            .get(&family)
            .and_then(|scopes| scopes.get(manifest_scope));
        for name in names {
            let manifest = self
                .framework_manifests
                .get(&(workspace.to_owned(), name.to_owned()))
                .or_else(|| self.standard_framework_manifests.get(name));
            let Some(manifest) = manifest else { continue };
            context
                .class_hierarchy_receivers
                .extend(manifest.class_hierarchy_receivers.iter().cloned());
            context
                .route_decorators
                .extend(manifest.route_decorators.iter().cloned());
            context
                .route_calls
                .extend(manifest.route_calls.iter().cloned());
            context.di_calls.extend(manifest.di_calls.iter().cloned());
            for rule in &manifest.semantic_rules {
                context
                    .semantic_rules
                    .push(super::semantic::EffectiveSemanticRule {
                        pack: manifest.name.clone(),
                        rule: rule.clone(),
                        scope: manifest_scope.to_owned(),
                        active: rule.activation.is_empty()
                            || rule.activation.iter().any(|activation| {
                                let super::manifests::Activation::Package { name } = activation;
                                declared_packages.is_some_and(|packages| packages.contains(name))
                            }),
                    });
            }
        }
        let context = Arc::new(context);
        LANGUAGE_CONTEXT_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            if cache.len() >= 128 {
                cache.clear();
            }
            cache
                .entry(key)
                .or_insert_with(|| Arc::clone(&context))
                .clone()
        })
    }

    /// Reports removed eligible bundled identities for the complete current file inventory.
    ///
    /// Entries are `(workspace-qualified path, actual language)`. Diagnostics describe
    /// the current substitution, independently of which files were reparsed or cached.
    pub fn framework_rule_diagnostics<'a>(
        &self,
        inventory: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Vec<RuleCoverageDiagnostic> {
        let mut removed_by_group = std::collections::BTreeMap::<
            (String, String, String, RuleDomain),
            BTreeSet<RuleIdentity>,
        >::new();
        for (path, language) in inventory {
            let Some(profile) = super::by_id(language) else {
                continue;
            };
            let grammar = profile.grammar(path);
            let (workspace, _) = super::workspace_path(path);
            let call = ["call", "call_expression"]
                .into_iter()
                .any(|name| named_kind(&grammar, name).is_some());
            let routes = matches!(language, "python" | "typescript" | "javascript");
            for ((scope, framework), effective) in &self.framework_manifests {
                if scope != workspace {
                    continue;
                }
                let Some(bundled) = self.standard_framework_manifests.get(framework) else {
                    continue;
                };
                for (domain, eligible) in [
                    (
                        RuleDomain::ClassHierarchyReceivers,
                        language == "python" && named_kind(&grammar, "class_definition").is_some(),
                    ),
                    (RuleDomain::DiCalls, call),
                    (RuleDomain::RouteCalls, routes && call),
                    (
                        RuleDomain::RouteDecorators,
                        routes && call && named_kind(&grammar, "decorator").is_some(),
                    ),
                    (
                        RuleDomain::SemanticRules,
                        language == "python" && named_kind(&grammar, "call").is_some(),
                    ),
                ] {
                    if !eligible {
                        continue;
                    }
                    let baseline = bundled.rule_identities(domain);
                    let current = effective.rule_identities(domain);
                    let removed = baseline.difference(&current).filter(|identity| {
                        if let RuleIdentity::Semantic { id } = identity {
                            // Activation determines whether the bundled owner is eligible
                            // here. A payload change under an existing identity remains
                            // that identity even when its effective activation changes.
                            bundled.semantic_rules.iter().any(|rule| {
                                &rule.id == id
                                    && (rule.activation.is_empty()
                                        || rule.activation.iter().any(|activation| {
                                            let super::manifests::Activation::Package { name } =
                                                activation;
                                            self.declares_for_path(
                                                LanguageFamily("python"),
                                                path,
                                                name,
                                            )
                                        }))
                            })
                        } else {
                            true
                        }
                    });
                    for identity in removed {
                        removed_by_group
                            .entry((
                                framework.clone(),
                                workspace.to_owned(),
                                language.to_owned(),
                                domain,
                            ))
                            .or_default()
                            .insert(identity.clone());
                    }
                }
            }
        }
        removed_by_group
            .into_iter()
            .map(|((framework, workspace, language, rule_domain), removed)| {
                RuleCoverageDiagnostic {
                    framework,
                    workspace,
                    language,
                    rule_domain,
                    code: RuleCoverageCode::EligibleRuleIdentityRemoved,
                    removed_eligible_rule_identities: removed.into_iter().collect(),
                }
            })
            .collect()
    }

    pub(crate) fn class_hierarchy_receiver_for_path(
        &self,
        path: &str,
        base: &str,
    ) -> Option<&super::manifests::ClassHierarchyReceiverRule> {
        let (workspace, _) = super::workspace_path(path);
        let mut names: BTreeSet<&str> = self
            .standard_framework_manifests
            .keys()
            .map(String::as_str)
            .collect();
        names.extend(
            self.framework_manifests
                .keys()
                .filter(|(scope, _)| scope == workspace)
                .map(|(_, name)| name.as_str()),
        );
        names.into_iter().find_map(|name| {
            self.framework_manifests
                .get(&(workspace.to_owned(), name.to_owned()))
                .or_else(|| self.standard_framework_manifests.get(name))?
                .class_hierarchy_receivers
                .iter()
                .find(|rule| rule.bases.iter().any(|candidate| candidate == base))
        })
    }

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
            return Self::bundled_framework_rules();
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
        if let Some(adapter) = adapter {
            let manifests = crate::engine::scanner::dependency_manifests_for_build(root, adapter)?;
            Self::try_collect(root, Some(adapter), Some(&manifests))
        } else {
            Self::try_collect(root, None, None)
        }
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
        let mut snapshots =
            std::collections::BTreeMap::<String, sha2::digest::Output<Sha256>>::new();
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
                let Some(relative_path) = path.strip_prefix(&root).ok() else {
                    continue;
                };
                let origin = format!(
                    "{}{}",
                    prefix,
                    relative_path.to_string_lossy().replace('\\', "/")
                );
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
                snapshots.insert(origin.clone(), Sha256::digest(content.as_bytes()));
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
                &prefix,
                &mut registry,
                &mut snapshots,
            )?;
        }
        registry.javascript_packages.apply_pnpm_workspaces();
        registry.typescript_paths.resolve_all();
        let mut hasher = Sha256::new();
        hasher.update(b"forge-dependency-registry-v1\0");
        let standard_digest = Self::standard_manifests_digest_static();
        hasher.update((standard_digest.len() as u64).to_be_bytes());
        hasher.update(standard_digest.as_bytes());
        for (path, digest) in snapshots {
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

    /// Exposes the deterministic digest of bundled standard framework manifests.
    pub fn standard_manifests_digest(&self) -> &str {
        Self::standard_manifests_digest_static()
    }

    /// Exposes the static deterministic digest of bundled standard framework manifests.
    pub fn standard_manifests_digest_static() -> &'static str {
        STANDARD_FRAMEWORK_MANIFESTS_DIGEST.get_or_init(|| {
            let mut hasher = Sha256::new();
            hasher.update(b"forge-standard-framework-manifests-v1\0");
            for (name, source) in STANDARD_FRAMEWORK_MANIFEST_SOURCES {
                hasher.update((name.len() as u64).to_be_bytes());
                hasher.update(name.as_bytes());
                hasher.update((source.len() as u64).to_be_bytes());
                hasher.update(source.as_bytes());
            }
            hex::encode(hasher.finalize())
        })
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

const STANDARD_FRAMEWORK_MANIFEST_SOURCES: [(&str, &str); 9] = [
    ("alpinejs", include_str!("manifests/alpinejs.toml")),
    ("django", include_str!("manifests/django.toml")),
    ("fastapi", include_str!("manifests/fastapi.toml")),
    ("gridviewspec", include_str!("manifests/gridviewspec.toml")),
    ("jinja2", include_str!("manifests/jinja2.toml")),
    ("nestjs", include_str!("manifests/nestjs.toml")),
    ("nuxt", include_str!("manifests/nuxt.toml")),
    ("pydantic", include_str!("manifests/pydantic.toml")),
    ("unittest", include_str!("manifests/unittest.toml")),
];

static STANDARD_FRAMEWORK_MANIFESTS_DIGEST: std::sync::OnceLock<String> =
    std::sync::OnceLock::new();

fn load_standard_framework_manifests(registry: &mut DependencyRegistry) -> Result<()> {
    for (name, source) in STANDARD_FRAMEWORK_MANIFEST_SOURCES {
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
    origin_prefix: &str,
    registry: &mut DependencyRegistry,
    snapshots: &mut std::collections::BTreeMap<String, sha2::digest::Output<Sha256>>,
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
        let relative_path = path.strip_prefix(root).with_context(|| {
            format!(
                "framework manifest {} escaped its workspace root {}",
                path.display(),
                root.display()
            )
        })?;
        let origin = format!(
            "{}{}",
            origin_prefix,
            relative_path.to_string_lossy().replace('\\', "/")
        );
        snapshots.insert(origin, Sha256::digest(content.as_bytes()));
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
    fn dependency_registry_digest_is_stable_across_worktrees() {
        struct TemporaryDirectory(PathBuf);
        impl Drop for TemporaryDirectory {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temporary = TemporaryDirectory(std::env::temp_dir().join(format!(
            "forge-cross-worktree-registry-{}-{nonce}",
            std::process::id()
        )));
        let first_root = temporary.0.join("commerce");
        let second_root = temporary.0.join("commerce-worktree-b");
        for root in [&first_root, &second_root] {
            fs::create_dir_all(root.join(".forge/frameworks")).unwrap();
            fs::write(
                root.join("pyproject.toml"),
                "[project]\nname='commerce-fixture'\ndependencies=['django']\n",
            )
            .unwrap();
            fs::write(
                root.join(".forge/frameworks/django.toml"),
                "replace = true\nreceivers = []\nbuiltins = []\nfilters = []\nroutes = []\n",
            )
            .unwrap();
        }

        let first = DependencyRegistry::try_collect(&first_root, None, None).unwrap();
        let second = DependencyRegistry::try_collect(&second_root, None, None).unwrap();

        assert_eq!(
            first.digest(),
            second.digest(),
            "identical manifests must produce one registry digest across worktree roots"
        );

        fs::write(
            first_root.join("pyproject.toml"),
            "[project]\nname='commerce-fixture'\ndependencies=['django', 'fastapi']\n",
        )
        .unwrap();
        let changed = DependencyRegistry::try_collect(&first_root, None, None).unwrap();
        assert_ne!(
            changed.digest(),
            second.digest(),
            "manifest content changes must produce a new registry digest"
        );

        fs::create_dir_all(first_root.join("src")).unwrap();
        fs::create_dir_all(first_root.join("unindexed")).unwrap();
        fs::write(first_root.join("forge-mcp.yaml"), "roots: [src]\n").unwrap();
        fs::write(
            first_root.join("unindexed/requirements.txt"),
            "outside_source_roots>=1\n",
        )
        .unwrap();
        let adapter = crate::engine::scanner::load_adapter(&first_root, None).unwrap();
        let cold =
            DependencyRegistry::try_collect_with_paths(&first_root, Some(&adapter), &[]).unwrap();
        let admission =
            DependencyRegistry::try_collect_with_scan_config(&first_root, Some(&adapter)).unwrap();
        assert_eq!(
            cold.digest(),
            admission.digest(),
            "cold and admission discovery must exclude manifests outside configured source roots"
        );
        fs::write(
            first_root.join("unindexed/requirements.txt"),
            "outside_source_roots>=2\n",
        )
        .unwrap();
        let unchanged =
            DependencyRegistry::try_collect_with_scan_config(&first_root, Some(&adapter)).unwrap();
        assert_eq!(cold.digest(), unchanged.digest());
    }

    #[cfg(feature = "lang-python")]
    #[test]
    fn project_framework_packs_merge_replace_and_activate() {
        use super::super::manifests::{FrameworkManifest, FrameworkManifestValue};
        use std::collections::BTreeMap;

        let mut documented_examples = Vec::new();
        for (extension, source) in [
            (
                "toml",
                include_str!("../../../skills/contextunity-forge/references/frameworks/declarative-rules.toml.example"),
            ),
            (
                "yaml",
                include_str!("../../../skills/contextunity-forge/references/frameworks/declarative-rules.yaml.example"),
            ),
        ] {
            let parsed = FrameworkManifest::parse("django".to_owned(), extension, source)
                .unwrap_or_else(|error| panic!("invalid documented {extension} example: {error:#}"));
            assert_eq!(parsed.class_hierarchy_receivers.len(), 1, "{extension}");
            assert_eq!(parsed.route_decorators.len(), 1, "{extension}");
            assert_eq!(parsed.route_calls.len(), 1, "{extension}");
            assert_eq!(parsed.di_calls.len(), 1, "{extension}");
            assert_eq!(parsed.semantic_rules.len(), 1, "{extension}");
            documented_examples.push(parsed);
        }
        assert_eq!(
            documented_examples[0], documented_examples[1],
            "complete documented TOML/YAML payloads, including semantic rules, must agree"
        );
        let gridviewspec = FrameworkManifest::parse(
            "gridviewspec".to_owned(),
            "toml",
            include_str!("../../../skills/contextunity-forge/references/frameworks/gridviewspec.toml.example"),
        )
        .unwrap_or_else(|error| panic!("invalid documented GridViewSpec example: {error:#}"));
        assert_eq!(gridviewspec.class_hierarchy_receivers.len(), 28);

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
        let semantic_root = temporary.0.join("semantic-replacement");
        fs::create_dir_all(semantic_root.join(".forge/frameworks")).unwrap();
        fs::write(
            semantic_root.join("pyproject.toml"),
            "[project]\nname='call-file-fixture'\ndependencies=['django']\n",
        )
        .unwrap();
        let replacement = semantic_root.join(".forge/frameworks/django.toml");
        for (phase, enabled) in [
            ("bundled", true),
            ("empty replacement", false),
            ("restored", true),
        ] {
            if phase == "empty replacement" {
                fs::write(
                    &replacement,
                    "replace = true\nreceivers = []\nbuiltins = []\nfilters = []\nroutes = []\n",
                )
                .unwrap();
            } else if phase == "restored" {
                fs::remove_file(&replacement).unwrap();
            }
            let registry =
                DependencyRegistry::try_collect_with_paths(&semantic_root, None, &[]).unwrap();
            let manifest = registry
                .framework_manifest_for_path(
                    LanguageFamily("python"),
                    "views.py",
                    "django",
                    "django",
                )
                .expect("package/project pack remains available");
            let _ = manifest;
            let extracted = crate::engine::ast::extract_typed_with_registry(
                "views.py",
                "python",
                "from django.shortcuts import render as show_page\n\ndef page(request):\n    return show_page(request, 'catalog/card.html')\n\ndef shadowed(request, show_page):\n    return show_page(request, 'catalog/card.html')\n\ndef starred(request, args):\n    return show_page(*args, 'catalog/card.html')\n\ndef comprehension(request, callbacks):\n    return [show_page(request, 'catalog/card.html') for show_page in callbacks]\n",
                &registry,
            )
            .unwrap();
            assert!(extracted.facts.errors.is_empty(), "{phase}: parse errors");
            let owner = extracted
                .facts
                .nodes
                .iter()
                .find(|node| node.name == "page")
                .expect("ordinary caller declaration survives replacement");
            let consumers: Vec<_> = extracted
                .facts
                .nodes
                .iter()
                .flat_map(|node| {
                    node.details
                        .get("semantic_facts")
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                })
                .filter(|fact| fact["role"]["kind"] == "call_to_file")
                .collect();
            if enabled {
                assert_eq!(
                    consumers.len(),
                    1,
                    "{phase}: exact imported-call consumer fact"
                );
                assert_eq!(
                    consumers[0]["occurrence"]["owner_id"], owner.id,
                    "{phase}: actual caller declaration"
                );
                assert_eq!(
                    consumers[0]["role"],
                    serde_json::json!({"kind":"call_to_file","import_owner":"module:views.py","alias":"show_page","module":"django.shortcuts","export":"render","relation":"renders_template"})
                );
                assert_eq!(
                    consumers[0]["identity"]["path"], "catalog/card.html",
                    "{phase}: literal identity"
                );
                assert_eq!(
                    consumers[0]["identity"]["target"],
                    serde_json::json!({"kind":"project_path_suffix","prefix":"templates/","language":"html"})
                );
                assert_eq!(consumers[0]["occurrence"]["line"], 4, "{phase}: occurrence");
                for field in ["", "/identity", "/occurrence", "/role"] {
                    let mut malformed = consumers[0].clone();
                    malformed
                        .pointer_mut(field)
                        .unwrap()
                        .as_object_mut()
                        .unwrap()
                        .insert("unsupported".into(), true.into());
                    assert!(
                        serde_json::from_value::<super::super::semantic::SemanticFact>(malformed)
                            .is_err(),
                        "strict stored DTO field {field}"
                    );
                }
            } else {
                assert!(
                    consumers.is_empty(),
                    "empty replacement must remove consumer facts: {consumers:?}"
                );
            }
        }
        let mut documented = Vec::new();
        for block in include_str!("../../../docs/reference/framework-manifests.md").split("```") {
            let Some((extension @ ("toml" | "yaml"), source)) = block.split_once('\n') else {
                continue;
            };
            if source.contains("call_to_file") {
                documented.push(
                    FrameworkManifest::parse("documented".into(), extension, source).unwrap(),
                );
            }
        }
        assert_eq!(
            documented.len(),
            2,
            "both complete shipped semantic examples parse"
        );
        let valid = serde_json::to_value(&documented[0]).unwrap();
        for extension in ["toml", "yaml", "yml"] {
            let replacement = semantic_root.join(format!(".forge/frameworks/django.{extension}"));
            for (pointer, value) in [
                ("/unexpected", true.into()),
                ("/semantic_rules/0/unexpected", true.into()),
                ("/semantic_rules/0/scope", "cross_language_project".into()),
                ("/semantic_rules/0/namespace", "".into()),
                ("/semantic_rules/0/activation/0/unexpected", true.into()),
                ("/semantic_rules/0/activation/0/kind", "import".into()),
                ("/semantic_rules/0/operation/unexpected", true.into()),
                ("/semantic_rules/0/operation/kind", "member_provider".into()),
                ("/semantic_rules/0/operation/language", "javascript".into()),
                ("/semantic_rules/0/operation/argument", (-1).into()),
                ("/semantic_rules/0/operation/callee/unexpected", true.into()),
                ("/semantic_rules/0/operation/callee/kind", "global".into()),
                (
                    "/semantic_rules/0/operation/callee/module",
                    "django..shortcuts".into(),
                ),
                ("/semantic_rules/0/operation/target/unexpected", true.into()),
                ("/semantic_rules/0/operation/target/kind", "basename".into()),
                (
                    "/semantic_rules/0/operation/target/language",
                    "python".into(),
                ),
                (
                    "/semantic_rules/0/operation/target/prefix",
                    "../templates/".into(),
                ),
                (
                    "/semantic_rules/0/operation/target/prefix",
                    "/templates/".into(),
                ),
                (
                    "/semantic_rules/0/operation/target/prefix",
                    "templates".into(),
                ),
                ("/semantic_rules/0/operation/relation", "calls".into()),
            ] {
                let mut invalid = valid.clone();
                invalid["replace"] = true.into();
                let (parent, field) = pointer.rsplit_once('/').unwrap();
                invalid
                    .pointer_mut(parent)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .insert(field.into(), value);
                let source = if extension == "toml" {
                    toml::to_string(&invalid).unwrap()
                } else {
                    serde_yaml::to_string(&invalid).unwrap()
                };
                fs::write(&replacement, source).unwrap();
                let error = DependencyRegistry::try_collect_with_paths(&semantic_root, None, &[])
                    .err()
                    .expect("malformed replacement must fail instead of resurrecting the bundle");
                assert!(
                    format!("{error:#}").contains(&replacement.display().to_string()),
                    "actionable source diagnostic: {error:#}"
                );
            }
            fs::remove_file(replacement).unwrap();
        }
        // Existing project-first merge owns semantic IDs as well as T1 rule IDs.
        let mut bundled = documented[0].clone();
        let mut project = documented[0].clone();
        project.semantic_rules[0].namespace = "override".into();
        let mut independent = bundled.semantic_rules[0].clone();
        independent.id = "independent".into();
        bundled.semantic_rules.push(independent);
        let merged = project.with_bundled_rules(&bundled);
        assert_eq!(merged.semantic_rules.len(), 2);
        assert_eq!(merged.semantic_rules[0].namespace, "override");
        let mut duplicate = valid.clone();
        let rule = duplicate["semantic_rules"][0].clone();
        duplicate["semantic_rules"]
            .as_array_mut()
            .unwrap()
            .push(rule);
        assert!(FrameworkManifest::parse(
            "duplicate".into(),
            "toml",
            &toml::to_string(&duplicate).unwrap()
        )
        .unwrap_err()
        .to_string()
        .contains("duplicate semantic rule id"));
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
[[class_hierarchy_receivers]]
bases = ["django.custom.Base"]
provider = "django.custom.Base"
members = ["deliver"]
[[class_hierarchy_receivers]]
bases = ["django.core.management.base.BaseCommand"]
provider = "project.management.BaseCommand"
members = ["write"]
[[route_decorators]]
callee = "portal.expose"
method = "POST"
path_argument = 0
[[route_calls]]
callee = "register_endpoint"
method = "GET"
path_argument = 0
handler_argument = 1
[[route_calls]]
callee = "path"
method = "ANY"
path_argument = 0
handler_argument = 2
[[di_calls]]
callee = "InjectService"
argument = 0
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
class_hierarchy_receivers:
  - bases: [django.custom.Base]
    provider: django.custom.Base
    members: [deliver]
  - bases: [django.core.management.base.BaseCommand]
    provider: project.management.BaseCommand
    members: [write]
route_decorators:
  - callee: portal.expose
    method: POST
    path_argument: 0
route_calls:
  - callee: register_endpoint
    method: GET
    path_argument: 0
    handler_argument: 1
  - callee: path
    method: ANY
    path_argument: 0
    handler_argument: 2
di_calls:
  - callee: InjectService
    argument: 0
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
                            "typed class rule collision uses project provider",
                            manifest
                                .class_hierarchy_receivers
                                .iter()
                                .find(|rule| {
                                    rule.bases == ["django.core.management.base.BaseCommand"]
                                })
                                .is_some_and(|rule| {
                                    rule.provider == "project.management.BaseCommand"
                                }),
                        ),
                        (
                            "typed class rule collision is unique",
                            manifest
                                .class_hierarchy_receivers
                                .iter()
                                .filter(|rule| {
                                    rule.bases == ["django.core.management.base.BaseCommand"]
                                })
                                .count()
                                == 1,
                        ),
                        (
                            "noncolliding bundled class rule follows replace mode",
                            manifest
                                .class_hierarchy_receivers
                                .iter()
                                .any(|rule| rule.bases.contains(&"django.test.TestCase".to_owned()))
                                != replace,
                        ),
                        (
                            "typed route rule collision uses project handler position",
                            manifest
                                .route_calls
                                .iter()
                                .find(|rule| rule.callee == "path" && rule.method == "ANY")
                                .is_some_and(|rule| rule.handler_argument == Some(2)),
                        ),
                        (
                            "typed route rule collision is unique",
                            manifest
                                .route_calls
                                .iter()
                                .filter(|rule| rule.callee == "path" && rule.method == "ANY")
                                .count()
                                == 1,
                        ),
                        (
                            "noncolliding bundled route rule follows replace mode",
                            manifest
                                .route_calls
                                .iter()
                                .any(|rule| rule.callee == "re_path" && rule.method == "ANY")
                                != replace,
                        ),
                        (
                            "project decorator rule remains under replace mode",
                            manifest
                                .route_decorators
                                .iter()
                                .any(|rule| rule.callee == "portal.expose" && rule.method == "POST"),
                        ),
                        (
                            "project dependency rule remains under replace mode",
                            manifest
                                .di_calls
                                .iter()
                                .any(|rule| rule.callee == "InjectService" && rule.argument == 0),
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
                    for (invalid, typed_rule) in [
                        (
                            "empty class hierarchy base list",
                            "[[class_hierarchy_receivers]]\nbases = []\nprovider = \"provider.Base\"\nmembers = [\"send\"]\n",
                        ),
                        (
                            "empty class hierarchy provider",
                            "[[class_hierarchy_receivers]]\nbases = [\"provider.Base\"]\nprovider = \" \"\nmembers = [\"send\"]\n",
                        ),
                        (
                            "invalid route method",
                            "[[route_decorators]]\ncallee = \"router.get\"\nmethod = \"get\"\npath_argument = 0\n",
                        ),
                        (
                            "same path and handler argument",
                            "[[route_calls]]\ncallee = \"register\"\nmethod = \"GET\"\npath_argument = 0\nhandler_argument = 0\n",
                        ),
                        (
                            "negative dependency argument",
                            "[[di_calls]]\ncallee = \"Inject\"\nargument = -1\n",
                        ),
                        (
                            "unknown typed rule field",
                            "[[di_calls]]\ncallee = \"Inject\"\nargument = 0\nextra = true\n",
                        ),
                    ] {
                        let content = format!(
                            "receivers = []\nbuiltins = []\nfilters = []\nroutes = []\n{typed_rule}"
                        );
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

        {
            use crate::db::reader;
            use crate::engine::pipeline;
            let coverage_root = temporary.0.join("rule-coverage");
            fs::create_dir_all(coverage_root.join(".forge/frameworks")).unwrap();
            fs::write(
                coverage_root.join("pyproject.toml"),
                "[project]\nname = 'rule-coverage'\ndependencies = ['fastapi']\n",
            )
            .unwrap();
            fs::write(
                coverage_root.join("app.py"),
                "def original_provider(): return 1\ndef project_provider(): return 2\ndef original(value=Depends(original_provider, project_provider)): return value\ndef project(value=ProjectDepends(project_provider)): return value\ndef endpoint(): return None\napp.get('/stable', endpoint)\n",
            ).unwrap();
            fs::write(coverage_root.join("peer.py"), "def ordinary(): return 1\n").unwrap();
            fs::write(
                coverage_root.join("README.md"),
                "# Rule coverage\n\nRevision 0.\n",
            )
            .unwrap();
            let coverage_db = coverage_root.join(".forge/code-map.sqlite");
            let coverage_cold = coverage_root.join(".forge/rule-coverage-cold.sqlite");
            let framework_path = ".forge/frameworks/fastapi.toml";
            let bundled = include_str!("manifests/fastapi.toml");
            let replacement = format!(
                "replace = true\n{}",
                bundled.replace("callee = \"Depends\"", "callee = \"ProjectDepends\"")
            );
            let payload_override = format!(
                "replace = true\n{}",
                bundled.replace(
                    "callee = \"Depends\"\nargument = 0",
                    "callee = \"Depends\"\nargument = 1",
                )
            );
            let addition = "receivers = []\nbuiltins = []\nfilters = []\nroutes = []\n[[di_calls]]\ncallee = 'ProjectDepends'\nargument = 0\n";
            let duplicate = "receivers = []\nbuiltins = []\nfilters = []\nroutes = []\n[[di_calls]]\ncallee = 'Depends'\nargument = 0\n[[di_calls]]\ncallee = 'Depends'\nargument = 0\n";
            let clean = serde_json::json!([]);
            let lost_depends = serde_json::json!([{
                "framework": "fastapi",
                "workspace": "",
                "language": "python",
                "rule_domain": "di_calls",
                "code": "eligible_rule_identity_removed",
                "removed_eligible_rule_identities": [{"callee": "Depends"}]
            }]);
            let mut coverage_failures = Vec::new();
            let check_report = |phase: &str,
                                report: &serde_json::Value,
                                expected: &serde_json::Value,
                                failures: &mut Vec<String>| {
                if report.get("framework_rule_diagnostics") != Some(expected) {
                    failures.push(format!(
                        "{phase}: framework_rule_diagnostics must be {expected}, got {:?}",
                        report.get("framework_rule_diagnostics")
                    ));
                }
            };
            let check_persisted = |phase: &str,
                                   database: &std::path::Path,
                                   expected_injections: &[(&str, i64)],
                                   failures: &mut Vec<String>| {
                let persisted = reader::open(database, &coverage_root).unwrap();
                crate::core::commitments::verify(&persisted).unwrap();
                let parse_errors: i64 = persisted
                    .query_row("SELECT count(*) FROM errors", [], |row| row.get(0))
                    .unwrap();
                if parse_errors != 0 {
                    failures.push(format!(
                        "{phase}: rule diagnostics must not become parse errors: {parse_errors}"
                    ));
                }
                let injections: Vec<(String, i64)> = persisted
                    .prepare(
                        "SELECT target.qualname,count(*) FROM edge_occurrences occurrence \
                         JOIN nodes target ON target.node_hash=occurrence.dst_hash \
                         WHERE occurrence.kind='injects' GROUP BY target.qualname \
                         ORDER BY target.qualname",
                    )
                    .unwrap()
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .unwrap()
                    .map(|row| row.unwrap())
                    .collect();
                let expected: Vec<(String, i64)> = expected_injections
                    .iter()
                    .map(|(target, occurrences)| ((*target).to_owned(), *occurrences))
                    .collect();
                if injections != expected {
                    failures.push(format!(
                        "{phase}: current effective DI rules must reach exact persisted occurrences: expected {expected:?}, got {injections:?}"
                    ));
                }
                let stable_route: i64 = persisted
                    .query_row(
                        "SELECT count(*) FROM nodes WHERE kind='route' AND name='GET /stable'",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap();
                if stable_route != 1 {
                    failures.push(format!(
                        "{phase}: unchanged bundled route must remain unique: {stable_route}"
                    ));
                }
            };
            let initial = pipeline::build(&coverage_root, &coverage_db, None).unwrap();
            check_report("clean full build", &initial, &clean, &mut coverage_failures);
            check_persisted(
                "clean full build",
                &coverage_db,
                &[("app.original_provider", 1)],
                &mut coverage_failures,
            );

            struct CoverageCase<'a> {
                name: &'static str,
                manifest: Option<&'a str>,
                loses_depends: bool,
                injections: &'static [(&'static str, i64)],
            }
            let coverage_cases = [
                CoverageCase {
                    name: "equal-count replacement",
                    manifest: Some(&replacement),
                    loses_depends: true,
                    injections: &[("app.project_provider", 1)],
                },
                CoverageCase {
                    name: "default extension adds identity",
                    manifest: Some(addition),
                    loses_depends: false,
                    injections: &[("app.original_provider", 1), ("app.project_provider", 1)],
                },
                CoverageCase {
                    name: "extension deduplicates identity",
                    manifest: Some(duplicate),
                    loses_depends: false,
                    injections: &[("app.original_provider", 1)],
                },
                CoverageCase {
                    name: "same identity changes payload",
                    manifest: Some(&payload_override),
                    loses_depends: false,
                    injections: &[("app.project_provider", 1)],
                },
                CoverageCase {
                    name: "override deletion restores bundled coverage",
                    manifest: None,
                    loses_depends: false,
                    injections: &[("app.original_provider", 1)],
                },
                CoverageCase {
                    name: "override restoration reinstates loss",
                    manifest: Some(&replacement),
                    loses_depends: true,
                    injections: &[("app.project_provider", 1)],
                },
            ];
            for (revision, case) in coverage_cases.iter().enumerate() {
                match case.manifest {
                    Some(manifest) => {
                        fs::write(coverage_root.join(framework_path), manifest).unwrap()
                    }
                    None => fs::remove_file(coverage_root.join(framework_path)).unwrap(),
                }
                let expected = if case.loses_depends {
                    &lost_depends
                } else {
                    &clean
                };
                let reloaded = DependencyRegistry::collect_with_scan_config(&coverage_root, None);
                let effective = reloaded
                    .framework_manifest_for_path(
                        LanguageFamily("python"),
                        "app.py",
                        "fastapi",
                        "fastapi",
                    )
                    .unwrap();
                let expected_di_count = if case.name == "default extension adds identity" {
                    2
                } else {
                    1
                };
                if effective.di_calls.len() != expected_di_count {
                    coverage_failures.push(format!(
                        "{}: expected {expected_di_count} unique effective DI identities, got {:?}",
                        case.name, effective.di_calls
                    ));
                }
                let fallback = pipeline::delta(
                    &coverage_root,
                    &coverage_db,
                    &[PathBuf::from(framework_path)],
                )
                .unwrap();
                if fallback.get("schema_version").is_none()
                    || fallback.get("changed_files").is_some()
                {
                    coverage_failures.push(format!(
                        "{}: framework digest change must use the full-build fallback report: {fallback}",
                        case.name
                    ));
                }
                check_report(case.name, &fallback, expected, &mut coverage_failures);
                check_persisted(
                    case.name,
                    &coverage_db,
                    case.injections,
                    &mut coverage_failures,
                );
                let cold = pipeline::build(&coverage_root, &coverage_cold, None).unwrap();
                check_report(case.name, &cold, expected, &mut coverage_failures);
                check_persisted(
                    case.name,
                    &coverage_cold,
                    case.injections,
                    &mut coverage_failures,
                );

                fs::write(
                    coverage_root.join("README.md"),
                    format!("# Rule coverage\n\nRevision {}.\n", revision + 1),
                )
                .unwrap();
                let ordinary =
                    pipeline::delta(&coverage_root, &coverage_db, &[PathBuf::from("README.md")])
                        .unwrap();
                if ordinary["changed_files"] != 1 || ordinary["reparsed_files"] != 1 {
                    coverage_failures.push(format!(
                        "{}: Markdown-only edit must take ordinary delta: {ordinary}",
                        case.name
                    ));
                }
                check_report(case.name, &ordinary, expected, &mut coverage_failures);
                check_persisted(
                    case.name,
                    &coverage_db,
                    case.injections,
                    &mut coverage_failures,
                );
            }
            let linked_root = temporary.0.join("linked-rule-coverage");
            fs::create_dir_all(linked_root.join(".forge/frameworks")).unwrap();
            fs::write(
                linked_root.join("pyproject.toml"),
                "[project]\nname='linked-rule-coverage'\ndependencies=['fastapi']\n",
            )
            .unwrap();
            fs::write(
                linked_root.join("app.py"),
                "def original_provider(): return 1\ndef project_provider(): return 2\ndef original(value=Depends(original_provider, project_provider)): return value\ndef project(value=ProjectDepends(project_provider)): return value\ndef linked(value=LinkedDepends(original_provider)): return value\ndef endpoint(): return None\napp.get('/stable', endpoint)\n",
            ).unwrap();
            fs::write(
                coverage_root.join("forge-mcp.yaml"),
                format!("roots: [.]\nlinked_workspaces:\n  - name: linked\n    path: {}\n    roots: [.]\n", linked_root.display()),
            ).unwrap();
            let linked_replacement = replacement.replace("ProjectDepends", "LinkedDepends");
            let mut linked_loss = lost_depends[0].clone();
            linked_loss["workspace"] = serde_json::json!("[linked]");
            let scoped_losses = serde_json::json!([lost_depends[0], linked_loss]);
            for (phase, manifest, expected, linked_owner, linked_target) in [
                (
                    "linked payload keeps identity",
                    &payload_override,
                    &lost_depends,
                    "original",
                    "project_provider",
                ),
                (
                    "linked replacement removes identity",
                    &linked_replacement,
                    &scoped_losses,
                    "linked",
                    "original_provider",
                ),
                (
                    "linked payload restoration",
                    &payload_override,
                    &lost_depends,
                    "original",
                    "project_provider",
                ),
            ] {
                fs::write(linked_root.join(framework_path), manifest).unwrap();
                let report = if phase == "linked payload keeps identity" {
                    pipeline::build(&coverage_root, &coverage_db, None).unwrap()
                } else {
                    let fallback = pipeline::delta(
                        &coverage_root,
                        &coverage_db,
                        &[PathBuf::from("[linked]/.forge/frameworks/fastapi.toml")],
                    )
                    .unwrap();
                    if fallback.get("schema_version").is_none()
                        || fallback.get("changed_files").is_some()
                    {
                        coverage_failures.push(format!("{phase}: linked manifest change must use full-build fallback: {fallback}"));
                    }
                    fallback
                };
                check_report(phase, &report, expected, &mut coverage_failures);
                let cold = pipeline::build(&coverage_root, &coverage_cold, None).unwrap();
                check_report(phase, &cold, expected, &mut coverage_failures);
                fs::write(coverage_root.join("README.md"), format!("# {phase}\n")).unwrap();
                let ordinary =
                    pipeline::delta(&coverage_root, &coverage_db, &[PathBuf::from("README.md")])
                        .unwrap();
                if ordinary["changed_files"] != 1 || ordinary["reparsed_files"] != 1 {
                    coverage_failures.push(format!("{phase}: linked diagnostics must survive ordinary Markdown delta: {ordinary}"));
                }
                check_report(phase, &ordinary, expected, &mut coverage_failures);
                for database in [&coverage_db, &coverage_cold] {
                    let persisted = reader::open(database, &coverage_root).unwrap();
                    crate::core::commitments::verify(&persisted).unwrap();
                    let errors: i64 = persisted
                        .query_row("SELECT count(*) FROM errors", [], |row| row.get(0))
                        .unwrap();
                    if errors != 0 {
                        coverage_failures.push(format!("{phase}: linked parse errors: {errors}"));
                    }
                    let injections: Vec<(String, String, String, String, i64)> = persisted.prepare(
                        "SELECT sp.path,source.name,tp.path,target.name,count(*) FROM edge_occurrences occurrence JOIN nodes source ON source.node_hash=occurrence.src_hash JOIN nodes target ON target.node_hash=occurrence.dst_hash JOIN path_dictionary sp ON sp.path_id=source.path_id JOIN path_dictionary tp ON tp.path_id=target.path_id WHERE occurrence.kind='injects' GROUP BY sp.path,source.name,tp.path,target.name ORDER BY sp.path,source.name,tp.path,target.name"
                    ).unwrap().query_map([], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).unwrap().map(|row| row.unwrap()).collect();
                    let expected_injections = vec![
                        (
                            "[linked]/app.py".to_owned(),
                            linked_owner.to_owned(),
                            "[linked]/app.py".to_owned(),
                            linked_target.to_owned(),
                            1,
                        ),
                        (
                            "app.py".to_owned(),
                            "project".to_owned(),
                            "app.py".to_owned(),
                            "project_provider".to_owned(),
                            1,
                        ),
                    ];
                    if injections != expected_injections {
                        coverage_failures.push(format!("{phase}: workspace-local persisted DI sources/targets/occurrences: expected {expected_injections:?}, got {injections:?}"));
                    }
                    let routes: Vec<(String, i64)> = persisted.prepare("SELECT p.path,count(*) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.kind='route' AND n.name='GET /stable' GROUP BY p.path ORDER BY p.path").unwrap().query_map([], |row| Ok((row.get(0)?,row.get(1)?))).unwrap().map(|row| row.unwrap()).collect();
                    if routes != vec![("[linked]/app.py".to_owned(), 1), ("app.py".to_owned(), 1)] {
                        coverage_failures.push(format!(
                            "{phase}: workspace-local stable route identity: {routes:?}"
                        ));
                    }
                }
            }
            assert!(
                coverage_failures.is_empty(),
                "framework rule coverage reports:\n{}",
                coverage_failures.join("\n")
            );
        }
        #[cfg(feature = "lang-typescript")]
        {
            use crate::{db::reader, engine::pipeline};
            let mixed_root = temporary.0.join("mixed-grammar");
            fs::create_dir_all(mixed_root.join(".forge/frameworks")).unwrap();
            fs::write(mixed_root.join(".forge/frameworks/mixed.toml"), "receivers=[]\nbuiltins=[]\nfilters=[]\nroutes=[]\n[[route_calls]]\ncallee='publish'\nmethod='GET'\npath_argument=0\nhandler_argument=1\n[[di_calls]]\ncallee='InjectMixed'\nargument=0\n").unwrap();
            let database = mixed_root.join(".forge/code-map.sqlite");
            let cold_database = mixed_root.join(".forge/cold.sqlite");
            for revision in 0..3 {
                for (file, route, suffix) in [
                    ("a.ts", "/typescript", ""),
                    ("b.tsx", "/tsx", "export const markup = <section />;"),
                ] {
                    fs::write(mixed_root.join(file), format!("export function endpoint() {{ return {revision}; }}\npublish('{route}', endpoint);\nInjectMixed(endpoint);\n{suffix}\n")).unwrap();
                }
                let report = if revision == 0 {
                    pipeline::build(&mixed_root, &database, None).unwrap()
                } else {
                    let ordinary = pipeline::delta(
                        &mixed_root,
                        &database,
                        &[PathBuf::from("a.ts"), PathBuf::from("b.tsx")],
                    )
                    .unwrap();
                    assert_eq!(
                        ordinary["changed_files"], 2,
                        "both grammars in one ordinary registry operation"
                    );
                    assert_eq!(ordinary["reparsed_files"], 2);
                    ordinary
                };
                assert_eq!(report["framework_rule_diagnostics"], serde_json::json!([]));
                let cold = pipeline::build(&mixed_root, &cold_database, None).unwrap();
                assert_eq!(cold["framework_rule_diagnostics"], serde_json::json!([]));
                for output in [&database, &cold_database] {
                    let persisted = reader::open(output, &mixed_root).unwrap();
                    crate::core::commitments::verify(&persisted).unwrap();
                    let errors: i64 = persisted
                        .query_row("SELECT count(*) FROM errors", [], |row| row.get(0))
                        .unwrap();
                    assert_eq!(errors, 0, "mixed actual grammars must parse without errors");
                    let routes: Vec<(String,String,i64)> = persisted.prepare("SELECT p.path,n.name,count(*) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.kind='route' GROUP BY p.path,n.name ORDER BY p.path,n.name").unwrap().query_map([], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap().map(|row| row.unwrap()).collect();
                    assert_eq!(
                        routes,
                        vec![
                            ("a.ts".to_owned(), "GET /typescript".to_owned(), 1),
                            ("b.tsx".to_owned(), "GET /tsx".to_owned(), 1)
                        ],
                        "actual grammar cache dispatch, revision {revision}"
                    );
                    let injections: Vec<(String,String,String,i64)> = persisted.prepare("SELECT sp.path,tp.path,target.name,count(*) FROM edge_occurrences occurrence JOIN nodes source ON source.node_hash=occurrence.src_hash JOIN nodes target ON target.node_hash=occurrence.dst_hash JOIN path_dictionary sp ON sp.path_id=source.path_id JOIN path_dictionary tp ON tp.path_id=target.path_id WHERE occurrence.kind='injects' GROUP BY sp.path,tp.path,target.name ORDER BY sp.path,tp.path,target.name").unwrap().query_map([], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap().map(|row| row.unwrap()).collect();
                    assert_eq!(
                        injections,
                        vec![
                            (
                                "a.ts".to_owned(),
                                "a.ts".to_owned(),
                                "endpoint".to_owned(),
                                1
                            ),
                            (
                                "b.tsx".to_owned(),
                                "b.tsx".to_owned(),
                                "endpoint".to_owned(),
                                1
                            )
                        ],
                        "grammar-specific DI targets and occurrences, revision {revision}"
                    );
                }
            }
        }
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
