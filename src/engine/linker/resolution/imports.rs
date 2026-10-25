use super::*;

pub(super) fn phase_imports<'a, 'file, F>(resolver: &mut FileResolver<'a, 'file, F>)
where
    'a: 'file,
    F: AsRef<Facts> + Sync,
{
    let input = resolver.input;
    let path = resolver.path;
    let facts = resolver.facts;
    let references = resolver.references;
    let profile = input.profiles.get(path).copied();
    let language_linker = profile.map_or(&GENERIC_LINKER as &dyn LanguageLinker, |profile| {
        languages::linker_for(profile.id())
    });
    let language_package_exports = input
        .package_exports
        .get(profile.map_or("", |profile| profile.id()))
        .expect("package export index exists for every linked language");
    let file_module = resolver.file_module;
    let aliases = &mut resolver.aliases;
    let external_aliases = &mut resolver.external_aliases;
    let type_only_aliases = &mut resolver.type_only_aliases;
    let graph = &mut resolver.graph;
    let lookup_key = &mut resolver.lookup_key;
    let all = input.all;
    let dependency_registry = input.dependency_registry;
    let normalized_imports = input.normalized_imports;
    let modules_by_namespace = input.modules_by_namespace;
    let by_id = input.by_id;
    let by_qual = input.by_qual;
    let by_module = input.by_module;
    let semantic_context = input.semantic_context;
    #[cfg(feature = "lang-rust")]
    let rust_forwarding = input.rust_forwarding;
    for r in facts.references.iter().filter(|r| r.kind == "imports") {
        let callable_path = semantic_context.callable_import_path(path, r);
        let normalized = callable_path.as_ref().or_else(|| {
            normalized_imports
                .get(&(path, r.module.as_deref().unwrap_or(&r.expression)))
                .expect("imports are normalized before parallel resolution")
                .as_ref()
        });
        let mut modules = Vec::new();
        let mut selected_namespace = "";
        if let (Some(profile), Some(normalized)) = (profile, normalized) {
            (modules, selected_namespace) =
                import_provider_modules(path, normalized, profile.family(), modules_by_namespace);
        }
        let symbol = normalized
            .as_ref()
            .map_or_else(|| r.expression.clone(), |n| n.namespace.clone());
        let mut candidates = Vec::new();
        let resolution = language_linker.resolve_import(&mut ImportContext {
            path,
            interner: input.interner,
            dependencies: dependency_registry,
            reference: r,
            #[cfg(feature = "lang-rust")]
            rust_forwarding: Some(rust_forwarding),
            normalized,
            selected_namespace,
            family: profile.map_or(LanguageFamily(""), |profile| profile.family()),
            modules: &mut modules,
            candidates: &mut candidates,
            modules_by_namespace,
            by_module,
            by_qual,
            package_exports: language_package_exports,
            lookup_key: &mut *lookup_key,
        });
        let ImportResolution {
            paired_stub,
            child_module,
            stub_symbol,
            reexport_symbol,
        } = resolution;
        candidates.sort_by(|a, b| a.id.cmp(&b.id));
        candidates.dedup_by_key(|n| &n.id);
        crate::engine::linker::traits::filter_stubs_and_overloads(&mut candidates);
        let external = if modules.is_empty() && normalized.as_ref().is_some_and(|n| !n.relative) {
            profile.and_then(|p| {
                dependency_registry.classification_for_path(
                    p,
                    r.module.as_deref().unwrap_or(&r.expression),
                    path,
                )
            })
        } else {
            None
        };
        let scope = input
            .interner
            .lookup_node(by_id, r.source.as_str())
            .map_or(file_module, |n| n.qualname.as_str());
        let mut imported_targets = Vec::new();
        let is_wildcard =
            r.alias.as_deref() == Some("*") || (r.expression == "*" && r.alias.is_none());
        let export_type_only = modules.first().is_some_and(|m| {
            m.details["exports"].as_array().is_some_and(|bindings| {
                bindings.iter().any(|b| {
                    b["name"].as_str() == Some(r.expression.as_str()) && b["type_only"] == true
                })
            })
        });
        let is_type_only = matches!(r.receiver_hint.as_ref(), Some(ReceiverHint::TypeOnlyImport))
            || export_type_only;
        let unproven_guard = is_type_only
            && profile.is_some_and(|p| p.id() == "python")
            && !super::super::python::semantic_context::has_proven_type_checking(
                path,
                facts,
                modules_by_namespace,
                dependency_registry,
                profile.unwrap().family(),
                input.interner,
                input.node_rebindings,
            );
        if is_wildcard {
            for target_module in &modules {
                if let Some(module_nodes) = by_module
                    .get(target_module.path.as_str())
                    .filter(|_| !profile.is_some_and(|profile| profile.id() == "rust"))
                {
                    for n in module_nodes {
                        if n.kind != "module" && !n.name.is_empty() {
                            aliases
                                .entry(scope)
                                .or_default()
                                .entry(n.name.as_str())
                                .or_default()
                                .push(n);
                        }
                    }
                }
                #[cfg(feature = "lang-rust")]
                if modules.len() == 1
                    && profile.is_some_and(|profile| profile.id() == "rust")
                    && file_module
                        .strip_prefix(&target_module.qualname)
                        .is_some_and(|suffix| suffix.starts_with('.'))
                {
                    if let Some(target_facts) = all.get(&target_module.path) {
                        for parent_import in
                            target_facts.as_ref().references.iter().filter(|import| {
                                import.kind == "imports"
                                    && import.source == target_module.id
                                    && import.alias.as_deref().is_some_and(|alias| alias != "*")
                            })
                        {
                            let module = parent_import
                                .module
                                .as_deref()
                                .unwrap_or(&parent_import.expression);
                            let Some(provider) = normalized_imports
                                .get(&(target_module.path.as_str(), module))
                                .and_then(Option::as_ref)
                            else {
                                continue;
                            };
                            let workspace = languages::workspace_path(path).0;
                            let alias = parent_import.alias.as_deref().unwrap_or_default();
                            if input
                                .interner
                                .lookup_node(by_id, r.source.as_str())
                                .is_some_and(|owner| owner.kind != "module")
                                && (references.iter().any(|reference| {
                                    reference.kind == "imports"
                                        && reference.source == r.source
                                        && reference.alias.as_deref() == Some(alias)
                                }) || by_qual
                                    .get(qualified(&mut *lookup_key, scope, alias))
                                    .is_some_and(|nodes| {
                                        nodes.iter().any(|node| node.path == *path)
                                    }))
                            {
                                continue;
                            }
                            if target_facts
                                .as_ref()
                                .references
                                .iter()
                                .filter(|import| {
                                    import.kind == "imports"
                                        && import.source == target_module.id
                                        && import.alias.as_deref() == Some(alias)
                                })
                                .count()
                                != 1
                            {
                                continue;
                            }
                            let mut local_providers: Vec<&Node> = by_qual
                                .get(provider.namespace.as_str())
                                .into_iter()
                                .flatten()
                                .copied()
                                .filter(|node| {
                                    node.language == "rust"
                                        && languages::workspace_path(&node.path).0 == workspace
                                })
                                .collect();
                            if local_providers.is_empty() {
                                local_providers.extend(
                                    rust_forwarding
                                        .targets(target_module, alias, file_module)
                                        .iter()
                                        .copied(),
                                );
                            }
                            if !local_providers.is_empty() {
                                aliases
                                    .entry(scope)
                                    .or_default()
                                    .entry(alias)
                                    .or_default()
                                    .extend(local_providers);
                            } else if !provider.relative
                                && !aliases
                                    .get(scope)
                                    .is_some_and(|entries| entries.contains_key(alias))
                                && profile
                                    .and_then(|profile| {
                                        dependency_registry.classification_for_path(
                                            profile,
                                            module,
                                            &target_module.path,
                                        )
                                    })
                                    .is_some()
                            {
                                aliases.entry(scope).or_default().entry(alias).or_default();
                                external_aliases
                                    .entry(scope)
                                    .or_default()
                                    .insert(alias, Some((provider.namespace.clone(), r.line)));
                            }
                        }
                    }
                }
            }
            #[cfg(feature = "lang-rust")]
            if profile.is_some_and(|profile| profile.id() == "rust")
                && modules.len() == 1
                && input
                    .interner
                    .lookup_node(by_id, r.source.as_str())
                    .is_some_and(|owner| owner.kind != "module")
            {
                for (name, targets) in rust_forwarding.visible_bindings(modules[0], file_module) {
                    if references.iter().any(|reference| {
                        reference.kind == "imports"
                            && reference.source == r.source
                            && reference.alias.as_deref() == Some(name)
                    }) {
                        continue;
                    }
                    if by_qual
                        .get(qualified(&mut *lookup_key, scope, name))
                        .is_some_and(|nodes| nodes.iter().any(|node| node.path == *path))
                    {
                        aliases.entry(scope).or_default().remove(name);
                        continue;
                    }
                    use hashbrown::hash_map::Entry;
                    match aliases.entry(scope).or_default().entry(name) {
                        Entry::Vacant(entry) => {
                            entry.insert(targets.to_vec());
                        }
                        Entry::Occupied(mut entry) => {
                            if entry.get().is_empty() || targets.is_empty() {
                                entry.get_mut().clear();
                            } else {
                                entry.get_mut().extend_from_slice(targets);
                                entry.get_mut().sort_by_key(|node| node.id.as_str());
                                entry.get_mut().dedup_by_key(|node| node.id.as_str());
                            }
                            if !targets.is_empty() {
                                if let Some(origin) = external_aliases
                                    .get_mut(scope)
                                    .and_then(|bindings| bindings.get_mut(name))
                                {
                                    *origin = None;
                                }
                            }
                        }
                    }
                }
            }
        } else if let Some(alias) = r.alias.as_deref() {
            let local_declaration = by_qual
                .get(qualified(&mut *lookup_key, scope, alias))
                .is_some_and(|nodes| {
                    nodes
                        .iter()
                        .any(|n| n.path == *path && n.kind != "module_declaration")
                });
            let mut targets = if local_declaration {
                Vec::new()
            } else {
                candidates.clone()
            };
            let symbol_name = symbol.rsplit('.').next().unwrap_or(&symbol);
            if targets.is_empty() && !local_declaration {
                let mut inherited_ext = None;
                for target_module in &modules {
                    let target_scope = target_module.qualname.as_str();
                    if let Some(target_alias) = aliases
                        .get(target_scope)
                        .and_then(|entries| entries.get(symbol_name))
                    {
                        targets.extend(target_alias.iter().copied());
                    }
                    if inherited_ext.is_none() {
                        inherited_ext = external_aliases
                            .get(target_scope)
                            .and_then(|entries| entries.get(symbol_name))
                            .and_then(Option::as_ref)
                            .cloned();
                    }
                }
                if let Some(target_ext) = inherited_ext {
                    external_aliases
                        .entry(scope)
                        .or_default()
                        .insert(alias, Some(target_ext));
                }
            }
            let alias_taken = aliases
                .get(scope)
                .is_some_and(|entries| entries.contains_key(alias));
            if alias_taken {
                if let Some(provenance) = external_aliases
                    .get_mut(scope)
                    .and_then(|entries| entries.get_mut(alias))
                {
                    *provenance = None;
                }
            } else if external.is_some() && !local_declaration {
                external_aliases
                    .entry(scope)
                    .or_default()
                    .insert(alias, Some((symbol.clone(), r.line)));
            }
            if is_type_only && !unproven_guard {
                type_only_aliases.entry(scope).or_default().insert(alias);
            }
            if !unproven_guard {
                aliases
                    .entry(scope)
                    .or_default()
                    .entry(alias)
                    .or_default()
                    .extend(targets.clone());
                imported_targets = targets;
            }
        }
        let resolved = !unproven_guard
            && modules.len() == 1
            && (r.alias.is_none()
                || is_wildcard
                || candidates.len() == 1
                || imported_targets.len() == 1);
        let is_external_alias = r.alias.as_deref().is_some_and(|a| {
            external_aliases
                .get(scope)
                .and_then(|e| e.get(a))
                .is_some_and(Option::is_some)
        });
        let status = if unproven_guard {
            "unresolved"
        } else if resolved {
            "resolved"
        } else if modules.len() > 1 || candidates.len() > 1 || imported_targets.len() > 1 {
            "ambiguous"
        } else if external.is_some() || is_external_alias {
            "external"
        } else {
            "unresolved"
        };
        let evidence: Cow<'static, str> = if unproven_guard {
            "type guard condition is unproven or shadowed".into()
        } else if is_wildcard && resolved {
            format!(
                "wildcard import from {symbol}: imported from {}",
                modules[0].path
            )
            .into()
        } else if reexport_symbol {
            format!("import {symbol}: explicit local package re-export").into()
        } else if stub_symbol {
            format!("import {symbol}: declaration from paired type stub; runtime declaration not indexed").into()
        } else if let Some(kind) = external {
            format!("{kind}; no indexed provider for {symbol}").into()
        } else if modules.is_empty() && normalized.as_ref().is_some_and(|n| !n.relative) {
            format!("no indexed provider for absolute import {symbol}; external dependency or missing source remains unverified").into()
        } else {
            format!(
                "import {symbol}: {} modules, {} alias targets",
                modules.len(),
                candidates.len()
            )
            .into()
        };
        graph.reference_coverage(path, r, status, evidence);
        let import_owner = input
            .interner
            .lookup_node(by_id, r.source.as_str())
            .filter(|owner| {
                owner.language == "html"
                    || (owner.kind == "template_scope"
                        && owner.details["embedded_language"] == "javascript")
            })
            .and_then(|owner| owner.details["html_import_owner"].as_str())
            .filter(|id| {
                input.interner.lookup_node(by_id, id).is_some_and(|node| {
                    node.kind == "module" && node.language == "html" && node.path == *path
                })
            })
            .unwrap_or(r.source.as_str());
        if modules.len() == 1 {
            graph.edges.push(Edge {
                src: import_owner.into(),
                dst: modules[0].id.clone(),
                kind: "imports".into(),
                path: path.to_owned(),
                line: r.line,
                evidence: r.expression.clone(),
                confidence: "exact".into(),
            });
            if profile.is_some_and(|profile| profile.id() == "typescript") {
                if let [declaration] = candidates.as_slice() {
                    if matches!(declaration.kind.as_str(), "type" | "interface")
                        && declaration.path == modules[0].path
                    {
                        graph.edges.push(Edge {
                            src: import_owner.into(),
                            dst: declaration.id.clone(),
                            kind: "imports".into(),
                            path: path.to_owned(),
                            line: r.line,
                            evidence: r.expression.clone(),
                            confidence: "exact".into(),
                        });
                    }
                }
            }
            if child_module {
                graph.edges.push(Edge {
                    src: import_owner.into(),
                    dst: candidates[0].id.clone(),
                    kind: "imports".into(),
                    path: path.to_owned(),
                    line: r.line,
                    evidence: r.expression.clone(),
                    confidence: "exact".into(),
                });
            }
            if stub_symbol {
                if let Some(stub) = paired_stub {
                    graph.edges.push(Edge {
                        src: import_owner.into(),
                        dst: stub.id.clone(),
                        kind: "imports".into(),
                        path: path.to_owned(),
                        line: r.line,
                        evidence: r.expression.clone(),
                        confidence: "exact".into(),
                    });
                }
            }
            if reexport_symbol {
                graph.edges.push(Edge {
                    src: import_owner.into(),
                    dst: candidates[0].id.clone(),
                    kind: "imports".into(),
                    path: path.to_owned(),
                    line: r.line,
                    evidence: r.expression.clone(),
                    confidence: "exact".into(),
                });
            } else if profile.is_some_and(|profile| profile.id() == "python")
                && !child_module
                && !stub_symbol
                && imported_targets.len() == 1
            {
                graph.edges.push(Edge {
                    src: import_owner.into(),
                    dst: imported_targets[0].id.clone(),
                    kind: "imports".into(),
                    path: path.to_owned(),
                    line: r.line,
                    evidence: r.expression.clone(),
                    confidence: "exact".into(),
                });
            }
        }
    }
    #[cfg(feature = "lang-rust")]
    if let Some(module) = facts
        .nodes
        .iter()
        .find(|node| node.kind == "module" && node.language == "rust")
    {
        if references.iter().any(|reference| {
            reference.kind == "imports"
                && reference.source == module.id
                && reference.alias.as_deref() == Some("*")
        }) {
            let bindings = aliases.entry(module.qualname.as_str()).or_default();
            for (name, targets) in rust_forwarding.visible_bindings(module, &module.qualname) {
                if by_qual
                    .get(qualified(&mut *lookup_key, &module.qualname, name))
                    .is_some_and(|nodes| {
                        nodes
                            .iter()
                            .any(|node| node.path == *path && node.kind != "module_declaration")
                    })
                {
                    bindings.remove(name);
                } else {
                    bindings.insert(name, targets.to_vec());
                }
            }
        }
    }
}
