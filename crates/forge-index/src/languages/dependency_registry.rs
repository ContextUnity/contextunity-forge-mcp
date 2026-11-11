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
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

type ManifestSnapshots = BTreeMap<String, sha2::digest::Output<Sha256>>;

/// A manifest's UTF-8 contents at a canonical workspace-relative path.
#[derive(Clone, Copy, Debug)]
pub struct ManifestSource<'a> {
    /// A dependency filename or `.forge/frameworks/<identity>.{toml,yaml,yml}`.
    pub path: &'a str,
    /// The manifest contents, bounded to 4 MiB.
    pub source: &'a str,
}

/// An explicit manifest inventory isolated to one workspace.
pub struct WorkspaceManifestSources<'a> {
    /// Empty for the primary workspace; otherwise an opaque linked-workspace name.
    pub name: &'a str,
    /// Each canonical path occurs once within this workspace.
    pub manifests: &'a [ManifestSource<'a>],
}

type LanguageContextKey = (String, Vec<(String, String)>);

#[derive(Clone, Debug)]
pub(crate) struct ManifestEntrypoint {
    pub origin: String,
    pub scope: String,
    pub name: String,
    pub target: String,
    pub python: bool,
}

struct DirectoryContextEntry {
    registry_digest: String,
    dir: String,
    context: Arc<super::LanguageContext>,
}

thread_local! {
    // Compatibility-only reuse for direct bundled/manual registry callers.
    // Collected inventories use the immutable registry-owned contexts below.
    static LEGACY_LANGUAGE_CONTEXT_CACHE: RefCell<HashMap<(String, LanguageContextKey), Arc<super::LanguageContext>>> =
        RefCell::new(HashMap::new());
    static DIRECTORY_LANGUAGE_CONTEXT_CACHE: RefCell<HashMap<u64, DirectoryContextEntry>> =
        RefCell::new(HashMap::new());
}

#[derive(Default)]
/// Represents dependency registry data.
pub struct DependencyRegistry {
    packages: HashMap<LanguageFamily, HashSet<String>>,
    scoped_packages: HashMap<LanguageFamily, HashMap<String, HashSet<String>>>,
    scoped_origins: HashMap<LanguageFamily, HashMap<String, HashMap<String, String>>>,
    own_packages: HashMap<String, HashSet<String>>,
    workspaces: HashSet<String>,
    named_provider_scopes: HashMap<super::manifests::projections::NamedProviderScope, String>,
    rust_libraries: HashMap<String, String>,
    framework_manifests: HashMap<(String, String), FrameworkManifest>,
    standard_framework_manifests: HashMap<String, FrameworkManifest>,
    language_contexts: HashMap<LanguageContextKey, Arc<super::LanguageContext>>,
    rule_ir: HashMap<String, RuleIRHandle>,
    javascript_packages: JavascriptPackageRegistry,
    typescript_paths: TypeScriptPathsRegistry,
    #[cfg(feature = "lang-rust")]
    rust_root_crates: HashMap<String, String>,
    digest: String,
    manifest_entrypoints: Vec<ManifestEntrypoint>,
}

impl DependencyRegistry {
    /// Collects validated rules and dependency scopes without accessing the filesystem.
    ///
    /// # Errors
    ///
    /// Rejects duplicate workspaces, invalid or duplicate paths, unsupported filenames,
    /// oversized inputs, and invalid framework rules or cross-workspace policies.
    pub fn try_collect_from_sources<'a>(
        workspaces: impl IntoIterator<Item = WorkspaceManifestSources<'a>>,
    ) -> Result<Self> {
        let mut registry = Self::default();
        load_standard_framework_manifests(&mut registry)?;
        let mut snapshots = ManifestSnapshots::new();
        for workspace in workspaces {
            let label = if workspace.name.is_empty() {
                String::new()
            } else {
                format!("[{}]", workspace.name)
            };
            anyhow::ensure!(
                registry.workspaces.insert(label.clone()),
                "duplicate manifest workspace: {label}"
            );
            let prefix = if label.is_empty() {
                String::new()
            } else {
                format!("{label}/")
            };
            let mut paths = HashSet::new();
            for manifest in workspace.manifests {
                anyhow::ensure!(
                    super::manifests::semantic::normal_relative_path(manifest.path)
                        && super::manifests::projections::confined_path(manifest.path, false)
                        && !manifest.path.starts_with('['),
                    "invalid manifest source path: {}",
                    manifest.path
                );
                anyhow::ensure!(
                    paths.insert(manifest.path),
                    "duplicate manifest source path: {}",
                    manifest.path
                );
                anyhow::ensure!(
                    manifest.source.len() <= 4 * 1024 * 1024,
                    "manifest source exceeds the 4 MiB limit: {}",
                    manifest.path
                );
                let path = Path::new(manifest.path);
                if path.parent() == Some(Path::new(".forge/frameworks"))
                    && matches!(
                        path.extension().and_then(|value| value.to_str()),
                        Some("toml" | "yaml" | "yml")
                    )
                {
                    record_framework_manifest(
                        &label,
                        &prefix,
                        path,
                        manifest.source,
                        &mut registry,
                        &mut snapshots,
                    )
                    .with_context(|| {
                        format!("failed to load framework manifest {}", manifest.path)
                    })?;
                } else {
                    anyhow::ensure!(
                        is_dependency_manifest_filename(
                            path.file_name().and_then(|value| value.to_str()).unwrap()
                        ),
                        "unsupported manifest source path: {}",
                        manifest.path
                    );
                    record_dependency_manifest(
                        &prefix,
                        path,
                        manifest.source,
                        &mut registry,
                        &mut snapshots,
                    );
                }
            }
        }
        finish_registry(registry, snapshots)
    }

    pub(crate) fn manifest_entrypoints(&self) -> &[ManifestEntrypoint] {
        &self.manifest_entrypoints
    }
    /// Loads the built-in framework rules without requiring dependency manifests.
    pub(crate) fn bundled_framework_rules() -> Self {
        let mut registry = Self::default();
        load_standard_framework_manifests(&mut registry)
            .expect("bundled framework manifests are validated at compile time");
        registry
    }

    /// Reuses merged extraction rules for files with the same workspace and package scope.
    pub fn language_context_for_path(&self, path: &str) -> Arc<super::LanguageContext> {
        let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&(self.digest.as_str(), dir), &mut hasher);
        let hash = std::hash::Hasher::finish(&hasher);
        if let Some(context) = DIRECTORY_LANGUAGE_CONTEXT_CACHE.with(|cache| {
            cache.borrow().get(&hash).and_then(|entry| {
                (entry.registry_digest == self.digest && entry.dir == dir)
                    .then(|| Arc::clone(&entry.context))
            })
        }) {
            return context;
        }

        let key = self.language_context_key(path);
        let context = if let Some(context) = self.language_contexts.get(&key) {
            Arc::clone(context)
        } else {
            // Direct/manual registries and unregistered workspace paths retain their
            // bounded compatibility preparation; collected inventories are ready.
            let key = (self.digest.clone(), key);
            if let Some(context) =
                LEGACY_LANGUAGE_CONTEXT_CACHE.with(|cache| cache.borrow().get(&key).cloned())
            {
                context
            } else {
                let mut context = self.build_language_context(path);
                if !self.rule_ir.contains_key(super::workspace_path(path).0) {
                    context.rule_ir = RuleIRHandle::prepare(&context);
                }
                let context = Arc::new(context);
                LEGACY_LANGUAGE_CONTEXT_CACHE.with(|cache| {
                    let mut cache = cache.borrow_mut();
                    if cache.len() >= 128 {
                        cache.clear();
                    }
                    cache.insert(key, Arc::clone(&context));
                });
                context
            }
        };

        DIRECTORY_LANGUAGE_CONTEXT_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            if cache.len() >= 512 {
                cache.clear();
            }
            cache.insert(
                hash,
                DirectoryContextEntry {
                    registry_digest: self.digest.clone(),
                    dir: dir.to_owned(),
                    context: Arc::clone(&context),
                },
            );
        });
        context
    }

    fn language_context_key(&self, path: &str) -> LanguageContextKey {
        let (workspace, _) = super::workspace_path(path);
        let families: BTreeSet<_> = profiles().map(|profile| profile.family().0).collect();
        let scopes = families
            .into_iter()
            .map(|family| {
                let scope = self
                    .nearest_manifest_scope_for_path(LanguageFamily(family), path)
                    .unwrap_or(workspace);
                (family.to_owned(), scope.to_owned())
            })
            .collect();
        (workspace.to_owned(), scopes)
    }

    fn build_language_context(&self, path: &str) -> super::LanguageContext {
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

        let mut context = super::LanguageContext::default();
        for name in names {
            let manifest = self.effective_framework_manifest(path, name);
            let Some(manifest) = manifest else { continue };
            for (index, rule) in manifest.route_decorators.iter().enumerate() {
                context.route_rule_origins.insert(
                    (true, context.route_decorators.len() + index),
                    crate::core::semantic::ExtractionRuleOrigin {
                        pack: manifest.name.clone(),
                        rule: serde_json::to_string(&rule.identity())
                            .expect("typed route identity"),
                    },
                );
            }
            for (index, rule) in manifest.route_calls.iter().enumerate() {
                context.route_rule_origins.insert(
                    (false, context.route_calls.len() + index),
                    crate::core::semantic::ExtractionRuleOrigin {
                        pack: manifest.name.clone(),
                        rule: serde_json::to_string(&rule.identity())
                            .expect("typed route identity"),
                    },
                );
            }
            context
                .class_hierarchy_receivers
                .extend(manifest.class_hierarchy_receivers.iter().cloned());
            context
                .route_decorators
                .extend(manifest.route_decorators.iter().cloned());
            context
                .route_calls
                .extend(manifest.route_calls.iter().cloned().map(|mut rule| {
                    rule.registration_guard = rule.registration_guard.map(|id| {
                        serde_json::to_string(&(&manifest.name, id))
                            .expect("rule identity strings serialize")
                    });
                    rule
                }));
            for (index, rule) in manifest.di_calls.iter().enumerate() {
                context.di_rule_origins.insert(
                    context.di_calls.len() + index,
                    crate::core::semantic::ExtractionRuleOrigin {
                        pack: manifest.name.clone(),
                        rule: serde_json::to_string(&rule.identity()).expect("typed DI identity"),
                    },
                );
            }
            context.di_calls.extend(manifest.di_calls.iter().cloned());
            context.syntax_rules.extend(
                manifest
                    .syntax_rules
                    .iter()
                    .filter(|rule| {
                        let rule_family = rule.activation_family();
                        rule.activation.is_empty()
                            || rule_family.is_some_and(|family| {
                                rule.activation.iter().any(|activation| {
                                    let super::manifests::Activation::Package { name } = activation;
                                    self.declares_for_path(family, path, name)
                                })
                            })
                    })
                    .cloned()
                    .map(|mut rule| {
                        rule.pack = manifest.name.clone();
                        rule
                    }),
            );
            for rule in &manifest.semantic_rules {
                let family = super::by_id(rule.language()).map(|profile| profile.family());
                let scope = family
                    .and_then(|family| self.nearest_manifest_scope_for_path(family, path))
                    .unwrap_or(workspace);
                let declared_packages = family.and_then(|family| {
                    self.scoped_packages
                        .get(&family)
                        .and_then(|scopes| scopes.get(scope))
                });
                context
                    .semantic_rules
                    .push(super::semantic::EffectiveSemanticRule {
                        pack: manifest.name.clone(),
                        rule: rule.clone(),
                        scope: scope.to_owned(),
                        active: rule.activation.is_empty()
                            || family.is_some_and(|family| {
                                rule.activation.iter().any(|activation| {
                                    let super::manifests::Activation::Package { name } = activation;
                                    declared_packages.is_some_and(|packages| {
                                        packages.contains(&package_identity(family, name))
                                    })
                                })
                            }),
                    });
            }
        }
        context.prepare_named_member_rules();
        if let Some(rule_ir) = self.rule_ir.get(workspace) {
            context.rule_ir = rule_ir.clone();
        }
        context
    }

    fn prepare_language_contexts(&mut self) {
        // Effective language package scopes can change only at these collected
        // directory boundaries. Their union covers every admitted scope vector.
        let scopes: BTreeSet<String> = std::iter::once(String::new())
            .chain(self.workspaces.iter().cloned())
            .chain(
                self.framework_manifests
                    .keys()
                    .map(|(scope, _)| scope.clone()),
            )
            .chain(
                self.scoped_packages
                    .values()
                    .flat_map(|scopes| scopes.keys().cloned()),
            )
            .collect();
        self.language_contexts.clear();
        self.rule_ir.clear();
        for scope in scopes {
            let path = if scope.is_empty() {
                "__context_probe__".to_owned()
            } else {
                format!("{scope}/__context_probe__")
            };
            let key = self.language_context_key(&path);
            if self.language_contexts.contains_key(&key) {
                continue;
            }
            let mut context = self.build_language_context(&path);
            // Route/DI rows and their origins depend on workspace manifest merge,
            // not package activation. Compile once per workspace, before workers.
            let rule_ir = self
                .rule_ir
                .entry(key.0.clone())
                .or_insert_with(|| RuleIRHandle::prepare(&context));
            context.rule_ir = rule_ir.clone();
            self.language_contexts.insert(key, Arc::new(context));
        }
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
            if !profile.has_grammar() {
                continue;
            }
            let grammar = profile.grammar(path);
            let (workspace, _) = super::workspace_path(path);
            let call = ["call", "call_expression"]
                .into_iter()
                .any(|name| named_kind(&grammar, name).is_some());
            let routes = profile.route_extraction_enabled();
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
                        named_kind(&grammar, "call").is_some()
                            && bundled
                                .semantic_rules
                                .iter()
                                .any(|rule| rule.language() == language),
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
                                    && rule.language() == language
                                    && (rule.activation.is_empty()
                                        || rule.activation.iter().any(|activation| {
                                            let super::manifests::Activation::Package { name } =
                                                activation;
                                            super::by_id(rule.language()).is_some_and(|profile| {
                                                self.declares_for_path(profile.family(), path, name)
                                            })
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

    /// Selects one surviving finite hierarchy identity, optionally within its originating pack.
    pub(crate) fn class_hierarchy_authority_for_path(
        &self,
        path: &str,
        base: &str,
        pack: Option<&str>,
    ) -> Option<(&str, &super::manifests::ClassHierarchyReceiverRule)> {
        let mut candidates = self.hierarchy_rows_for_path(path).filter(|(name, rule)| {
            pack.is_none_or(|pack| pack == *name)
                && rule.bases.iter().any(|candidate| candidate == base)
        });
        let first = candidates.next()?;
        candidates.next().is_none().then_some(first)
    }

    /// Distinct explicit return targets retain their originating effective pack identity.
    pub(crate) fn hierarchy_return_targets_for_path(&self, path: &str) -> BTreeSet<(&str, &str)> {
        self.hierarchy_rows_for_path(path)
            .flat_map(|(pack, row)| {
                row.returns.values().filter_map(move |policy| match policy {
                    super::manifests::ReturnPolicy::Receiver { base } => {
                        Some((pack, base.as_str()))
                    }
                    super::manifests::ReturnPolicy::SelfReceiver => None,
                })
            })
            .collect()
    }

    fn hierarchy_rows_for_path<'s, 'p>(
        &'s self,
        path: &'p str,
    ) -> impl Iterator<Item = (&'s str, &'s super::manifests::ClassHierarchyReceiverRule)> + 'p
    where
        's: 'p,
    {
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
        names
            .into_iter()
            .filter_map(move |name| {
                self.effective_framework_manifest(path, name)
                    .map(|manifest| (name, manifest))
            })
            .flat_map(|(name, manifest)| {
                manifest
                    .class_hierarchy_receivers
                    .iter()
                    .map(move |rule| (name, rule))
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

    #[cfg(feature = "lang-rust")]
    pub(crate) fn root_rust_crate_for_path(&self, path: &str) -> Option<&str> {
        let workspace = super::workspace_path(path).0;
        let name = self.rust_root_crates.get(workspace)?;
        if self.nearest_manifest_scope_for_path(LanguageFamily("rust"), path)? != workspace {
            return None;
        }
        Some(name)
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
    pub fn try_collect_with_scan_config(
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
    pub fn try_collect_with_paths(
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
        for (root, prefix, ignored_names) in roots {
            registry
                .workspaces
                .insert(prefix.trim_end_matches('/').to_owned());
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
                let Some(relative_path) = path.strip_prefix(&root).ok() else {
                    continue;
                };
                record_dependency_manifest(
                    &prefix,
                    relative_path,
                    &content,
                    &mut registry,
                    &mut snapshots,
                );
            }
            load_framework_manifests(
                &root,
                prefix.trim_end_matches('/'),
                &prefix,
                &mut registry,
                &mut snapshots,
            )?;
        }
        finish_registry(registry, snapshots)
    }

    fn validate_type_flow_policies(&self) -> Result<()> {
        let scopes: BTreeSet<&str> = std::iter::once("")
            .chain(
                self.framework_manifests
                    .keys()
                    .map(|(scope, _)| scope.as_str()),
            )
            .chain(
                self.scoped_packages
                    .values()
                    .flat_map(|scopes| scopes.keys().map(String::as_str)),
            )
            .collect();
        for scope in scopes {
            let path = if scope.is_empty() {
                "__type_flow_probe__".to_owned()
            } else {
                format!("{scope}/__type_flow_probe__")
            };
            let context = self.language_context_for_path(&path);
            anyhow::ensure!(
                context
                    .class_hierarchy_receivers
                    .iter()
                    .filter(|rule| !rule.returns.is_empty())
                    .count()
                    <= 128,
                "effective return-policy row bound exceeded"
            );
            for row in &context.class_hierarchy_receivers {
                row.validate_returns(4096)?;
            }
            let mut receiver_type_limits = BTreeMap::<&str, (usize, usize)>::new();
            for rule in &context.syntax_rules {
                let super::manifests::syntax::SyntaxOperation::ReceiverType { receiver } =
                    &rule.operation
                else {
                    continue;
                };
                let (clauses, literals) = receiver_type_limits
                    .entry(rule.language.as_str())
                    .or_default();
                *clauses += receiver.argument_factories.len();
                *literals += receiver
                    .argument_factories
                    .iter()
                    .map(|factory| factory.literals.len())
                    .sum::<usize>();
            }
            for (clauses, literals) in receiver_type_limits.values() {
                anyhow::ensure!(
                    *clauses <= 128 && *literals <= 4096,
                    "effective literal-factory bounds exceeded"
                );
            }
        }
        Ok(())
    }

    fn prepare_scoped_policies(&mut self) -> Result<()> {
        use super::manifests::{projections::NamedBindingForm, syntax::SyntaxOperation};
        for manifest in self
            .framework_manifests
            .values()
            .chain(self.standard_framework_manifests.values())
        {
            for rule in &manifest.syntax_rules {
                match &rule.operation {
                    SyntaxOperation::NamedBinding {
                        binding:
                            NamedBindingForm::CallReference {
                                provider_scopes, ..
                            },
                        ..
                    } => {
                        for provider in provider_scopes {
                            let workspace = if provider.workspace.is_empty() {
                                String::new()
                            } else {
                                format!("[{}]", provider.workspace)
                            };
                            anyhow::ensure!(
                                self.workspaces.contains(&workspace),
                                "unknown or disabled named provider workspace: {}",
                                provider.workspace
                            );
                            let scope = match (workspace.is_empty(), provider.project.is_empty()) {
                                (true, _) => provider.project.clone(),
                                (false, true) => workspace,
                                (false, false) => format!("{workspace}/{}", provider.project),
                            };
                            if self
                                .own_packages
                                .get(&scope)
                                .is_some_and(|packages| packages.contains(&provider.package))
                            {
                                self.named_provider_scopes.insert(provider.clone(), scope);
                            }
                        }
                    }
                    SyntaxOperation::HttpClientCall {
                        target_context: Some(context),
                        ..
                    } => {
                        for path in context.request_owners.iter().chain(&context.route_owners) {
                            anyhow::ensure!(
                                self.workspaces.contains(super::workspace_path(path).0),
                                "unknown or disabled HTTP context workspace: {path}"
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Resolves only explicit, already collected project/package identities.
    pub(crate) fn named_provider_scope(
        &self,
        provider: &super::manifests::projections::NamedProviderScope,
    ) -> Option<&str> {
        self.named_provider_scopes.get(provider).map(String::as_str)
    }

    /// Existing scoped policy relation used by both linking and selective hydration.
    pub fn named_scope_admits(&self, consumer: &str, provider: &str, domain: &str) -> bool {
        use super::manifests::{projections::NamedBindingForm, syntax::SyntaxOperation};
        let provider_scope = crate::engine::linker::named_binding_scope(provider, self);
        if crate::engine::linker::named_binding_scope(consumer, self) == provider_scope {
            return true;
        }
        self.language_context_for_path(consumer).syntax_rules.iter().any(|rule| {
            matches!(&rule.operation, SyntaxOperation::NamedBinding { domain: configured, binding: NamedBindingForm::CallReference { provider_scopes, .. } } if configured == domain && provider_scopes.iter().any(|scope| self.named_provider_scope(scope) == Some(provider_scope.as_str())))
        })
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
            for (name, extension, source) in STANDARD_FRAMEWORK_MANIFESTS {
                hasher.update((name.len() as u64).to_be_bytes());
                hasher.update(name.as_bytes());
                hasher.update((extension.len() as u64).to_be_bytes());
                hasher.update(extension.as_bytes());
                hasher.update((source.len() as u64).to_be_bytes());
                hasher.update(source.as_bytes());
            }
            hex::encode(hasher.finalize())
        })
    }

    /// Returns the root Cargo library name normalized for Rust import syntax.
    pub(crate) fn rust_library_for_path(&self, path: &str) -> Option<&str> {
        let workspace = super::workspace_path(path).0;
        if self.nearest_manifest_scope_for_path(LanguageFamily("rust"), path) != Some(workspace) {
            return None;
        }
        self.rust_libraries.get(workspace).map(String::as_str)
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
            .is_some_and(|packages| packages.contains(&package_identity(family, package)))
    }

    /// Returns a workspace-local manifest unconditionally, otherwise a bundled
    pub(crate) fn effective_framework_manifest(
        &self,
        path: &str,
        pack: &str,
    ) -> Option<&FrameworkManifest> {
        let workspace = super::workspace_path(path).0;
        self.framework_manifests
            .get(&(workspace.to_owned(), pack.to_owned()))
            .or_else(|| self.standard_framework_manifests.get(pack))
            .filter(|manifest| manifest.enabled)
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
        if self
            .framework_manifests
            .contains_key(&(workspace.to_owned(), framework.to_owned()))
            || self.declares_for_path(family, path, package)
        {
            self.effective_framework_manifest(path, framework)
        } else {
            None
        }
    }

    /// Resolves nearest manifest package scope for a given file path.
    pub fn nearest_manifest_scope_for_path(
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

fn record_dependency_manifest(
    prefix: &str,
    relative_path: &Path,
    content: &str,
    registry: &mut DependencyRegistry,
    snapshots: &mut ManifestSnapshots,
) {
    let filename = relative_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap();
    let directory = relative_path.parent().unwrap_or_else(|| Path::new(""));
    let origin = format!(
        "{}{}",
        prefix,
        relative_path.to_string_lossy().replace('\\', "/")
    );
    let mut scope = prefix.to_owned();
    scope.push_str(&directory.to_string_lossy().replace('\\', "/"));
    let scope = scope.trim_end_matches('/').to_owned();
    let profiles: Vec<_> = profiles()
        .filter(|profile| !profile.manifest_filenames().is_empty())
        .collect();
    let matches = |profile: &&dyn LanguageProfile| {
        profile
            .manifest_filenames()
            .iter()
            .any(|pattern| filename_matches(pattern, filename))
    };
    if filename == "package.json" {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(content) {
            let mut record = |name: &str, target: &str| {
                registry.manifest_entrypoints.push(ManifestEntrypoint {
                    origin: origin.clone(),
                    scope: scope.clone(),
                    name: name.to_owned(),
                    target: target.to_owned(),
                    python: false,
                });
            };
            if let Some(target) = value.get("main").and_then(serde_json::Value::as_str) {
                record("main", target);
            }
            if let Some(bin) = value.get("bin") {
                if let Some(target) = bin.as_str() {
                    record("bin:", target);
                } else if let Some(bins) = bin.as_object() {
                    for (name, target) in bins {
                        if let Some(target) = target.as_str() {
                            record(&format!("bin:{name}"), target);
                        }
                    }
                }
            }
            if let Some(name) = value.get("name").and_then(serde_json::Value::as_str) {
                registry
                    .own_packages
                    .entry(scope.clone())
                    .or_default()
                    .insert(name.to_owned());
            }
        }
    } else if filename == "pyproject.toml" {
        super::toml_manifest::visit_toml_pairs(content, &mut |key, value| {
            if let Some(name) = key.strip_prefix("project.scripts.") {
                if let Some(target) = super::toml_manifest::toml_string(value, content) {
                    registry.manifest_entrypoints.push(ManifestEntrypoint {
                        origin: origin.clone(),
                        scope: scope.clone(),
                        name: name.to_owned(),
                        target,
                        python: true,
                    });
                }
            }
            if matches!(key, "project.name" | "tool.poetry.name") {
                if let Some(name) = super::toml_manifest::toml_string(value, content) {
                    registry
                        .own_packages
                        .entry(scope.clone())
                        .or_default()
                        .insert(name);
                }
            }
        });
    }
    #[cfg(feature = "lang-rust")]
    if filename == "Cargo.toml" && scope == prefix.trim_end_matches('/') {
        let mut package_name = None;
        let mut library_name = None;
        let mut library_path = None;
        let mut invalid_library_path = false;
        let mut invalid_library_name = false;
        let mut invalid_package_name = false;
        let mut autolib = None;
        let mut autolib_present = false;
        let mut invalid_autolib = false;
        super::toml_manifest::visit_toml_pairs(content, &mut |key, value| match key {
            "package.name" => {
                package_name = super::toml_manifest::toml_string(value, content);
                invalid_package_name = package_name.is_none();
            }
            "lib.name" => {
                library_name = super::toml_manifest::toml_string(value, content);
                invalid_library_name = library_name.is_none();
            }
            "lib.path" => {
                library_path = super::toml_manifest::toml_string(value, content);
                invalid_library_path = library_path.is_none();
            }
            "package.autolib" => {
                autolib_present = true;
                autolib =
                    value
                        .utf8_text(content.as_bytes())
                        .ok()
                        .and_then(|text| match text.trim() {
                            "true" => Some(true),
                            "false" => Some(false),
                            _ => None,
                        });
                invalid_autolib = autolib.is_none();
            }
            _ => {}
        });
        let has_library_table = super::toml_manifest::has_table(content, "lib");
        let autolib_allows_library = match (autolib_present, autolib) {
            (false, _) | (true, Some(true)) => true,
            (true, Some(false)) => has_library_table,
            (true, None) => false,
        };
        let conventional_library_path = !invalid_library_path
            && library_path
                .as_deref()
                .is_none_or(|path| path == "src/lib.rs");
        if autolib_allows_library
            && !invalid_autolib
            && !invalid_library_name
            && !invalid_package_name
            && package_name.as_deref().is_some_and(|name| !name.is_empty())
            && conventional_library_path
        {
            if let Some(name) = library_name.or(package_name) {
                registry
                    .rust_libraries
                    .insert(scope.clone(), name.replace('-', "_"));
            }
        }
    }
    if filename == "package.json" {
        registry
            .javascript_packages
            .record_manifest(&scope, content);
    } else if filename == "pnpm-workspace.yaml" {
        registry
            .javascript_packages
            .record_pnpm_workspace(&scope, content);
    } else if filename.starts_with("tsconfig") && filename.ends_with(".json") {
        registry
            .typescript_paths
            .record_config(&scope, filename, content);
    }
    snapshots.insert(origin.clone(), Sha256::digest(content.as_bytes()));
    if filename == "pnpm-workspace.yaml"
        || (filename.starts_with("tsconfig") && filename.ends_with(".json"))
    {
        return;
    }
    let mut parsed_families = Vec::with_capacity(profiles.len());
    for profile in profiles.iter().copied().filter(|profile| matches(profile)) {
        let family = profile.family();
        if parsed_families.contains(&family) {
            continue;
        }
        parsed_families.push(family);
        #[cfg(feature = "lang-rust")]
        let dependencies = if filename == "Cargo.toml" && family == LanguageFamily("rust") {
            let (crate_name, dependencies) = super::rust::cargo_manifest_metadata(content);
            if directory.as_os_str().is_empty() {
                if let Some(crate_name) = crate_name {
                    registry.rust_root_crates.insert(scope.clone(), crate_name);
                }
            }
            dependencies
        } else {
            profile.extract_manifest_dependencies(filename, content)
        };
        #[cfg(not(feature = "lang-rust"))]
        let dependencies = profile.extract_manifest_dependencies(filename, content);
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

fn finish_registry(
    mut registry: DependencyRegistry,
    snapshots: ManifestSnapshots,
) -> Result<DependencyRegistry> {
    registry.javascript_packages.apply_pnpm_workspaces();
    registry.typescript_paths.resolve_all();
    registry.prepare_scoped_policies()?;
    let mut hasher = Sha256::new();
    hasher.update(b"forge-dependency-registry-v1\0");
    let standard_digest = DependencyRegistry::standard_manifests_digest_static();
    hasher.update((standard_digest.len() as u64).to_be_bytes());
    hasher.update(standard_digest.as_bytes());
    for (path, digest) in snapshots {
        hasher.update((path.len() as u64).to_be_bytes());
        hasher.update(path.as_bytes());
        hasher.update(digest);
    }
    registry.digest = hex::encode(hasher.finalize());
    registry.prepare_language_contexts();
    registry.validate_type_flow_policies()?;
    Ok(registry)
}

fn record_framework_manifest(
    workspace: &str,
    origin_prefix: &str,
    relative_path: &Path,
    content: &str,
    registry: &mut DependencyRegistry,
    snapshots: &mut ManifestSnapshots,
) -> Result<()> {
    let extension = relative_path
        .extension()
        .and_then(|value| value.to_str())
        .context("framework manifest extension missing")?;
    let name = relative_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .context("framework manifest filename must have a UTF-8 identifier")?
        .to_owned();
    let manifest = FrameworkManifest::parse(name.clone(), extension, content)?;
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
    let origin = format!(
        "{}{}",
        origin_prefix,
        relative_path.to_string_lossy().replace('\\', "/")
    );
    snapshots.insert(origin, Sha256::digest(content.as_bytes()));
    Ok(())
}

fn package_identity(family: LanguageFamily, name: &str) -> String {
    #[cfg(feature = "lang-python")]
    if family == LanguageFamily("python") {
        return super::python::dependencies::package(name).unwrap_or_else(|| name.to_owned());
    }
    let _ = family;
    name.to_owned()
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
        .require_git(false)
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

const STANDARD_FRAMEWORK_MANIFESTS: &[(&str, &str, &str)] =
    include!(concat!(env!("OUT_DIR"), "/framework_manifests.rs"));

static STANDARD_FRAMEWORK_MANIFESTS_DIGEST: std::sync::OnceLock<String> =
    std::sync::OnceLock::new();

static PARSED_STANDARD_FRAMEWORK_MANIFESTS: std::sync::OnceLock<
    std::result::Result<HashMap<String, FrameworkManifest>, String>,
> = std::sync::OnceLock::new();

fn load_standard_framework_manifests(registry: &mut DependencyRegistry) -> Result<()> {
    let manifests = PARSED_STANDARD_FRAMEWORK_MANIFESTS.get_or_init(|| {
        STANDARD_FRAMEWORK_MANIFESTS
            .iter()
            .map(|&(name, extension, source)| {
                FrameworkManifest::parse(name.to_owned(), extension, source)
                    .with_context(|| format!("invalid standard framework manifest {name}"))
                    .map(|manifest| (name.to_owned(), manifest))
                    .map_err(|error| format!("{error:#}"))
            })
            .collect()
    });
    registry.standard_framework_manifests = manifests
        .as_ref()
        .map_err(|error| anyhow::anyhow!("{error}"))?
        .clone();
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
            });
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
            });
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
        let Some("toml" | "yaml" | "yml") =
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
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .context("framework manifest filename must have a UTF-8 identifier")?;
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
        let relative_path = path.strip_prefix(root).with_context(|| {
            format!(
                "framework manifest {} escaped its workspace root {}",
                path.display(),
                root.display()
            )
        })?;
        record_framework_manifest(
            workspace,
            origin_prefix,
            relative_path,
            &content,
            registry,
            snapshots,
        )
        .with_context(|| format!("failed to load framework manifest {}", path.display()))?;
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
    matches!(
        filename,
        "Cargo.toml" | "pyproject.toml" | "package.json" | "pnpm-workspace.yaml" | "go.mod"
    ) || profiles().any(|profile| {
        profile
            .manifest_filenames()
            .iter()
            .any(|pattern| filename_matches(pattern, filename))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_registry(files: &[(&str, &str)]) -> Result<DependencyRegistry> {
        let manifests: Vec<_> = files
            .iter()
            .map(|&(path, source)| ManifestSource { path, source })
            .collect();
        DependencyRegistry::try_collect_from_sources([WorkspaceManifestSources {
            name: "",
            manifests: &manifests,
        }])
    }

    #[test]
    fn source_inventory_rejects_noncanonical_and_unsupported_paths() {
        for path in [
            "",
            "/package.json",
            "../package.json",
            "a/../package.json",
            "./package.json",
            "a//package.json",
            "a/./package.json",
            "a\\package.json",
            "[other]/package.json",
            "a/[other]/package.json",
            "C:/package.json",
            "package.json\0",
            "package.json?x",
            "unknown.txt",
            ".forge/frameworks/pack.json",
            ".forge/frameworks/nested/pack.toml",
        ] {
            assert!(source_registry(&[(path, "")]).is_err(), "{path:?}");
        }
        for name in ["../other", "[other]", "a/b", "two words", "", "other"] {
            let inventories = [
                WorkspaceManifestSources {
                    name,
                    manifests: &[],
                },
                WorkspaceManifestSources {
                    name,
                    manifests: &[],
                },
            ];
            assert!(
                DependencyRegistry::try_collect_from_sources(inventories).is_err(),
                "{name:?}"
            );
        }
    }

    #[test]
    fn source_inventory_rejects_duplicate_paths_and_pack_identities() {
        for files in [
            vec![("package.json", "{}"), ("package.json", "{}")],
            vec![
                (".forge/frameworks/local.toml", "enabled=false"),
                (".forge/frameworks/local.yaml", "enabled: false"),
            ],
        ] {
            assert!(source_registry(&files).is_err(), "{files:?}");
        }
    }

    #[test]
    fn source_inventory_enforces_framework_parser_and_byte_limit() {
        for (extension, source) in [
            ("toml", "extend=true\nreplace=true"),
            ("toml", "enabled='false'"),
            ("toml", "syntax_rules=["),
            ("yaml", "enabled: 0"),
            ("yml", "syntax_rules: ["),
            ("toml", "receivers=[]"),
        ] {
            assert!(
                source_registry(&[(&format!(".forge/frameworks/local.{extension}"), source)])
                    .is_err(),
                "{source}"
            );
        }
        for bytes in [4 * 1024 * 1024 - 1, 4 * 1024 * 1024, 4 * 1024 * 1024 + 1] {
            let source = format!("#{}", "x".repeat(bytes - 1));
            assert_eq!(
                source_registry(&[(".forge/frameworks/local.toml", &source)]).is_ok(),
                bytes <= 4 * 1024 * 1024,
                "{bytes}"
            );
        }
    }

    #[cfg(feature = "lang-python")]
    #[test]
    fn source_scopes_and_local_tombstones_preserve_bundle_activation() {
        let dependency = ("requirements.txt", "django\n");
        let bundled = source_registry(&[dependency, ("nested/requirements.txt", "")]).unwrap();
        let admits = |registry: &DependencyRegistry, path| {
            registry
                .framework_manifest_for_path(LanguageFamily("python"), path, "django", "django")
                .is_some()
        };
        assert!(admits(&bundled, "app.py"));
        assert!(!admits(&bundled, "nested/app.py"));
        for (extension, disabled, enabled) in [
            ("toml", "enabled=false", "enabled=true"),
            ("yaml", "enabled: false", "enabled: true"),
            ("yml", "enabled: false", "enabled: true"),
        ] {
            let path = format!(".forge/frameworks/django.{extension}");
            let disabled = source_registry(&[dependency, (&path, disabled)]).unwrap();
            assert!(!admits(&disabled, "app.py"));
            let restored = source_registry(&[dependency, (&path, enabled)]).unwrap();
            assert!(admits(&restored, "app.py"));
            assert_ne!(disabled.digest(), restored.digest());
            assert!(admits(&bundled, "app.py"));
            assert!(!admits(&disabled, "app.py"));
        }
        let empty = source_registry(&[]).unwrap();
        assert!(!admits(&empty, "app.py"));
        let local = source_registry(&[(".forge/frameworks/django.toml", "")]).unwrap();
        assert!(admits(&local, "app.py"));
        let deleted = source_registry(&[dependency]).unwrap();
        assert!(admits(&deleted, "app.py"));
    }

    #[cfg(feature = "lang-python")]
    #[test]
    fn source_local_rules_merge_and_replace_bundled_rules() {
        let local_rule = "[[route_calls]]\ncallee='LocalRoute'\nmethod='GET'\npath_argument=0\nhandler_argument=1\n";
        let registry = |mode: &str| {
            source_registry(&[
                ("requirements.txt", "django"),
                (
                    ".forge/frameworks/django.toml",
                    &format!("{mode}{local_rule}"),
                ),
            ])
            .unwrap()
        };
        let bundled = DependencyRegistry::collect(None);
        let bundled_count = bundled.standard_framework_manifests["django"]
            .route_calls
            .len();
        let merged = registry("extend=true\n");
        let replaced = registry("replace=true\n");
        assert_eq!(
            merged
                .effective_framework_manifest("app.py", "django")
                .unwrap()
                .route_calls
                .len(),
            bundled_count + 1
        );
        assert_eq!(
            replaced
                .effective_framework_manifest("app.py", "django")
                .unwrap()
                .route_calls
                .len(),
            1
        );
        assert_ne!(merged.digest(), replaced.digest());
    }

    #[cfg(feature = "lang-python")]
    #[test]
    fn source_registries_remain_independent_across_threads() {
        let mut manifest = "enabled=true".to_owned();
        let retained = source_registry(&[(".forge/frameworks/django.toml", &manifest)]).unwrap();
        manifest.replace_range(.., "enabled=false");
        let changed = source_registry(&[(".forge/frameworks/django.toml", &manifest)]).unwrap();
        assert!(changed
            .effective_framework_manifest("app.py", "django")
            .is_none());
        std::thread::scope(|scope| {
            let disabled = scope.spawn(|| {
                source_registry(&[(".forge/frameworks/django.toml", "enabled=false")]).unwrap()
            });
            let enabled = scope.spawn(|| {
                source_registry(&[(".forge/frameworks/django.toml", "enabled=true")]).unwrap()
            });
            assert!(disabled
                .join()
                .unwrap()
                .effective_framework_manifest("app.py", "django")
                .is_none());
            assert!(enabled
                .join()
                .unwrap()
                .effective_framework_manifest("app.py", "django")
                .is_some());
        });
        assert!(retained
            .effective_framework_manifest("app.py", "django")
            .is_some());
    }

    #[cfg(feature = "lang-python")]
    #[test]
    fn source_registry_digests_bind_paths_and_bytes_without_input_order() {
        let first = ("requirements.txt", "django\n");
        let second = ("nested/requirements.txt", "");
        let registry = source_registry(&[first, second]).unwrap();
        assert_eq!(
            registry.digest(),
            source_registry(&[second, first]).unwrap().digest()
        );
        for files in [
            vec![("requirements.txt", "django\n# changed bytes\n"), second],
            vec![("other/requirements.txt", "django\n"), second],
        ] {
            assert_ne!(registry.digest(), source_registry(&files).unwrap().digest());
        }
    }

    #[cfg(all(feature = "lang-python", feature = "lang-typescript"))]
    #[test]
    fn source_registry_validates_named_workspace_scope_before_rule_preparation() {
        let rule = serde_json::json!({"syntax_rules":[{"id":"named","language":"python","operation":{"kind":"named_binding","domain":"paint","binding":{"kind":"call_reference","callee":{"kind":"imported","module":"tablekit","export":"Column"},"key":{"kind":"keyword","name":"renderer"},"provider_scopes":[{"workspace":"frontend","project":"","package":"runtime"}]}}}]});
        let source = toml::to_string(&rule).unwrap();
        let main = [ManifestSource {
            path: ".forge/frameworks/local.toml",
            source: &source,
        }];
        let linked = [ManifestSource {
            path: "package.json",
            source: "{\"name\":\"runtime\"}",
        }];
        let inventory = |name| WorkspaceManifestSources {
            name,
            manifests: &linked,
        };
        let primary = || WorkspaceManifestSources {
            name: "",
            manifests: &main,
        };
        assert!(DependencyRegistry::try_collect_from_sources([primary()]).is_err());
        let registry =
            DependencyRegistry::try_collect_from_sources([primary(), inventory("frontend")])
                .unwrap();
        let provider = super::super::manifests::projections::NamedProviderScope {
            workspace: "frontend".into(),
            project: "".into(),
            package: "runtime".into(),
        };
        assert_eq!(registry.named_provider_scope(&provider), Some("[frontend]"));
        assert_eq!(
            registry.digest(),
            DependencyRegistry::try_collect_from_sources([inventory("frontend"), primary()])
                .unwrap()
                .digest()
        );
        assert!(registry
            .framework_manifest_for_path(
                LanguageFamily("python"),
                "[frontend]/app.py",
                "local",
                "tablekit"
            )
            .is_none());
    }

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
                "replace = true\n",
            )
            .unwrap();
            #[cfg(feature = "lang-rust")]
            {
                fs::create_dir_all(root.join("src")).unwrap();
                fs::create_dir_all(root.join("tests")).unwrap();
                fs::write(
                    root.join("Cargo.toml"),
                    "[package]\nname='commerce-fixture'\nversion='0.1.0'\n",
                )
                .unwrap();
                fs::write(root.join("src/lib.rs"), "pub mod api;\n").unwrap();
                fs::write(root.join("src/api.rs"), "pub fn route() {}\n").unwrap();
            }
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
        use super::super::manifests::{syntax::SyntaxOperation, FrameworkManifest};

        let mut documented_examples = Vec::new();
        for (extension, source) in [
            (
                "toml",
                include_str!(
                    "../../../../skills/contextunity-forge/references/frameworks/declarative-rules.toml.example"
                ),
            ),
            (
                "yaml",
                include_str!(
                    "../../../../skills/contextunity-forge/references/frameworks/declarative-rules.yaml.example"
                ),
            ),
        ] {
            let parsed = FrameworkManifest::parse("django".to_owned(), extension, source)
                .unwrap_or_else(|error| {
                    panic!("invalid documented {extension} example: {error:#}")
                });
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
            include_str!(
                "../../../../skills/contextunity-forge/references/frameworks/gridviewspec.toml.example"
            ),
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
                fs::write(&replacement, "replace = true\n").unwrap();
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
            let native_owner = &extracted
                .facts
                .nodes
                .iter()
                .find(|node| node.name == "page")
                .expect("native ordinary caller declaration survives replacement")
                .id;
            let native_consumers: Vec<_> = extracted
                .facts
                .semantic
                .entries()
                .flat_map(|(_, entry)| match &entry.state {
                    crate::core::typed_facts::SemanticState::Valid { facts, .. } => facts.iter(),
                    crate::core::typed_facts::SemanticState::Invalid { .. } => {
                        panic!("{phase}: canonical native semantic facts must remain valid")
                    }
                })
                .filter(|fact| {
                    matches!(
                        fact.role,
                        crate::core::semantic::SemanticFactRole::CallToFile { .. }
                    )
                })
                .collect();
            assert_eq!(
                native_consumers.len(),
                usize::from(enabled),
                "{phase}: native exact imported-call consumer fact"
            );
            assert!(
                native_consumers
                    .iter()
                    .all(|fact| &fact.occurrence.owner_id == native_owner),
                "{phase}: native actual caller declaration"
            );
            let extracted = serde_json::to_value(&extracted).unwrap();
            let nodes = extracted["nodes"].as_array().unwrap();
            let owner = nodes
                .iter()
                .find(|node| node["name"] == "page")
                .expect("ordinary caller declaration survives replacement");
            let consumers: Vec<_> = nodes
                .iter()
                .flat_map(|node| {
                    node["details"]
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
                    consumers[0]["occurrence"]["owner_id"], owner["id"],
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
        for block in include_str!("../../../../docs/reference/framework-manifests.md").split("```")
        {
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
                        r#"[[syntax_rules]]
 id = "project-template-symbols"
 language = "html"
  operation = { kind = "template_symbols", requires_loader = false, tags = ["project", "if"], filters = ["project", "upper"], libraries = [], library_routes = { project_route = "project_catalog" } }
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
                        r#"syntax_rules:
  - id: project-template-symbols
    language: html
    operation:
      kind: template_symbols
      requires_loader: false
      tags: [project, if]
      filters: [project, upper]
      libraries: []
      library_routes: { project_route: project_catalog }
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
                    for (claim, satisfied) in [
                        (
                            "project template rule is retained as typed data",
                            manifest.syntax_rules.iter().any(|rule| {
                                rule.id == "project-template-symbols"
                                    && matches!(
                                        &rule.operation,
                                        SyntaxOperation::TemplateSymbols { tags, filters, library_routes, .. }
                                            if tags.iter().any(|tag| tag == "project")
                                                && filters.iter().any(|filter| filter == "project")
                                                && library_routes.get("project_route").is_some_and(|library| library == "project_catalog")
                                    )
                            }),
                        ),
                        (
                            "bundled template tag follows merge mode",
                            manifest.syntax_rules.iter().any(|rule| matches!(
                                &rule.operation,
                                SyntaxOperation::TemplateSymbols { tags, .. }
                                    if tags.iter().any(|tag| tag == "block")
                            )) != replace,
                        ),
                        (
                            "bundled template filter follows merge mode",
                            manifest.syntax_rules.iter().any(|rule| matches!(
                                &rule.operation,
                                SyntaxOperation::TemplateSymbols { filters, .. }
                                    if filters.iter().any(|filter| filter == "lower")
                            )) != replace,
                        ),
                        (
                            "bundled library route follows merge mode",
                            manifest.syntax_rules.iter().any(|rule| matches!(
                                &rule.operation,
                                SyntaxOperation::TemplateSymbols { library_routes, .. }
                                    if library_routes.get("static").is_some_and(|library| library == "static")
                            )) != replace,
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
                        ("removed legacy section", "receivers = []\n"),
                        ("unknown section", "unknown = []\n"),
                        ("wrong rule shape", "semantic_rules = {}\n"),
                        ("conflicting modes", "extend = true\nreplace = true\n"),
                        ("wrong mode shape", "extend = \"true\"\n"),
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
                        let content = typed_rule.to_string();
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
            Some(manifest) if manifest.class_hierarchy_receivers.iter().any(|rule| rule.bases.iter().any(|base| base == "grid_view_spec.types.spec.GridViewSpec")) => {}
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
