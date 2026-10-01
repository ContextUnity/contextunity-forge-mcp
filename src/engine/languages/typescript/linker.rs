use super::*;
use crate::core::semantic::SourcePosition;
use crate::engine::languages;
use crate::engine::linker::value_flow::{SemanticResolver, Symbol, SymbolRole, TypeTarget};
use hashbrown::{HashMap, HashSet};
use std::collections::BTreeSet;
use std::hash::BuildHasher;

type MemberMap<'a> = HashMap<(&'a str, bool), Vec<&'a Node>>;

pub(crate) struct TypeScriptMembers<'a> {
    members: HashMap<&'a str, MemberMap<'a>>,
}

impl<'a> TypeScriptMembers<'a> {
    pub(crate) fn build<F: AsRef<Facts>>(
        all: &'a BTreeMap<String, F>,
        resolver: &impl SemanticResolver<'a>,
    ) -> Self {
        let classes: Vec<_> = all
            .values()
            .flat_map(|facts| &facts.as_ref().nodes)
            .filter(|node| {
                matches!(node.language.as_str(), "typescript" | "javascript" | "vue")
                    && matches!(node.kind.as_str(), "class" | "interface")
            })
            .collect();
        let mut own = HashMap::<&str, MemberMap<'a>>::new();
        let mut parents = HashMap::<&str, Option<Vec<&Node>>>::new();
        for class in &classes {
            let Some(facts) = all.get(&class.path) else {
                continue;
            };
            let facts = facts.as_ref();
            let prefix = format!("{}.", class.qualname);
            let methods = own.entry(class.id.as_str()).or_default();
            for method in &facts.nodes {
                if !matches!(method.kind.as_str(), "method" | "function") {
                    continue;
                }
                if let Some(name) = method
                    .qualname
                    .strip_prefix(&prefix)
                    .filter(|name| !name.contains('.'))
                {
                    methods
                        .entry((name, method.details["is_static"] == true))
                        .or_default()
                        .push(method);
                }
            }
            for candidates in methods.values_mut() {
                if candidates
                    .iter()
                    .any(|node| node.details["is_stub"] != true)
                {
                    candidates.retain(|node| node.details["is_stub"] != true);
                }
                candidates.sort_unstable_by(|a, b| a.id.cmp(&b.id));
            }
            let mut bases = Vec::new();
            let mut invalid = false;
            for reference in facts
                .references
                .iter()
                .filter(|reference| reference.source == class.id && reference.kind == "inherits")
            {
                match resolver.resolve_symbol(
                    class,
                    &reference.expression,
                    SourcePosition {
                        line: reference.line,
                        column: reference.column,
                    },
                    SymbolRole::Type,
                ) {
                    Symbol::Type(TypeTarget::Local(base))
                        if matches!(base.kind.as_str(), "class" | "interface") =>
                    {
                        if !bases.iter().any(|node: &&Node| node.id == base.id) {
                            bases.push(base);
                        }
                    }
                    _ => invalid = true,
                }
            }
            if class.kind == "class" && bases.len() > 1 {
                invalid = true;
            }
            parents.insert(class.id.as_str(), (!invalid).then_some(bases));
        }
        let mut members = HashMap::new();
        let mut states = HashMap::new();
        let mut invalid = HashSet::new();
        for class in &classes {
            validate_chain(class.id.as_str(), &parents, &mut states, &mut invalid, 0);
        }
        for class in &invalid {
            parents.insert(class, None);
        }
        for class in classes {
            inherited(class, &own, &parents, &mut members, &mut HashSet::new(), 0);
        }
        Self { members }
    }

    pub(crate) fn lookup(&self, receiver: &Node, member: &str, associated: bool) -> &[&'a Node] {
        let Some(members) = self.members.get(receiver.id.as_str()) else {
            return &[];
        };
        let hash = members.hasher().hash_one((member, associated));
        members
            .raw_entry()
            .from_hash(hash, |(name, is_static)| {
                *name == member && *is_static == associated
            })
            .map_or(&[], |(_, nodes)| nodes.as_slice())
    }
}

fn validate_chain<'a>(
    class: &'a str,
    parents: &HashMap<&'a str, Option<Vec<&'a Node>>>,
    states: &mut HashMap<&'a str, u8>,
    invalid: &mut HashSet<&'a str>,
    depth: usize,
) -> bool {
    if states.get(class) == Some(&2) {
        return !invalid.contains(class);
    }
    if depth >= 128 || states.get(class) == Some(&1) {
        invalid.insert(class);
        return false;
    }
    states.insert(class, 1);
    let valid = match parents.get(class) {
        Some(Some(bases)) => bases
            .iter()
            .all(|base| validate_chain(base.id.as_str(), parents, states, invalid, depth + 1)),
        Some(None) | None => false,
    };
    if !valid {
        invalid.insert(class);
    }
    states.insert(class, 2);
    valid
}

fn inherited<'a>(
    class: &'a Node,
    own: &HashMap<&'a str, MemberMap<'a>>,
    parents: &HashMap<&'a str, Option<Vec<&'a Node>>>,
    completed: &mut HashMap<&'a str, MemberMap<'a>>,
    visiting: &mut HashSet<&'a str>,
    depth: usize,
) -> MemberMap<'a> {
    if let Some(members) = completed.get(class.id.as_str()) {
        return members.clone();
    }
    let direct = own.get(class.id.as_str()).cloned().unwrap_or_default();
    if depth >= 128 || !visiting.insert(class.id.as_str()) {
        return direct;
    }
    let mut result = direct.clone();
    if let Some(Some(bases)) = parents.get(class.id.as_str()) {
        for base in bases {
            if visiting.contains(base.id.as_str()) {
                continue;
            }
            for (key, methods) in inherited(base, own, parents, completed, visiting, depth + 1) {
                if direct.contains_key(&key) {
                    continue;
                }
                let candidates = result.entry(key).or_default();
                for method in methods {
                    if !candidates.iter().any(|node| node.id == method.id) {
                        candidates.push(method);
                    }
                }
                candidates.sort_unstable_by(|a, b| a.id.cmp(&b.id));
            }
        }
    }
    visiting.remove(class.id.as_str());
    completed.insert(class.id.as_str(), result.clone());
    result
}
use crate::engine::linker::traits::{
    resolve_default_import, ImportContext, ImportResolution, LanguageLinker, ModulesByNamespace,
    PackageExports,
};

pub(crate) struct TypeScriptLinker;
pub(crate) static TYPESCRIPT_LINKER: TypeScriptLinker = TypeScriptLinker;

fn javascript(file: &Facts) -> bool {
    file.nodes
        .iter()
        .any(|node| matches!(node.language.as_str(), "typescript" | "javascript" | "vue"))
}

fn provider<'a>(
    path: &str,
    module: &str,
    modules: &ModulesByNamespace<'a>,
    root: Option<&Path>,
) -> Option<&'a Node> {
    let namespace = TYPESCRIPT.normalize_import_with_root(root, path, module)?;
    let candidates = modules
        .get(&LanguageFamily("javascript"))?
        .get(&namespace.namespace)?;
    let owner = languages::workspace_path(path).0;
    let local: Vec<_> = candidates
        .iter()
        .copied()
        .filter(|node| languages::workspace_path(&node.path).0 == owner)
        .collect();
    let candidates = if local.is_empty() && !namespace.relative {
        candidates.as_slice()
    } else {
        &local
    };
    match candidates {
        [node] => Some(*node),
        _ => None,
    }
}

fn target<'a>(exports: &PackageExports<'a>, module: &Node, name: &str) -> Option<&'a Node> {
    exports
        .get(module.path.as_str())?
        .get(name)
        .copied()
        .flatten()
}

impl LanguageLinker for TypeScriptLinker {
    fn tracks_external_aliases(&self) -> bool {
        true
    }
    fn resolve_import<'ctx, 'a, 'input>(
        &self,
        context: &mut ImportContext<'ctx, 'a, 'input>,
    ) -> ImportResolution<'a> {
        let mut result = resolve_default_import(context);
        context
            .candidates
            .retain(|node| !matches!(node.kind.as_str(), "type" | "interface"));
        if let (Some([module]), Some(name)) = (
            Some(context.modules.as_slice()),
            context.tail().map(str::to_owned),
        ) {
            if !name.is_empty() {
                let exported = target(context.package_exports, module, &name);
                if name != "default" || exported.is_some() {
                    context.candidates.clear();
                }
                if let Some(node) = exported {
                    result.reexport_symbol = node.path != module.path || node.name != name;
                    context.candidates.push(node);
                }
            }
        }
        result
    }
    fn resolve_imported_member<'a>(
        &self,
        module: &'a Node,
        member: &str,
        _by_module: &HashMap<&'a str, Vec<&'a Node>>,
        _by_qual: &HashMap<&'a str, Vec<&'a Node>>,
        exports: &PackageExports<'a>,
        _lookup_key: &mut String,
    ) -> Vec<&'a Node> {
        target(exports, module, member).into_iter().collect()
    }
    fn package_exports<'a>(
        &self,
        all: &'a BTreeMap<String, Facts>,
        modules: &ModulesByNamespace<'a>,
        by_module: &HashMap<&'a str, Vec<&'a Node>>,
        root: Option<&Path>,
    ) -> PackageExports<'a> {
        package_exports(all, modules, by_module, root)
    }
    fn package_exports_borrowed<'a>(
        &self,
        all: &'a BTreeMap<String, &'a Facts>,
        modules: &ModulesByNamespace<'a>,
        by_module: &HashMap<&'a str, Vec<&'a Node>>,
        root: Option<&Path>,
    ) -> PackageExports<'a> {
        package_exports(all, modules, by_module, root)
    }
    fn needs_reference_identity(&self, _path: &str, facts: &Facts) -> bool {
        javascript(facts)
    }
    fn reference_identity_changed(&self, _path: &str, old: &Facts, new: &Facts) -> bool {
        let imports = |facts: &Facts| {
            facts
                .references
                .iter()
                .filter(|reference| reference.kind == "imports")
                .map(|reference| {
                    (
                        reference.expression.clone(),
                        reference.alias.clone(),
                        reference.module.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        imports(old) != imports(new)
    }
    fn required_full_facts(
        &self,
        facts: &BTreeMap<String, Facts>,
        _affected: &BTreeSet<String>,
        catalog: &[(&str, &str)],
    ) -> BTreeSet<String> {
        required_full_facts(facts, _affected, catalog)
    }
    fn required_full_facts_borrowed(
        &self,
        facts: &BTreeMap<String, &Facts>,
        _affected: &BTreeSet<String>,
        catalog: &[(&str, &str)],
    ) -> BTreeSet<String> {
        required_full_facts(facts, _affected, catalog)
    }
}

fn package_exports<'a, F: AsRef<Facts>>(
    all: &'a BTreeMap<String, F>,
    modules: &ModulesByNamespace<'a>,
    by_module: &HashMap<&'a str, Vec<&'a Node>>,
    root: Option<&Path>,
) -> PackageExports<'a> {
    let mut exports = PackageExports::new();
    let mut pending = Vec::new();
    let mut seen = HashSet::new();
    let mut stars = Vec::new();
    let mut providers = HashMap::new();
    let mut resolve_provider = |path: &'a str, source: &'a str| {
        *providers
            .entry((path, source))
            .or_insert_with(|| provider(path, source, modules, root))
    };
    for (path, file) in all.iter().filter(|(_, file)| javascript(file.as_ref())) {
        let file = file.as_ref();
        let Some(module) = file.nodes.iter().find(|node| node.kind == "module") else {
            continue;
        };
        let Some(bindings) = module.details["exports"].as_array() else {
            continue;
        };
        for binding in bindings {
            if binding["type_only"] == true {
                continue;
            }
            if binding["star"] == true {
                if let Some(provider) = binding["module"]
                    .as_str()
                    .and_then(|source| resolve_provider(path, source))
                {
                    stars.push((path.as_str(), provider));
                }
                continue;
            }
            let Some(name) = binding["name"].as_str() else {
                continue;
            };
            let local = binding["local"].as_str();
            if local.is_some_and(|local| {
                module.details["rebindings"]
                    .as_array()
                    .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(local)))
            }) {
                continue;
            }
            if !seen.insert((path.as_str(), name)) {
                exports.entry(path.as_str()).or_default().insert(name, None);
                pending.retain(|(owner, alias, _, _)| *owner != path.as_str() || *alias != name);
                continue;
            }
            let mut source = binding["module"].as_str();
            let mut member = local;
            if source.is_none() {
                if let Some(reference) = file.references.iter().find(|reference| {
                    reference.kind == "imports"
                        && reference.source == module.id
                        && reference.alias.as_deref() == local
                }) {
                    source = reference.module.as_deref();
                    member = if reference.expression == "*"
                        || source == Some(reference.expression.as_str())
                    {
                        None
                    } else {
                        Some(reference.expression.as_str())
                    };
                }
            }
            let owner = exports.entry(path.as_str()).or_default();
            owner.insert(name, None);
            if let Some(source) = source {
                if let Some(provider) = resolve_provider(path, source) {
                    if let Some(member) = member {
                        pending.push((path.as_str(), name, provider, member));
                    } else {
                        owner.insert(name, Some(provider));
                    }
                }
            } else if let Some(local) = local {
                let qualname = format!("{}.{}", module.qualname, local);
                let mut nodes = by_module
                    .get(path.as_str())
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|node| {
                        node.qualname == qualname
                            && !matches!(node.kind.as_str(), "type" | "interface")
                    });
                if let Some(node) = nodes.next() {
                    if nodes.next().is_none() {
                        owner.insert(name, Some(node));
                    }
                }
            }
        }
    }
    loop {
        let mut additions = HashMap::<(&str, &str), Option<&Node>>::new();
        for (owner, provider) in &stars {
            for (&name, &node) in exports.get(provider.path.as_str()).into_iter().flatten() {
                if name == "default" || seen.contains(&(*owner, name)) {
                    continue;
                }
                let entry = additions.entry((*owner, name)).or_insert(node);
                if entry.map(|node| node.id.as_str()) != node.map(|node| node.id.as_str()) {
                    *entry = None;
                }
            }
        }
        let mut changed = false;
        for ((owner, name), node) in additions {
            let existing = exports.entry(owner).or_default();
            if !existing.contains_key(name)
                || existing
                    .get(name)
                    .copied()
                    .flatten()
                    .map(|node| node.id.as_str())
                    != node.map(|node| node.id.as_str())
            {
                existing.insert(name, node);
                changed = true;
            }
        }
        for (owner, name, provider, member) in &pending {
            let node = target(&exports, provider, member);
            let existing = exports.get_mut(owner).expect("export owner exists");
            if existing
                .get(name)
                .copied()
                .flatten()
                .map(|node| node.id.as_str())
                != node.map(|node| node.id.as_str())
            {
                existing.insert(name, node);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    exports
}

fn required_full_facts<F: AsRef<Facts>>(
    facts: &BTreeMap<String, F>,
    _affected: &BTreeSet<String>,
    catalog: &[(&str, &str)],
) -> BTreeSet<String> {
    let mut requested = HashSet::new();
    for (path, file) in facts.iter().filter(|(_, file)| javascript(file.as_ref())) {
        let file = file.as_ref();
        let imports = file
            .references
            .iter()
            .filter(|reference| reference.kind == "imports")
            .filter_map(|reference| reference.module.as_deref());
        let exports = file
            .nodes
            .iter()
            .filter(|node| node.kind == "module")
            .flat_map(|module| module.details["exports"].as_array().into_iter().flatten())
            .filter_map(|binding| binding["module"].as_str());
        for import in imports.chain(exports) {
            if let Some(import) = TYPESCRIPT.normalize_import(path, import) {
                requested.insert((
                    languages::workspace_path(path).0,
                    import.namespace,
                    import.relative,
                ));
            }
        }
    }
    catalog
        .iter()
        .filter_map(|(path, language)| {
            if !matches!(*language, "javascript" | "typescript" | "vue") {
                return None;
            }
            let (workspace, local) = languages::workspace_path(path);
            let names = [TYPESCRIPT.module_name(path), TYPESCRIPT.module_name(local)];
            requested
                .iter()
                .any(|(owner, name, relative)| {
                    (!relative || *owner == workspace) && names.contains(name)
                })
                .then(|| (*path).to_owned())
        })
        .collect()
}
