//! Canonical symbol admission for the shared value-flow reducer.
//!
//! Import providers are selected once using the same language linker and
//! workspace namespaces as ordinary graph linking. No disk or SQL is consulted
//! while reducing values or resolving calls.

use super::traits::{ImportContext, ModulesByNamespace, PackageExports};
use super::value_flow::{SemanticResolver, Symbol, SymbolRole, TypeTarget};
use crate::core::{
    models::{Facts, Node, Reference},
    semantic::SourcePosition,
};
use crate::engine::languages::{self, ImportPath};
use hashbrown::HashMap;
use std::collections::BTreeMap;

enum Imported<'a> {
    Nodes(Vec<&'a Node>, SourcePosition),
    External(String, SourcePosition, &'a Reference, bool),
    Unknown,
}

pub(super) enum Member<'a> {
    Local(&'a Node),
    External(&'a str, usize),
    Builtin,
    Ambiguous,
    Unknown,
}

pub(super) struct Context<'a> {
    flows: Option<&'a HashMap<&'a str, &'a crate::core::typed_facts::FlowState>>,
    by_qual: &'a HashMap<&'a str, Vec<&'a Node>>,
    exports: &'a HashMap<&'a str, PackageExports<'a>>,
    imports: HashMap<&'a str, HashMap<&'a str, Imported<'a>>>,
    commonjs: super::commonjs::CommonJsBindings<'a>,
    members: HashMap<(&'a str, &'a str, bool), Vec<&'a Node>>,
    python: Option<&'a super::receivers::PythonReceivers<'a>>,
    #[cfg(feature = "lang-rust")]
    rust: Option<&'a languages::rust::linker::RustMembers<'a>>,
    #[cfg(feature = "lang-typescript")]
    typescript: Option<&'a languages::typescript::linker::TypeScriptMembers<'a>>,
}

impl<'a> Context<'a> {
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
        normalized: &HashMap<(&str, &str), Option<ImportPath>>,
    ) -> Self {
        let mut context = Self {
            flows: None,
            by_qual,
            exports,
            imports: HashMap::new(),
            commonjs: super::commonjs::CommonJsBindings::build(all),
            members: HashMap::new(),
            python: None,
            #[cfg(feature = "lang-rust")]
            rust: None,
            #[cfg(feature = "lang-typescript")]
            typescript: None,
        };
        for (path, facts) in all {
            let facts = facts.as_ref();
            let Some(module) = facts.nodes.iter().find(|node| node.kind == "module") else {
                continue;
            };
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
                let Some(normalized) = normalized.get(&key).and_then(Option::as_ref) else {
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
                let target = if providers.len() == 1 && !candidates.is_empty() {
                    Imported::Nodes(candidates, position)
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
        let mut candidates = nodes.filter(|node| match role {
            SymbolRole::Type => matches!(
                node.kind.as_str(),
                "class" | "struct" | "enum" | "type" | "type_alias" | "interface" | "trait"
            ),
            SymbolRole::Constructor => matches!(node.kind.as_str(), "class" | "struct" | "enum"),
            SymbolRole::Callable => matches!(
                node.kind.as_str(),
                "function" | "method" | "class" | "struct" | "enum"
            ),
        });
        let Some(node) = candidates.next() else {
            return Symbol::Unknown;
        };
        if candidates.next().is_some() {
            return Symbol::Ambiguous;
        }
        if matches!(node.kind.as_str(), "function" | "method") {
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
                    Some(super::receivers::Member::External(module, line)) => {
                        Member::External(module, line)
                    }
                    _ => Member::Unknown,
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
                    Some(nodes) if nodes.len() > 1 => Member::Ambiguous,
                    _ => Member::Unknown,
                }
            }
            TypeTarget::Object(members) => members
                .get(member)
                .map_or(Member::Unknown, |node| Member::Local(node)),
            TypeTarget::External {
                module,
                import_line,
            } => Member::External(module, *import_line),
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
}

impl<'a> SemanticResolver<'a> for Context<'a> {
    fn resolve_external_factory(
        &self,
        owner: &'a Node,
        callee: &str,
        at: SourcePosition,
    ) -> Option<TypeTarget<'a>> {
        if owner.language != "python" {
            return None;
        }
        let (head, member) = callee.split_once('.').unwrap_or((callee, ""));
        if !matches!(member, "" | "getLogger") {
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
                    if imported_module != "logging" || *position > at {
                        return None;
                    }
                    let canonical = match (reference.expression.as_str(), member) {
                        ("logging", "getLogger") | ("getLogger", "") => "logging.Logger",
                        _ => return None,
                    };
                    let Symbol::Type(TypeTarget::External {
                        module,
                        import_line,
                    }) = self.resolve_symbol(owner, callee, at, SymbolRole::Callable)
                    else {
                        return None;
                    };
                    if module != "logging" || position.line != import_line {
                        return None;
                    }
                    return Some(TypeTarget::External {
                        module: canonical.to_owned(),
                        import_line,
                    });
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
        _args: &[crate::core::semantic::TypeExpr],
        at: SourcePosition,
    ) -> TypeTarget<'a> {
        match self.resolve_symbol(owner, base, at, SymbolRole::Type) {
            Symbol::Type(TypeTarget::Builtin(name))
                if languages::by_id(&owner.language)
                    .is_some_and(|profile| profile.builtin_generic(&name)) =>
            {
                TypeTarget::Builtin(name)
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
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '$'))
        {
            return Symbol::Unknown;
        }
        if name == "Self" && owner.language == "rust" {
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
        let (head, member) = name.split_once('.').unwrap_or((&name, ""));
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
                            Imported::Nodes(_, position)
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
                        return match import {
                            Imported::Nodes(targets, position) if *position <= at => {
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
                            Imported::External(module, position, _, _) if *position <= at => {
                                if (owner.language == "python"
                                    && matches!(role, SymbolRole::Constructor))
                                    || (matches!(module.as_str(), "typing" | "typing_extensions")
                                        && member == "Any")
                                {
                                    Symbol::Unknown
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
}
