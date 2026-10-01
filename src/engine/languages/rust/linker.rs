use crate::core::models::{Facts, Node};
use crate::engine::linker::traits::LanguageLinker;
use hashbrown::{HashMap, HashSet};
use std::collections::BTreeMap;

pub(crate) struct RustLinker;
pub(crate) static RUST_LINKER: RustLinker = RustLinker;
impl LanguageLinker for RustLinker {
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
        for node in all
            .values()
            .flat_map(|facts| &facts.as_ref().nodes)
            .filter(|node| node.language == "rust")
        {
            by_qual.entry(&node.qualname).or_default().push(node);
        }
        let mut index = Self {
            inherent: HashMap::new(),
            traits: HashMap::new(),
            visible_traits: HashMap::new(),
            trait_members: HashMap::new(),
            receivers_by_method: HashMap::new(),
        };
        for facts in all.values() {
            let facts = facts.as_ref();
            let by_id: HashMap<_, _> = facts
                .nodes
                .iter()
                .map(|node| (node.id.as_str(), node))
                .collect();
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
                if let Some(name) = import.alias.as_deref() {
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
            for owner in facts
                .nodes
                .iter()
                .filter(|node| node.kind == "impl" && node.details["generic_impl"] != true)
            {
                let Some(receiver) = owner.details["receiver_type"]
                    .as_str()
                    .and_then(|name| resolve_type(facts, owner, name, &by_qual))
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
            for edge in facts.edges.iter().filter(|edge| edge.kind == "contains") {
                let (Some(owner), Some(method)) =
                    (by_id.get(edge.src.as_str()), by_id.get(edge.dst.as_str()))
                else {
                    continue;
                };
                if owner.kind != "impl"
                    || method.kind != "method"
                    || owner.details["generic_impl"] == true
                {
                    continue;
                }
                let Some((receiver, trait_node)) = impl_receivers.get(owner.id.as_str()) else {
                    continue;
                };
                index.receivers_by_method.insert(&method.id, receiver);
                let key = (
                    receiver.id.as_str(),
                    method.name.as_str(),
                    method.details["is_static"] == true,
                );
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
