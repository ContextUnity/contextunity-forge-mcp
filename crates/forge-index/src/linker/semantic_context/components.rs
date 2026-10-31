use super::*;

type ComponentRuleKey = (
    super::super::interner::SymbolId,
    super::super::interner::SymbolId,
    super::super::interner::SymbolId,
);
type ResultScopeKey = (ComponentRuleKey, super::super::interner::SymbolId);

fn component_rule_key(
    interner: &super::super::interner::LinkerInterner<'_>,
    config: &Node,
    rule: &languages::manifests::syntax::SyntaxRule,
) -> ComponentRuleKey {
    (
        interner
            .lookup_symbol(&config.id)
            .expect("catalog configuration"),
        interner
            .lookup_symbol(&rule.pack)
            .expect("component rule pack is interned"),
        interner
            .lookup_symbol(&rule.id)
            .expect("component rule id is interned"),
    )
}

fn relative_under_directory<'a>(path: &'a str, project: &str, root: &str) -> Option<&'a str> {
    if project.is_empty() {
        path.strip_prefix(root)?.strip_prefix('/')
    } else {
        path.strip_prefix(project)?
            .strip_prefix('/')?
            .strip_prefix(root)?
            .strip_prefix('/')
    }
}

fn scoped_provider_modules<'a>(
    path: &str,
    facts: &'a Facts,
    scope: &str,
    dependencies: &languages::dependency_registry::DependencyRegistry,
) -> impl Iterator<Item = &'a Node> {
    let same_scope = dependencies
        .nearest_manifest_scope_for_path(languages::LanguageFamily("javascript"), path)
        == Some(scope);
    facts
        .nodes
        .iter()
        .filter(move |node| same_scope && node.kind == "module")
}

pub(super) fn component_names(
    path: &str,
    project: &str,
    root: &str,
    extension: &str,
) -> Option<(String, String)> {
    let relative = relative_under_directory(path, project, root)?;
    let stem = relative.strip_suffix(extension)?;
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

impl<'a> super::Context<'a> {
    pub(super) fn prepare_component_environments<F: AsRef<Facts>>(
        &mut self,
        all: &'a BTreeMap<String, F>,
        dependencies: &languages::dependency_registry::DependencyRegistry,
    ) {
        use languages::manifests::syntax::SyntaxOperation;
        let mut implicit_builtins_cache: HashMap<
            (usize, SymbolId),
            (std::sync::Arc<ImplicitBuiltinMap>, bool),
        > = HashMap::new();
        let mut pinned_rules: Vec<std::sync::Arc<languages::LanguageContext>> = Vec::new();
        let mut result_catalog: HashMap<ResultScopeKey, std::sync::Arc<ResultScope<'a>>> =
            HashMap::new();
        let mut callable_catalog: HashMap<ComponentRuleKey, std::sync::Arc<CallableScope<'a>>> =
            HashMap::new();
        for (path, source) in all {
            let Some(module) = source
                .as_ref()
                .nodes
                .iter()
                .find(|node| node.kind == "module")
            else {
                continue;
            };
            let path_id = self
                .interner
                .lookup_path(path)
                .expect("component catalog path is interned");
            let language = self
                .interner
                .lookup_symbol(&module.language)
                .expect("component language is interned");
            let rules = dependencies.language_context_for_path(path);
            let rules_key = (std::sync::Arc::as_ptr(&rules) as usize, language);
            let (implicit, has_component_env) =
                implicit_builtins_cache.entry(rules_key).or_insert_with(|| {
                    pinned_rules.push(std::sync::Arc::clone(&rules));
                    let mut names_map = HashMap::new();
                    let mut has_component = false;
                    for rule in &rules.syntax_rules {
                        if matches!(rule.operation, SyntaxOperation::ComponentEnvironment { .. }) {
                            has_component = true;
                        }
                        if rule.language == module.language {
                            if let SyntaxOperation::ImplicitBuiltins { names } = &rule.operation {
                                names_map.extend(names.iter().filter_map(|name| {
                                    self.interner.lookup_symbol(name).map(|name| {
                                        (
                                            name,
                                            format!(
                                                "{}; rule:{}:{}",
                                                rule.display
                                                    .builtin
                                                    .as_deref()
                                                    .unwrap_or("builtin:configured"),
                                                rule.pack,
                                                rule.id
                                            ),
                                        )
                                    })
                                }));
                            }
                        }
                    }
                    (std::sync::Arc::new(names_map), has_component)
                });
            if !implicit.is_empty() {
                self.implicit_builtins
                    .insert(path_id, std::sync::Arc::clone(implicit));
            }
            if !*has_component_env {
                continue;
            }
            let Some(scope) = dependencies
                .nearest_manifest_scope_for_path(languages::LanguageFamily("javascript"), path)
            else {
                continue;
            };
            for rule in &rules.syntax_rules {
                let SyntaxOperation::ComponentEnvironment {
                    package,
                    config_file,
                    directory,
                    extension,
                    autoimports,
                    builtins,
                    modules,
                    callable_imports,
                    ..
                } = &rule.operation
                else {
                    continue;
                };
                if !rule
                    .activation_family()
                    .is_some_and(|family| dependencies.declares_for_path(family, path, package))
                {
                    continue;
                }
                let config_path = if scope.is_empty() {
                    config_file.clone()
                } else {
                    format!("{scope}/{config_file}")
                };
                let config = all.get(&config_path).and_then(|facts| {
                    facts
                        .as_ref()
                        .nodes
                        .iter()
                        .find(|node| node.kind == "module")
                });
                if rule.language == module.language && config.is_some() {
                    self.autoimports.entry(path_id).or_default().extend(
                        autoimports.iter().filter_map(|name| {
                            self.interner.lookup_symbol(name).map(|name| {
                                (
                                    name,
                                    format!(
                                        "{}; rule:{}:{}",
                                        rule.display
                                            .autoimport
                                            .as_deref()
                                            .unwrap_or("builtin:configured_autoimport"),
                                        rule.pack,
                                        rule.id
                                    ),
                                )
                            })
                        }),
                    );
                }
                let configuration = config
                    .and_then(|node| {
                        self.interner
                            .lookup_symbol(node.id.as_str())
                            .and_then(|id| self.flows?.get(&id))
                    })
                    .and_then(|state| match state {
                        crate::core::typed_facts::FlowState::Valid { facts, .. } => {
                            Some(facts.component_configurations())
                        }
                        _ => None,
                    })
                    .into_iter()
                    .flatten()
                    .filter(|fact| fact.origin.pack == rule.pack && fact.origin.rule == rule.id)
                    .collect::<Vec<_>>();
                if let ([configuration], Some(config_node)) = (configuration.as_slice(), config) {
                    if let (Some(descriptor), Some(state)) =
                        (callable_imports, &configuration.callable_imports)
                    {
                        let key = component_rule_key(self.interner, config_node, rule);
                        if !callable_catalog.contains_key(&key) {
                            let mut catalog = CallableScope {
                                implicit: state.enabled && state.scan,
                                aliases: descriptor
                                    .aliases
                                    .iter()
                                    .map(|alias| {
                                        self.interner
                                            .lookup_symbol(alias)
                                            .expect("callable import alias is interned")
                                    })
                                    .collect(),
                                names: HashMap::new(),
                                modules: HashMap::new(),
                                incomplete: false,
                            };
                            for (provider_path, file) in all {
                                let Some(selector) = descriptor.exports.iter().find(|selector| {
                                    relative_under_directory(
                                        provider_path,
                                        scope,
                                        &selector.directory,
                                    )
                                    .is_some_and(|tail| {
                                        !tail.contains('/') && tail.ends_with(&selector.extension)
                                    })
                                }) else {
                                    continue;
                                };
                                let Some(provider) = scoped_provider_modules(
                                    provider_path,
                                    file.as_ref(),
                                    scope,
                                    dependencies,
                                )
                                .next() else {
                                    continue;
                                };
                                let mut exported =
                                    HashMap::<SymbolId, CallableCandidates<'a>>::new();
                                for binding in
                                    provider.details["exports"].as_array().into_iter().flatten()
                                {
                                    if binding["type_only"] == true || binding["name"] == "default"
                                    {
                                        continue;
                                    }
                                    let Some(public) = binding["name"].as_str() else {
                                        catalog.incomplete = true;
                                        continue;
                                    };
                                    let public_id = self
                                        .interner
                                        .lookup_symbol(public)
                                        .expect("callable export name is interned");
                                    let candidates = exported.entry(public_id).or_default();
                                    let target = binding["local"]
                                        .as_str()
                                        .filter(|_| binding["module"].is_null())
                                        .and_then(|local| {
                                            self.exports
                                                .get(provider.language.as_str())?
                                                .get(&self.interner.lookup_path(provider_path)?)?
                                                .get(&public_id)
                                                .copied()
                                                .flatten()
                                                .filter(|node| {
                                                    node.path == *provider_path
                                                        && node.name == local
                                                        && node.kind == "function"
                                                })
                                        });
                                    if let Some(target) = target {
                                        if !candidates.nodes.iter().any(|node| node.id == target.id)
                                        {
                                            candidates.nodes.push(target);
                                        }
                                    } else {
                                        candidates.incomplete = true;
                                    }
                                }
                                for (name, candidates) in &exported {
                                    let combined = catalog.names.entry(*name).or_default();
                                    combined.incomplete |= candidates.incomplete;
                                    for node in &candidates.nodes {
                                        if !combined
                                            .nodes
                                            .iter()
                                            .any(|previous| previous.id == node.id)
                                        {
                                            if combined.nodes.len() < 8 {
                                                combined.nodes.push(node);
                                            } else {
                                                combined.incomplete = true;
                                            }
                                        }
                                    }
                                }
                                let relative = if scope.is_empty() {
                                    Some(provider_path.as_str())
                                } else {
                                    relative_under_directory(provider_path, "", scope)
                                };
                                let Some(relative) = relative else {
                                    continue;
                                };
                                if let Some(stem) = relative.strip_suffix(&selector.extension) {
                                    let module = std::sync::Arc::new(CallableModule {
                                        owner: provider,
                                        exports: exported,
                                    });
                                    catalog.modules.insert(
                                        self.interner
                                            .lookup_path(stem)
                                            .expect("callable module stem is interned"),
                                        module.clone(),
                                    );
                                    catalog.modules.insert(
                                        self.interner
                                            .lookup_path(relative)
                                            .expect("callable module path is interned"),
                                        module,
                                    );
                                }
                            }
                            callable_catalog.insert(key, std::sync::Arc::new(catalog));
                        }
                        self.callable_scopes
                            .entry(path_id)
                            .or_default()
                            .push(callable_catalog[&key].clone());
                    }
                    for name in &configuration.modules {
                        let Some(addition) = modules.get(name).filter(|addition| {
                            rule.activation_family().is_some_and(|family| {
                                dependencies.declares_for_path(family, path, &addition.package)
                            })
                        }) else {
                            continue;
                        };
                        let module_id = self
                            .interner
                            .lookup_symbol(name)
                            .expect("configured module is interned");
                        let key: ResultScopeKey = (
                            component_rule_key(self.interner, config_node, rule),
                            module_id,
                        );
                        if let hashbrown::hash_map::Entry::Vacant(entry) = result_catalog.entry(key)
                        {
                            let mut exports = HashMap::<SymbolId, Vec<(&Node, SymbolId)>>::new();
                            for (provider_path, provider) in all {
                                if !addition.exports.iter().any(|entry| {
                                    relative_under_directory(provider_path, scope, &entry.directory)
                                        .is_some_and(|tail| {
                                            !tail.contains('/') && tail.ends_with(&entry.extension)
                                        })
                                }) {
                                    continue;
                                }
                                let Some(provider) = scoped_provider_modules(
                                    provider_path,
                                    provider.as_ref(),
                                    scope,
                                    dependencies,
                                )
                                .next() else {
                                    continue;
                                };
                                for binding in
                                    provider.details["exports"].as_array().into_iter().flatten()
                                {
                                    if binding["type_only"] == true
                                        || binding["module"].as_str().is_some()
                                    {
                                        continue;
                                    }
                                    let (Some(public), Some(local)) =
                                        (binding["name"].as_str(), binding["local"].as_str())
                                    else {
                                        continue;
                                    };
                                    if public == "default" {
                                        continue;
                                    }
                                    exports
                                        .entry(
                                            self.interner
                                                .lookup_symbol(public)
                                                .expect("result export name is interned"),
                                        )
                                        .or_default()
                                        .push((
                                            provider,
                                            self.interner
                                                .lookup_symbol(local)
                                                .expect("result export binding is interned"),
                                        ));
                                }
                            }
                            let calls = addition
                                .call_results
                                .iter()
                                .map(|(callee, members)| {
                                    let callee = self
                                        .interner
                                        .lookup_symbol(callee)
                                        .expect("result callee is interned");
                                    let members = members
                                        .iter()
                                        .map(|member| {
                                            self.interner
                                                .lookup_symbol(member)
                                                .expect("result member is interned")
                                        })
                                        .collect();
                                    (
                                        callee,
                                        std::sync::Arc::new(
                                            super::super::value_flow::ConfiguredCallResult {
                                                evidence: format!(
                                                    "configured result; config:{}; module:{}; package:{}; rule:{}:{}",
                                                    config_node.path, name, addition.package, rule.pack, rule.id
                                                ),
                                                callee,
                                                members,
                                            },
                                        ),
                                    )
                                })
                                .collect();
                            entry.insert(std::sync::Arc::new(ResultScope {
                                config: config_node,
                                module: module_id,
                                exports,
                                calls,
                            }));
                        }
                        self.result_scopes
                            .entry(path_id)
                            .or_default()
                            .push(result_catalog[&key].clone());
                    }
                }
                if !path.ends_with(extension) {
                    continue;
                }
                let builtin = self.component_builtins.entry(path_id).or_default();
                for name in builtins {
                    builtin.insert(
                        self.interner
                            .lookup_symbol(name)
                            .expect("component builtin is interned"),
                        format!(
                            "{}; rule:{}:{}",
                            rule.display
                                .component
                                .as_deref()
                                .unwrap_or("builtin:configured_component"),
                            rule.pack,
                            rule.id
                        ),
                    );
                }
                let [config] = configuration.as_slice() else {
                    continue;
                };
                for name in &config.modules {
                    if let Some(addition) = modules.get(name).filter(|addition| {
                        rule.activation_family().is_some_and(|family| {
                            dependencies.declares_for_path(family, path, &addition.package)
                        })
                    }) {
                        for component in &addition.components {
                            builtin.insert(
                                self.interner
                                    .lookup_symbol(component)
                                    .expect("configured component is interned"),
                                format!(
                                    "{}:{name}; rule:{}:{}",
                                    rule.display
                                        .component
                                        .as_deref()
                                        .unwrap_or("builtin:configured_component"),
                                    rule.pack,
                                    rule.id
                                ),
                            );
                        }
                    }
                }
                if !config.enabled {
                    continue;
                }
                let key = (
                    self.interner
                        .lookup_path(scope)
                        .expect("component manifest scope is interned"),
                    self.interner
                        .lookup_symbol(&rule.pack)
                        .expect("component rule pack is interned"),
                    self.interner
                        .lookup_symbol(&rule.id)
                        .expect("component rule id is interned"),
                );
                let (index, inserted) = if let Some(&index) = self.component_candidate_ids.get(&key)
                {
                    (index, false)
                } else {
                    let index = self.component_candidates.len();
                    self.component_candidate_ids.insert(key, index);
                    self.component_candidates.push(HashMap::new());
                    (index, true)
                };
                self.component_consumers
                    .entry(path_id)
                    .or_default()
                    .push(index);
                if !inserted {
                    continue;
                }
                let candidates = &mut self.component_candidates[index];
                for (provider_path, provider) in all {
                    let Some((pascal, kebab)) =
                        component_names(provider_path, scope, directory, extension)
                    else {
                        continue;
                    };
                    let Some(provider_module) = scoped_provider_modules(
                        provider_path,
                        provider.as_ref(),
                        scope,
                        dependencies,
                    )
                    .next() else {
                        continue;
                    };
                    for name in [pascal, kebab] {
                        let name = self
                            .interner
                            .lookup_symbol(&name)
                            .expect("component candidate name is interned");
                        let providers = candidates.entry(name).or_default();
                        if !providers
                            .iter()
                            .any(|existing| existing.id == provider_module.id)
                        {
                            providers.push(provider_module);
                        }
                    }
                }
            }
        }
    }
}
