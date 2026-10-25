use super::interner::{LinkerInterner, SymbolId};
use crate::{
    core::models::{Facts, Node, Reference},
    engine::{
        languages::{
            self, ImportPath, LanguageFamily, LanguageProfile,
            dependency_registry::DependencyRegistry,
        },
        linker::traits::{ImportContext, ModulesByNamespace, PackageExports},
    },
};
use hashbrown::{HashMap, HashSet};
use std::{collections::BTreeMap, path::Path};

pub(super) struct NamespaceIndexes<'a> {
    pub(super) profiles: HashMap<&'a str, &'static dyn LanguageProfile>,
    pub(super) normalized_imports: HashMap<(&'a str, &'a str), Option<ImportPath>>,
    pub(super) modules_by_namespace: ModulesByNamespace<'a>,
    #[cfg(feature = "lang-rust")]
    pub(super) rust_forwarding: languages::rust::linker::RustForwarding<'a>,
}

impl<'a> NamespaceIndexes<'a> {
    pub(super) fn build<F: AsRef<Facts>>(
        all: &'a BTreeMap<String, F>,
        nodes: &[&'a Node],
        root: Option<&Path>,
        dependency_registry: &'a DependencyRegistry,
        interner: &mut LinkerInterner<'a>,
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
        let rust_library_modules: HashMap<&str, (&str, &Node)> = all
            .iter()
            .filter_map(|(path, facts)| {
                let (workspace, local_path) = languages::workspace_path(path);
                if local_path != "src/lib.rs" {
                    return None;
                }
                let module = facts
                    .as_ref()
                    .nodes
                    .iter()
                    .find(|node| node.kind == "module" && node.language == "rust")?;
                let name = dependency_registry.rust_library_for_path(path)?;
                Some((workspace, (name, module)))
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
                        if profile.is_some_and(|profile| profile.id() == "rust") {
                            if let Some((library, _root_module)) =
                                rust_library_modules.get(languages::workspace_path(path).0)
                            {
                                let (first, tail) = module.split_once("::").unwrap_or((module, ""));
                                if first == *library {
                                    if dependency_registry.rust_library_for_path(path)
                                        != Some(*library)
                                    {
                                        return None;
                                    }
                                    let namespace = if tail.is_empty() {
                                        "src".to_owned()
                                    } else {
                                        format!("src.{}", tail.replace("::", "."))
                                    };
                                    return Some(ImportPath {
                                        namespace,
                                        relative: true,
                                        symbol_path: true,
                                    });
                                }
                            }
                        }
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
                #[cfg(feature = "lang-rust")]
                if local_path == "src/lib.rs"
                    && rust_library_modules
                        .get(workspace)
                        .is_some_and(|(_, root_module)| root_module.id == node.id)
                {
                    namespaces.push("src".to_owned());
                }
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

        #[cfg(feature = "lang-rust")]
        let rust_forwarding = languages::rust::linker::RustForwarding::build(
            all,
            &modules_by_namespace,
            &normalized_imports,
            interner,
        );

        Self {
            profiles,
            normalized_imports,
            modules_by_namespace,
            #[cfg(feature = "lang-rust")]
            rust_forwarding,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn package_exports<'index, 'a>(
    profiles: &'index HashMap<&'a str, &'static dyn LanguageProfile>,
    modules_by_namespace: &'index ModulesByNamespace<'a>,
    by_module: &'index HashMap<&'a str, Vec<&'a Node>>,
    node_rebindings: &'index HashMap<SymbolId, HashSet<SymbolId>>,
    public_all: Option<&'a BTreeMap<String, Facts>>,
    borrowed: Option<&'index BTreeMap<String, &'a Facts>>,
    root: Option<&Path>,
    interner: &mut LinkerInterner<'a>,
    #[cfg(feature = "lang-rust")] rust_forwarding: &'index languages::rust::linker::RustForwarding<
        'a,
    >,
) -> HashMap<&'static str, PackageExports<'a>> {
    let mut language_ids: HashSet<&'static str> =
        profiles.values().map(|profile| profile.id()).collect();
    #[cfg(feature = "lang-rust")]
    language_ids.remove("rust");
    let mut package_exports: HashMap<&'static str, PackageExports<'a>> = language_ids
        .into_iter()
        .map(|id| {
            (
                id,
                if let Some(public_all) = public_all {
                    languages::linker_for(id).package_exports(
                        interner,
                        node_rebindings,
                        public_all,
                        modules_by_namespace,
                        by_module,
                        root,
                    )
                } else {
                    languages::linker_for(id).package_exports_borrowed(
                        interner,
                        node_rebindings,
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
    #[cfg(feature = "lang-rust")]
    package_exports.insert("rust", rust_forwarding.package_exports().clone());
    package_exports
}

pub(super) struct InheritanceIndexes<'a> {
    pub(super) python_receivers: super::python::receivers::PythonReceivers<'a>,
    #[cfg(feature = "lang-rust")]
    pub(super) rust_members: languages::rust::linker::RustMembers<'a>,
}

impl<'a> InheritanceIndexes<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build<'index, F: AsRef<Facts>>(
        all: &'a BTreeMap<String, F>,
        interner: &LinkerInterner<'a>,
        #[cfg(feature = "lang-rust")] flows: Option<
            &'index HashMap<SymbolId, &'a crate::core::typed_facts::FlowState>,
        >,
        by_id: &'index HashMap<SymbolId, &'a Node>,
        by_qual: &'index HashMap<&'a str, Vec<&'a Node>>,
        node_bindings: &'index HashMap<SymbolId, HashSet<SymbolId>>,
        node_rebindings: &'index HashMap<SymbolId, HashSet<SymbolId>>,
        by_module: &'index HashMap<&'a str, Vec<&'a Node>>,
        namespaces: &'index NamespaceIndexes<'a>,
        package_exports: &'index HashMap<&'static str, PackageExports<'a>>,
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
                if let (Some(alias), Some(owner)) = (
                    import.alias.as_deref(),
                    interner.lookup_node(by_id, import.source.as_str()),
                ) {
                    inheritance_imports
                        .entry((path.as_str(), owner.qualname.as_str(), alias))
                        .or_default()
                        .push(import);
                }
            }
        }

        let python_receivers = super::python::receivers::PythonReceivers::build(
            all,
            |class, reference| {
                let expression = reference.expression.as_str();
                let (head, tail) = expression.split_once('.').unwrap_or((expression, ""));
                let Some(head_id) = interner.lookup_symbol(head) else {
                    return super::python::receivers::Base::Unknown;
                };
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
                                && interner
                                    .lookup_symbol(&node.id)
                                    .and_then(|node_id| node_rebindings.get(&node_id))
                                    .is_some_and(|bindings| bindings.contains(&head_id))
                        })
                    }) {
                        return super::python::receivers::Base::Unknown;
                    }
                    let local: Vec<_> =
                        nodes_by_qualname(by_qual, super::qualified(&mut key, scope, expression))
                            .filter(|node| node.path == class.path && node.kind == "class")
                            .collect();
                    if local.len() == 1 {
                        if imports
                            .iter()
                            .any(|import| import.line > local[0].line && import.line <= class.line)
                        {
                            return super::python::receivers::Base::Unknown;
                        }
                        return super::python::receivers::Base::Local(&local[0].id);
                    }
                    if local.len() > 1 {
                        return super::python::receivers::Base::Unknown;
                    }
                    if !imports.is_empty() {
                        if imports.len() != 1 {
                            return super::python::receivers::Base::Unknown;
                        }
                        let import = imports[0];
                        if import.line > class.line
                            || by_qual.get(scope).is_some_and(|nodes| {
                                nodes.iter().any(|node| {
                                    node.path == class.path
                                        && interner
                                            .lookup_symbol(&node.id)
                                            .and_then(|node_id| node_rebindings.get(&node_id))
                                            .is_some_and(|bindings| bindings.contains(&head_id))
                                })
                            })
                        {
                            return super::python::receivers::Base::Unknown;
                        }
                        let Some(profile) = namespaces.profiles.get(class.path.as_str()).copied()
                        else {
                            return super::python::receivers::Base::Unknown;
                        };
                        let normalized = namespaces
                            .normalized_imports
                            .get(&(
                                class.path.as_str(),
                                import.module.as_deref().unwrap_or(&import.expression),
                            ))
                            .and_then(Option::as_ref);
                        let Some(normalized) = normalized else {
                            return super::python::receivers::Base::Unknown;
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
                            interner,
                            dependencies: dependency_registry,
                            reference: import,
                            #[cfg(feature = "lang-rust")]
                            rust_forwarding: Some(&namespaces.rust_forwarding),
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
                                    nodes_by_qualname(
                                        by_qual,
                                        &format!("{}.{}", target.qualname, tail),
                                    )
                                    .filter(move |node| node.path == target.path)
                                })
                                .collect();
                        }
                        targets.retain(|target| target.kind == "class");
                        targets.sort_by_key(|target| &target.id);
                        targets.dedup_by_key(|target| &target.id);
                        if targets.len() == 1 {
                            return super::python::receivers::Base::Local(&targets[0].id);
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
                            return super::python::receivers::Base::External(
                                canonical,
                                module,
                                import.line,
                            );
                        }
                        return super::python::receivers::Base::Unknown;
                    }
                    if by_qual.get(scope).is_some_and(|nodes| {
                        nodes.iter().any(|node| {
                            node.path == class.path
                                && interner
                                    .lookup_symbol(&node.id)
                                    .and_then(|node_id| node_bindings.get(&node_id))
                                    .is_some_and(|bindings| bindings.contains(&head_id))
                        })
                    }) {
                        return super::python::receivers::Base::Unknown;
                    }
                    scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
                }
                if expression == "object" {
                    super::python::receivers::Base::External(
                        "builtins.object".into(),
                        "builtins",
                        0,
                    )
                } else {
                    super::python::receivers::Base::Unknown
                }
            },
            |path, base| {
                let (pack, rule) =
                    dependency_registry.class_hierarchy_authority_for_path(path, base, None)?;
                Some((rule.provider.as_str(), pack, rule))
            },
        );
        drop(inheritance_imports);

        Self {
            python_receivers,
            #[cfg(feature = "lang-rust")]
            rust_members: languages::rust::linker::RustMembers::build(
                all,
                interner,
                flows,
                &namespaces.rust_forwarding,
            ),
        }
    }
}

fn nodes_by_qualname<'index, 'node>(
    by_qual: &'index HashMap<&'node str, Vec<&'node Node>>,
    qualname: &str,
) -> impl Iterator<Item = &'node Node> + 'index {
    by_qual.get(qualname).into_iter().flatten().copied()
}
