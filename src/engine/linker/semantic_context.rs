//! Canonical symbol admission for the shared value-flow reducer.
//!
//! Import providers are selected once using the same language linker and
//! workspace namespaces as ordinary graph linking. No disk or SQL is consulted
//! while reducing values or resolving calls.

use super::traits::{ImportContext, ModulesByNamespace, PackageExports};
use super::value_flow::{SemanticResolver, Symbol, SymbolRole, TypeTarget};
use crate::core::{
    models::{Facts, Node, ReceiverHint, Reference},
    semantic::SourcePosition,
};
use crate::engine::languages::{self, ImportPath};
use hashbrown::{HashMap, HashSet};
use std::collections::BTreeMap;

fn nuxt_component_names(path: &str, project: &str) -> Option<(String, String)> {
    let relative = if project.is_empty() {
        path.strip_prefix("components/")?
    } else {
        path.strip_prefix(project)?.strip_prefix("/components/")?
    };
    let stem = relative.strip_suffix(".vue")?;
    if stem.is_empty()
        || !stem
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_'))
    {
        return None;
    }
    let mut pascal = String::new();
    for segment in stem.split('/') {
        if segment.is_empty() {
            return None;
        }
        for word in segment.split(['-', '_']) {
            let mut chars = word.chars();
            let first = chars.next()?;
            pascal.push(first.to_ascii_uppercase());
            pascal.extend(chars);
        }
    }
    let mut kebab = String::with_capacity(pascal.len() + 4);
    for (index, character) in pascal.chars().enumerate() {
        if character.is_ascii_uppercase() && index > 0 {
            kebab.push('-');
        }
        kebab.push(character.to_ascii_lowercase());
    }
    Some((pascal, kebab))
}

enum Imported<'a> {
    Nodes(Vec<&'a Node>, SourcePosition, bool),
    External(String, SourcePosition, &'a Reference, bool),
    Unknown,
}

pub(super) enum Member<'a> {
    Local(&'a Node),
    External(&'a str, usize, Option<&'static str>),
    Builtin,
    Ambiguous,
    Unknown,
}

fn rust_import_path(module: &str) -> std::borrow::Cow<'_, str> {
    if module.contains("::") {
        std::borrow::Cow::Owned(module.replace("::", "."))
    } else {
        std::borrow::Cow::Borrowed(module)
    }
}

fn rust_deref_wrapper(target: &TypeTarget<'_>) -> Option<&'static str> {
    match target {
        TypeTarget::Builtin(name) if name == "Box" => Some("Box"),
        TypeTarget::External { module, .. } => {
            let wrapper = match rust_import_path(module).as_ref() {
                "std.boxed.Box" | "alloc.boxed.Box" => "Box",
                "std.sync.Arc" | "alloc.sync.Arc" => "Arc",
                "std.rc.Rc" | "alloc.rc.Rc" => "Rc",
                _ => return None,
            };
            Some(wrapper)
        }
        _ => None,
    }
}

fn rust_standard_member(module: &str, member: &str) -> bool {
    if matches!(member, "new" | "from" | "with_capacity" | "default") {
        return false;
    }
    let module = rust_import_path(module);
    let nominal = match module.as_ref() {
        "std.collections.HashMap"
        | "std.collections.HashSet"
        | "std.collections.BTreeMap"
        | "std.collections.BTreeSet" => module.rsplit_once('.').map_or("", |(_, name)| name),
        "std.string.String" | "alloc.string.String" => "String",
        "std.vec.Vec" | "alloc.vec.Vec" => "Vec",
        "std.path.Path" => "Path",
        "std.path.PathBuf" => "PathBuf",
        "std.option.Option" | "core.option.Option" => "Option",
        "std.result.Result" | "core.result.Result" => "Result",
        _ => return false,
    };
    languages::by_id("rust").is_some_and(|profile| profile.builtin_member(nominal, member))
}

fn name_rebound(owner: &Node, name: &str) -> bool {
    owner.details["rebindings"]
        .as_array()
        .is_some_and(|names| names.iter().any(|item| item.as_str() == Some(name)))
}

fn rust_wrapper_value_member(wrapper: &str, member: &str) -> bool {
    match wrapper {
        "Box" => matches!(member, "as_ref" | "as_mut"),
        "Arc" | "Rc" => matches!(member, "clone" | "as_ref"),
        _ => false,
    }
}

pub(super) struct Context<'a> {
    flows: Option<&'a HashMap<&'a str, &'a crate::core::typed_facts::FlowState>>,
    by_qual: &'a HashMap<&'a str, Vec<&'a Node>>,
    exports: &'a HashMap<&'a str, PackageExports<'a>>,
    imports: HashMap<&'a str, HashMap<&'a str, Imported<'a>>>,
    python_psycopg_paths: HashSet<&'a str>,
    nuxt_runtime_projects: HashMap<&'a str, String>,
    nuxt_consumer_projects: HashMap<&'a str, String>,
    nuxt_components: HashMap<String, HashMap<String, Vec<&'a Node>>>,
    radix_vue_projects: HashSet<String>,
    commonjs: super::commonjs::CommonJsBindings<'a>,
    members: HashMap<(&'a str, &'a str, bool), Vec<&'a Node>>,
    python: Option<&'a super::receivers::PythonReceivers<'a>>,
    #[cfg(feature = "lang-rust")]
    rust: Option<&'a languages::rust::linker::RustMembers<'a>>,
    #[cfg(feature = "lang-typescript")]
    typescript: Option<&'a languages::typescript::linker::TypeScriptMembers<'a>>,
}

impl<'a> Context<'a> {
    pub(super) fn exact_external_import(
        &self,
        owner: &Node,
        alias: &str,
        module: &str,
        symbol: &str,
        at: SourcePosition,
    ) -> bool {
        matches!(
            self.imports.get(owner.id.as_str()).and_then(|imports| imports.get(alias)),
            Some(Imported::External(origin, position, reference, _))
                if origin == module
                    && *position < at
                    && reference.source == owner.id
                    && reference.module.as_deref() == Some(module)
                    && reference.expression == symbol
                    && reference.alias.as_deref() == Some(alias)
        )
    }

    pub(super) fn nuxt_builtin_component(&self, path: &str, tag: &str) -> bool {
        self.nuxt_runtime_projects.contains_key(path)
            && matches!(tag, "ClientOnly" | "NuxtLayout" | "NuxtPage")
    }

    pub(super) fn radix_vue_component(&self, path: &str, tag: &str) -> bool {
        self.nuxt_runtime_projects
            .get(path)
            .is_some_and(|project| self.radix_vue_projects.contains(project))
            && matches!(
                tag,
                "DialogRoot"
                    | "DialogPortal"
                    | "DialogOverlay"
                    | "DialogContent"
                    | "DialogTitle"
                    | "DialogDescription"
                    | "DialogClose"
            )
    }

    pub(super) fn nuxt_component_candidates(&self, path: &str, tag: &str) -> Option<&[&'a Node]> {
        let project = self.nuxt_consumer_projects.get(path)?;
        self.nuxt_components
            .get(project)?
            .get(tag)
            .map(Vec::as_slice)
    }

    fn unshadowed_global(&self, owner: &'a Node, name: &str, at: SourcePosition) -> bool {
        let mut scope = owner.qualname.as_str();
        loop {
            if self.by_qual.get(scope).into_iter().flatten().any(|node| {
                node.path == owner.path
                    && (node.details["bindings"].as_array().is_some_and(|bindings| {
                        bindings
                            .iter()
                            .any(|binding| binding.as_str() == Some(name))
                    }) || node.details["rebindings"]
                        .as_array()
                        .is_some_and(|rebindings| {
                            rebindings
                                .iter()
                                .any(|binding| binding.as_str() == Some(name))
                        })
                        || self
                            .imports
                            .get(node.id.as_str())
                            .is_some_and(|imports| imports.contains_key(name))
                        || self.value_flow(node).is_some_and(|flow| {
                            flow.bindings
                                .iter()
                                .any(|binding| binding.name == name && binding.position <= at)
                        }))
            }) || self
                .by_qual
                .get(format!("{scope}.{name}").as_str())
                .into_iter()
                .flatten()
                .any(|node| node.path == owner.path)
            {
                return false;
            }
            let Some((parent, _)) = scope.rsplit_once('.') else {
                break;
            };
            scope = parent;
        }
        true
    }

    fn dom_factory(
        &self,
        owner: &'a Node,
        callee: &str,
        at: SourcePosition,
    ) -> Option<TypeTarget<'a>> {
        if !matches!(owner.language.as_str(), "javascript" | "typescript")
            || !matches!(
                callee,
                "document.createElement" | "document.querySelector" | "document.getElementById"
            )
            || !self.unshadowed_global(owner, "document", at)
        {
            return None;
        }
        Some(TypeTarget::Builtin("Element".to_owned()))
    }

    fn fetch_response_factory(
        &self,
        owner: &'a Node,
        callee: &str,
        at: SourcePosition,
    ) -> Option<TypeTarget<'a>> {
        if !matches!(owner.language.as_str(), "javascript" | "typescript")
            || callee != "await fetch"
            || !self.unshadowed_global(owner, "fetch", at)
        {
            return None;
        }
        Some(TypeTarget::Builtin("Web.Response".to_owned()))
    }

    pub(super) fn admits_commonjs_binding(
        &self,
        scope: &Node,
        alias: &str,
        owner: &Node,
        at: SourcePosition,
    ) -> bool {
        self.commonjs.admits(scope, alias, owner, at)
    }
    fn visible(owner: &Node, candidate: &Node, at: SourcePosition, role: SymbolRole) -> bool {
        if candidate.language == "rust" && candidate.kind == "impl" {
            return false;
        }
        if matches!(role, SymbolRole::Type) || owner.language == "rust" {
            return true;
        }
        if candidate
            .qualname
            .rsplit_once('.')
            .is_none_or(|(parent, _)| parent != owner.qualname)
        {
            return true;
        }
        if matches!(owner.language.as_str(), "javascript" | "typescript" | "vue")
            && candidate.kind == "function"
        {
            return true;
        }
        SourcePosition {
            line: candidate.line,
            column: candidate.details["column"].as_u64().unwrap_or(0) as usize,
        } < at
    }
    pub(super) fn build<F: AsRef<Facts>>(
        all: &'a BTreeMap<String, F>,
        modules: &'a ModulesByNamespace<'a>,
        by_module: &'a HashMap<&'a str, Vec<&'a Node>>,
        by_qual: &'a HashMap<&'a str, Vec<&'a Node>>,
        exports: &'a HashMap<&'a str, PackageExports<'a>>,
        normalized_imports: &HashMap<(&str, &str), Option<ImportPath>>,
        dependencies: &languages::manifests::DependencyRegistry,
    ) -> Self {
        let mut context = Self {
            flows: None,
            by_qual,
            exports,
            imports: HashMap::new(),
            python_psycopg_paths: HashSet::new(),
            nuxt_runtime_projects: HashMap::new(),
            nuxt_consumer_projects: HashMap::new(),
            nuxt_components: HashMap::new(),
            radix_vue_projects: HashSet::new(),
            commonjs: super::commonjs::CommonJsBindings::build(all),
            members: HashMap::new(),
            python: None,
            #[cfg(feature = "lang-rust")]
            rust: None,
            #[cfg(feature = "lang-typescript")]
            typescript: None,
        };
        let mut nuxt_projects = HashSet::new();
        for (path, facts) in all {
            let facts = facts.as_ref();
            let Some(module) = facts.nodes.iter().find(|node| node.kind == "module") else {
                continue;
            };
            if module.language == "python"
                && facts.references.iter().any(|reference| {
                    reference.kind == "imports"
                        && (reference.module.as_deref() == Some("psycopg")
                            || reference.expression == "psycopg")
                })
                && dependencies.declares_for_path(
                    languages::LanguageFamily("python"),
                    path,
                    "psycopg",
                )
            {
                context.python_psycopg_paths.insert(path.as_str());
            }
            if module.details["nuxt_default_components_enabled"] == true
                && dependencies.declares_for_path(
                    languages::LanguageFamily("javascript"),
                    path,
                    "nuxt",
                )
            {
                if let Some(project) = path
                    .strip_suffix("/nuxt.config.ts")
                    .or_else(|| (path == "nuxt.config.ts").then_some(""))
                {
                    if dependencies.nearest_manifest_scope_for_path(
                        languages::LanguageFamily("javascript"),
                        path,
                    ) == Some(project)
                    {
                        nuxt_projects.insert(project.to_owned());
                    }
                }
            }
            if module.details["nuxt_radix_vue_enabled"] == true
                && dependencies.declares_for_path(
                    languages::LanguageFamily("javascript"),
                    path,
                    "nuxt",
                )
                && dependencies.declares_for_path(
                    languages::LanguageFamily("javascript"),
                    path,
                    "radix-vue",
                )
            {
                if let Some(project) = path
                    .strip_suffix("/nuxt.config.ts")
                    .or_else(|| (path == "nuxt.config.ts").then_some(""))
                {
                    if dependencies.nearest_manifest_scope_for_path(
                        languages::LanguageFamily("javascript"),
                        path,
                    ) == Some(project)
                    {
                        context.radix_vue_projects.insert(project.to_owned());
                    }
                }
            }
        }
        for (path, facts) in all {
            let facts = facts.as_ref();
            let Some(module) = facts.nodes.iter().find(|node| node.kind == "module") else {
                continue;
            };
            if module.language == "vue" {
                if let Some(project) = dependencies
                    .nearest_manifest_scope_for_path(languages::LanguageFamily("javascript"), path)
                {
                    if dependencies.declares_for_path(
                        languages::LanguageFamily("javascript"),
                        path,
                        "nuxt",
                    ) {
                        context
                            .nuxt_runtime_projects
                            .insert(path.as_str(), project.to_owned());
                    }
                    if nuxt_projects.contains(project) {
                        context
                            .nuxt_consumer_projects
                            .insert(path.as_str(), project.to_owned());
                        if let Some((pascal, kebab)) = nuxt_component_names(path, project) {
                            let components = context
                                .nuxt_components
                                .entry(project.to_owned())
                                .or_default();
                            components.entry(pascal).or_default().push(module);
                            components.entry(kebab).or_default().push(module);
                        }
                    }
                }
            }
            let Some(profile) = languages::by_id(&module.language) else {
                continue;
            };
            let linker = languages::linker_for(&module.language);
            let Some(package_exports) = exports.get(profile.id()) else {
                continue;
            };
            let owners: HashMap<&str, &Node> = facts
                .nodes
                .iter()
                .map(|node| (node.id.as_str(), node))
                .collect();
            for reference in facts
                .references
                .iter()
                .filter(|reference| reference.kind == "imports")
            {
                let Some(alias) = reference.alias.as_deref().filter(|alias| *alias != "*") else {
                    continue;
                };
                let Some(owner) = owners.get(reference.source.as_str()).copied() else {
                    continue;
                };
                let key = (
                    path.as_str(),
                    reference.module.as_deref().unwrap_or(&reference.expression),
                );
                let Some(normalized) = normalized_imports.get(&key).and_then(Option::as_ref) else {
                    continue;
                };
                let workspace = languages::workspace_path(path).0;
                let mut providers = Vec::new();
                let mut selected = "";
                for local in [true, false] {
                    if !local && normalized.relative {
                        break;
                    }
                    let mut namespace = normalized.namespace.as_str();
                    loop {
                        if local || namespace.contains('.') {
                            providers = modules
                                .get(&profile.family())
                                .and_then(|map| map.get(namespace))
                                .into_iter()
                                .flatten()
                                .copied()
                                .filter(|node| {
                                    (languages::workspace_path(&node.path).0 == workspace) == local
                                })
                                .collect();
                        }
                        if !providers.is_empty() {
                            selected = namespace;
                            break;
                        }
                        if !normalized.symbol_path {
                            break;
                        }
                        let Some((parent, _)) = namespace.rsplit_once('.') else {
                            break;
                        };
                        namespace = parent;
                    }
                    if !providers.is_empty() {
                        break;
                    }
                }
                let mut candidates = Vec::new();
                let mut lookup_key = String::new();
                linker.resolve_import(&mut ImportContext {
                    path,
                    reference,
                    normalized: Some(normalized),
                    selected_namespace: selected,
                    family: profile.family(),
                    modules: &mut providers,
                    candidates: &mut candidates,
                    modules_by_namespace: modules,
                    by_module,
                    by_qual,
                    package_exports,
                    lookup_key: &mut lookup_key,
                });
                let position = SourcePosition {
                    line: reference.line,
                    column: reference.column,
                };
                let is_type_only = matches!(
                    reference.receiver_hint.as_ref(),
                    Some(ReceiverHint::TypeOnlyImport)
                );
                let unproven_guard = is_type_only
                    && profile.id() == "python"
                    && !super::python_has_proven_type_checking(
                        path,
                        facts,
                        modules,
                        dependencies,
                        profile.family(),
                    );
                let target = if unproven_guard {
                    Imported::Unknown
                } else if providers.len() == 1 && !candidates.is_empty() {
                    Imported::Nodes(candidates, position, is_type_only)
                } else if providers.is_empty() && !normalized.relative {
                    let module_name = reference.module.as_deref().unwrap_or(&reference.expression);
                    if matches!(module_name, "typing" | "typing_extensions")
                        && reference.expression == "Any"
                    {
                        Imported::Unknown
                    } else {
                        let factory_safe = module.details["python_logging_factories"]
                            .as_array()
                            .is_some_and(|entries| {
                                let mut matching = entries.iter().filter(|entry| {
                                    entry["alias"].as_str() == Some(alias)
                                        && entry["line"].as_u64() == Some(position.line as u64)
                                        && entry["column"].as_u64() == Some(position.column as u64)
                                });
                                matching.next().is_some_and(|entry| {
                                    entry.as_object().is_some_and(|object| object.len() == 4)
                                        && entry["safe"].as_bool() == Some(true)
                                }) && matching.next().is_none()
                            });
                        Imported::External(
                            module_name.to_owned(),
                            position,
                            reference,
                            factory_safe,
                        )
                    }
                } else {
                    Imported::Unknown
                };
                let aliases = context.imports.entry(owner.id.as_str()).or_default();
                if aliases.contains_key(alias) {
                    aliases.insert(alias, Imported::Unknown);
                } else {
                    aliases.insert(alias, target);
                }
            }
        }
        for node in all
            .values()
            .flat_map(|facts| &facts.as_ref().nodes)
            .filter(|node| node.kind == "method")
        {
            let Some((parent, _)) = node.qualname.rsplit_once('.') else {
                continue;
            };
            let mut classes = by_qual
                .get(parent)
                .into_iter()
                .flatten()
                .copied()
                .filter(|class| {
                    class.path == node.path && matches!(class.kind.as_str(), "class" | "interface")
                });
            let Some(class) = classes.next() else {
                continue;
            };
            if classes.next().is_none() {
                context
                    .members
                    .entry((
                        class.id.as_str(),
                        node.name.as_str(),
                        node.details["is_static"] == true,
                    ))
                    .or_default()
                    .push(node);
            }
        }
        context
    }

    pub(super) fn with_flows(
        mut self,
        flows: Option<&'a HashMap<&'a str, &'a crate::core::typed_facts::FlowState>>,
    ) -> Self {
        self.flows = flows;
        self
    }

    pub(super) fn with_python(
        mut self,
        members: &'a super::receivers::PythonReceivers<'a>,
    ) -> Self {
        self.python = Some(members);
        self
    }

    #[cfg(feature = "lang-rust")]
    pub(super) fn with_rust(
        mut self,
        members: &'a languages::rust::linker::RustMembers<'a>,
    ) -> Self {
        self.rust = Some(members);
        self
    }

    #[cfg(feature = "lang-typescript")]
    pub(super) fn with_typescript(
        mut self,
        members: &'a languages::typescript::linker::TypeScriptMembers<'a>,
    ) -> Self {
        self.typescript = Some(members);
        self
    }

    fn classify(&self, nodes: impl Iterator<Item = &'a Node>, role: SymbolRole) -> Symbol<'a> {
        let mut candidates: Vec<&'a Node> = nodes
            .filter(|node| {
                node.language != "javascript"
                    || node.details["prototype"] != true
                    || node.details["prototype_valid"] == true
            })
            .filter(|node| match role {
                SymbolRole::Type => matches!(
                    node.kind.as_str(),
                    "class" | "struct" | "enum" | "type" | "type_alias" | "interface" | "trait"
                ),
                SymbolRole::Constructor => {
                    matches!(node.kind.as_str(), "class" | "struct" | "enum")
                        || (node.language == "javascript"
                            && node.kind == "function"
                            && node.details["prototype_constructor"] == true)
                }
                SymbolRole::Callable => matches!(
                    node.kind.as_str(),
                    "function" | "method" | "class" | "struct" | "enum"
                ),
            })
            .collect();
        if candidates.len() > 1 {
            let non_stubs: Vec<&'a Node> = candidates
                .iter()
                .copied()
                .filter(|n| {
                    let is_stub = n
                        .details
                        .get("is_stub")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let is_overload = n
                        .details
                        .get("is_overload")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    !is_stub && !is_overload
                })
                .collect();
            if !non_stubs.is_empty() && non_stubs.len() < candidates.len() {
                candidates = non_stubs;
            }
        }
        if candidates.is_empty() {
            return Symbol::Unknown;
        }
        if candidates.len() > 1 {
            return Symbol::Ambiguous;
        }
        let node = candidates[0];
        if matches!(node.kind.as_str(), "function" | "method")
            && !matches!(role, SymbolRole::Constructor)
        {
            Symbol::Callable(node)
        } else {
            Symbol::Type(TypeTarget::Local(node))
        }
    }

    fn member(
        &self,
        receiver: &'a Node,
        member: &str,
        associated: bool,
        caller: &'a Node,
    ) -> Symbol<'a> {
        #[cfg(feature = "lang-typescript")]
        if matches!(
            receiver.language.as_str(),
            "typescript" | "javascript" | "vue"
        ) {
            if let Some(index) = self.typescript {
                return self.classify(
                    index.lookup(receiver, member, associated).iter().copied(),
                    SymbolRole::Callable,
                );
            }
        }
        if receiver.language == "python" && receiver.kind == "class" {
            return match self
                .python
                .map(|index| index.lookup(receiver, member, false))
            {
                Some(super::receivers::Member::Local(node)) => {
                    self.classify(std::iter::once(node), SymbolRole::Callable)
                }
                Some(super::receivers::Member::Ambiguous) => Symbol::Ambiguous,
                _ => Symbol::Unknown,
            };
        }
        #[cfg(feature = "lang-rust")]
        if receiver.language == "rust" {
            return self.rust.map_or(Symbol::Unknown, |index| {
                self.classify(
                    index
                        .lookup(receiver, member, associated, caller)
                        .iter()
                        .copied(),
                    SymbolRole::Callable,
                )
            });
        }
        let _ = (associated, caller);
        let qualified = format!("{}.{}", receiver.qualname, member);
        self.classify(
            self.by_qual
                .get(qualified.as_str())
                .into_iter()
                .flatten()
                .copied()
                .filter(|node| node.path == receiver.path),
            SymbolRole::Callable,
        )
    }

    pub(super) fn lookup_member<'s>(
        &'s self,
        receiver: &'s TypeTarget<'a>,
        member: &str,
        caller: &'a Node,
    ) -> Member<'s> {
        self.lookup_member_mode(receiver, member, caller, false)
    }

    pub(super) fn lookup_member_mode<'s>(
        &'s self,
        receiver: &'s TypeTarget<'a>,
        member: &str,
        caller: &'a Node,
        associated: bool,
    ) -> Member<'s> {
        match receiver {
            TypeTarget::Local(receiver)
                if receiver.language == "python" && receiver.kind == "class" =>
            {
                match self
                    .python
                    .map(|index| index.lookup(receiver, member, false))
                {
                    Some(super::receivers::Member::Local(node)) => Member::Local(node),
                    Some(super::receivers::Member::External(module, line, provider)) => {
                        Member::External(module, line, provider)
                    }
                    Some(super::receivers::Member::Ambiguous) => Member::Ambiguous,
                    _ => Member::Unknown,
                }
            }
            #[cfg(feature = "lang-rust")]
            TypeTarget::RustDeref {
                wrapper, target, ..
            } if caller.language == "rust" => {
                if associated {
                    return Member::Unknown;
                }
                if rust_wrapper_value_member(wrapper, member) {
                    return Member::Builtin;
                }
                match target.as_ref() {
                    TypeTarget::External { module, .. } => {
                        if rust_standard_member(module, member) {
                            Member::Builtin
                        } else {
                            Member::Unknown
                        }
                    }
                    _ => self.lookup_member_mode(target, member, caller, associated),
                }
            }
            #[cfg(feature = "lang-rust")]
            TypeTarget::Local(receiver) if receiver.language == "rust" => {
                match self
                    .rust
                    .map(|index| index.lookup(receiver, member, associated, caller))
                {
                    Some([node]) => Member::Local(node),
                    Some(nodes) if nodes.len() > 1 => Member::Ambiguous,
                    _ => Member::Unknown,
                }
            }
            TypeTarget::Local(receiver) => {
                #[cfg(feature = "lang-typescript")]
                if matches!(
                    receiver.language.as_str(),
                    "typescript" | "javascript" | "vue"
                ) {
                    if let Some(index) = self.typescript {
                        return match index.lookup(receiver, member, associated) {
                            [node] => Member::Local(node),
                            nodes if nodes.len() > 1 => Member::Ambiguous,
                            _ => Member::Unknown,
                        };
                    }
                }
                match self
                    .members
                    .get(&(receiver.id.as_str(), member, associated))
                    .map(Vec::as_slice)
                {
                    Some([node]) => Member::Local(node),
                    Some(nodes) if nodes.len() > 1 => {
                        let non_stubs: Vec<&Node> = nodes
                            .iter()
                            .copied()
                            .filter(|n| {
                                let is_stub = n
                                    .details
                                    .get("is_stub")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                let is_overload = n
                                    .details
                                    .get("is_overload")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                !is_stub && !is_overload
                            })
                            .collect();
                        if non_stubs.len() == 1 {
                            Member::Local(non_stubs[0])
                        } else {
                            Member::Ambiguous
                        }
                    }
                    _ => Member::Unknown,
                }
            }
            TypeTarget::Object(members) => members
                .get(member)
                .map_or(Member::Unknown, |node| Member::Local(node)),
            TypeTarget::External {
                module,
                import_line,
            } => {
                if caller.language == "python" && module == "psycopg.AsyncConnection" {
                    if self.python_psycopg_paths.contains(caller.path.as_str())
                        && matches!(member, "execute" | "cursor")
                    {
                        Member::External(module, *import_line, Some("psycopg.AsyncConnection"))
                    } else {
                        Member::Unknown
                    }
                } else if caller.language == "python" && module == "sqlite3.Cursor" {
                    if matches!(
                        member,
                        "execute"
                            | "executemany"
                            | "executescript"
                            | "fetchone"
                            | "fetchmany"
                            | "fetchall"
                            | "close"
                    ) {
                        Member::External(module, *import_line, Some("builtin:sqlite3"))
                    } else {
                        Member::Unknown
                    }
                } else if caller.language == "python" && module == "sqlite3.Connection" {
                    if matches!(
                        member,
                        "execute"
                            | "executemany"
                            | "executescript"
                            | "cursor"
                            | "commit"
                            | "rollback"
                            | "close"
                            | "total_changes"
                            | "interrupt"
                    ) {
                        Member::External(module, *import_line, Some("builtin:sqlite3"))
                    } else {
                        Member::Unknown
                    }
                } else if caller.language == "python" && module == "logging.LoggerAdapter" {
                    if matches!(
                        member,
                        "info"
                            | "warning"
                            | "error"
                            | "debug"
                            | "critical"
                            | "exception"
                            | "log"
                            | "isEnabledFor"
                            | "setLevel"
                            | "addHandler"
                            | "removeHandler"
                            | "process"
                    ) {
                        Member::External(
                            module,
                            *import_line,
                            Some("builtin:logging.LoggerAdapter"),
                        )
                    } else {
                        Member::Unknown
                    }
                } else {
                    Member::External(module, *import_line, None)
                }
            }
            TypeTarget::Builtin(receiver) => {
                if languages::by_id(&caller.language)
                    .is_some_and(|profile| profile.builtin_member(receiver, member))
                {
                    Member::Builtin
                } else {
                    Member::Unknown
                }
            }
            TypeTarget::Ambiguous => Member::Ambiguous,
            _ => Member::Unknown,
        }
    }

    #[cfg(feature = "lang-typescript")]
    pub(super) fn lookup_vue_template_field<'s>(
        &'s self,
        receiver: &'s TypeTarget<'a>,
        member: &str,
    ) -> Member<'s> {
        let TypeTarget::Local(receiver) = receiver else {
            return Member::Unknown;
        };
        if !matches!(
            receiver.language.as_str(),
            "typescript" | "javascript" | "vue"
        ) || !matches!(receiver.kind.as_str(), "class" | "interface")
        {
            return Member::Unknown;
        }
        match self
            .typescript
            .map(|index| index.lookup_field(receiver, member))
        {
            Some([field]) => Member::Local(field),
            Some(fields) if fields.len() > 1 => Member::Ambiguous,
            _ => Member::Unknown,
        }
    }

    #[cfg(not(feature = "lang-typescript"))]
    pub(super) fn lookup_vue_template_field<'s>(
        &'s self,
        _receiver: &'s TypeTarget<'a>,
        _member: &str,
    ) -> Member<'s> {
        Member::Unknown
    }
}

impl<'a> SemanticResolver<'a> for Context<'a> {
    fn resolve_external_factory(
        &self,
        owner: &'a Node,
        callee: &str,
        at: SourcePosition,
    ) -> Option<TypeTarget<'a>> {
        if matches!(owner.language.as_str(), "javascript" | "typescript") {
            return self
                .dom_factory(owner, callee, at)
                .or_else(|| self.fetch_response_factory(owner, callee, at));
        }
        if owner.language != "python" {
            return None;
        }
        let (head, member) = callee.split_once('.').unwrap_or((callee, ""));
        if !matches!(member, "" | "getLogger" | "LoggerAdapter" | "connect") {
            return None;
        }
        let mut scope = owner.qualname.as_str();
        loop {
            for node in self
                .by_qual
                .get(scope)
                .into_iter()
                .flatten()
                .copied()
                .filter(|node| node.path == owner.path)
            {
                if let Some(import) = self
                    .imports
                    .get(node.id.as_str())
                    .and_then(|aliases| aliases.get(head))
                {
                    let Imported::External(imported_module, position, reference, true) = import
                    else {
                        return None;
                    };
                    if !matches!(imported_module.as_str(), "logging" | "sqlite3") || *position > at
                    {
                        return None;
                    }
                    let canonical = match (reference.expression.as_str(), member) {
                        ("logging", "getLogger") | ("getLogger", "") => "logging.Logger",
                        ("logging", "LoggerAdapter") | ("LoggerAdapter", "") => {
                            "logging.LoggerAdapter"
                        }
                        ("sqlite3", "connect") | ("connect", "") => "sqlite3.Connection",
                        _ => return None,
                    };
                    let Symbol::Type(TypeTarget::External {
                        module,
                        import_line,
                    }) = self.resolve_symbol(owner, callee, at, SymbolRole::Callable)
                    else {
                        return None;
                    };
                    if !matches!(module.as_str(), "logging" | "sqlite3")
                        || position.line != import_line
                    {
                        return None;
                    }
                    return Some(TypeTarget::Builtin(canonical.to_owned()));
                }
            }
            scope = scope.rsplit_once('.')?.0;
        }
    }
    fn value_flow(&self, node: &'a Node) -> Option<&'a crate::core::semantic::ValueFlowFacts> {
        match self.flows?.get(node.id.as_str()).copied()? {
            crate::core::typed_facts::FlowState::Valid { facts, .. } => Some(facts),
            crate::core::typed_facts::FlowState::Invalid(_) => None,
        }
    }

    fn invalid_value_flow(&self, node: &'a Node) -> bool {
        self.flows.is_some_and(|flows| {
            matches!(
                flows.get(node.id.as_str()),
                Some(crate::core::typed_facts::FlowState::Invalid(_))
            )
        })
    }

    fn has_typed_flows(&self) -> bool {
        self.flows.is_some()
    }

    fn raw_value_flow(&self, node: &'a Node) -> Option<&'a serde_json::Value> {
        match self.flows?.get(node.id.as_str()).copied()? {
            crate::core::typed_facts::FlowState::Invalid(value) => Some(value),
            crate::core::typed_facts::FlowState::Valid { .. } => None,
        }
    }
    fn resolve_applied(
        &self,
        owner: &'a Node,
        base: &str,
        args: &[crate::core::semantic::TypeExpr],
        at: SourcePosition,
    ) -> TypeTarget<'a> {
        let clean_base = base.rsplit_once('.').map_or(base, |(_, name)| name);
        let nominal = self.resolve_symbol(owner, base, at, SymbolRole::Type);
        if owner.language == "rust" && args.len() == 1 {
            if let Symbol::Type(outer) = &nominal {
                if let Some(wrapper) = rust_deref_wrapper(outer) {
                    let mut target = super::value_flow::resolve_type(
                        owner,
                        &args[0],
                        at,
                        self,
                        super::value_flow::SemanticLimits::default(),
                    );
                    if matches!(target, TypeTarget::Unknown) {
                        if let crate::core::semantic::TypeExpr::Applied { base, .. } = &args[0] {
                            if let Symbol::Type(TypeTarget::External {
                                module,
                                import_line,
                            }) = self.resolve_symbol(owner, base, at, SymbolRole::Type)
                            {
                                if matches!(
                                    rust_import_path(&module).split('.').next(),
                                    Some("std" | "alloc" | "core")
                                ) {
                                    target = TypeTarget::External {
                                        module,
                                        import_line,
                                    };
                                }
                            }
                        }
                    }
                    return TypeTarget::RustDeref {
                        wrapper,
                        target: Box::new(target),
                    };
                }
            }
        }
        match nominal {
            Symbol::Type(TypeTarget::Local(node))
                if owner.language == "rust" && matches!(node.kind.as_str(), "struct" | "enum") =>
            {
                TypeTarget::Local(node)
            }
            Symbol::Type(TypeTarget::External {
                module,
                import_line,
            }) if owner.language == "python" && module == "psycopg.AsyncConnection" => {
                TypeTarget::External {
                    module,
                    import_line,
                }
            }
            Symbol::Type(TypeTarget::Builtin(name))
                if languages::by_id(&owner.language)
                    .is_some_and(|profile| profile.builtin_generic(&name)) =>
            {
                TypeTarget::Builtin(name)
            }
            Symbol::Type(TypeTarget::External { module, .. })
                if matches!(
                    module.as_str(),
                    "typing" | "typing_extensions" | "collections.abc"
                ) && languages::by_id(&owner.language)
                    .is_some_and(|profile| profile.builtin_generic(clean_base)) =>
            {
                TypeTarget::Builtin(clean_base.to_string())
            }
            _ => TypeTarget::Unknown,
        }
    }
    fn resolve_member(
        &self,
        receiver: &TypeTarget<'a>,
        member: &str,
        owner: &'a Node,
        _at: SourcePosition,
    ) -> Symbol<'a> {
        match receiver {
            TypeTarget::Local(node) => self.member(node, member, false, owner),
            TypeTarget::RustDeref {
                wrapper, target, ..
            } => {
                if rust_wrapper_value_member(wrapper, member) {
                    Symbol::Unknown
                } else {
                    self.resolve_member(target, member, owner, _at)
                }
            }
            TypeTarget::Object(members) => match members.get(member) {
                Some(node) => Symbol::Callable(node),
                None => Symbol::Unknown,
            },
            _ => Symbol::Unknown,
        }
    }
    fn resolve_symbol(
        &self,
        owner: &'a Node,
        name: &str,
        at: SourcePosition,
        role: SymbolRole,
    ) -> Symbol<'a> {
        let Some(name) = super::receiver_type_name(name) else {
            return Symbol::Unknown;
        };
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '$' | ':'))
        {
            return Symbol::Unknown;
        }
        if owner.language == "rust" {
            if name == "Self" {
                #[cfg(feature = "lang-rust")]
                if let Some(receiver) = self.rust.and_then(|members| members.receiver_for(owner)) {
                    return Symbol::Type(TypeTarget::Local(receiver));
                }
                if let Some(receiver) = owner.details["receiver_type"]
                    .as_str()
                    .filter(|name| *name != "Self")
                {
                    return self.resolve_symbol(owner, receiver, at, SymbolRole::Type);
                }
            }
            if let Some(member) = name
                .strip_prefix("Self::")
                .or_else(|| name.strip_prefix("Self."))
            {
                #[cfg(feature = "lang-rust")]
                if let Some(receiver) = self.rust.and_then(|members| members.receiver_for(owner)) {
                    let methods = self.rust.map_or(&[] as &[&'a Node], |m| {
                        m.lookup(receiver, member, true, owner)
                    });
                    if let Some(target) = methods.first() {
                        return Symbol::Callable(target);
                    }
                    if member == "new" || member == "default" {
                        return Symbol::Type(TypeTarget::Local(receiver));
                    }
                }
            }
        }
        let (head, member) = name.split_once('.').unwrap_or((&name, ""));
        if owner.language == "python"
            && matches!(head, "self" | "cls")
            && owner.details["receiver_name"].as_str() == Some(head)
            && !name_rebound(owner, head)
        {
            let mut scope = owner.qualname.as_str();
            while let Some((parent, _)) = scope.rsplit_once('.') {
                if let Some(nodes) = self.by_qual.get(parent) {
                    if let Some(class) = nodes
                        .iter()
                        .copied()
                        .find(|n| n.path == owner.path && n.kind == "class")
                    {
                        if member.is_empty() {
                            return Symbol::Type(TypeTarget::Local(class));
                        } else {
                            return self.member(class, member, false, owner);
                        }
                    }
                }
                scope = parent;
            }
        }
        if matches!(owner.language.as_str(), "javascript" | "typescript")
            && head == "this"
            && owner.details["receiver_name"].as_str() == Some("this")
            && !name_rebound(owner, head)
        {
            let associated = owner.details["is_static"] == true;
            let mut scope = owner.qualname.as_str();
            while let Some((parent, _)) = scope.rsplit_once('.') {
                if let Some(nodes) = self.by_qual.get(parent) {
                    if let Some(class) = nodes.iter().copied().find(|n| {
                        n.path == owner.path && matches!(n.kind.as_str(), "class" | "interface")
                    }) {
                        if member.is_empty() {
                            return Symbol::Type(TypeTarget::Local(class));
                        } else {
                            return self.member(class, member, associated, owner);
                        }
                    }
                }
                scope = parent;
            }
        }
        let mut scope = owner.qualname.as_str();
        loop {
            let scope_nodes = self.by_qual.get(scope);
            let qualified = format!("{scope}.{name}");
            if let Some(nodes) = scope_nodes {
                for node in nodes.iter().copied().filter(|node| node.path == owner.path) {
                    if node.details["rebindings"]
                        .as_array()
                        .is_some_and(|names| names.iter().any(|name| name.as_str() == Some(head)))
                        && !self.admits_commonjs_binding(node, head, owner, at)
                    {
                        return Symbol::Unknown;
                    }
                    if let Some(import) = self
                        .imports
                        .get(node.id.as_str())
                        .and_then(|aliases| aliases.get(head))
                    {
                        let imported_at = match import {
                            Imported::Nodes(_, position, _)
                            | Imported::External(_, position, _, _) => Some(*position),
                            Imported::Unknown => None,
                        };
                        if let Some(imported_at) = imported_at {
                            let local = self
                                .by_qual
                                .get(qualified.as_str())
                                .into_iter()
                                .flatten()
                                .copied()
                                .filter(|candidate| {
                                    candidate.path == owner.path
                                        && SourcePosition {
                                            line: candidate.line,
                                            column: candidate.details["column"]
                                                .as_u64()
                                                .unwrap_or(0)
                                                as usize,
                                        } > imported_at
                                        && Self::visible(owner, candidate, at, role)
                                });
                            if local.clone().next().is_some() {
                                return self.classify(local, role);
                            }
                        }
                        let type_only = match import {
                            Imported::Nodes(_, _, type_only) => *type_only,
                            Imported::External(_, _, reference, _) => matches!(
                                reference.receiver_hint.as_ref(),
                                Some(ReceiverHint::TypeOnlyImport)
                            ),
                            Imported::Unknown => false,
                        };
                        if type_only
                            && matches!(role, SymbolRole::Callable | SymbolRole::Constructor)
                        {
                            return Symbol::Unknown;
                        }
                        return match import {
                            Imported::Nodes(targets, position, _)
                                if *position <= at
                                    || (owner.kind == "template_scope"
                                        && owner.language == "vue") =>
                            {
                                if member.is_empty() {
                                    self.classify(targets.iter().copied(), role)
                                } else {
                                    if matches!(role, SymbolRole::Callable) {
                                        if let [target] = targets.as_slice() {
                                            if matches!(
                                                target.kind.as_str(),
                                                "class" | "struct" | "enum" | "interface"
                                            ) {
                                                return self.member(target, member, true, owner);
                                            }
                                        }
                                    }
                                    let mut nodes = Vec::new();
                                    for target in targets {
                                        if target.kind == "module"
                                            && matches!(
                                                target.language.as_str(),
                                                "typescript" | "javascript" | "vue"
                                            )
                                        {
                                            let (export, rest) =
                                                member.split_once('.').unwrap_or((member, ""));
                                            let Some(exported) = self
                                                .exports
                                                .get(target.language.as_str())
                                                .and_then(|exports| {
                                                    exports.get(target.path.as_str())
                                                })
                                                .and_then(|exports| exports.get(export))
                                            else {
                                                continue;
                                            };
                                            let Some(exported) = exported else {
                                                return Symbol::Ambiguous;
                                            };
                                            if rest.is_empty() {
                                                nodes.push(*exported);
                                            } else if targets.len() == 1
                                                && matches!(role, SymbolRole::Callable)
                                                && matches!(
                                                    exported.kind.as_str(),
                                                    "class" | "interface"
                                                )
                                            {
                                                return self.member(exported, rest, true, owner);
                                            }
                                            continue;
                                        }
                                        let qualified = format!("{}.{}", target.qualname, member);
                                        nodes.extend(
                                            self.by_qual
                                                .get(qualified.as_str())
                                                .into_iter()
                                                .flatten()
                                                .copied()
                                                .filter(|node| node.path == target.path),
                                        );
                                    }
                                    self.classify(nodes.into_iter(), role)
                                }
                            }
                            Imported::External(module, position, reference, _)
                                if *position <= at =>
                            {
                                if (owner.language == "python"
                                    && matches!(role, SymbolRole::Constructor))
                                    || (matches!(module.as_str(), "typing" | "typing_extensions")
                                        && (member == "Any" || head == "Any"))
                                {
                                    Symbol::Unknown
                                } else if owner.language == "python"
                                    && matches!(
                                        module.as_str(),
                                        "typing" | "typing_extensions" | "collections.abc"
                                    )
                                    && languages::by_id("python").is_some_and(|p| {
                                        p.builtin_generic(head) || p.builtin_member(head, "get")
                                    })
                                {
                                    Symbol::Type(TypeTarget::Builtin(head.to_string()))
                                } else if owner.language == "python"
                                    && matches!(role, SymbolRole::Type)
                                    && module == "psycopg"
                                    && self.python_psycopg_paths.contains(owner.path.as_str())
                                    && ((reference.expression == "AsyncConnection"
                                        && member.is_empty())
                                        || (reference.expression == "psycopg"
                                            && member == "AsyncConnection"))
                                {
                                    Symbol::Type(TypeTarget::External {
                                        module: "psycopg.AsyncConnection".to_owned(),
                                        import_line: position.line,
                                    })
                                } else if owner.language == "python"
                                    && module == "logging"
                                    && (matches!(member, "Logger" | "LoggerAdapter")
                                        || matches!(head, "Logger" | "LoggerAdapter"))
                                {
                                    let log_class = if matches!(member, "Logger" | "LoggerAdapter")
                                    {
                                        member
                                    } else {
                                        head
                                    };
                                    Symbol::Type(TypeTarget::External {
                                        module: format!("logging.{log_class}"),
                                        import_line: position.line,
                                    })
                                } else if owner.language == "python"
                                    && matches!(role, SymbolRole::Type)
                                    && module == "sqlite3"
                                    && ((reference.expression == "sqlite3"
                                        && matches!(member, "Cursor" | "Connection"))
                                        || (matches!(
                                            reference.expression.as_str(),
                                            "Cursor" | "Connection"
                                        ) && member.is_empty()))
                                {
                                    let cls = if reference.expression == "sqlite3" {
                                        member
                                    } else {
                                        reference.expression.as_str()
                                    };
                                    Symbol::Type(TypeTarget::External {
                                        module: format!("sqlite3.{cls}"),
                                        import_line: position.line,
                                    })
                                } else {
                                    Symbol::Type(TypeTarget::External {
                                        module: module.clone(),
                                        import_line: position.line,
                                    })
                                }
                            }
                            _ => Symbol::Unknown,
                        };
                    }
                }
            }
            if !member.is_empty() && matches!(role, SymbolRole::Callable) {
                let qualified = format!("{scope}.{head}");
                if let Symbol::Type(TypeTarget::Local(receiver)) = self.classify(
                    self.by_qual
                        .get(qualified.as_str())
                        .into_iter()
                        .flatten()
                        .copied()
                        .filter(|node| {
                            node.path == owner.path && Self::visible(owner, node, at, role)
                        }),
                    SymbolRole::Type,
                ) {
                    return self.member(receiver, member, true, owner);
                }
            }
            let local = self
                .by_qual
                .get(qualified.as_str())
                .into_iter()
                .flatten()
                .copied()
                .filter(|node| node.path == owner.path && Self::visible(owner, node, at, role));
            if local.clone().next().is_some() {
                return self.classify(local, role);
            }
            if scope_nodes.is_some_and(|nodes| {
                nodes
                    .iter()
                    .filter(|node| node.path == owner.path)
                    .any(|node| {
                        node.details["bindings"].as_array().is_some_and(|names| {
                            names.iter().any(|name| name.as_str() == Some(head))
                        })
                    })
            }) {
                return Symbol::Unknown;
            }
            let Some((parent, _)) = scope.rsplit_once('.') else {
                break;
            };
            scope = parent;
        }
        let builtin =
            languages::by_id(&owner.language).is_some_and(|profile| profile.builtin_type(&name));
        if builtin {
            Symbol::Type(TypeTarget::Builtin(name.into_owned()))
        } else {
            Symbol::Unknown
        }
    }

    fn resolve_import_module(
        &self,
        owner: &'a Node,
        name: &str,
        at: SourcePosition,
    ) -> Option<&'a Node> {
        let (head, _) = name.split_once('.').unwrap_or((name, ""));
        let mut scope = owner.qualname.as_str();
        loop {
            if let Some(nodes) = self.by_qual.get(scope) {
                for node in nodes.iter().copied().filter(|node| node.path == owner.path) {
                    if let Some(Imported::Nodes(targets, position, _)) = self
                        .imports
                        .get(node.id.as_str())
                        .and_then(|aliases| aliases.get(head))
                    {
                        if *position <= at
                            || (owner.kind == "template_scope" && owner.language == "vue")
                        {
                            if let [target] = targets.as_slice() {
                                if target.kind == "module" {
                                    return Some(*target);
                                }
                            }
                        }
                    }
                }
            }
            match scope.rsplit_once('.') {
                Some((parent, _)) => scope = parent,
                None => break,
            }
        }
        None
    }
}
