use super::*;

pub(super) struct Provider<'a> {
    pub(super) node: &'a Node,
    pub(super) line: usize,
    pub(super) column: usize,
}

struct Include<'a> {
    parent: &'a str,
    line: usize,
}

pub(super) struct Registry<'a> {
    by_owner: HashMap<&'a str, HashMap<&'a str, Vec<Provider<'a>>>>,
    incoming: HashMap<&'a str, Vec<Include<'a>>>,
}

impl<'a> Registry<'a> {
    pub(super) fn build<F: AsRef<Facts> + Sync>(
        all: &'a BTreeMap<String, F>,
        normalized_imports: &HashMap<(&str, &str), Option<ImportPath>>,
        modules_by_namespace: &HashMap<LanguageFamily, HashMap<String, Vec<&'a Node>>>,
        dependencies: &languages::dependency_registry::DependencyRegistry,
    ) -> Self {
        let mut by_owner: HashMap<&str, HashMap<&str, Vec<Provider<'a>>>> = HashMap::new();
        let mut incoming: HashMap<&str, Vec<Include<'a>>> = HashMap::new();
        for (path, source) in all {
            let facts = source.as_ref();
            if !facts
                .nodes
                .iter()
                .any(|node| node.kind == "module" && node.language == "html")
            {
                continue;
            }
            let workspace = languages::workspace_path(path).0;
            let scope =
                dependencies.nearest_manifest_scope_for_path(LanguageFamily("javascript"), path);
            for reference in facts
                .references
                .iter()
                .filter(|reference| reference.kind == "includes")
            {
                let Some(target) =
                    languages::html::local_template_target(path, &reference.expression)
                else {
                    continue;
                };
                let Some((target_path, target_facts)) = all.get_key_value(&target) else {
                    continue;
                };
                if !target_facts
                    .as_ref()
                    .nodes
                    .iter()
                    .any(|node| node.kind == "module" && node.language == "html")
                    || languages::workspace_path(target_path).0 != workspace
                    || dependencies
                        .nearest_manifest_scope_for_path(LanguageFamily("javascript"), target_path)
                        != scope
                {
                    continue;
                }
                incoming
                    .entry(target_path.as_str())
                    .or_default()
                    .push(Include {
                        parent: path.as_str(),
                        line: reference.line,
                    });
            }
            for reference in facts.references.iter().filter(|reference| {
                reference.kind == "imports"
                    && matches!(
                        reference.receiver_hint.as_ref(),
                        Some(ReceiverHint::HtmlClassicScriptSource)
                    )
            }) {
                let module = reference.module.as_deref().unwrap_or(&reference.expression);
                let Some(normalized) = normalized_imports
                    .get(&(path.as_str(), module))
                    .and_then(Option::as_ref)
                    .filter(|import| import.relative)
                else {
                    continue;
                };
                let Some(candidates) = modules_by_namespace
                    .get(&LanguageFamily("javascript"))
                    .and_then(|namespaces| namespaces.get(&normalized.namespace))
                else {
                    continue;
                };
                let mut matching = candidates.iter().copied().filter(|candidate| {
                    candidate.language == "javascript"
                        && languages::workspace_path(&candidate.path).0 == workspace
                        && dependencies.nearest_manifest_scope_for_path(
                            LanguageFamily("javascript"),
                            &candidate.path,
                        ) == scope
                });
                let (Some(provider), None) = (matching.next(), matching.next()) else {
                    continue;
                };
                let Some(provider_facts) = all.get(&provider.path) else {
                    continue;
                };
                for function in provider_facts.as_ref().nodes.iter().filter(|node| {
                    node.kind == "function"
                        && node.qualname.rsplit_once('.').is_some_and(|(owner, name)| {
                            owner == provider.qualname && name == node.name
                        })
                        && !provider.details["rebindings"]
                            .as_array()
                            .is_some_and(|bindings| {
                                bindings
                                    .iter()
                                    .any(|binding| binding.as_str() == Some(node.name.as_str()))
                            })
                }) {
                    by_owner
                        .entry(path.as_str())
                        .or_default()
                        .entry(&function.name)
                        .or_default()
                        .push(Provider {
                            node: function,
                            line: reference.line,
                            column: reference.column,
                        });
                }
            }
        }
        for parents in incoming.values_mut() {
            parents
                .sort_by(|left, right| (left.parent, left.line).cmp(&(right.parent, right.line)));
        }
        Self { by_owner, incoming }
    }

    pub(super) fn providers(
        &self,
        path: &str,
        name: &str,
        line: usize,
        column: usize,
    ) -> Vec<&'a Node> {
        let mut visiting = HashSet::new();
        let mut providers = self
            .collect(path, name, (line, column), &mut visiting)
            .unwrap_or_default();
        providers.sort_by(|left, right| left.id.cmp(&right.id));
        providers.dedup_by(|left, right| left.id == right.id);
        providers
    }

    fn collect(
        &self,
        path: &str,
        name: &str,
        position: (usize, usize),
        visiting: &mut HashSet<String>,
    ) -> Option<Vec<&'a Node>> {
        if visiting.len() >= 16 || !visiting.insert(path.to_owned()) {
            return None;
        }
        let result = (|| {
            let mut providers: Vec<&Node> = self
                .by_owner
                .get(path)
                .and_then(|functions| functions.get(name))
                .into_iter()
                .flatten()
                .filter(|provider| (provider.line, provider.column) < position)
                .map(|provider| provider.node)
                .collect();
            if let Some(parents) = self.incoming.get(path) {
                for parent in parents {
                    let inherited =
                        self.collect(parent.parent, name, (parent.line, 0), visiting)?;
                    if inherited.is_empty() {
                        return None;
                    }
                    providers.extend(inherited);
                }
            }
            Some(providers)
        })();
        visiting.remove(path);
        result
    }
}
