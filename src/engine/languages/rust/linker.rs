use crate::core::models::{Facts, Node};
use crate::engine::languages::{self, ImportPath, LanguageFamily, LanguageProfile};
use crate::engine::linker::traits::{
    resolve_default_import, ImportContext, ImportResolution, LanguageLinker, ModulesByNamespace,
    PackageExports,
};
use hashbrown::{HashMap, HashSet};
use std::collections::BTreeMap;

const RUST_ALIAS_DEPTH: usize = 64;

fn direct_members<'a, F: AsRef<Facts>>(
    all: &'a BTreeMap<String, F>,
    module: &'a Node,
    name: &str,
) -> Vec<&'a Node> {
    let qualified = format!("{}.{}", module.qualname, name);
    all.get(module.path.as_str())
        .into_iter()
        .flat_map(|facts| facts.as_ref().nodes.iter())
        .filter(|node| node.qualname == qualified && node.kind != "module")
        .collect()
}

fn is_public_export(module: &Node, name: &str) -> bool {
    module.details["exports"].as_array().is_some_and(|exports| {
        exports
            .iter()
            .any(|export| export["name"].as_str() == Some(name))
    })
}

fn modules_at_import<'a>(
    modules_by_namespace: &ModulesByNamespace<'a>,
    family: LanguageFamily,
    normalized: &ImportPath,
    workspace: &str,
) -> Vec<(&'a Node, String)> {
    let mut namespace = normalized.namespace.as_str();
    loop {
        let modules: Vec<_> = modules_by_namespace
            .get(&family)
            .and_then(|namespaces| namespaces.get(namespace))
            .into_iter()
            .flatten()
            .copied()
            .filter(|module| languages::workspace_path(&module.path).0 == workspace)
            .collect();
        if !modules.is_empty() {
            let tail = normalized
                .namespace
                .strip_prefix(namespace)
                .unwrap_or_default()
                .trim_start_matches('.')
                .to_owned();
            return modules
                .into_iter()
                .map(|module| (module, tail.clone()))
                .collect();
        }
        if !normalized.symbol_path {
            return Vec::new();
        }
        let Some((parent, _)) = namespace.rsplit_once('.') else {
            return Vec::new();
        };
        namespace = parent;
    }
}

type AliasKey = (String, String, bool);

fn is_descendant_module(requester: &str, owner: &Node) -> bool {
    requester
        .strip_prefix(owner.qualname.as_str())
        .is_some_and(|suffix| suffix.starts_with('.'))
}

struct AliasResolver<'a, 'lookup, F: AsRef<Facts>> {
    all: &'a BTreeMap<String, F>,
    modules_by_namespace: &'lookup ModulesByNamespace<'a>,
    normalized_imports: &'lookup HashMap<(&'a str, &'a str), Option<ImportPath>>,
    memo: HashMap<AliasKey, Vec<&'a Node>>,
    visiting: HashSet<AliasKey>,
}

impl<'a, 'lookup, F: AsRef<Facts>> AliasResolver<'a, 'lookup, F> {
    fn resolve(
        &mut self,
        module: &'a Node,
        name: &str,
        public_only: bool,
        depth: usize,
    ) -> Vec<&'a Node> {
        if depth >= RUST_ALIAS_DEPTH || (public_only && !is_public_export(module, name)) {
            return Vec::new();
        }
        let key = (module.path.clone(), name.to_owned(), public_only);
        if let Some(candidates) = self.memo.get(&key) {
            return candidates.clone();
        }
        if !self.visiting.insert(key.clone()) {
            return Vec::new();
        }
        let mut candidates = Vec::new();
        if let Some(facts) = self.all.get(module.path.as_str()).map(AsRef::as_ref) {
            for reference in facts.references.iter().filter(|reference| {
                reference.kind == "imports"
                    && reference.source == module.id
                    && reference.alias.as_deref() == Some(name)
            }) {
                let raw = reference.module.as_deref().unwrap_or(&reference.expression);
                let Some(normalized) = self
                    .normalized_imports
                    .get(&(module.path.as_str(), raw))
                    .and_then(Option::as_ref)
                else {
                    continue;
                };
                let workspace = languages::workspace_path(&module.path).0;
                for (target_module, tail) in modules_at_import(
                    self.modules_by_namespace,
                    LanguageFamily("rust"),
                    normalized,
                    workspace,
                ) {
                    if tail.is_empty() {
                        candidates.push(target_module);
                        continue;
                    }
                    let direct = direct_members(self.all, target_module, &tail);
                    if !direct.is_empty() || tail.contains('.') {
                        candidates.extend(direct);
                    } else {
                        if !is_public_export(target_module, &tail)
                            && !is_descendant_module(&module.qualname, target_module)
                        {
                            continue;
                        }
                        candidates.extend(self.resolve(
                            target_module,
                            &tail,
                            public_only,
                            depth + 1,
                        ));
                    }
                }
            }
        }
        self.visiting.remove(&key);
        candidates.sort_by(|left, right| left.id.cmp(&right.id));
        candidates.dedup_by_key(|node| node.id.as_str());
        self.memo.insert(key, candidates.clone());
        candidates
    }
}

/// Canonical named-import targets prepared once from persisted Rust facts.
/// Private aliases are admitted only to descendant modules; public aliases use
/// only the finite export projection recorded by the Rust profile.
#[derive(Default)]
pub(crate) struct RustForwarding<'a> {
    private: HashMap<&'a str, HashMap<&'a str, Vec<&'a Node>>>,
    public: HashMap<&'a str, HashMap<&'a str, Vec<&'a Node>>>,
    exports: PackageExports<'a>,
}

impl<'a> RustForwarding<'a> {
    pub(crate) fn build<F: AsRef<Facts>>(
        all: &'a BTreeMap<String, F>,
        modules_by_namespace: &ModulesByNamespace<'a>,
        normalized_imports: &HashMap<(&'a str, &'a str), Option<ImportPath>>,
    ) -> Self {
        let mut resolver = AliasResolver {
            all,
            modules_by_namespace,
            normalized_imports,
            memo: HashMap::new(),
            visiting: HashSet::new(),
        };
        let mut private: HashMap<&'a str, HashMap<&'a str, Vec<&'a Node>>> = HashMap::new();
        let mut public: HashMap<&'a str, HashMap<&'a str, Vec<&'a Node>>> = HashMap::new();
        let mut exports = PackageExports::new();

        for facts in all.values() {
            let facts = facts.as_ref();
            let Some(module) = facts
                .nodes
                .iter()
                .find(|node| node.kind == "module" && node.language == "rust")
            else {
                continue;
            };
            let mut names = HashSet::new();
            for reference in facts.references.iter().filter(|reference| {
                reference.kind == "imports"
                    && reference.source == module.id
                    && reference.alias.as_deref().is_some_and(|name| name != "*")
            }) {
                if let Some(name) = reference.alias.as_deref() {
                    names.insert(name);
                }
            }

            for name in names {
                let private_targets = resolver.resolve(module, name, false, 0);
                if !private_targets.is_empty() {
                    private
                        .entry(module.path.as_str())
                        .or_default()
                        .insert(name, private_targets);
                }
                if is_public_export(module, name) {
                    let public_targets = resolver.resolve(module, name, true, 0);
                    public
                        .entry(module.path.as_str())
                        .or_default()
                        .insert(name, public_targets);
                }
            }

            if let Some(bindings) = module.details["exports"].as_array() {
                let members = exports.entry(module.path.as_str()).or_default();
                for name in bindings
                    .iter()
                    .filter_map(|binding| binding["name"].as_str())
                {
                    let targets = public
                        .get(module.path.as_str())
                        .and_then(|aliases| aliases.get(name))
                        .map(Vec::as_slice)
                        .unwrap_or_default();
                    members.insert(name, (targets.len() == 1).then(|| targets[0]));
                }
            }
        }

        Self {
            private,
            public,
            exports,
        }
    }

    pub(crate) fn package_exports(&self) -> &PackageExports<'a> {
        &self.exports
    }

    fn targets<'forwarding>(
        &'forwarding self,
        module: &Node,
        name: &str,
        requester: &str,
    ) -> &'forwarding [&'a Node] {
        if is_public_export(module, name) {
            return self
                .public
                .get(module.path.as_str())
                .and_then(|aliases| aliases.get(name))
                .map(Vec::as_slice)
                .unwrap_or_default();
        }
        if is_descendant_module(requester, module) {
            return self
                .private
                .get(module.path.as_str())
                .and_then(|aliases| aliases.get(name))
                .map(Vec::as_slice)
                .unwrap_or_default();
        }
        &[]
    }
}

fn declared_exports<'a, F: AsRef<Facts>>(all: &'a BTreeMap<String, F>) -> PackageExports<'a> {
    let mut exports = PackageExports::new();
    for facts in all.values() {
        let facts = facts.as_ref();
        let Some(module) = facts
            .nodes
            .iter()
            .find(|node| node.kind == "module" && node.language == "rust")
        else {
            continue;
        };
        let Some(bindings) = module.details["exports"].as_array() else {
            continue;
        };
        let members = exports.entry(module.path.as_str()).or_default();
        for name in bindings
            .iter()
            .filter_map(|binding| binding["name"].as_str())
        {
            members.insert(name, None);
        }
    }
    exports
}

pub(crate) struct RustLinker;
pub(crate) static RUST_LINKER: RustLinker = RustLinker;
impl LanguageLinker for RustLinker {
    fn needs_reference_identity(&self, _path: &str, facts: &Facts) -> bool {
        facts
            .references
            .iter()
            .any(|reference| reference.kind == "imports")
    }

    fn reference_identity_changed(&self, _path: &str, old: &Facts, new: &Facts) -> bool {
        fn imports(facts: &Facts) -> HashSet<(&str, Option<&str>, Option<&str>)> {
            facts
                .references
                .iter()
                .filter(|reference| reference.kind == "imports")
                .map(|reference| {
                    (
                        reference.expression.as_str(),
                        reference.alias.as_deref(),
                        reference.module.as_deref(),
                    )
                })
                .collect()
        }

        imports(old) != imports(new)
    }

    fn resolve_import<'ctx, 'a, 'input>(
        &self,
        context: &mut ImportContext<'ctx, 'a, 'input>,
    ) -> ImportResolution<'a> {
        let resolution = resolve_default_import(context);
        if !context.candidates.is_empty() {
            return resolution;
        }
        let Some(name) = context.tail().filter(|name| !name.is_empty()) else {
            return resolution;
        };
        if context.modules.len() != 1 {
            return resolution;
        }
        let module = context.modules[0];
        if module.language != "rust" {
            return resolution;
        }
        let Some(forwarding) = context.rust_forwarding else {
            return resolution;
        };
        let requester = context
            .by_module
            .get(context.path)
            .into_iter()
            .flatten()
            .find(|node| node.kind == "module")
            .map_or(module.qualname.as_str(), |owner| owner.qualname.as_str());
        context
            .candidates
            .extend(forwarding.targets(module, name, requester).iter().copied());
        resolution
    }

    fn resolve_imported_member<'a>(
        &self,
        module: &'a Node,
        member: &str,
        by_module: &HashMap<&'a str, Vec<&'a Node>>,
        _by_qual: &HashMap<&'a str, Vec<&'a Node>>,
        exports: &PackageExports<'a>,
        lookup_key: &mut String,
    ) -> Vec<&'a Node> {
        if let Some(Some(target)) = exports
            .get(module.path.as_str())
            .and_then(|members| members.get(member))
        {
            return vec![*target];
        }
        lookup_key.clear();
        lookup_key.push_str(&module.qualname);
        lookup_key.push('.');
        lookup_key.push_str(member);
        by_module
            .get(module.path.as_str())
            .into_iter()
            .flatten()
            .copied()
            .filter(|node| node.qualname == lookup_key.as_str() && node.kind != "module")
            .collect()
    }

    fn package_exports<'a>(
        &self,
        all: &'a BTreeMap<String, Facts>,
        _modules_by_namespace: &ModulesByNamespace<'a>,
        _by_module: &HashMap<&'a str, Vec<&'a Node>>,
        _root: Option<&std::path::Path>,
    ) -> PackageExports<'a> {
        declared_exports(all)
    }

    fn package_exports_borrowed<'a>(
        &self,
        all: &'a BTreeMap<String, &'a Facts>,
        _modules_by_namespace: &ModulesByNamespace<'a>,
        _by_module: &HashMap<&'a str, Vec<&'a Node>>,
        _root: Option<&std::path::Path>,
    ) -> PackageExports<'a> {
        declared_exports(all)
    }

    fn required_full_facts(
        &self,
        facts: &BTreeMap<String, Facts>,
        affected: &std::collections::BTreeSet<String>,
        catalog: &[(&str, &str)],
    ) -> std::collections::BTreeSet<String> {
        required_full_facts(facts, affected, catalog)
    }
    fn required_full_facts_borrowed(
        &self,
        facts: &BTreeMap<String, &Facts>,
        affected: &std::collections::BTreeSet<String>,
        catalog: &[(&str, &str)],
    ) -> std::collections::BTreeSet<String> {
        required_full_facts(facts, affected, catalog)
    }
}

#[derive(Hash)]
struct MemberLookup<'a>(&'a str, &'a str, bool);
impl hashbrown::Equivalent<(&str, &str, bool)> for MemberLookup<'_> {
    fn equivalent(&self, key: &(&str, &str, bool)) -> bool {
        self.0 == key.0 && self.1 == key.1 && self.2 == key.2
    }
}
#[derive(Hash)]
struct TraitLookup<'a>(&'a str, &'a str, bool, &'a str);
impl hashbrown::Equivalent<(&str, &str, bool, &str)> for TraitLookup<'_> {
    fn equivalent(&self, key: &(&str, &str, bool, &str)) -> bool {
        self.0 == key.0 && self.1 == key.1 && self.2 == key.2 && self.3 == key.3
    }
}

type TraitImplMap<'a> = HashMap<(&'a str, &'a str, bool), Vec<(&'a Node, &'a Node)>>;

pub(crate) struct RustMembers<'a> {
    inherent: HashMap<(&'a str, &'a str, bool), Vec<&'a Node>>,
    traits: TraitImplMap<'a>,
    visible_traits: HashMap<&'a str, HashSet<&'a str>>,
    trait_members: HashMap<(&'a str, &'a str, bool, &'a str), Vec<&'a Node>>,
    receivers_by_method: HashMap<&'a str, &'a Node>,
    dynamic_declarations: HashMap<(&'a str, &'a str), &'a Node>,
    dynamic_implementors: HashMap<(&'a str, &'a str), Vec<&'a Node>>,
}

fn namespace(path: &str, module: &str, name: &str) -> String {
    let (_, path) = super::workspace_path(path);
    if let Some(tail) = name.strip_prefix("crate::") {
        let root = path
            .split_once("/src/")
            .map(|(prefix, _)| format!("{}.src", prefix.replace('/', ".")))
            .unwrap_or_else(|| "src".to_owned());
        format!("{root}.{}", tail.replace("::", "."))
    } else if let Some(tail) = name.strip_prefix("self::") {
        format!("{module}.{}", tail.replace("::", "."))
    } else if let Some(mut tail) = name.strip_prefix("super::") {
        let mut module = module.rsplit_once('.').map_or("", |(parent, _)| parent);
        while let Some(rest) = tail.strip_prefix("super::") {
            module = module.rsplit_once('.').map_or("", |(parent, _)| parent);
            tail = rest;
        }
        format!("{module}.{}", tail.replace("::", "."))
    } else {
        format!("{module}.{}", name.replace("::", "."))
    }
}

fn resolve_type<'a>(
    facts: &'a Facts,
    scope: &Node,
    name: &str,
    by_qual: &HashMap<&str, Vec<&'a Node>>,
) -> Option<&'a Node> {
    if name.is_empty() || name.contains(['<', '>', '&', '[', '(', '!']) {
        return None;
    }
    let module = facts.nodes.iter().find(|node| node.kind == "module")?;
    let (head, tail) = name.split_once("::").unwrap_or((name, ""));
    let imports: Vec<_> = facts
        .references
        .iter()
        .filter(|reference| {
            reference.kind == "imports"
                && reference.alias.as_deref() == Some(head)
                && (reference.source == module.id || reference.source == scope.id)
        })
        .collect();
    let qualified = match imports.as_slice() {
        [] => namespace(&scope.path, &module.qualname, name),
        [import] => {
            let target = import.module.as_deref()?;
            let target = if tail.is_empty() {
                target.to_owned()
            } else {
                format!("{target}::{tail}")
            };
            namespace(&scope.path, &module.qualname, &target)
        }
        _ => return None,
    };
    let candidates = by_qual.get(qualified.as_str())?;
    let workspace = super::workspace_path(&scope.path).0;
    let mut candidates = candidates.iter().copied().filter(|node| {
        super::workspace_path(&node.path).0 == workspace
            && matches!(node.kind.as_str(), "struct" | "enum" | "trait" | "type")
    });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

impl<'a> RustMembers<'a> {
    pub(crate) fn build<F: AsRef<Facts>>(all: &'a BTreeMap<String, F>) -> Self {
        let mut by_qual = HashMap::<&str, Vec<&Node>>::new();
        let mut by_id = HashMap::<&str, &Node>::new();
        for node in all
            .values()
            .flat_map(|facts| &facts.as_ref().nodes)
            .filter(|node| node.language == "rust")
        {
            by_qual.entry(&node.qualname).or_default().push(node);
            by_id.insert(node.id.as_str(), node);
        }
        let mut trait_defaults = HashMap::<&str, Vec<&Node>>::new();
        for facts in all.values() {
            for edge in facts
                .as_ref()
                .edges
                .iter()
                .filter(|edge| edge.kind == "contains")
            {
                let (Some(owner), Some(method)) =
                    (by_id.get(edge.src.as_str()), by_id.get(edge.dst.as_str()))
                else {
                    continue;
                };
                if owner.kind == "trait"
                    && method.kind == "method"
                    && method.details["trait_default"] == true
                    && method.details["unproven_bounds"] != true
                {
                    trait_defaults
                        .entry(owner.id.as_str())
                        .or_default()
                        .push(method);
                }
            }
        }
        let mut index = Self {
            inherent: HashMap::new(),
            traits: HashMap::new(),
            visible_traits: HashMap::new(),
            trait_members: HashMap::new(),
            receivers_by_method: HashMap::new(),
            dynamic_declarations: HashMap::new(),
            dynamic_implementors: HashMap::new(),
        };
        for facts in all.values() {
            for edge in facts
                .as_ref()
                .edges
                .iter()
                .filter(|edge| edge.kind == "contains")
            {
                let (Some(owner), Some(method)) =
                    (by_id.get(edge.src.as_str()), by_id.get(edge.dst.as_str()))
                else {
                    continue;
                };
                if owner.kind == "trait"
                    && method.kind == "method"
                    && method.details["is_static"] != true
                    && method.details["unproven_bounds"] != true
                {
                    index
                        .dynamic_declarations
                        .insert((owner.id.as_str(), method.name.as_str()), method);
                }
            }
        }
        for facts in all.values() {
            let facts = facts.as_ref();
            let Some(module) = facts
                .nodes
                .iter()
                .find(|node| node.kind == "module" && node.language == "rust")
            else {
                continue;
            };
            for import in facts
                .references
                .iter()
                .filter(|reference| reference.kind == "imports")
            {
                if import.alias.as_deref() == Some("*") {
                    let target_expr = import.module.as_deref().unwrap_or(&import.expression);
                    if let Some(normalized) =
                        super::RUST.normalize_import(&module.path, target_expr)
                    {
                        for target in by_qual
                            .get(normalized.namespace.as_str())
                            .into_iter()
                            .flatten()
                        {
                            if target.kind == "trait" {
                                index
                                    .visible_traits
                                    .entry(&module.path)
                                    .or_default()
                                    .insert(&target.id);
                            }
                        }
                    }
                } else if let Some(name) = import.alias.as_deref() {
                    if let Some(target) = resolve_type(facts, module, name, &by_qual)
                        .filter(|node| node.kind == "trait")
                    {
                        index
                            .visible_traits
                            .entry(&module.path)
                            .or_default()
                            .insert(&target.id);
                    }
                }
            }
            for node in &facts.nodes {
                if node.kind == "trait"
                    && node
                        .qualname
                        .rsplit_once('.')
                        .is_some_and(|(scope, _)| scope == module.qualname)
                {
                    index
                        .visible_traits
                        .entry(&node.path)
                        .or_default()
                        .insert(&node.id);
                }
            }
            let mut impl_receivers = HashMap::new();
            for owner in facts.nodes.iter().filter(|node| {
                node.kind == "impl"
                    && (node.details["generic_impl"] != true
                        || node.details["uniform_generic_impl"] == true)
            }) {
                let Some(receiver) = owner.details["receiver_type"]
                    .as_str()
                    .and_then(|name| {
                        let nominal = if owner.details["uniform_generic_impl"] == true {
                            name.split_once('<').map_or(name, |(base, _)| base)
                        } else {
                            name
                        };
                        resolve_type(facts, owner, nominal, &by_qual)
                    })
                    .filter(|node| matches!(node.kind.as_str(), "struct" | "enum"))
                else {
                    continue;
                };
                let trait_node = match owner.details["impl_trait"].as_str() {
                    Some(name) => {
                        let Some(trait_node) = resolve_type(facts, owner, name, &by_qual)
                            .filter(|node| node.kind == "trait")
                        else {
                            continue;
                        };
                        Some(trait_node)
                    }
                    None => None,
                };
                impl_receivers.insert(owner.id.as_str(), (receiver, trait_node));
            }
            let mut implemented = HashMap::<&str, HashSet<(&str, bool)>>::new();
            for edge in facts.edges.iter().filter(|edge| edge.kind == "contains") {
                let (Some(owner), Some(method)) =
                    (by_id.get(edge.src.as_str()), by_id.get(edge.dst.as_str()))
                else {
                    continue;
                };
                if owner.kind != "impl"
                    || method.kind != "method"
                    || method.details["unproven_bounds"] == true
                {
                    continue;
                }
                let Some((receiver, trait_node)) = impl_receivers.get(owner.id.as_str()) else {
                    continue;
                };
                index.receivers_by_method.insert(&method.id, receiver);
                let associated = method.details["is_static"] == true;
                implemented
                    .entry(owner.id.as_str())
                    .or_default()
                    .insert((method.name.as_str(), associated));
                let key = (receiver.id.as_str(), method.name.as_str(), associated);
                if let Some(trait_node) = trait_node {
                    index
                        .traits
                        .entry(key)
                        .or_default()
                        .push((method, trait_node));
                } else {
                    index.inherent.entry(key).or_default().push(method);
                }
            }
            for (owner_id, (receiver, trait_node)) in impl_receivers {
                let Some(trait_node) = trait_node else {
                    continue;
                };
                for method in trait_defaults
                    .get(trait_node.id.as_str())
                    .into_iter()
                    .flatten()
                {
                    let associated = method.details["is_static"] == true;
                    if implemented.get(owner_id).is_some_and(|methods| {
                        methods.contains(&(method.name.as_str(), associated))
                    }) {
                        continue;
                    }
                    index
                        .traits
                        .entry((receiver.id.as_str(), method.name.as_str(), associated))
                        .or_default()
                        .push((method, trait_node));
                }
            }
        }
        for methods in index.inherent.values_mut() {
            methods.sort_by_key(|method| &method.id);
            methods.dedup_by_key(|method| method.id.as_str());
        }
        for methods in index.traits.values_mut() {
            methods.sort_by_key(|(method, _)| &method.id);
            methods.dedup_by_key(|(method, _)| method.id.as_str());
        }
        let mut trait_consumers = HashMap::<&str, Vec<&str>>::new();
        for (path, traits) in &index.visible_traits {
            for trait_id in traits {
                trait_consumers.entry(trait_id).or_default().push(path);
            }
        }
        for ((receiver, member, associated), methods) in &index.traits {
            for (method, trait_node) in methods {
                for path in trait_consumers
                    .get(trait_node.id.as_str())
                    .into_iter()
                    .flatten()
                {
                    index
                        .trait_members
                        .entry((receiver, member, *associated, path))
                        .or_default()
                        .push(method);
                }
            }
        }
        for methods in index.trait_members.values_mut() {
            methods.sort_by_key(|method| &method.id);
            methods.dedup_by_key(|method| method.id.as_str());
        }
        for ((_, member, associated), methods) in &index.traits {
            if *associated {
                continue;
            }
            for (method, trait_node) in methods {
                let key = (trait_node.id.as_str(), *member);
                if index
                    .dynamic_declarations
                    .get(&key)
                    .is_some_and(|declaration| declaration.id != method.id)
                {
                    index
                        .dynamic_implementors
                        .entry(key)
                        .or_default()
                        .push(method);
                }
            }
        }
        for methods in index.dynamic_implementors.values_mut() {
            methods.sort_by_key(|method| &method.id);
            methods.dedup_by_key(|method| method.id.as_str());
            if methods.len() > crate::engine::linker::value_flow::MAX_CANDIDATE_BRANCH_JOINS {
                methods.clear();
            }
        }
        index.traits.clear();
        index.visible_traits.clear();
        index
    }

    pub(crate) fn lookup(
        &self,
        receiver: &Node,
        member: &str,
        associated: bool,
        caller: &Node,
    ) -> &[&'a Node] {
        let key = MemberLookup(receiver.id.as_str(), member, associated);
        if let Some(methods) = self.inherent.get(&key) {
            return methods;
        }
        self.trait_members
            .get(&TraitLookup(
                receiver.id.as_str(),
                member,
                associated,
                caller.path.as_str(),
            ))
            .map_or(&[], Vec::as_slice)
    }

    pub(crate) fn receiver_for(&self, method: &Node) -> Option<&'a Node> {
        self.receivers_by_method.get(method.id.as_str()).copied()
    }
    pub(crate) fn dynamic_dispatch<'s>(
        &'s self,
        receiver: &'s Node,
        member: &'s str,
    ) -> Option<(&'a Node, &'s [&'a Node])> {
        let key = (receiver.id.as_str(), member);
        let declaration = self.dynamic_declarations.get(&key)?;
        Some((
            *declaration,
            self.dynamic_implementors
                .get(&key)
                .map_or(&[], Vec::as_slice),
        ))
    }
}

fn required_full_facts<F: AsRef<Facts>>(
    facts: &BTreeMap<String, F>,
    affected: &std::collections::BTreeSet<String>,
    catalog: &[(&str, &str)],
) -> std::collections::BTreeSet<String> {
    let workspaces: HashSet<_> = facts
        .iter()
        .filter(|(path, facts)| {
            let facts = facts.as_ref();
            affected.contains(*path)
                && facts.nodes.iter().any(|node| node.language == "rust")
                && facts.references.iter().any(|reference| {
                    reference.kind == "calls"
                        && (reference.expression.contains('.')
                            || reference.expression.contains("::"))
                })
        })
        .map(|(path, _)| super::workspace_path(path).0)
        .collect();
    catalog
        .iter()
        .filter(|(path, language)| {
            *language == "rust" && workspaces.contains(super::workspace_path(path).0)
        })
        .map(|(path, _)| (*path).to_owned())
        .collect()
}
