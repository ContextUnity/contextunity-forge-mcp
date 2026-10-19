use crate::{
    core::models::{Facts, Node, Reference},
    engine::{
        languages::{
            self, dependency_registry::DependencyRegistry, ImportPath, LanguageFamily,
            LanguageProfile,
        },
        linker::traits::{ImportContext, ModulesByNamespace, PackageExports},
    },
};
use hashbrown::{HashMap, HashSet};
use std::{collections::BTreeMap, path::Path};

pub(super) struct NamespaceIndexes<'a> {
    pub(super) profiles: HashMap<&'a str, &'static dyn LanguageProfile>,
    pub(super) nuxt_config_scopes: HashSet<&'a str>,
    pub(super) normalized_imports: HashMap<(&'a str, &'a str), Option<ImportPath>>,
    pub(super) modules_by_namespace: ModulesByNamespace<'a>,
}

impl<'a> NamespaceIndexes<'a> {
    pub(super) fn build<F: AsRef<Facts>>(
        all: &'a BTreeMap<String, F>,
        nodes: &[&'a Node],
        root: Option<&Path>,
        dependency_registry: &'a DependencyRegistry,
    ) -> Self {
        let profiles: HashMap<&str, &'static dyn LanguageProfile> = all
            .iter()
            .filter_map(|(path, facts)| {
                let facts = facts.as_ref();
                facts
                    .nodes
                    .iter()
                    .find(|node| node.kind == "module")
                    .and_then(|node| languages::by_id(&node.language))
                    .map(|profile| (path.as_str(), profile))
            })
            .collect();
        let indexed_javascript_modules: std::collections::HashSet<&str> = profiles
            .iter()
            .filter(|(_, profile)| profile.family() == LanguageFamily("javascript"))
            .map(|(path, _)| languages::module_stem(path).trim_end_matches("/index"))
            .collect();
        let nuxt_config_scopes: HashSet<&str> = profiles
            .keys()
            .filter_map(|path| {
                if *path == "nuxt.config.ts" {
                    Some("")
                } else {
                    path.strip_suffix("/nuxt.config.ts")
                }
            })
            .collect();
        let mut normalized_imports: HashMap<(&str, &str), Option<ImportPath>> = HashMap::new();
        for (path, facts) in all {
            let facts = facts.as_ref();
            let profile = profiles.get(path.as_str()).copied();
            #[cfg(feature = "lang-rust")]
            let root_rust_crate = profile
                .filter(|profile| profile.id() == "rust")
                .and_then(|_| dependency_registry.root_rust_crate_for_path(path))
                .filter(|_| {
                    let workspace = languages::workspace_path(path).0;
                    if workspace.is_empty() {
                        profiles.contains_key("src/lib.rs")
                    } else {
                        profiles.contains_key(format!("{workspace}/src/lib.rs").as_str())
                    }
                });
            for reference in facts
                .references
                .iter()
                .filter(|reference| reference.kind == "imports")
            {
                let module = reference.module.as_deref().unwrap_or(&reference.expression);
                normalized_imports
                    .entry((path.as_str(), module))
                    .or_insert_with(|| {
                        profile.and_then(|profile| {
                            #[cfg(feature = "lang-rust")]
                            if let Some(normalized) = root_rust_crate.and_then(|name| {
                                languages::rust::RUST.normalize_root_crate_import(name, module)
                            }) {
                                return Some(normalized);
                            }
                            dependency_registry
                                .normalize_javascript_import(
                                    profile.id(),
                                    path,
                                    module,
                                    &indexed_javascript_modules,
                                )
                                .or_else(|| profile.normalize_import_with_root(root, path, module))
                        })
                    });
            }
        }

        let mut modules_by_namespace: ModulesByNamespace<'a> = HashMap::new();
        for node in nodes.iter().filter(|node| node.kind == "module") {
            if let Some(profile) = profiles.get(node.path.as_str()) {
                let (workspace, local_path) = languages::workspace_path(&node.path);
                let local_module = profile.module_name(local_path);
                let mut namespaces = vec![node.qualname.clone()];
                if !workspace.is_empty() && node.qualname.starts_with(&format!("{workspace}.")) {
                    namespaces.push(node.qualname[workspace.len() + 1..].to_string());
                }
                if !namespaces.contains(&local_module) {
                    namespaces.push(local_module);
                }
                if profile.id() == "rust" && local_path == "src/lib.rs" {
                    namespaces.push("src".to_owned());
                }
                if let Some((_, source_relative)) = local_path.split_once("/src/") {
                    let source_module = profile.module_name(source_relative);
                    if !namespaces.contains(&source_module) {
                        namespaces.push(source_module);
                    }
                } else if let Some(source_relative) = local_path.strip_prefix("src/") {
                    let source_module = profile.module_name(source_relative);
                    if !namespaces.contains(&source_module) {
                        namespaces.push(source_module);
                    }
                }
                for namespace in namespaces {
                    let modules = modules_by_namespace
                        .entry(profile.family())
                        .or_default()
                        .entry(namespace)
                        .or_default();
                    if !modules.iter().any(|existing| existing.id == node.id) {
                        modules.push(node);
                    }
                }
            }
        }

        Self {
            profiles,
            nuxt_config_scopes,
            normalized_imports,
            modules_by_namespace,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn package_exports<'a>(
    profiles: &HashMap<&'a str, &'static dyn LanguageProfile>,
    modules_by_namespace: &'a ModulesByNamespace<'a>,
    by_module: &'a HashMap<&'a str, Vec<&'a Node>>,
    public_all: Option<&'a BTreeMap<String, Facts>>,
    borrowed: Option<&'a BTreeMap<String, &'a Facts>>,
    root: Option<&Path>,
) -> HashMap<&'static str, PackageExports<'a>> {
    let language_ids: HashSet<&'static str> =
        profiles.values().map(|profile| profile.id()).collect();
    let mut package_exports: HashMap<&'static str, PackageExports<'a>> = language_ids
        .into_iter()
        .map(|id| {
            (
                id,
                if let Some(public_all) = public_all {
                    languages::linker_for(id).package_exports(
                        public_all,
                        modules_by_namespace,
                        by_module,
                        root,
                    )
                } else {
                    languages::linker_for(id).package_exports_borrowed(
                        borrowed.expect("typed facts view exists"),
                        modules_by_namespace,
                        by_module,
                        root,
                    )
                },
            )
        })
        .collect();
    package_exports.insert("", HashMap::new());
    package_exports
}

pub(super) struct InheritanceIndexes<'a> {
    pub(super) python_receivers: languages::python_receivers::PythonReceivers<'a>,
    #[cfg(feature = "lang-rust")]
    pub(super) rust_members: languages::rust::linker::RustMembers<'a>,
}

impl<'a> InheritanceIndexes<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build<F: AsRef<Facts>>(
        all: &'a BTreeMap<String, F>,
        by_id: &HashMap<&'a str, &'a Node>,
        by_qual: &'a HashMap<&'a str, Vec<&'a Node>>,
        node_bindings: &HashMap<&'a str, HashSet<&'a str>>,
        node_rebindings: &HashMap<&'a str, HashSet<&'a str>>,
        by_module: &'a HashMap<&'a str, Vec<&'a Node>>,
        namespaces: &'a NamespaceIndexes<'a>,
        package_exports: &'a HashMap<&'static str, PackageExports<'a>>,
        dependency_registry: &'a DependencyRegistry,
    ) -> Self {
        let mut inheritance_imports: HashMap<(&str, &str, &str), Vec<&Reference>> = HashMap::new();
        for (path, facts) in all {
            let facts = facts.as_ref();
            if !facts
                .nodes
                .iter()
                .any(|node| node.language == "python" && node.kind == "class")
            {
                continue;
            }
            for import in facts
                .references
                .iter()
                .filter(|reference| reference.kind == "imports")
            {
                if let (Some(alias), Some(owner)) =
                    (import.alias.as_deref(), by_id.get(import.source.as_str()))
                {
                    inheritance_imports
                        .entry((path.as_str(), owner.qualname.as_str(), alias))
                        .or_default()
                        .push(import);
                }
            }
        }

        let lookup =
            |name: &str| -> &[&Node] { by_qual.get(name).map(Vec::as_slice).unwrap_or_default() };
        let python_receivers = languages::python_receivers::PythonReceivers::build(
            all,
            |class, reference| {
                let expression = reference.expression.as_str();
                let (head, tail) = expression.split_once('.').unwrap_or((expression, ""));
                let mut scope = class
                    .qualname
                    .rsplit_once('.')
                    .map_or("", |(parent, _)| parent);
                let mut key = String::new();
                while !scope.is_empty() {
                    let imports = inheritance_imports
                        .get(&(class.path.as_str(), scope, head))
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    if by_qual.get(scope).is_some_and(|nodes| {
                        nodes.iter().any(|node| {
                            node.path == class.path
                                && node_rebindings
                                    .get(node.id.as_str())
                                    .is_some_and(|bindings| bindings.contains(head))
                        })
                    }) {
                        return languages::python_receivers::Base::Unknown;
                    }
                    let local: Vec<_> = lookup(super::qualified(&mut key, scope, expression))
                        .iter()
                        .copied()
                        .filter(|node| node.path == class.path && node.kind == "class")
                        .collect();
                    if local.len() == 1 {
                        if imports
                            .iter()
                            .any(|import| import.line > local[0].line && import.line <= class.line)
                        {
                            return languages::python_receivers::Base::Unknown;
                        }
                        return languages::python_receivers::Base::Local(&local[0].id);
                    }
                    if local.len() > 1 {
                        return languages::python_receivers::Base::Unknown;
                    }
                    if !imports.is_empty() {
                        if imports.len() != 1 {
                            return languages::python_receivers::Base::Unknown;
                        }
                        let import = imports[0];
                        if import.line > class.line
                            || by_qual.get(scope).is_some_and(|nodes| {
                                nodes.iter().any(|node| {
                                    node.path == class.path
                                        && node_rebindings
                                            .get(node.id.as_str())
                                            .is_some_and(|bindings| bindings.contains(head))
                                })
                            })
                        {
                            return languages::python_receivers::Base::Unknown;
                        }
                        let Some(profile) = namespaces.profiles.get(class.path.as_str()).copied()
                        else {
                            return languages::python_receivers::Base::Unknown;
                        };
                        let normalized = namespaces
                            .normalized_imports
                            .get(&(
                                class.path.as_str(),
                                import.module.as_deref().unwrap_or(&import.expression),
                            ))
                            .and_then(Option::as_ref);
                        let Some(normalized) = normalized else {
                            return languages::python_receivers::Base::Unknown;
                        };
                        let mut modules: Vec<_> = namespaces
                            .modules_by_namespace
                            .get(&profile.family())
                            .and_then(|all_namespaces| all_namespaces.get(&normalized.namespace))
                            .into_iter()
                            .flatten()
                            .copied()
                            .collect();
                        let workspace = languages::workspace_path(&class.path).0;
                        if modules
                            .iter()
                            .any(|module| languages::workspace_path(&module.path).0 == workspace)
                        {
                            modules.retain(|module| {
                                languages::workspace_path(&module.path).0 == workspace
                            });
                        } else if normalized.relative {
                            modules.clear();
                        }
                        let mut targets = Vec::new();
                        languages::linker_for(profile.id()).resolve_import(&mut ImportContext {
                            path: &class.path,
                            dependencies: dependency_registry,
                            reference: import,
                            normalized: Some(normalized),
                            selected_namespace: &normalized.namespace,
                            family: profile.family(),
                            modules: &mut modules,
                            candidates: &mut targets,
                            modules_by_namespace: &namespaces.modules_by_namespace,
                            by_module,
                            by_qual,
                            package_exports: &package_exports[profile.id()],
                            lookup_key: &mut key,
                        });
                        if !tail.is_empty() {
                            targets = targets
                                .iter()
                                .flat_map(|target| {
                                    lookup(&format!("{}.{}", target.qualname, tail))
                                        .iter()
                                        .copied()
                                        .filter(move |node| node.path == target.path)
                                })
                                .collect();
                        }
                        targets.retain(|target| target.kind == "class");
                        targets.sort_by_key(|target| &target.id);
                        targets.dedup_by_key(|target| &target.id);
                        if targets.len() == 1 {
                            return languages::python_receivers::Base::Local(&targets[0].id);
                        }
                        if targets.is_empty() && modules.is_empty() && !normalized.relative {
                            let module = import.module.as_deref().unwrap_or(&import.expression);
                            let mut canonical = module.to_owned();
                            if import.expression != module {
                                canonical.push('.');
                                canonical.push_str(&import.expression);
                            }
                            if !tail.is_empty() {
                                canonical.push('.');
                                canonical.push_str(tail);
                            }
                            return languages::python_receivers::Base::External(
                                canonical,
                                module,
                                import.line,
                            );
                        }
                        return languages::python_receivers::Base::Unknown;
                    }
                    if by_qual.get(scope).is_some_and(|nodes| {
                        nodes.iter().any(|node| {
                            node.path == class.path
                                && node_bindings
                                    .get(node.id.as_str())
                                    .is_some_and(|bindings| bindings.contains(head))
                        })
                    }) {
                        return languages::python_receivers::Base::Unknown;
                    }
                    scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
                }
                if expression == "object" {
                    languages::python_receivers::Base::External(
                        "builtins.object".into(),
                        "builtins",
                        0,
                    )
                } else {
                    languages::python_receivers::Base::Unknown
                }
            },
            |path, base| {
                let rule = dependency_registry.class_hierarchy_receiver_for_path(path, base)?;
                Some((rule.provider.as_str(), rule.members.as_slice()))
            },
        );
        drop(inheritance_imports);

        Self {
            python_receivers,
            #[cfg(feature = "lang-rust")]
            rust_members: languages::rust::linker::RustMembers::build(all),
        }
    }
}
