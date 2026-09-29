use crate::core::models::{Facts, Node};
use crate::engine::linker::traits::{
    resolve_default_import, ImportContext, ImportResolution, LanguageLinker,
};
use crate::engine::languages::{self, LanguageFamily};
use hashbrown::{HashMap, HashSet};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(super) type PackageExports<'a> = HashMap<&'a str, HashMap<&'a str, Option<&'a Node>>>;

pub(crate) struct PythonLinker;

pub(crate) static PYTHON_LINKER: PythonLinker = PythonLinker;

impl LanguageLinker for PythonLinker {
    fn resolve_import<'ctx, 'a, 'input>(
        &self,
        context: &mut ImportContext<'ctx, 'a, 'input>,
    ) -> ImportResolution<'a> {
        let paired_stub = prefer_runtime_module(context.modules);
        let mut result = resolve_default_import(context);
        result.paired_stub = paired_stub;
        let Some(tail) = context.tail().map(str::to_owned) else {
            return result;
        };
        if tail.is_empty() {
            return result;
        }
        let Some(module) = context.modules.first().copied().filter(|_| context.modules.len() == 1) else {
            return result;
        };
        if context.candidates.is_empty() {
            let child_namespace = format!("{}.{}", context.selected_namespace, tail);
            let parent_workspace = languages::workspace_path(&module.path).0;
            context.candidates.extend(
                context
                    .modules_by_namespace
                    .get(&context.family)
                    .and_then(|namespaces| namespaces.get(&child_namespace))
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|node| {
                        languages::workspace_path(&node.path).0 == parent_workspace
                    }),
            );
            if let Some(stub) = prefer_runtime_module(context.candidates) {
                result.paired_stub = Some(stub);
            }
            result.child_module = context.candidates.len() == 1;
        }
        if context.candidates.is_empty() {
            if let Some(stub) = result.paired_stub {
                context.lookup_key.clear();
                context.lookup_key.push_str(&stub.qualname);
                context.lookup_key.push('.');
                context.lookup_key.push_str(&tail);
                let target = context.lookup_key.as_str();
                context.candidates.extend(
                    context
                        .by_module
                        .get(stub.path.as_str())
                        .into_iter()
                        .flatten()
                        .copied()
                        .filter(|node| {
                            node.kind != "component" && node.qualname == target
                        }),
                );
                result.stub_symbol = !context.candidates.is_empty();
            }
        }
        if context.candidates.is_empty() {
            if let Some(target) = resolve_export(context.package_exports, module, &tail) {
                context.candidates.push(target);
                result.reexport_symbol = true;
            }
        }
        result
    }

    fn package_exports<'a>(
        &self,
        all: &'a BTreeMap<String, Facts>,
        modules_by_namespace: &crate::engine::linker::traits::ModulesByNamespace<'a>,
        by_module: &HashMap<&'a str, Vec<&'a Node>>,
        root: Option<&Path>,
    ) -> crate::engine::linker::traits::PackageExports<'a> {
        package_exports(all, modules_by_namespace, by_module, root)
    }

    fn required_full_facts(
        &self,
        facts: &BTreeMap<String, Facts>,
        affected: &BTreeSet<String>,
        catalog: &[(&str, &str)],
    ) -> BTreeSet<String> {
        required_full_facts(facts, affected, catalog)
    }

    fn needs_reference_identity(&self, path: &str, facts: &Facts) -> bool {
        needs_reference_identity(path, facts)
    }

    fn reference_identity_changed(&self, path: &str, old: &Facts, new: &Facts) -> bool {
        reference_identity_changed(path, old, new)
    }

    fn is_declaration_only(&self, path: &str) -> bool {
        Path::new(path).extension().is_some_and(|extension| extension == "pyi")
    }

    fn tracks_external_aliases(&self) -> bool {
        true
    }

    fn resolve_imported_member<'a>(
        &self,
        module: &'a Node,
        member: &str,
        by_module: &HashMap<&'a str, Vec<&'a Node>>,
        by_qual: &HashMap<&'a str, Vec<&'a Node>>,
        lookup_key: &mut String,
    ) -> Vec<&'a Node> {
        if Path::new(&module.path).extension().is_none_or(|extension| extension != "py") {
            return Vec::new();
        }
        let stub_path = format!("{}i", module.path);
        let Some(stub) = by_module
            .get(stub_path.as_str())
            .into_iter()
            .flatten()
            .find(|node| node.kind == "module" && node.qualname == module.qualname)
        else {
            return Vec::new();
        };
        lookup_key.clear();
        lookup_key.push_str(&stub.qualname);
        lookup_key.push('.');
        lookup_key.push_str(member);
        let target = lookup_key.as_str();
        by_qual
            .get(target)
            .into_iter()
            .flatten()
            .copied()
            .filter(|node| node.path == stub.path)
            .collect()
    }
}

pub(super) fn needs_reference_identity(path: &str, facts: &Facts) -> bool {
    path.ends_with("/__init__.py")
        && facts
            .nodes
            .iter()
            .any(|node| node.kind == "module" && node.language == "python")
}

pub(super) fn reference_identity_changed(path: &str, old: &Facts, new: &Facts) -> bool {
    if !needs_reference_identity(path, new) {
        return false;
    }
    let imports = |facts: &Facts| {
        let mut entries: Vec<_> = facts
            .references
            .iter()
            .filter(|reference| {
                reference.kind == "imports"
                    && reference
                        .module
                        .as_deref()
                        .is_some_and(|module| module.starts_with('.'))
            })
            .map(|reference| {
                (
                    reference.source.clone(),
                    reference.expression.clone(),
                    reference.alias.clone(),
                    reference.module.clone(),
                )
            })
            .collect();
        entries.sort_unstable();
        entries
    };
    imports(old) != imports(new)
}

pub(super) fn required_full_facts(
    facts: &BTreeMap<String, Facts>,
    affected: &BTreeSet<String>,
    catalog: &[(&str, &str)],
) -> BTreeSet<String> {
    let Some(profile) = languages::by_id("python") else {
        return BTreeSet::new();
    };
    let mut requested = HashSet::new();
    for path in affected {
        let Some(file) = facts.get(path) else {
            continue;
        };
        if !file
            .nodes
            .iter()
            .any(|node| node.kind == "module" && node.language == "python")
        {
            continue;
        }
        for reference in file
            .references
            .iter()
            .filter(|reference| reference.kind == "imports")
        {
            if let Some(import) = reference
                .module
                .as_deref()
                .and_then(|module| profile.normalize_import(path, module))
            {
                requested.insert(import.namespace);
            }
        }
    }
    if requested.is_empty() {
        return BTreeSet::new();
    }
    catalog
        .iter()
        .filter_map(|(path, language)| {
            if *language != "python" || !path.ends_with("/__init__.py") {
                return None;
            }
            let (_, local_path) = languages::workspace_path(path);
            let local_name = profile.module_name(local_path);
            let src_name = local_path
                .split_once("/src/")
                .map(|(_, relative)| profile.module_name(relative))
                .or_else(|| {
                    local_path
                        .strip_prefix("src/")
                        .map(|relative| profile.module_name(relative))
                });
            (requested.contains(&local_name)
                || src_name
                    .as_ref()
                    .is_some_and(|name| requested.contains(name)))
            .then(|| (*path).to_owned())
        })
        .collect()
}

pub(super) fn resolve_export<'a>(
    exports: &PackageExports<'a>,
    module: &Node,
    name: &str,
) -> Option<&'a Node> {
    exports
        .get(module.path.as_str())?
        .get(name)
        .copied()
        .flatten()
}

pub(super) fn package_exports<'a>(
    all: &'a BTreeMap<String, Facts>,
    modules_by_namespace: &HashMap<LanguageFamily, HashMap<String, Vec<&'a Node>>>,
    by_module: &HashMap<&str, Vec<&'a Node>>,
    root: Option<&Path>,
) -> PackageExports<'a> {
    let mut exports = HashMap::new();
    let Some(profile) = languages::by_id("python") else {
        return exports;
    };
    for (path, facts) in all {
        if !path.ends_with("/__init__.py") {
            continue;
        }
        let Some(module) = facts
            .nodes
            .iter()
            .find(|node| node.kind == "module" && node.language == "python")
        else {
            continue;
        };
        for reference in facts
            .references
            .iter()
            .filter(|reference| reference.kind == "imports" && reference.source == module.id)
        {
            let (Some(alias), Some(import_path)) =
                (reference.alias.as_deref(), reference.module.as_deref())
            else {
                continue;
            };
            if !import_path.starts_with('.')
                || module.details["rebindings"]
                    .as_array()
                    .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(alias)))
            {
                continue;
            }
            let target = profile
                .normalize_import_with_root(root, path, import_path)
                .filter(|import| import.relative)
                .and_then(|import| {
                    modules_by_namespace
                        .get(&profile.family())
                        .and_then(|namespaces| namespaces.get(&import.namespace))
                })
                .map(|providers| {
                    providers
                        .iter()
                        .copied()
                        .filter(|provider| {
                            languages::workspace_path(&provider.path).0
                                == languages::workspace_path(path).0
                        })
                        .collect::<Vec<_>>()
                })
                .and_then(|mut providers| {
                    prefer_runtime_module(&mut providers);
                    let [provider] = providers.as_slice() else {
                        return None;
                    };
                    let target_name = format!("{}.{}", provider.qualname, reference.expression);
                    let mut candidates = by_module
                        .get(provider.path.as_str())
                        .into_iter()
                        .flatten()
                        .copied()
                        .filter(|node| node.qualname == target_name && node.kind != "component");
                    let candidate = candidates.next()?;
                    candidates.next().is_none().then_some(candidate)
                });
            exports
                .entry(path.as_str())
                .or_insert_with(HashMap::new)
                .entry(alias)
                .and_modify(|existing| *existing = None)
                .or_insert(target);
        }
    }
    exports
}

pub(super) fn prefer_runtime_module<'a>(modules: &mut Vec<&'a Node>) -> Option<&'a Node> {
    if modules.len() != 2 {
        return None;
    }
    let runtime = modules.iter().position(|node| node.path.ends_with(".py"));
    let stub = modules.iter().position(|node| node.path.ends_with(".pyi"));
    let (Some(runtime), Some(stub)) = (runtime, stub) else {
        return None;
    };
    let runtime_stem = modules[runtime].path.strip_suffix(".py");
    let stub_stem = modules[stub].path.strip_suffix(".pyi");
    if runtime_stem == stub_stem
        && languages::workspace_path(&modules[runtime].path).0
            == languages::workspace_path(&modules[stub].path).0
    {
        let stub_node = modules[stub];
        modules.swap(0, runtime);
        modules.truncate(1);
        return Some(stub_node);
    }
    None
}
