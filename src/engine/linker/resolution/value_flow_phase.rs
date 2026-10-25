use super::*;

mod members;
mod relation_edges;

fn has_node_binding(
    interner: &super::super::interner::LinkerInterner<'_>,
    bindings: &HashMap<super::super::interner::SymbolId, HashSet<super::super::interner::SymbolId>>,
    owner: &str,
    name: &str,
) -> bool {
    let (Some(owner), Some(name)) = (interner.lookup_symbol(owner), interner.lookup_symbol(name))
    else {
        return false;
    };
    bindings
        .get(&owner)
        .is_some_and(|bindings| bindings.contains(&name))
}

pub(super) fn phase_value_flow<'a, 'file, F>(resolver: &mut FileResolver<'a, 'file, F>)
where
    'a: 'file,
    F: AsRef<Facts> + Sync,
{
    let input = resolver.input;
    let path = resolver.path;
    let facts = resolver.facts;
    let references = resolver.references;
    let file_module = resolver.file_module;
    let file_language_context = resolver.language_context;
    let interner = input.interner;
    let dependency_registry = input.dependency_registry;
    let flows = input.flows;
    let by_name = input.by_name;
    let by_qual = input.by_qual;
    let by_id = input.by_id;
    let by_module = input.by_module;
    let node_bindings = input.node_bindings;
    let node_rebindings = input.node_rebindings;
    let semantic_context = input.semantic_context;
    let value_flow = input.value_flow;
    let profile = input.profiles.get(path).copied();
    let language_linker = profile.map_or(&GENERIC_LINKER as &dyn LanguageLinker, |profile| {
        languages::linker_for(profile.id())
    });
    let language_package_exports = input
        .package_exports
        .get(profile.map_or("", |profile| profile.id()))
        .expect("package export index exists for every linked language");
    let lookup =
        |name: &str| -> &[&Node] { by_qual.get(name).map(Vec::as_slice).unwrap_or_default() };
    let member_lookup = &resolver.linker.member_lookup;
    #[cfg(feature = "lang-html")]
    let html_templates = &resolver.linker.html_templates;
    #[cfg(feature = "lang-html")]
    let html_scripts = &resolver.linker.html_scripts;
    #[cfg(feature = "lang-html")]
    let template_loads = resolver.template_loads.as_slice();
    #[cfg(feature = "lang-html")]
    let template_registry_rules = resolver.template_registry_rules.as_slice();
    #[cfg(feature = "lang-html")]
    let template_rules = resolver.template_rules.as_slice();

    let mut value_queries = HashSet::new();
    for node in &facts.nodes {
        if let Some(FlowState::Valid { facts, .. }) = flows.and_then(|flows| {
            interner
                .lookup_symbol(node.id.as_str())
                .and_then(|id| flows.get(&id).copied())
        }) {
            if !facts.fact_limit_exceeded {
                for fact in facts
                    .annotation_references()
                    .filter(|fact| fact.value_query)
                {
                    value_queries.insert((
                        interner.lookup_symbol(&node.id).expect("catalog owner"),
                        fact.expression.as_str(),
                        fact.position.line,
                        fact.position.column,
                    ));
                }
            }
        }
    }
    #[cfg(feature = "lang-typescript")]
    let returned_object_field_writes =
        super::super::javascript::resolution::returned_object_field_writes(
            profile, references, by_id, interner,
        );
    #[cfg(feature = "lang-html")]
    let reference_profile = |reference: &Reference| {
        html::scripts::reference_profile(profile, reference, by_id, interner)
    };
    #[cfg(not(feature = "lang-html"))]
    let reference_profile = |_reference: &Reference| profile;
    let configured_autoimport = |name: &str| semantic_context.autoimport(path, name);
    #[cfg(feature = "lang-typescript")]
    let object_ownership = |node: &Node| {
        super::super::typescript::resolution::object_ownership(
            node,
            flows,
            interner,
            &file_language_context.syntax_rules,
        )
    };
    let component_module = facts.nodes.iter().find(|node| node.kind == "module");
    let commonjs_module = facts.nodes.iter().find(|node| {
        node.kind == "module"
            && matches!(node.language.as_str(), "javascript" | "typescript" | "vue")
    });
    let configured_component_macro =
        |name: &str, at: crate::core::semantic::SourcePosition, owner: &str| {
            let Some((rule, role)) = file_language_context.component_macro(name) else {
                return false;
            };
            let matches_facts = |owner: &Node| {
                flows
                    .and_then(|flows| {
                        interner
                            .lookup_symbol(owner.id.as_str())
                            .and_then(|id| flows.get(&id))
                    })
                    .is_some_and(|state| {
                        matches!(
                            state,
                            FlowState::Valid { facts, .. }
                                if facts.component_macros().any(|fact| {
                                    fact.callee == name
                                        && fact.role == role
                                        && fact.origin.pack == rule.pack
                                        && fact.origin.rule == rule.id
                                        && fact.position == at
                                })
                        )
                    })
            };
            component_module.is_some_and(&matches_facts)
                || facts
                    .nodes
                    .iter()
                    .find(|node| node.id == owner)
                    .is_some_and(matches_facts)
        };
    let builtin_in_scope = |expression: &str, reference: &Reference| {
        if configured_autoimport(expression) {
            return true;
        }
        if configured_component_macro(expression, reference.position(), &reference.source) {
            return true;
        }
        let active_profile = reference_profile(reference);
        #[cfg(feature = "lang-html")]
        if let Some(is_builtin) = html::templates::builtin_in_scope(
            expression,
            reference,
            active_profile,
            template_loads,
            template_rules,
        ) {
            return is_builtin;
        }
        active_profile.is_some_and(|profile| {
            profile.builtin(expression)
                || (profile.id() == "python"
                    && expression
                        .split_once('.')
                        .is_some_and(|(receiver, member)| {
                            profile.builtin_type(receiver)
                                && profile.builtin_member(receiver, member)
                        }))
                || super::super::typescript::resolution::builtin_property_chain(
                    expression,
                    profile,
                    path,
                    semantic_context,
                )
        })
    };
    let FileResolver {
        graph,
        aliases,
        external_aliases,
        type_only_aliases,
        lookup_key,
        ..
    } = resolver;
    {
        let alias_for = |name: &str, owner: Option<&Node>, definition_scope: bool| {
            let mut scope = owner.map(|n| reference_scope(n, path)).unwrap_or("");
            if definition_scope {
                scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
            }
            loop {
                let class_scope = !profile.is_some_and(|p| p.class_scope())
                    && !definition_scope
                    && owner.is_some_and(|n| matches!(n.kind.as_str(), "function" | "method"))
                    && by_qual.get(scope).is_some_and(|nodes| {
                        nodes
                            .iter()
                            .any(|n| n.path == *path && matches!(n.kind.as_str(), "class" | "impl"))
                    });
                if !class_scope {
                    if let Some(targets) = aliases.get(scope).and_then(|a| a.get(name)) {
                        return Some(targets);
                    }
                }
                if scope.is_empty() {
                    return None;
                }
                scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
            }
        };
        let external_for = |name: &str, owner: Option<&Node>, definition_scope: bool| {
            let mut scope = owner.map(|n| reference_scope(n, path)).unwrap_or("");
            if definition_scope {
                scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
            }
            loop {
                let class_scope = !profile.is_some_and(|p| p.class_scope())
                    && owner.is_some_and(|n| matches!(n.kind.as_str(), "function" | "method"))
                    && by_qual.get(scope).is_some_and(|nodes| {
                        nodes
                            .iter()
                            .any(|n| n.path == *path && matches!(n.kind.as_str(), "class" | "impl"))
                    });
                if !class_scope
                    && aliases
                        .get(scope)
                        .is_some_and(|entries| entries.contains_key(name))
                {
                    return external_aliases
                        .get(scope)
                        .and_then(|entries| entries.get(name))
                        .and_then(Option::as_ref);
                }
                if scope.is_empty() {
                    return None;
                }
                scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
            }
        };
        let commonjs_external_admitted =
            |name: &str, owner: Option<&Node>, at: crate::core::semantic::SourcePosition| {
                let Some(module) = commonjs_module else {
                    return true;
                };
                !semantic_context.tracks_commonjs_binding(module, name)
                    || owner.is_some_and(|owner| {
                        semantic_context.admits_commonjs_binding(module, name, owner, at)
                    })
            };
        let external_call_for = |name: &str, owner: Option<&Node>, at| {
            commonjs_external_admitted(name, owner, at)
                .then(|| external_for(name, owner, false))
                .flatten()
        };
        let python_nullable_parameter_wrapper = |raw_annotation: &str, node: &Node| {
            python::resolution::nullable_parameter_wrapper(
                profile,
                raw_annotation,
                node,
                references,
                |head, node| external_for(head, Some(node), true).map(|(_, line)| *line),
            )
        };
        let is_type_only = |name: &str, owner: Option<&Node>, definition_scope: bool| -> bool {
            let mut scope = owner.map(|n| reference_scope(n, path)).unwrap_or("");
            if definition_scope {
                scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
            }
            loop {
                let class_scope = !profile.is_some_and(|p| p.class_scope())
                    && owner.is_some_and(|n| matches!(n.kind.as_str(), "function" | "method"))
                    && by_qual.get(scope).is_some_and(|nodes| {
                        nodes
                            .iter()
                            .any(|n| n.path == *path && matches!(n.kind.as_str(), "class" | "impl"))
                    });
                if !class_scope
                    && type_only_aliases
                        .get(scope)
                        .is_some_and(|entries| entries.contains(name))
                {
                    return true;
                }
                if scope.is_empty() {
                    return false;
                }
                scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
            }
        };
        let mut alias_import_positions: HashMap<
            (&str, &str),
            crate::core::semantic::SourcePosition,
        > = HashMap::new();
        for reference in references
            .iter()
            .filter(|reference| reference.kind == "imports")
        {
            if let Some(alias) = reference.alias.as_deref() {
                let scope = interner
                    .lookup_node(by_id, reference.source.as_str())
                    .map_or(file_module, |node| node.qualname.as_str());
                let position = alias_import_positions.entry((scope, alias)).or_default();
                *position = (*position).max(reference.position());
            }
        }
        let python_resolution = python::resolution::PythonResolutionView {
            input,
            path,
            file_module,
            aliases: &*aliases,
            alias_import_positions: &alias_import_positions,
            member_lookup,
        };
        let path_constructor_aliases = python::resolution::path_constructor_aliases(
            profile,
            references,
            by_id,
            interner,
            file_module,
        );
        let dynamic_type_aliases = python::resolution::dynamic_type_aliases(
            profile,
            references,
            by_id,
            interner,
            file_module,
        );
        let mut typed_parameters: HashMap<&str, HashMap<&str, ReceiverType<'_>>> = HashMap::new();
        let mut type_candidates = Vec::new();
        for node in &facts.nodes {
            for (parameter, annotation) in node.parameter_types() {
                if has_node_binding(interner, node_rebindings, node.id.as_str(), parameter) {
                    continue;
                }
                let Some(raw_annotation) = annotation else {
                    continue;
                };
                let annotation = receiver_type_name(raw_annotation);
                #[cfg(feature = "lang-rust")]
                let annotation = annotation.or_else(|| {
                    rust::semantic_context::qualified_annotation_type(
                        profile,
                        raw_annotation,
                        path,
                        dependency_registry,
                    )
                });
                let Some(annotation) = annotation else {
                    continue;
                };
                let raw_head = receiver_head(raw_annotation);
                let (head, member) = annotation.split_once('.').unwrap_or((&annotation, ""));
                #[cfg(feature = "lang-rust")]
                let qualified_external_annotation = || {
                    rust::semantic_context::qualified_external_annotation(
                        profile,
                        raw_annotation,
                        head,
                        path,
                        dependency_registry,
                    )
                };
                #[cfg(not(feature = "lang-rust"))]
                let qualified_external_annotation = || None;
                #[cfg(feature = "lang-rust")]
                let external_annotation = || {
                    rust::semantic_context::external_annotation_type(
                        profile,
                        raw_head,
                        head,
                        path,
                        dependency_registry,
                    )
                };
                #[cfg(not(feature = "lang-rust"))]
                let external_annotation = || None;
                type_candidates.clear();
                let mut scope = node
                    .qualname
                    .rsplit_once('.')
                    .map_or("", |(parent, _)| parent);
                let mut annotation_shadowed = false;
                while !scope.is_empty() {
                    if by_qual.get(scope).is_some_and(|scope_nodes| {
                        scope_nodes.iter().any(|scope_node| {
                            scope_node.path == *path
                                && has_node_binding(
                                    interner,
                                    node_rebindings,
                                    scope_node.id.as_str(),
                                    head,
                                )
                        })
                    }) {
                        annotation_shadowed = true;
                        break;
                    }
                    type_candidates.extend(
                        lookup(qualified(lookup_key, scope, &annotation))
                            .iter()
                            .copied()
                            .filter(|candidate| candidate.path == *path),
                    );
                    if !type_candidates.is_empty()
                        && alias_import_positions
                            .get(&(scope, head))
                            .is_some_and(|position| {
                                position.line <= node.line
                                    && type_candidates
                                        .iter()
                                        .any(|candidate| candidate.line < position.line)
                            })
                    {
                        type_candidates.clear();
                        annotation_shadowed = true;
                        break;
                    }
                    if !type_candidates.is_empty() {
                        break;
                    }
                    if by_qual.get(scope).is_some_and(|scope_nodes| {
                        scope_nodes
                            .iter()
                            .filter(|scope_node| scope_node.path == *path)
                            .any(|scope_node| {
                                has_node_binding(
                                    interner,
                                    node_bindings,
                                    scope_node.id.as_str(),
                                    head,
                                )
                            })
                    }) {
                        annotation_shadowed = true;
                        break;
                    }
                    if aliases
                        .get(scope)
                        .is_some_and(|entries| entries.contains_key(head))
                    {
                        break;
                    }
                    scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
                }
                let imported_type = if annotation_shadowed || !type_candidates.is_empty() {
                    None
                } else {
                    alias_for(head, Some(node), true)
                };
                if let Some(imported) = imported_type {
                    if member.is_empty() {
                        type_candidates.extend_from_slice(imported);
                    } else {
                        for target in imported {
                            type_candidates.extend(
                                lookup(qualified(lookup_key, &target.qualname, member))
                                    .iter()
                                    .copied()
                                    .filter(|candidate| candidate.path == target.path),
                            );
                        }
                    }
                }
                if type_candidates.is_empty()
                    && imported_type.is_some_and(Vec::is_empty)
                    && !annotation_shadowed
                    && python_nullable_parameter_wrapper(raw_annotation, node)
                {
                    continue;
                }
                type_candidates.retain(|candidate| {
                    matches!(
                        candidate.kind.as_str(),
                        "class" | "struct" | "enum" | "type" | "interface"
                    )
                });
                type_candidates.sort_by(|a, b| a.id.cmp(&b.id));
                type_candidates.dedup_by_key(|candidate| candidate.id.as_str());
                let receiver = match type_candidates.as_slice() {
                    [target] => ReceiverType::Local(target),
                    [] => {
                        if imported_type.is_some_and(|targets| targets.is_empty()) {
                            external_for(head, Some(node), true)
                                .filter(|(_, import_line)| {
                                    *import_line <= node.line
                                        && !python::resolution::is_dynamic_typing_annotation(
                                            head,
                                            member,
                                            node,
                                            aliases,
                                            &dynamic_type_aliases,
                                        )
                                })
                                .map(|(module, line)| {
                                    ReceiverType::External(module.as_str(), *line)
                                })
                                .unwrap_or(ReceiverType::Unknown)
                        } else if matches!(head, "std" | "core" | "alloc") {
                            match head {
                                "std" => ReceiverType::External("std", 0),
                                "core" => ReceiverType::External("core", 0),
                                "alloc" => ReceiverType::External("alloc", 0),
                                _ => ReceiverType::Unknown,
                            }
                        } else if !annotation_shadowed
                            && imported_type.is_none()
                            && !member.is_empty()
                            && by_qual.get(annotation.as_ref()).is_none()
                        {
                            qualified_external_annotation()
                                .or_else(external_annotation)
                                .unwrap_or(ReceiverType::Unknown)
                        } else {
                            external_annotation().unwrap_or(ReceiverType::Unknown)
                        }
                    }
                    _ => ReceiverType::Ambiguous,
                };
                if !matches!(receiver, ReceiverType::Unknown) {
                    typed_parameters
                        .entry(node.id.as_str())
                        .or_default()
                        .insert(parameter, receiver);
                }
            }
        }

        #[cfg(feature = "lang-typescript")]
        let lexical_this_calls = super::super::typescript::resolution::lexical_this_calls(
            facts,
            references,
            object_ownership,
        );
        let local_receiver_class = |owner: &Node| {
            owner
                .qualname
                .rsplit_once('.')
                .and_then(|(scope, _)| by_qual.get(scope))
                .and_then(|nodes| {
                    let mut classes = nodes.iter().copied().filter(|node| {
                        node.path == *path && matches!(node.kind.as_str(), "class" | "interface")
                    });
                    let class = classes.next()?;
                    classes.next().is_none().then_some(class)
                })
        };
        let mut candidates = Vec::new();
        let ordinary_member_receivers: HashSet<&str> = facts
            .nodes
            .iter()
            .filter(|node| node.kind == "enum")
            .map(|node| node.name.as_str())
            .chain(
                facts
                    .references
                    .iter()
                    .filter(|reference| reference.kind == "imports")
                    .filter_map(|reference| reference.alias.as_deref()),
            )
            .collect();
        for r in references.iter().filter(|r| {
            matches!(
                r.kind.as_str(),
                "calls"
                    | "inherits"
                    | "implements"
                    | "bases"
                    | "decorates"
                    | "mutates"
                    | "handles"
                    | "references"
                    | "injects"
            )
        }) {
            let owner = interner.lookup_node(by_id, r.source.as_str());
            let active_profile = reference_profile(r);
            if let Some((result, emit)) = super::super::typescript::resolution::property_reference(
                input,
                r,
                &ordinary_member_receivers,
                active_profile,
            ) {
                if emit {
                    emit_inferred(graph, path, r, result, None);
                }
                continue;
            }

            if let Some(
                ReceiverHint::ConfiguredReference { origin }
                | ReceiverHint::ConfiguredTypedMember { origin, .. }
                | ReceiverHint::ConfiguredMacroCallable { origin, .. },
            ) = &r.receiver_hint
            {
                let context = &file_language_context;
                let effective = if r.kind == "injects" {
                    context
                        .di_rule_origins
                        .values()
                        .any(|candidate| candidate == origin)
                } else {
                    context
                        .syntax_rules
                        .iter()
                        .any(|rule| rule.pack == origin.pack && rule.id == origin.rule)
                };
                if !effective {
                    graph.reference_coverage(
                        path,
                        r,
                        "unresolved",
                        "extraction rule is no longer effective".into(),
                    );
                    continue;
                }
            }
            #[cfg(feature = "lang-rust")]
            if super::super::rust::resolution::trait_object_call(
                active_profile,
                r,
                owner,
                path,
                value_flow,
                semantic_context,
                graph,
            ) {
                continue;
            }
            let builtin_origin = match active_profile.map(|profile| profile.family().0) {
                Some("rust") => Some("Rust standard library"),
                Some("python") => Some("Python standard library"),
                Some("typescript" | "javascript") => Some("JavaScript/Web platform"),
                _ => None,
            };
            #[cfg(feature = "lang-html")]
            if html::templates::resolve_reference(
                input,
                html_templates,
                path,
                template_loads,
                template_registry_rules,
                r,
                graph,
            ) {
                continue;
            }
            if members::resolve_computed_call(input, path, r, active_profile, builtin_origin, graph)
            {
                continue;
            }
            if r.kind == "references" && owner.is_some_and(|owner| owner.kind == "template_scope") {
                if vue::resolution::resolve_component_tag(input, path, r, owner, graph) {
                    continue;
                }
                if let Some(ReceiverHint::ConfiguredTypedMember { field_id, origin }) =
                    &r.receiver_hint
                {
                    if !file_language_context.syntax_rules.iter().any(|rule| rule.pack == origin.pack && rule.id == origin.rule && matches!(rule.operation, languages::manifests::syntax::SyntaxOperation::ComponentMacros { .. })) { continue; }
                    if let Some(field) =
                        interner
                            .lookup_node(by_id, field_id.as_str())
                            .filter(|field| {
                                field.path == *path
                                    && field.kind == "field"
                                    && field.name
                                        == r.expression
                                            .rsplit('.')
                                            .next()
                                            .unwrap_or(r.expression.as_str())
                            })
                    {
                        graph.reference_edge(path, r, field.id.clone(), "references", "exact");
                        graph.reference_coverage(
                            path,
                            r,
                            "resolved",
                            format!(
                                "script setup typed macro field {}; rule:{}:{}",
                                field.qualname, origin.pack, origin.rule
                            )
                            .into(),
                        );
                        continue;
                    }
                }
            }
            let expression = if r.expression.contains("::") {
                Cow::Owned(r.expression.replace("::", "."))
            } else if profile.is_some_and(|profile| {
                matches!(profile.id(), "javascript" | "typescript" | "vue" | "html")
            }) && r.expression.contains("?.")
            {
                Cow::Owned(r.expression.replace("?.", "."))
            } else {
                Cow::Borrowed(r.expression.as_str())
            };
            #[cfg(feature = "lang-rust")]
            let rust_explicit_address = profile.is_some_and(|profile| {
                profile.id() == "rust"
                    && ["super::", "crate::", "self::"]
                        .iter()
                        .any(|prefix| r.expression.starts_with(prefix))
            });
            #[cfg(not(feature = "lang-rust"))]
            let rust_explicit_address = false;
            #[cfg(feature = "lang-rust")]
            let rust_module_target = super::super::rust::resolution::rust_explicit_module_target(
                profile,
                path,
                file_module,
                &r.expression,
            );
            #[cfg(not(feature = "lang-rust"))]
            let rust_module_target: Option<String> = None;
            let (first, tail) = expression.split_once('.').unwrap_or((&expression, ""));
            let owner = interner.lookup_node(by_id, r.source.as_str());
            if !r.dynamic && matches!(r.kind.as_str(), "calls" | "references") {
                if let Some(symbol) = owner.and_then(|owner| {
                    semantic_context.named_member(owner, &expression, r.position())
                }) {
                    let member = match symbol {
                        value_flow::Symbol::Callable(node)
                            if r.kind != "calls"
                                || matches!(node.kind.as_str(), "function" | "method") =>
                        {
                            semantic_context::Member::Local(node)
                        }
                        value_flow::Symbol::Type(value_flow::TypeTarget::Local(node))
                            if r.kind == "references" =>
                        {
                            semantic_context::Member::Local(node)
                        }
                        value_flow::Symbol::Ambiguous
                        | value_flow::Symbol::Type(value_flow::TypeTarget::Ambiguous) => {
                            semantic_context::Member::Ambiguous
                        }
                        _ => semantic_context::Member::Unknown,
                    };
                    emit_inferred(graph, path, r, member, None);
                    continue;
                }
            }
            #[cfg(feature = "lang-typescript")]
            let implicit_receiver = if first == "this" {
                tail
            } else if tail.is_empty() {
                first
            } else {
                ""
            };
            #[cfg(feature = "lang-typescript")]
            let static_implicit_reference = r.dynamic
                && r.kind == "references"
                && r.receiver_hint.is_none()
                && !implicit_receiver.is_empty()
                && expression
                    .chars()
                    .all(|character| character.is_alphanumeric() || "_.$:".contains(character))
                && active_profile
                    .filter(|profile| {
                        matches!(profile.id(), "javascript" | "typescript" | "vue" | "html")
                    })
                    .is_some_and(|_| {
                        file_language_context.syntax_rules.iter().any(|rule| {
                            let languages::manifests::syntax::SyntaxOperation::ImplicitReceivers {
                                names,
                            } = &rule.operation
                            else {
                                return false;
                            };
                            names.iter().any(|name| name == implicit_receiver)
                        })
                    });
            #[cfg(not(feature = "lang-typescript"))]
            let static_implicit_reference = false;
            if r.dynamic && !static_implicit_reference {
                let mut computed = receivers::Member::Unknown;
                let mut literal_builtin = false;
                if profile.is_some_and(|profile| profile.id() == "python") {
                    (computed, literal_builtin) = python::resolution::dynamic_receiver_member(
                        &python_resolution,
                        r,
                        &path_constructor_aliases,
                        &mut candidates,
                        lookup_key,
                        &alias_for,
                        &external_for,
                    );
                }
                let (status, evidence): (&str, Cow<'static, str>) = match computed {
                    receivers::Member::Local(method)
                        if matches!(method.kind.as_str(), "method" | "function") =>
                    {
                        graph.reference_edge(
                            path,
                            r,
                            method.id.clone(),
                            r.kind.as_str(),
                            "inferred",
                        );
                        (
                            "resolved",
                            format!(
                                "computed receiver has statically proven member {}",
                                method.qualname
                            )
                            .into(),
                        )
                    }
                    receivers::Member::External(module, line, Some(provider), _) => {
                        let diagnostic_marker = if profile.is_some_and(|profile| {
                            profile.id() == "python" && !profile.is_stdlib(module)
                        }) {
                            "; diagnostic_reason=framework_magic"
                        } else {
                            ""
                        };
                        ("external", format!("finite Python framework member of {provider}: {} via external import {module} at line {line}{diagnostic_marker}", r.expression).into())
                    }
                    receivers::Member::External(module, _, None, _) => (
                        "external",
                        format!(
                            "call through external import {module}; callable target unverified"
                        )
                        .into(),
                    ),
                    _ if literal_builtin => (
                        "resolved",
                        format!("standard library or built-in callee: {}", r.expression).into(),
                    ),
                    _ => (
                        "unresolved",
                        "computed receiver or dynamic callee; callable identity is unknown".into(),
                    ),
                };
                graph.reference_coverage(path, r, status, evidence);
                continue;
            }
            #[cfg(feature = "lang-typescript")]
            if super::super::javascript::resolution::has_returned_object_builtin_this_member(
                input,
                path,
                r,
                &expression,
                active_profile,
                &returned_object_field_writes,
            ) {
                emit_inferred(
                    graph,
                    path,
                    r,
                    semantic_context::Member::Builtin,
                    builtin_origin,
                );
                continue;
            }
            #[cfg(feature = "lang-typescript")]
            if super::super::javascript::resolution::object_factory_this_member(
                input,
                path,
                r,
                aliases,
                object_ownership,
                &lexical_this_calls,
                graph,
            ) {
                continue;
            }
            #[cfg(feature = "lang-typescript")]
            if super::super::typescript::resolution::lexical_this_member(
                input,
                path,
                r,
                &expression,
                &lexical_this_calls,
                lookup_key,
                graph,
            ) {
                continue;
            }
            if r.kind == "calls" && tail.is_empty() {
                let origin =
                    vue::resolution::component_macro_callable_origin(r, file_language_context);
                if let Some(origin) = origin {
                    graph.reference_coverage(
                        path,
                        r,
                        "external",
                        format!("{origin} callable from this file's Vue script setup binding")
                            .into(),
                    );
                    continue;
                }
            }
            if r.kind == "references"
                && tail.is_empty()
                && owner.is_some_and(|owner| owner.kind == "template_scope")
            {
                if let Some(binding) = vue::resolution::template_lexical_binding(
                    input,
                    path,
                    r,
                    owner,
                    first,
                    component_module,
                ) {
                    graph.reference_edge(path, r, binding.id.clone(), "references", "exact");
                    graph.reference_coverage(
                        path,
                        r,
                        "resolved",
                        format!("Vue template lexical binding in {}", binding.qualname).into(),
                    );
                    continue;
                }
            }
            #[cfg(feature = "lang-rust")]
            if rust::resolution::resolve_self_reference(
                profile,
                r,
                owner,
                path,
                member_lookup,
                graph,
            ) {
                continue;
            }
            if r.kind != "imports"
                && matches!(r.receiver_hint.as_ref(), Some(ReceiverHint::TypeOnlyImport))
            {
                graph.reference_coverage(
                    path,
                    r,
                    "unresolved",
                    "type-only import cannot be referenced at runtime".into(),
                );
                continue;
            }
            let mut scope = owner.map(|n| reference_scope(n, path)).unwrap_or("");
            let definition_scope = matches!(
                r.kind.as_str(),
                "decorates" | "inherits" | "implements" | "bases"
            );
            if definition_scope {
                scope = scope.rsplit_once('.').map_or("", |(p, _)| p);
            }
            if matches!(r.kind.as_str(), "calls" | "mutates" | "handles")
                && is_type_only(first, owner, definition_scope)
            {
                graph.reference_coverage(
                    path,
                    r,
                    "unresolved",
                    "type-only import cannot be used as a runtime callable".into(),
                );
                continue;
            }
            if !tail.is_empty() && is_type_only(first, owner, definition_scope) {
                graph.reference_coverage(
                    path,
                    r,
                    "unresolved",
                    "type-only import member cannot be referenced at runtime".into(),
                );
                continue;
            }
            if r.kind == "calls" {
                if let Some(evidence) = owner.and_then(|owner| {
                    value_flow.configured_callable(owner, &expression, r.position())
                }) {
                    graph.reference_coverage(path, r, "external", evidence.into());
                    continue;
                }
            }
            let mut shadowed = false;
            let mut typed_receiver = None;
            let mut lexical_head_declared = false;
            let builtin_member_candidate = !tail.is_empty() && builtin_in_scope(&expression, r);
            let imported = alias_for(first, owner, definition_scope).filter(|_| {
                python::resolution::import_visible_at(&python_resolution, r, definition_scope)
            });
            candidates.clear();
            while !scope.is_empty() {
                if let Some(scope_nodes) = by_qual.get(scope) {
                    if !profile.is_some_and(|p| p.class_scope())
                        && !definition_scope
                        && owner.is_some_and(|n| matches!(n.kind.as_str(), "function" | "method"))
                        && scope_nodes
                            .iter()
                            .any(|n| n.path == *path && matches!(n.kind.as_str(), "class" | "impl"))
                    {
                        scope = scope.rsplit_once('.').map_or("", |(p, _)| p);
                        continue;
                    }
                    if profile.is_none_or(|p| p.value_binding_applies(r)) {
                        let mut binding_owners = scope_nodes.iter().filter(|node| {
                            node.path == *path
                                && has_node_binding(
                                    interner,
                                    node_bindings,
                                    node.id.as_str(),
                                    first,
                                )
                                && !owner.is_some_and(|owner| {
                                    semantic_context.admits_commonjs_binding(
                                        node,
                                        first,
                                        owner,
                                        r.position(),
                                    )
                                })
                        });
                        if let Some(binding_owner) = binding_owners.next() {
                            shadowed = true;
                            if !tail.is_empty() && binding_owners.next().is_none() {
                                typed_receiver = typed_parameters
                                    .get(binding_owner.id.as_str())
                                    .and_then(|parameters| parameters.get(first));
                            }
                            break;
                        }
                    }
                }
                if aliases.get(scope).is_some_and(|a| a.contains_key(first)) {
                    break;
                }
                if tail.is_empty() {
                    candidates.extend(
                        lookup(qualified(lookup_key, scope, &expression))
                            .iter()
                            .copied()
                            .filter(|n| n.path == *path),
                    );
                    lexical_head_declared |= !candidates.is_empty();
                    if !candidates.is_empty() {
                        break;
                    }
                } else if builtin_member_candidate {
                    lexical_head_declared |= lookup(qualified(lookup_key, scope, first))
                        .iter()
                        .any(|n| n.path == *path);
                }
                scope = scope.rsplit_once('.').map_or("", |(p, _)| p);
            }
            let known_receiver = !tail.is_empty()
                && owner.is_some_and(|n| {
                    profile.is_some_and(|p| language_linker.resolve_receiver(p, first, n))
                })
                && owner.is_some_and(|n| {
                    if has_node_binding(interner, node_rebindings, n.id.as_str(), first) {
                        return false;
                    }
                    n.qualname.rsplit_once('.').is_some_and(|(scope, _)| {
                        by_qual.get(scope).is_some_and(|nodes| {
                            nodes.iter().any(|n| {
                                n.path == *path && matches!(n.kind.as_str(), "class" | "impl")
                            })
                        })
                    })
                });
            #[cfg(feature = "lang-typescript")]
            if let Some(evidence) = active_profile
                .filter(|profile| {
                    !shadowed
                        && imported.is_none()
                        && !lexical_head_declared
                        && candidates.is_empty()
                        && !implicit_receiver.is_empty()
                        && matches!(profile.id(), "javascript" | "typescript" | "vue" | "html")
                })
                .and_then(|_| {
                    file_language_context.syntax_rules.iter().find_map(|rule| {
                        let languages::manifests::syntax::SyntaxOperation::ImplicitReceivers {
                            names,
                        } = &rule.operation
                        else {
                            return None;
                        };
                        names.iter().any(|name| name == implicit_receiver).then(|| {
                            format!(
                                "{}; rule:{}:{}",
                                rule.display
                                    .builtin
                                    .as_deref()
                                    .unwrap_or("framework builtin: configured implicit receiver"),
                                rule.pack,
                                rule.id
                            )
                        })
                    })
                })
            {
                let locally_owned_member = first == "this"
                    && owner.is_some_and(|owner| {
                        let receiver = known_receiver
                            .then(|| local_receiver_class(owner))
                            .flatten()
                            .map(value_flow::TypeTarget::Local);
                        (known_receiver && receiver.is_none())
                            || receiver
                                .as_ref()
                                .or_else(|| value_flow.lookup(owner, first, r.position()))
                                .is_some_and(|receiver| {
                                    matches!(
                                        receiver,
                                        value_flow::TypeTarget::Unknown
                                            | value_flow::TypeTarget::Ambiguous
                                            | value_flow::TypeTarget::Candidates { .. }
                                    ) || !matches!(
                                        semantic_context.lookup_member_mode(
                                            receiver,
                                            implicit_receiver
                                                .split('.')
                                                .next()
                                                .unwrap_or(implicit_receiver),
                                            owner,
                                            false
                                        ),
                                        semantic_context::Member::Unknown
                                    )
                                })
                    });
                if !locally_owned_member {
                    graph.reference_coverage(path, r, "external", evidence.into());
                    continue;
                }
            }
            let component_template_member =
                vue::resolution::is_component_template_member(r, owner, tail);
            let typed_member_read = r.kind == "references"
                && !tail.is_empty()
                && !known_receiver
                && profile.is_some_and(|profile| {
                    matches!(profile.family().0, "typescript" | "javascript")
                });
            if matches!(r.kind.as_str(), "calls" | "handles")
                || component_template_member
                || typed_member_read
            {
                if let Some(owner) = owner {
                    let at = r.position();
                    #[cfg(feature = "lang-typescript")]
                    if component_template_member && !tail.contains('.') {
                        if let Some(result) = vue::resolution::component_slot_member(
                            input, path, owner, first, tail, alias_for,
                        ) {
                            emit_inferred(graph, path, r, result, builtin_origin);
                            continue;
                        }
                    }
                    let implicit_receiver = if known_receiver {
                        #[cfg(feature = "lang-rust")]
                        let rust_receiver = member_lookup.rust_receiver(owner);
                        #[cfg(not(feature = "lang-rust"))]
                        let rust_receiver: Option<&Node> = None;
                        rust_receiver
                            .or_else(|| local_receiver_class(owner))
                            .map(value_flow::TypeTarget::Local)
                    } else {
                        None
                    };
                    let inferred = implicit_receiver.as_ref().or_else(|| {
                        let receiver = value_flow.lookup(owner, first, at)?;
                        let incomplete_reference = matches!(
                            receiver,
                            value_flow::TypeTarget::RustReference { target, .. }
                                if matches!(target.as_ref(), value_flow::TypeTarget::Unknown)
                        );
                        if incomplete_reference
                            && matches!(typed_receiver, Some(ReceiverType::External(..)))
                        {
                            None
                        } else {
                            Some(receiver)
                        }
                    });
                    if members::resolve_candidate_join(
                        path,
                        r,
                        tail,
                        owner,
                        inferred,
                        semantic_context,
                        graph,
                    ) {
                        continue;
                    }
                    if let Some(receiver) = inferred
                        .filter(|receiver| !matches!(receiver, value_flow::TypeTarget::Unknown))
                    {
                        if tail.is_empty() {
                            if let value_flow::TypeTarget::Callable(callable) = receiver {
                                emit_inferred(
                                    graph,
                                    path,
                                    r,
                                    semantic_context::Member::Local(callable),
                                    builtin_origin,
                                );
                                continue;
                            }
                        } else {
                            let (fields, member) = tail.rsplit_once('.').unwrap_or(("", tail));
                            if let Some(nested_receiver) = chained_member_receiver(
                                receiver,
                                fields,
                                owner,
                                path,
                                r.position(),
                                input,
                                profile,
                            ) {
                                let receiver = nested_receiver.as_ref();
                                let associated = implicit_receiver.is_some()
                                    && fields.is_empty()
                                    && receiver_language_requires_static(owner)
                                    && owner.details["is_static"] == true;
                                let result = semantic_context
                                    .lookup_member_mode(receiver, member, owner, associated);
                                let result = if r.kind == "references"
                                    && active_profile.is_some_and(|profile| {
                                        matches!(profile.id(), "javascript" | "typescript")
                                    })
                                    && matches!(result, semantic_context::Member::Unknown)
                                {
                                    semantic_context.lookup_property(receiver, member, owner)
                                } else {
                                    result
                                };
                                let result = match result {
                                semantic_context::Member::Local(node)
                                    if !super::super::typescript::semantic_context::prototype_visible_at(node, path, r.line, r.column) => semantic_context::Member::Unknown,
                                other => other,
                            };
                                let result = match (r.kind.as_str(), &result) {
                                    ("calls", semantic_context::Member::Local(node))
                                        if node.kind == "field" =>
                                    {
                                        semantic_context::Member::Unknown
                                    }
                                    ("mutates", semantic_context::Member::Local(node))
                                        if node.kind == "method" =>
                                    {
                                        semantic_context::Member::Unknown
                                    }
                                    _ => result,
                                };
                                let origin =
                                    builtin_member_origin(receiver, active_profile, builtin_origin);
                                emit_inferred(graph, path, r, result, origin);
                                continue;
                            }
                        }
                    }
                }
            }
            let receiver_member = python::resolution::implicit_receiver_member(
                &python_resolution,
                r,
                typed_receiver.copied(),
                known_receiver,
                shadowed,
                imported,
                lookup_key,
            );
            if shadowed
                && !known_receiver
                && !typed_receiver
                    .is_some_and(|receiver| !matches!(receiver, ReceiverType::Unknown))
                && matches!(r.kind.as_str(), "calls" | "handles" | "mutates")
            {
                graph.reference_coverage(path, r, "unresolved", "callee is shadowed by a parameter or local binding of unknown callable identity"
                        .into());
                continue;
            }
            if let Some(ReceiverType::Local(receiver)) = typed_receiver {
                candidates.extend(
                    lookup(qualified(lookup_key, &receiver.qualname, tail))
                        .iter()
                        .copied()
                        .filter(|candidate| candidate.path == receiver.path),
                );
            }
            if let Some(receivers::Member::Local(method)) = receiver_member {
                candidates.clear();
                candidates.push(method);
            }
            if matches!(
                receiver_member,
                Some(
                    receivers::Member::Unknown
                        | receivers::Member::Ambiguous
                        | receivers::Member::External(..)
                )
            ) {
                candidates.clear();
            }
            if candidates.is_empty() && typed_receiver.is_none() && receiver_member.is_none() {
                if let Some(imported) = imported.filter(|_| !shadowed) {
                    if tail.is_empty() {
                        let commonjs_default =
                            super::super::javascript::resolution::commonjs_default_callable(
                                r,
                                active_profile,
                                first,
                                owner,
                                facts,
                                imported,
                                semantic_context,
                                interner,
                                language_linker,
                                by_module,
                                by_qual,
                                language_package_exports,
                                lookup_key,
                            );
                        if let Some(callable) = commonjs_default {
                            candidates.push(callable);
                        } else {
                            candidates.extend_from_slice(imported);
                        }
                    } else {
                        if let (Some(owner), [receiver]) = (owner, imported.as_slice()) {
                            if matches!(
                                receiver.language.as_str(),
                                "rust" | "typescript" | "javascript" | "vue"
                            ) && matches!(
                                receiver.kind.as_str(),
                                "class" | "struct" | "enum" | "interface"
                            ) {
                                let receiver = value_flow::TypeTarget::Local(receiver);
                                let member = semantic_context
                                    .lookup_member_mode(&receiver, tail, owner, true);
                                let member = match (r.kind.as_str(), &member) {
                                    ("calls", semantic_context::Member::Local(node))
                                        if node.kind == "field" =>
                                    {
                                        semantic_context::Member::Unknown
                                    }
                                    ("mutates", semantic_context::Member::Local(node))
                                        if node.kind == "method" =>
                                    {
                                        semantic_context::Member::Unknown
                                    }
                                    _ => member,
                                };
                                if !matches!(member, semantic_context::Member::Unknown)
                                    || receiver_language_requires_static(imported[0])
                                {
                                    emit_inferred(graph, path, r, member, builtin_origin);
                                    continue;
                                }
                            }
                        }
                        if matches!(r.kind.as_str(), "calls" | "handles")
                            && alias_import_positions
                                .get(&(scope, first))
                                .is_some_and(|position| *position <= r.position())
                        {
                            if let [module] = imported.as_slice() {
                                if let Some(receivers::Member::External(
                                    origin,
                                    line,
                                    provider,
                                    _,
                                )) = super::super::python::resolution::imported_module_member(
                                    profile,
                                    module,
                                    tail,
                                    node_rebindings,
                                    interner,
                                    by_qual,
                                    lookup_key,
                                    member_lookup,
                                ) {
                                    emit_inferred(
                                        graph,
                                        path,
                                        r,
                                        semantic_context::Member::External(origin, line, provider),
                                        builtin_origin,
                                    );
                                    continue;
                                }
                            }
                        }
                        for n in imported {
                            if n.kind == "module"
                                && matches!(
                                    n.language.as_str(),
                                    "typescript" | "javascript" | "vue"
                                )
                            {
                                candidates.extend(language_linker.resolve_imported_member(
                                    interner,
                                    n,
                                    tail,
                                    by_module,
                                    by_qual,
                                    language_package_exports,
                                    lookup_key,
                                ));
                                continue;
                            }
                            candidates.extend(
                                lookup(qualified(lookup_key, &n.qualname, tail))
                                    .iter()
                                    .copied()
                                    .filter(|target| target.path == n.path),
                            );
                        }
                        if candidates.is_empty() {
                            if let [module] = imported.as_slice() {
                                if module.kind == "module"
                                    && !matches!(
                                        module.language.as_str(),
                                        "typescript" | "javascript" | "vue"
                                    )
                                {
                                    candidates.extend(language_linker.resolve_imported_member(
                                        interner,
                                        module,
                                        tail,
                                        by_module,
                                        by_qual,
                                        language_package_exports,
                                        lookup_key,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            if candidates.is_empty()
                && imported.is_none()
                && typed_receiver.is_none()
                && receiver_member.is_none()
            {
                if !tail.is_empty() && !shadowed {
                    candidates.extend_from_slice(lookup(&expression));
                }
                if candidates.is_empty() {
                    if tail.is_empty() {
                        candidates.extend_from_slice(lookup(qualified(
                            lookup_key,
                            file_module,
                            &expression,
                        )));
                    } else if known_receiver {
                        if let Some(owner) = owner {
                            let scope = owner.qualname.rsplit_once('.').map_or("", |(p, _)| p);
                            candidates
                                .extend_from_slice(lookup(qualified(lookup_key, scope, tail)));
                        }
                    } else if !shadowed {
                        candidates.extend_from_slice(lookup(qualified(
                            lookup_key,
                            file_module,
                            &expression,
                        )));
                    }
                }
            }
            if rust_explicit_address {
                candidates.clear();
                if let Some(target) = rust_module_target.as_deref() {
                    candidates.extend_from_slice(lookup(target));
                }
            }
            if imported.is_none()
                && typed_receiver.is_none()
                && !matches!(receiver_member, Some(receivers::Member::Local(_)))
            {
                candidates.retain(|n| {
                    n.path == *path
                        || (profile.is_some_and(|p| p.sibling_accessible(path, &n.path))
                            && by_module
                                .get(n.path.as_str())
                                .into_iter()
                                .flatten()
                                .any(|m| m.kind == "module" && m.qualname == file_module))
                        || (!shadowed
                            && r.kind == "references"
                            && n.qualname == expression.as_ref())
                        || rust_module_target
                            .as_deref()
                            .is_some_and(|target| n.qualname == target)
                        || rust_explicit_address
                });
            }
            #[cfg(feature = "lang-html")]
            if r.kind == "calls"
                && tail.is_empty()
                && owner.is_some_and(|owner| {
                    owner.kind == "template_scope" && owner.details["html_handler"] == true
                })
                && imported.is_none()
                && typed_receiver.is_none()
            {
                candidates.extend(html_scripts.providers(path, first, r.line, r.column));
            }
            if r.kind == "calls"
                && tail.is_empty()
                && imported.is_none()
                && !shadowed
                && candidates.is_empty()
            {
                if let Some(owner) = owner {
                    candidates.extend(semantic_context.implicit_callables(owner, first));
                }
            }
            candidates.retain(|n| {
                #[cfg(feature = "lang-rust")]
                let rust_macro_reference =
                    rust::resolution::macro_reference_candidate(active_profile, owner, n);
                #[cfg(not(feature = "lang-rust"))]
                let rust_macro_reference = false;
                match r.kind.as_str() {
                    "inherits" | "bases" => {
                        matches!(n.kind.as_str(), "class" | "interface" | "struct")
                    }
                    "implements" => matches!(n.kind.as_str(), "trait" | "interface"),
                    "mutates" => n.kind == "field",
                    "references" => {
                        matches!(
                            n.kind.as_str(),
                            "class" | "enum" | "type" | "struct" | "trait" | "interface" | "field"
                        ) || (n.kind == "function"
                            && tail.is_empty()
                            && imported.is_some()
                            && !is_type_only(first, owner, definition_scope)
                            && interner.lookup_symbol(&r.source).is_some_and(|source| {
                                value_queries.contains(&(
                                    source,
                                    r.expression.as_str(),
                                    r.line,
                                    r.column,
                                ))
                            }))
                            || rust_macro_reference
                            || (n.kind == "module"
                                && n.language == "vue"
                                && n.details["default_export"] == true)
                    }
                    _ => matches!(
                        n.kind.as_str(),
                        "function" | "method" | "class" | "struct" | "enum" | "macro"
                    ),
                }
            });
            if matches!(r.kind.as_str(), "calls" | "handles") && !tail.is_empty() {
                candidates.retain(|candidate| {
                    typescript::resolution::admits_static_receiver_candidate(candidate, &expression)
                });
            }
            candidates.sort_by(|a, b| a.id.cmp(&b.id));
            candidates.dedup_by_key(|n| &n.id);
            crate::engine::linker::traits::filter_stubs_and_overloads(&mut candidates);
            #[cfg(feature = "lang-rust")]
            super::super::rust::resolution::macro_analysis_gap(
                profile,
                r,
                &candidates,
                path,
                graph,
            );
            let mut source = Some(r.source.clone());
            let mut source_heuristic = false;
            #[cfg(feature = "lang-rust")]
            if r.kind == "implements" {
                if let Some(implementation) = owner.filter(|node| node.kind == "impl") {
                    (source, source_heuristic) = rust::resolution::impl_source(
                        implementation,
                        &implementation.qualname,
                        file_module,
                        by_qual,
                        by_name,
                        aliases,
                        lookup_key,
                    );
                }
            }
            let is_builtin = candidates.is_empty()
                && !shadowed
                && imported.is_none()
                && !lexical_head_declared
                && builtin_in_scope(&expression, r);
            let rust_stdlib_builtin = is_builtin
                && profile.is_some_and(|profile| profile.id() == "rust")
                && first != "Self";
            let python_stdlib_builtin =
                is_builtin && profile.is_some_and(|profile| profile.id() == "python");
            let html_template_builtin = is_builtin
                && expression.starts_with("template.")
                && active_profile.is_some_and(|profile| profile.id() == "html");
            let js_builtin_origin = is_builtin
                .then(|| {
                    javascript::resolution::builtin_origin(
                        first,
                        active_profile,
                        semantic_context.autoimport_evidence(path, first),
                    )
                })
                .flatten();
            #[cfg(feature = "lang-html")]
            let html_builtin_evidence = html_template_builtin
                .then(|| html::templates::builtin_evidence(&expression, template_rules));
            #[cfg(feature = "lang-html")]
            let html_builtin_origin = html_builtin_evidence
                .as_deref()
                .unwrap_or("builtin:shared_template");
            #[cfg(not(feature = "lang-html"))]
            let html_builtin_origin = "builtin:shared_template";
            let component_builtin_origin = is_builtin
                .then(|| file_language_context.component_macro(&expression))
                .flatten()
                .map(|(rule, _)| {
                    format!(
                        "{}; rule:{}:{}",
                        rule.display
                            .builtin
                            .as_deref()
                            .unwrap_or("configured macro"),
                        rule.pack,
                        rule.id
                    )
                })
                .or_else(|| {
                    (is_builtin && configured_autoimport(&expression)).then(|| {
                        semantic_context
                            .autoimport_evidence(path, &expression)
                            .unwrap_or("configured runtime")
                            .to_owned()
                    })
                });
            let component_builtin_origin = component_builtin_origin.as_deref();
            let status = if matches!(typed_receiver, Some(ReceiverType::Ambiguous))
                || matches!(receiver_member, Some(receivers::Member::Ambiguous))
            {
                "ambiguous"
            } else if (candidates.len() == 1 || is_builtin) && source.is_some() && !source_heuristic
            {
                "resolved"
            } else if candidates.is_empty() || source.is_none() || source_heuristic {
                "unresolved"
            } else {
                "ambiguous"
            };
            #[cfg(feature = "lang-rust")]
            let rust_external_import =
                super::super::rust::semantic_context::external_annotation_type(
                    profile,
                    first,
                    first,
                    path,
                    dependency_registry,
                )
                .is_some();
            #[cfg(not(feature = "lang-rust"))]
            let rust_external_import = false;
            let external_origin =
                if let Some(receivers::Member::External(module, import_line, _, _)) =
                    receiver_member
                {
                    Some((module, import_line))
                } else if let Some(ReceiverType::External(module, import_line)) = typed_receiver {
                    (*import_line <= r.line).then_some((*module, *import_line))
                } else if rust_stdlib_builtin {
                    Some(("Rust standard library", 0))
                } else if python_stdlib_builtin {
                    Some(("Python standard library", 0))
                } else if html_template_builtin {
                    Some((html_builtin_origin, 0))
                } else if let Some(origin) = component_builtin_origin {
                    Some((origin, 0))
                } else if let Some(origin) = js_builtin_origin {
                    Some((origin, 0))
                } else if status == "unresolved"
                    && !shadowed
                    && candidates.is_empty()
                    && (imported.is_some_and(|targets| targets.is_empty())
                        || rust_external_import
                        || external_call_for(first, owner, r.position()).is_some())
                {
                    if let Some(target) = external_call_for(first, owner, r.position())
                        .filter(|(_, import_line)| *import_line <= r.line)
                    {
                        Some((target.0.as_str(), target.1))
                    } else if rust_external_import {
                        Some((first, 0))
                    } else {
                        None
                    }
                } else {
                    None
                };
            let framework_provider = match receiver_member {
                Some(receivers::Member::External(_, _, provider, _)) => provider,
                _ => None,
            };
            let status = if external_origin.is_some() {
                "external"
            } else {
                status
            };
            let stub_target = match candidates.as_slice() {
                [candidate] if language_linker.is_declaration_only(&candidate.path) => {
                    Some(candidate.path.as_str())
                }
                _ => None,
            };
            let evidence: Cow<'static, str> = if let Some(stub_path) = stub_target {
                format!("type-stub declaration at {stub_path}; runtime implementation not statically indexed").into()
            } else if rust_stdlib_builtin {
                format!("Rust standard library prelude: {expression}").into()
            } else if python_stdlib_builtin {
                format!("Python standard library built-in: {expression}").into()
            } else if html_template_builtin {
                format!("{html_builtin_origin} {expression}").into()
            } else if let Some(origin) = component_builtin_origin {
                format!("{origin} built-in: {expression}").into()
            } else if let Some(origin) = js_builtin_origin {
                format!("{origin} built-in: {expression}").into()
            } else if let Some((module, import_line)) = external_origin {
                if let Some(provider) = framework_provider {
                    let diagnostic_marker = if profile.is_some_and(|profile| {
                        profile.id() == "python" && !profile.is_stdlib(module)
                    }) {
                        "; diagnostic_reason=framework_magic"
                    } else {
                        ""
                    };
                    format!("finite Python framework member {provider}.{tail} via external import {module} at line {import_line}{diagnostic_marker}").into()
                } else if profile.is_some_and(|profile| {
                    matches!(profile.id(), "javascript" | "typescript") && profile.is_stdlib(module)
                }) {
                    format!("builtin:node import {module}: {expression}").into()
                } else {
                    match (r.kind == "calls", profile.is_some_and(|profile| profile.id() == "rust") && import_line > 0) {
                (true, true) => format!("call through external import {module} at line {import_line}; callable target unverified").into(),
                (true, false) => format!("call through external import {module}; callable target unverified").into(),
                (false, true) => format!("reference through external import {module} at line {import_line}; target unverified").into(),
                (false, false) => format!("reference through external import {module}; target unverified").into(),
            }
                }
            } else if let Some(ReceiverType::Local(receiver)) = typed_receiver {
                format!(
                    "parameter receiver type {}; {} statically inferred candidates",
                    receiver.qualname,
                    candidates.len()
                )
                .into()
            } else if matches!(typed_receiver, Some(ReceiverType::Ambiguous)) {
                "parameter receiver type has ambiguous declarations".into()
            } else if is_builtin {
                format!("standard library or built-in callee: {expression}").into()
            } else {
                format!("{} lexically justified candidates; implementor resolved={}, heuristic={source_heuristic}", candidates.len(), source.is_some()).into()
            };
            graph.reference_coverage(path, r, status, evidence);
            if let (Some(source), [candidate]) = (source, candidates.as_slice()) {
                let exact_same_class_member_reference = r.kind == "references"
                    && matches!(
                        &receiver_member,
                        Some(receivers::Member::Local(member))
                            if matches!(member.kind.as_str(), "field" | "class")
                    );
                let target = candidate.id.clone();
                let (src, dst) = if r.kind == "decorates" {
                    (target, source)
                } else {
                    (source, target)
                };
                let kind_str = if r.kind == "bases" || r.kind == "inherits" {
                    let is_interface = candidate.kind == "interface"
                        || python::resolution::has_protocol_or_abc_base(candidate);
                    if is_interface && owner.is_some_and(|n| n.kind != "interface") {
                        "implements"
                    } else {
                        "inherits"
                    }
                } else {
                    r.kind.as_str()
                };
                graph.edges.push(Edge {
                    src: src.clone(),
                    dst: dst.clone(),
                    kind: kind_str.into(),
                    path: path.to_owned(),
                    line: r.line,
                    evidence: r.expression.clone(),
                    confidence: if source_heuristic {
                        "heuristic"
                    } else if typed_receiver.is_some()
                        || (receiver_member.is_some() && !exact_same_class_member_reference)
                    {
                        "inferred"
                    } else {
                        "exact"
                    }
                    .into(),
                });
                if (kind_str == "implements" || kind_str == "inherits") && r.kind != "decorates" {
                    let source_container_id = if owner.is_some_and(|n| n.kind == "impl") {
                        r.source.as_str()
                    } else {
                        src.as_str()
                    };
                    relation_edges::emit_inherited_method_edges(
                        input,
                        graph,
                        path,
                        candidate,
                        source_container_id,
                    );
                }
            }
        }
    }
}
