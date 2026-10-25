use super::*;

impl<'a> Context<'a> {
    pub(super) fn prepare_callable_aliases<F: AsRef<Facts>>(
        &mut self,
        all: &'a BTreeMap<String, F>,
    ) {
        let mut updates = Vec::new();
        for (path, file) in all {
            let Some(path_id) = self.interner.lookup_path(path) else {
                continue;
            };
            if !self.callable_scopes.contains_key(&path_id) {
                continue;
            }
            let file = file.as_ref();
            if self.interner.file_import_mask_for_path(path_id) == 0
                && !file
                    .references
                    .iter()
                    .any(|reference| reference.kind == "imports")
            {
                continue;
            }
            let mut counts = HashMap::new();
            for reference in file
                .references
                .iter()
                .filter(|reference| reference.kind == "imports")
            {
                if let Some(alias) = reference.alias.as_deref() {
                    *counts
                        .entry((reference.source.as_str(), alias))
                        .or_insert(0usize) += 1;
                }
            }
            for reference in file
                .references
                .iter()
                .filter(|reference| reference.kind == "imports")
            {
                let Some(alias) = reference.alias.as_deref().filter(|alias| *alias != "*") else {
                    continue;
                };
                if counts.get(&(reference.source.as_str(), alias)) != Some(&1) {
                    continue;
                }
                let Some((_, target)) = self.callable_import_module(path, reference) else {
                    continue;
                };
                updates.push((
                    reference.source.as_str(),
                    alias,
                    Imported::Nodes(
                        target.nodes.clone(),
                        reference.position(),
                        matches!(reference.receiver_hint, Some(ReceiverHint::TypeOnlyImport)),
                    ),
                ));
            }
        }
        for (owner, alias, target) in updates {
            let owner = self
                .interner
                .lookup_symbol(owner)
                .expect("callable import owner is interned");
            let alias = self
                .interner
                .lookup_symbol(alias)
                .expect("callable import alias is interned");
            self.imports.entry(owner).or_default().insert(alias, target);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::engine::linker) fn build<F: AsRef<Facts>>(
        all: &'a BTreeMap<String, F>,
        interner: &'a super::super::interner::LinkerInterner<'a>,
        modules: &'a ModulesByNamespace<'a>,
        by_module: &'a HashMap<&'a str, Vec<&'a Node>>,
        by_qual: &'a HashMap<&'a str, Vec<&'a Node>>,
        by_id: &'a HashMap<SymbolId, &'a Node>,
        node_bindings: &'a super::super::NodeBindingIndex,
        node_rebindings: &'a super::super::NodeBindingIndex,
        exports: &'a HashMap<&'a str, PackageExports<'a>>,
        normalized_imports: &'a HashMap<(&'a str, &'a str), Option<ImportPath>>,
        #[cfg(feature = "lang-rust")] rust_forwarding: &'a languages::rust::linker::RustForwarding<
            'a,
        >,
        dependencies: &'a languages::dependency_registry::DependencyRegistry,
    ) -> Self {
        let mut context = Self {
            interner,
            dependencies,
            flows: None,
            named: Default::default(),
            by_qual,
            by_id,
            node_bindings,
            node_rebindings,
            exports,
            imports: HashMap::new(),
            factory_imports: HashMap::new(),
            factory_bindings: HashMap::new(),
            factory_declarations: HashMap::new(),
            factory_callees: HashSet::new(),
            receiver_types: HashMap::new(),
            argument_factories: HashMap::new(),
            hierarchy_results: HashMap::new(),
            component_builtins: HashMap::new(),
            component_candidate_ids: HashMap::new(),
            component_candidates: Vec::new(),
            component_consumers: HashMap::new(),
            implicit_builtins: HashMap::new(),
            autoimports: HashMap::new(),
            result_scopes: HashMap::new(),
            callable_scopes: HashMap::new(),
            commonjs: super::super::commonjs::CommonJsBindings::build(all),
            members: HashMap::new(),
            python: None,
            #[cfg(feature = "lang-rust")]
            rust: None,
            #[cfg(feature = "lang-typescript")]
            typescript: None,
        };
        let mut hierarchy_cache: HashMap<usize, Option<std::sync::Arc<HierarchyAuthorities>>> =
            HashMap::new();
        let mut receiver_types_cache: HashMap<
            (usize, &'static str),
            Option<std::sync::Arc<ReceiverTypes>>,
        > = HashMap::new();
        let mut argument_factories_cache: HashMap<
            (usize, &'static str),
            Option<std::sync::Arc<ArgumentFactories>>,
        > = HashMap::new();
        let mut pinned_rules: Vec<std::sync::Arc<languages::LanguageContext>> = Vec::new();
        for (path, facts) in all {
            let facts = facts.as_ref();
            let Some(module) = facts.nodes.iter().find(|node| node.kind == "module") else {
                continue;
            };
            let rules = dependencies.language_context_for_path(path);
            let rules_ptr = std::sync::Arc::as_ptr(&rules) as usize;
            if module.language == "python" {
                let cached = hierarchy_cache.entry(rules_ptr).or_insert_with(|| {
                    pinned_rules.push(std::sync::Arc::clone(&rules));
                    let mut map = HashMap::new();
                    for (origin_pack, base) in dependencies.hierarchy_return_targets_for_path(path)
                    {
                        if let Some((pack, rule)) = dependencies.class_hierarchy_authority_for_path(
                            path,
                            base,
                            Some(origin_pack),
                        ) {
                            map.insert(
                                (
                                    interner
                                        .lookup_symbol(pack)
                                        .expect("hierarchy pack is interned"),
                                    interner
                                        .lookup_symbol(base)
                                        .expect("hierarchy base is interned"),
                                ),
                                std::sync::Arc::new(
                                    super::super::value_flow::ConfiguredReceiverAuthority::Hierarchy {
                                        pack: pack.to_owned(),
                                        rule: std::sync::Arc::new(rule.clone()),
                                    },
                                ),
                            );
                        }
                    }
                    (!map.is_empty()).then(|| std::sync::Arc::new(map))
                });
                if let Some(hierarchy) = cached {
                    let path_id = context.interner.lookup_path(path).expect("catalog path");
                    context.hierarchy_results.insert(path_id, hierarchy.clone());
                }
            }
            let language = languages::by_id(&module.language)
                .map(|profile| profile.id())
                .unwrap_or("");
            let key = (rules_ptr, language);
            if !receiver_types_cache.contains_key(&key) {
                pinned_rules.push(std::sync::Arc::clone(&rules));
                let mut indices = Vec::new();
                let mut factories: ArgumentFactories = HashMap::new();
                for (index, rule) in rules.syntax_rules.iter().enumerate() {
                    if let languages::manifests::syntax::SyntaxOperation::ReceiverType { .. } =
                        &rule.operation
                    {
                        if languages::by_id(&module.language).is_some_and(|profile| {
                            rule.activation_family() == Some(profile.family())
                        }) {
                            indices.push(index);
                            if matches!(module.language.as_str(), "javascript" | "typescript")
                                && rule.language == module.language
                            {
                                if let languages::manifests::syntax::SyntaxOperation::ReceiverType {
                                    receiver,
                                } = &rule.operation
                                {
                                    for factory in &receiver.argument_factories {
                                        if let languages::manifests::projections::Api::GlobalMember {
                                            receiver: global,
                                            member,
                                        } = &factory.callee
                                        {
                                            let callee = format!("{global}.{member}");
                                            let Some(callee) =
                                                interner.lookup_symbol(&callee)
                                            else {
                                                continue;
                                            };
                                            let authority = std::sync::Arc::new(super::super::value_flow::ConfiguredReceiverAuthority::Nominal { pack: rule.pack.clone(), rule_id: rule.id.clone(), receiver: std::sync::Arc::new(super::super::value_flow::ConfiguredNominalReceiver {rule:receiver.clone()}) });
                                            for literal in &factory.literals {
                                                let Some(literal) =
                                                    interner.lookup_symbol(literal)
                                                else {
                                                    continue;
                                                };
                                                factories
                                                    .entry((
                                                        callee,
                                                        factory.argument,
                                                        literal,
                                                    ))
                                                    .or_default()
                                                    .push(authority.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                receiver_types_cache.insert(
                    key,
                    (!indices.is_empty()).then(|| {
                        std::sync::Arc::new(ReceiverTypes {
                            rules: rules.clone(),
                            indices,
                        })
                    }),
                );
                argument_factories_cache.insert(
                    key,
                    (!factories.is_empty()).then(|| std::sync::Arc::new(factories)),
                );
            }
            if let Some(Some(receiver_types)) = receiver_types_cache.get(&key) {
                let path_id = context.interner.lookup_path(path).expect("catalog path");
                context
                    .receiver_types
                    .insert(path_id, receiver_types.clone());
            }
            if let Some(Some(argument_factories)) = argument_factories_cache.get(&key) {
                let path_id = context.interner.lookup_path(path).expect("catalog path");
                context
                    .argument_factories
                    .insert(path_id, argument_factories.clone());
            }
        }
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
                let Some(normalized) = normalized_imports.get(&key).and_then(Option::as_ref) else {
                    continue;
                };
                let (mut providers, selected) =
                    import_provider_modules(path, normalized, profile.family(), modules);
                let mut candidates = Vec::new();
                let mut lookup_key = String::new();
                linker.resolve_import(&mut ImportContext {
                    path,
                    interner: context.interner,
                    dependencies,
                    reference,
                    #[cfg(feature = "lang-rust")]
                    rust_forwarding: Some(rust_forwarding),
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
                let position = reference.position();
                let owner_id = context
                    .interner
                    .lookup_symbol(&owner.id)
                    .expect("catalog owner");
                let alias_id = context
                    .interner
                    .lookup_symbol(alias)
                    .expect("import alias is interned");
                let is_type_only = matches!(
                    reference.receiver_hint.as_ref(),
                    Some(ReceiverHint::TypeOnlyImport)
                );
                let unproven_guard = is_type_only
                    && super::super::python::semantic_context::type_only_import_is_unproven(
                        profile,
                        path,
                        facts,
                        modules,
                        dependencies,
                        context.interner,
                        context.node_rebindings,
                    );
                if context
                    .imports
                    .get(&owner_id)
                    .is_some_and(|aliases| aliases.contains_key(&alias_id))
                {
                    context.factory_imports.insert((owner_id, alias_id), None);
                } else if !is_type_only && providers.len() == 1 {
                    let binding = context
                        .interner
                        .lookup_symbol(&reference.expression)
                        .expect("factory import binding is interned");
                    context.factory_imports.insert(
                        (owner_id, alias_id),
                        Some((providers[0], binding, position)),
                    );
                }
                let target = if unproven_guard {
                    Imported::Unknown
                } else if providers.len() == 1 && !candidates.is_empty() {
                    Imported::Nodes(candidates, position, is_type_only)
                } else if providers.is_empty() && !normalized.relative {
                    let module_name = reference.module.as_deref().unwrap_or(&reference.expression);
                    if super::super::python::semantic_context::dynamic_type_alias(
                        &module.language,
                        module_name,
                        &reference.expression,
                    ) {
                        Imported::Unknown
                    } else {
                        let factory_safe =
                            super::super::python::semantic_context::logging_factory_import_is_safe(
                                module, alias, position,
                            );
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
                let aliases = context.imports.entry(owner_id).or_default();
                if aliases.contains_key(&alias_id) {
                    aliases.insert(alias_id, Imported::Unknown);
                } else {
                    aliases.insert(alias_id, target);
                }
            }
        }
        #[cfg(feature = "lang-rust")]
        super::super::rust::semantic_context::import_globs(
            &mut context,
            all,
            by_id,
            normalized_imports,
            modules,
            rust_forwarding,
        );
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
                        interner.lookup_symbol(&class.id).expect("catalog class"),
                        interner.lookup_symbol(&node.name).expect("catalog member"),
                        node.details["is_static"] == true,
                    ))
                    .or_default()
                    .push(node);
            }
        }
        context
    }
}
