use crate::core::models::{Facts, Node};
use crate::engine::languages::{self, LanguageFamily};
use crate::engine::linker::traits::{
    resolve_default_import, ImportContext, ImportResolution, LanguageLinker,
};
use hashbrown::{HashMap, HashSet};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(super) type PackageExports<'a> = HashMap<&'a str, HashMap<&'a str, Option<&'a Node>>>;

pub(crate) struct PythonLinker;

pub(crate) static PYTHON_LINKER: PythonLinker = PythonLinker;

fn prefer_live_declaration(candidates: &mut Vec<&Node>) -> bool {
    let latest_assignment = candidates
        .iter()
        .filter(|node| node.kind == "variable")
        .map(|node| node.line)
        .max();
    let latest_declaration = candidates
        .iter()
        .filter(|node| node.kind != "variable")
        .map(|node| node.line)
        .max();
    if let (Some(assignment), Some(declaration)) = (latest_assignment, latest_declaration) {
        if assignment > declaration {
            candidates.clear();
            return true;
        } else {
            candidates.retain(|node| node.kind != "variable");
        }
    }
    false
}

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
        if context.modules.is_empty() {
            let Some(normalized) = context.normalized else {
                return result;
            };
            let child_namespace = format!("{}.{}", normalized.namespace, tail);
            let workspace = languages::workspace_path(context.path).0;
            context.modules.extend(
                context
                    .modules_by_namespace
                    .get(&context.family)
                    .and_then(|namespaces| namespaces.get(&child_namespace))
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|node| languages::workspace_path(&node.path).0 == workspace),
            );
            if let Some(stub) = prefer_runtime_module(context.modules) {
                result.paired_stub = Some(stub);
            }
            context.candidates.extend(context.modules.iter().copied());
            result.child_module = context.candidates.len() == 1;
            return result;
        }
        let Some(module) = context
            .modules
            .first()
            .copied()
            .filter(|_| context.modules.len() == 1)
        else {
            return result;
        };
        let export = context
            .package_exports
            .get(module.path.as_str())
            .and_then(|exports| exports.get(tail.as_str()));
        if let Some(export) = export {
            if context.candidates.iter().all(|candidate| candidate.kind == "variable") {
                context.candidates.clear();
                if let Some(target) = export {
                    context.candidates.push(target);
                    result.reexport_symbol = true;
                }
                return result;
            }
        }
        let blocked_assignment = prefer_live_declaration(context.candidates);
        if blocked_assignment {
            return result;
        }
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
                    .filter(|node| languages::workspace_path(&node.path).0 == parent_workspace),
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
                        .filter(|node| node.kind != "component" && node.qualname == target),
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

    fn package_exports_borrowed<'a>(
        &self,
        all: &'a BTreeMap<String, &'a Facts>,
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

    fn required_full_facts_borrowed(
        &self,
        facts: &BTreeMap<String, &Facts>,
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
        Path::new(path)
            .extension()
            .is_some_and(|extension| extension == "pyi")
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
        exports: &crate::engine::linker::traits::PackageExports<'a>,
        lookup_key: &mut String,
    ) -> Vec<&'a Node> {
        if Path::new(&module.path)
            .extension()
            .is_none_or(|extension| extension != "py")
        {
            return Vec::new();
        }
        let stub_path = format!("{}i", module.path);
        let Some(stub) = by_module
            .get(stub_path.as_str())
            .into_iter()
            .flatten()
            .find(|node| node.kind == "module" && node.qualname == module.qualname)
        else {
            return resolve_export(exports, module, member)
                .into_iter()
                .collect();
        };
        lookup_key.clear();
        lookup_key.push_str(&stub.qualname);
        lookup_key.push('.');
        lookup_key.push_str(member);
        let target = lookup_key.as_str();
        let mut candidates: Vec<_> = by_qual
            .get(target)
            .into_iter()
            .flatten()
            .copied()
            .filter(|node| node.path == stub.path)
            .collect();
        if candidates.is_empty() {
            candidates.extend(resolve_export(exports, module, member));
        }
        candidates
    }
}

pub(super) fn needs_reference_identity(_path: &str, facts: &Facts) -> bool {
    facts
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
            .filter(|reference| reference.kind == "imports")
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

pub(super) fn required_full_facts<F: AsRef<Facts>>(
    facts: &BTreeMap<String, F>,
    affected: &BTreeSet<String>,
    catalog: &[(&str, &str)],
) -> BTreeSet<String> {
    let Some(profile) = languages::by_id("python") else {
        return BTreeSet::new();
    };
    let mut requested = HashMap::<&str, HashSet<String>>::new();
    let mut cross_workspace = HashSet::<String>::new();
    for (path, file) in facts {
        let file = file.as_ref();
        let Some(module) = file
            .nodes
            .iter()
            .find(|node| node.kind == "module" && node.language == "python")
        else {
            continue;
        };
        let workspace = languages::workspace_path(path).0;
        for reference in file.references.iter().filter(|reference| {
            reference.kind == "imports"
                && (affected.contains(path) || reference.source == module.id)
        }) {
            if let Some(import) = reference
                .module
                .as_deref()
                .and_then(|name| profile.normalize_import(path, name))
            {
                let names = requested.entry(workspace).or_default();
                if reference.expression != "*"
                    && reference.module.as_deref() != Some(reference.expression.as_str())
                {
                    names.insert(format!("{}.{}", import.namespace, reference.expression));
                }
                if !import.relative && import.namespace.contains('.') {
                    cross_workspace.insert(import.namespace.clone());
                    if reference.expression != "*"
                        && reference.module.as_deref() != Some(reference.expression.as_str())
                    {
                        cross_workspace
                            .insert(format!("{}.{}", import.namespace, reference.expression));
                    }
                }
                names.insert(import.namespace);
            }
        }
        for export in module.details["lazy_exports"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(import) = lazy_import_path(module, export)
                .and_then(|name| profile.normalize_import(path, &name))
            {
                if !import.relative
                    && export["relative_to_module"].as_bool() != Some(true)
                    && import.namespace.contains('.')
                {
                    cross_workspace.insert(import.namespace.clone());
                }
                requested
                    .entry(workspace)
                    .or_default()
                    .insert(import.namespace);
            }
        }
    }
    catalog
        .iter()
        .filter_map(|(path, language)| {
            if *language != "python" {
                return None;
            }
            let (workspace, local_path) = languages::workspace_path(path);
            let names = requested.get(workspace);
            let matches = |name: &str| {
                names.is_some_and(|names| names.contains(name)) || cross_workspace.contains(name)
            };
            let full_name = profile.module_name(path);
            let local_name = profile.module_name(local_path);
            let src_name = local_path
                .split_once("/src/")
                .map(|(_, path)| profile.module_name(path))
                .or_else(|| {
                    local_path
                        .strip_prefix("src/")
                        .map(|path| profile.module_name(path))
                });
            (matches(&full_name)
                || matches(&local_name)
                || src_name.as_ref().is_some_and(|name| matches(name)))
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

fn lazy_import_path(module: &Node, export: &serde_json::Value) -> Option<String> {
    let path = export["module"].as_str()?;
    if export["relative_to_module"].as_bool() != Some(true) || !path.starts_with('.') {
        return Some(path.to_owned());
    }
    let levels = path.bytes().take_while(|byte| *byte == b'.').count();
    let mut namespace = module.qualname.as_str();
    for _ in 1..levels {
        namespace = namespace.rsplit_once('.')?.0;
    }
    let tail = &path[levels..];
    Some(if tail.is_empty() {
        namespace.to_owned()
    } else {
        format!("{namespace}.{tail}")
    })
}

fn export_provider<'a>(
    path: &str,
    module_path: &str,
    modules: &HashMap<LanguageFamily, HashMap<String, Vec<&'a Node>>>,
    root: Option<&Path>,
) -> Option<&'a Node> {
    let profile = languages::by_id("python")?;
    let import = profile.normalize_import_with_root(root, path, module_path)?;
    let providers = modules.get(&profile.family())?.get(&import.namespace)?;
    let workspace = languages::workspace_path(path).0;
    let mut selected: Vec<_> = providers
        .iter()
        .copied()
        .filter(|provider| languages::workspace_path(&provider.path).0 == workspace)
        .collect();
    if selected.is_empty() && !import.relative && import.namespace.contains('.') {
        selected.extend(
            providers
                .iter()
                .copied()
                .filter(|provider| languages::workspace_path(&provider.path).0 != workspace),
        );
    }
    prefer_runtime_module(&mut selected);
    match selected.as_slice() {
        [provider] => Some(*provider),
        _ => None,
    }
}

struct ExportBinding<'a> {
    owner: &'a str,
    name: &'a str,
    provider: Option<&'a Node>,
    member: Option<&'a str>,
}

fn register_export<'a>(
    exports: &mut PackageExports<'a>,
    pending: &mut Vec<(&'a str, &'a str, &'a Node, &'a str)>,
    seen: &mut HashSet<(&'a str, &'a str)>,
    binding: ExportBinding<'a>,
    by_module: &HashMap<&str, Vec<&'a Node>>,
) {
    let ExportBinding {
        owner,
        name,
        provider,
        member,
    } = binding;
    if !seen.insert((owner, name)) {
        exports.entry(owner).or_default().insert(name, None);
        pending.retain(|(path, alias, _, _)| *path != owner || *alias != name);
        return;
    }
    let Some(provider) = provider else {
        return;
    };
    let target = exports
        .entry(owner)
        .or_default()
        .entry(name)
        .or_insert(None);
    let Some(member) = member else {
        *target = Some(provider);
        return;
    };
    let qualified = format!("{}.{}", provider.qualname, member);
    let mut candidates: Vec<&Node> = by_module
        .get(provider.path.as_str())
        .into_iter()
        .flatten()
        .copied()
        .filter(|node| node.kind != "component" && node.qualname == qualified)
        .collect();
    let _ = prefer_live_declaration(&mut candidates);
    if candidates.len() > 1 {
        let non_stubs: Vec<&Node> = candidates
            .iter()
            .copied()
            .filter(|n| {
                let is_stub = n.details.get("is_stub").and_then(|v| v.as_bool()).unwrap_or(false);
                let is_overload = n.details.get("is_overload").and_then(|v| v.as_bool()).unwrap_or(false);
                !is_stub && !is_overload
            })
            .collect();
        if !non_stubs.is_empty() && non_stubs.len() < candidates.len() {
            candidates = non_stubs;
        }
    }
    if candidates.len() == 1
        && candidates[0].kind == "variable"
        && provider.details["exports"]
            .as_array()
            .is_some_and(|items| items.iter().any(|export| {
                export["name"].as_str() == Some(member)
                    && export["local"].as_str().is_some()
            }))
    {
        pending.push((owner, name, provider, member));
        return;
    }
    if candidates.len() == 1 {
        *target = Some(candidates[0]);
    } else if candidates.is_empty() {
        pending.push((owner, name, provider, member));
    }
}

pub(super) fn package_exports<'a, F: AsRef<Facts>>(
    all: &'a BTreeMap<String, F>,
    modules_by_namespace: &HashMap<LanguageFamily, HashMap<String, Vec<&'a Node>>>,
    by_module: &HashMap<&str, Vec<&'a Node>>,
    root: Option<&Path>,
) -> PackageExports<'a> {
    let mut exports: PackageExports<'a> = HashMap::new();
    let mut pending = Vec::new();
    let mut seen = HashSet::new();
    for (path, facts) in all {
        let facts = facts.as_ref();
        let Some(module) = facts
            .nodes
            .iter()
            .find(|node| node.kind == "module" && node.language == "python")
        else {
            continue;
        };
        let rebound = |name| {
            module.details["rebindings"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(name)))
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
            if rebound(alias) {
                exports.entry(path.as_str()).or_default().insert(alias, None);
                continue;
            }
            if reference.expression == import_path
                && import_path.contains('.')
                && import_path.split('.').next() == Some(alias)
            {
                continue;
            }
            let mut provider = export_provider(path, import_path, modules_by_namespace, root);
            let mut member = (reference.module.as_deref() != Some(reference.expression.as_str()))
                .then_some(reference.expression.as_str());
            if let (Some(parent), Some(name)) = (provider, member) {
                let qualified = format!("{}.{}", parent.qualname, name);
                let declared = by_module
                    .get(parent.path.as_str())
                    .into_iter()
                    .flatten()
                    .any(|node| node.qualname == qualified && node.kind != "component");
                if !declared {
                    let child_path = format!(
                        "{import_path}{}{name}",
                        if import_path.ends_with('.') { "" } else { "." }
                    );
                    if let Some(child) =
                        export_provider(path, &child_path, modules_by_namespace, root)
                    {
                        provider = Some(child);
                        member = None;
                    }
                }
            }
            register_export(
                &mut exports,
                &mut pending,
                &mut seen,
                ExportBinding {
                    owner: path,
                    name: alias,
                    provider,
                    member,
                },
                by_module,
            );
        }
        for export in module.details["exports"].as_array().into_iter().flatten() {
            let Some(name) = export["name"].as_str() else {
                continue;
            };
            exports.entry(path.as_str()).or_default().insert(name, None);
            let provider = export["module"]
                .as_str()
                .and_then(|module_path| export_provider(path, module_path, modules_by_namespace, root))
                .or_else(|| (export["local"].as_str().is_some() && export["module"].is_null()).then_some(module));
            register_export(
                &mut exports,
                &mut pending,
                &mut seen,
                ExportBinding {
                    owner: path,
                    name,
                    provider,
                    member: export["local"].as_str(),
                },
                by_module,
            );
        }
        for export in module.details["lazy_exports"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let Some(name) = export["name"].as_str() else {
                continue;
            };
            if seen.contains(&(path.as_str(), name)) {
                continue;
            }
            if rebound(name) {
                continue;
            }
            let Some(import_path) = lazy_import_path(module, export) else {
                continue;
            };
            let provider = export_provider(path, &import_path, modules_by_namespace, root).filter(
                |provider| {
                    export["relative_to_module"].as_bool() != Some(true)
                        || languages::workspace_path(&provider.path).0
                            == languages::workspace_path(path).0
                },
            );
            register_export(
                &mut exports,
                &mut pending,
                &mut seen,
                ExportBinding {
                    owner: path,
                    name,
                    provider,
                    member: export["member"].as_str(),
                },
                by_module,
            );
        }
    }
    loop {
        let before = pending.len();
        pending.retain(|(owner, name, provider, member)| {
            let target = resolve_export(&exports, provider, member);
            if let Some(target) = target {
                exports
                    .get_mut(owner)
                    .expect("pending export owner exists")
                    .insert(name, Some(target));
                false
            } else {
                true
            }
        });
        if pending.len() == before {
            break;
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
