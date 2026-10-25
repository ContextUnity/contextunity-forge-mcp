use super::*;

impl<'a> SemanticResolver<'a> for Context<'a> {
    fn configured_result_member(
        &self,
        result: &super::super::value_flow::ConfiguredCallResult,
        member: &str,
    ) -> bool {
        self.symbol_id(member)
            .is_some_and(|member| result.members.contains(&member))
    }

    fn resolve_member_result(
        &self,
        receiver: &TypeTarget<'a>,
        member: &str,
        owner: &'a Node,
        _at: SourcePosition,
    ) -> Option<TypeTarget<'a>> {
        use super::super::value_flow::ConfiguredReceiverAuthority as Authority;
        use languages::manifests::ReturnPolicy;
        enum Origin<'origin, 'tree> {
            External(&'origin str, usize),
            Borrowed(&'origin TypeTarget<'tree>),
        }
        let (pack, policy, origin) = match receiver {
            TypeTarget::Local(class) if class.language == "python" => {
                let (pack, _row, policy) = self.python?.return_policy(class, member)?;
                let super::super::receivers::Member::External(module, line, _, _) =
                    self.python?.lookup(class, member, false)
                else {
                    return None;
                };
                (pack, policy, Origin::External(module, line))
            }
            TypeTarget::ConfiguredReceiver {
                authority, origin, ..
            } => {
                let Authority::Hierarchy { pack, rule } = authority.as_ref() else {
                    return None;
                };
                if !rule.members.iter().any(|allowed| allowed == member) {
                    return None;
                }
                (
                    pack.as_str(),
                    rule.returns.get(member)?,
                    Origin::Borrowed(origin.as_ref()),
                )
            }
            _ => return None,
        };
        match policy {
            ReturnPolicy::SelfReceiver => Some(receiver.clone()),
            ReturnPolicy::Receiver { base } => {
                let authority = self
                    .hierarchy_results
                    .get(&self.path_id(&owner.path)?)?
                    .get(&(self.symbol_id(pack)?, self.symbol_id(base)?))?
                    .clone();
                let origin = match origin {
                    Origin::External(module, import_line) => TypeTarget::External {
                        module: module.to_owned(),
                        import_line,
                    },
                    Origin::Borrowed(origin) => origin.clone(),
                };
                Some(TypeTarget::ConfiguredReceiver {
                    authority,
                    origin: Box::new(origin),
                    fallback: None,
                    evidence: format!(
                        "{}:{}:{}:{}",
                        self.dependencies.digest(),
                        owner.path,
                        pack,
                        match receiver {
                            TypeTarget::Local(class) => class.id.as_str(),
                            TypeTarget::ConfiguredReceiver { evidence, .. } => evidence.as_str(),
                            _ => "",
                        }
                    ),
                })
            }
        }
    }

    fn resolve_call_result(
        &self,
        owner: &'a Node,
        callee: &str,
        site: Option<&crate::core::semantic::CallSiteEvidence>,
        at: SourcePosition,
        fallback: Option<TypeTarget<'a>>,
    ) -> Option<TypeTarget<'a>> {
        use crate::core::semantic::StaticArgument;
        if !matches!(owner.language.as_str(), "javascript" | "typescript") {
            return None;
        }
        let site = site?;
        let (global, _member) = callee.split_once('.')?;
        let mappings = self
            .interner
            .lookup_path(&owner.path)
            .and_then(|path| self.argument_factories.get(&path))?;
        let callee_id = self.symbol_id(callee)?;
        let mut selected: Option<
            std::sync::Arc<super::super::value_flow::ConfiguredReceiverAuthority>,
        > = None;
        for (argument, literal) in site.arguments.iter().enumerate() {
            let StaticArgument::String { value } = literal else {
                continue;
            };
            let Some(value_id) = self.symbol_id(value) else {
                continue;
            };
            for candidate in mappings
                .get(&(callee_id, argument, value_id))
                .into_iter()
                .flatten()
            {
                if let Some(previous) = &selected {
                    let same = match (previous.as_ref(), candidate.as_ref()) {
                        (
                            super::super::value_flow::ConfiguredReceiverAuthority::Nominal {
                                pack: a,
                                rule_id: x,
                                ..
                            },
                            super::super::value_flow::ConfiguredReceiverAuthority::Nominal {
                                pack: b,
                                rule_id: y,
                                ..
                            },
                        ) => a == b && x == y,
                        _ => false,
                    };
                    if !same {
                        return fallback;
                    }
                } else {
                    if !self.unshadowed_global(owner, global, at) {
                        return None;
                    }
                    selected = Some(candidate.clone());
                }
            }
        }
        Some(TypeTarget::ConfiguredReceiver {
            authority: selected?,
            origin: Box::new(TypeTarget::External {
                module: callee.to_owned(),
                import_line: site.position.line,
            }),
            fallback: fallback.map(Box::new),
            evidence: format!("{}:{}", self.dependencies.digest(), owner.path),
        })
    }
    fn property_type(&self, scope: &'a Node, receiver: &str, member: &str) -> Option<String> {
        self.property_type_for_path(&scope.path, receiver, member)
    }
    fn resolve_external_factory(
        &self,
        owner: &'a Node,
        callee: &str,
        at: SourcePosition,
    ) -> Option<TypeTarget<'a>> {
        if owner.kind == "module"
            && self
                .symbol_id(&owner.id)
                .and_then(|owner| self.flows.and_then(|flows| flows.get(&owner)))
                .is_some_and(|state| {
                    matches!(
                        state,
                        crate::core::typed_facts::FlowState::Valid { facts, .. }
                            if facts.component_macros().any(|fact| {
                                fact.callee == callee
                                    && fact.role == crate::core::semantic::ComponentMacroRole::Props
                                    && fact.position <= at
                            })
                    )
                })
        {
            return Some(TypeTarget::Local(owner));
        }
        if let Some(result) = self.resolve_factory_result(owner, callee, at) {
            return Some(result);
        }
        if let Some(result) = super::super::javascript::semantic_context::external_factory_result(
            self, owner, callee, at,
        ) {
            return Some(result);
        }
        super::super::python::semantic_context::external_factory_result(self, owner, callee, at)
    }
    fn resolve_factory_result(
        &self,
        owner: &'a Node,
        callee: &str,
        at: SourcePosition,
    ) -> Option<TypeTarget<'a>> {
        if !matches!(owner.language.as_str(), "javascript" | "typescript" | "vue")
            || callee.contains('.')
        {
            return None;
        }
        let callee_id = self.symbol_id(callee)?;
        if !self.factory_callees.contains(&callee_id) {
            return None;
        }
        let mut scope = owner.qualname.as_str();
        loop {
            for lexical in self.nodes_in_path(scope, &owner.path) {
                let Some(lexical_id) = self.interner.lookup_symbol(&lexical.id) else {
                    return Some(TypeTarget::Unknown);
                };
                let imported = self.factory_imports.get(&(lexical_id, callee_id));
                if imported.is_some()
                    && self.factory_bindings.contains_key(&(lexical_id, callee_id))
                {
                    return Some(TypeTarget::Unknown);
                }
                if matches!(imported, Some(None)) {
                    return Some(TypeTarget::Unknown);
                }
                let (provider, binding) =
                    if let Some(Some((provider, binding, position))) = imported {
                        if *position >= at {
                            return Some(TypeTarget::Unknown);
                        }
                        (*provider, *binding)
                    } else {
                        (lexical, callee_id)
                    };
                if let Some(value) = self.factory_value(provider, binding, owner) {
                    return Some(value);
                }
                if imported.is_some() {
                    return None;
                }
                if self.factory_declares(lexical, callee) {
                    return None;
                }
            }
            let Some((parent, _)) = scope.rsplit_once('.') else {
                break;
            };
            scope = parent;
        }
        if let Some(result) = self.implicit_factory_result(owner, callee) {
            return Some(result);
        }
        let (Some(path), Some(callee)) = (self.path_id(&owner.path), self.symbol_id(callee)) else {
            return None;
        };
        let mut results = self
            .result_scopes
            .get(&path)
            .into_iter()
            .flatten()
            .filter_map(|scope| scope.calls.get(&callee));
        let result = results.next()?;
        if !self.configured_unshadowed(owner, self.interner.resolve_symbol(callee)) {
            return None;
        }
        if results.next().is_some() {
            return Some(TypeTarget::Ambiguous);
        }
        Some(TypeTarget::ResultObject(result.clone()))
    }
    fn deref_target(&self, receiver: &'a Node) -> Option<&'a Node> {
        if receiver.language == "rust" {
            return super::super::rust::semantic_context::deref_target(
                &LanguageMemberLookup::new(self),
                receiver,
            );
        }
        let _ = receiver;
        None
    }

    fn value_flow(&self, node: &'a Node) -> Option<&'a crate::core::semantic::ValueFlowFacts> {
        let node_id = self.symbol_id(&node.id)?;
        match self.flows?.get(&node_id).copied()? {
            crate::core::typed_facts::FlowState::Valid { facts, .. } => Some(facts),
            crate::core::typed_facts::FlowState::Invalid(_) => None,
        }
    }

    fn invalid_value_flow(&self, node: &'a Node) -> bool {
        self.symbol_id(&node.id).is_some_and(|node_id| {
            self.flows.is_some_and(|flows| {
                matches!(
                    flows.get(&node_id),
                    Some(crate::core::typed_facts::FlowState::Invalid(_))
                )
            })
        })
    }

    fn has_typed_flows(&self) -> bool {
        self.flows.is_some()
    }

    fn raw_value_flow(&self, node: &'a Node) -> Option<&'a serde_json::Value> {
        let node_id = self.symbol_id(&node.id)?;
        match self.flows?.get(&node_id).copied()? {
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
        if super::super::python::semantic_context::is_nullable_generic(owner, clean_base, &nominal)
        {
            if owner.kind == "type" {
                return TypeTarget::Unknown;
            }
            let Some(payload) =
                super::super::python::semantic_context::nullable_payload(clean_base, args)
            else {
                return TypeTarget::Unknown;
            };
            return super::super::value_flow::resolve_type(
                owner,
                payload,
                at,
                self,
                super::super::value_flow::SemanticLimits::default(),
            );
        }
        if let Some(target) =
            super::super::rust::semantic_context::nominal_application(owner, &nominal)
        {
            return target;
        }
        if let Some(target) =
            super::super::typescript::semantic_context::resolve_applied(owner, args, &nominal)
        {
            return target;
        }
        if let Some(target) =
            super::super::rust::value_flow::resolve_applied(owner, base, args, &nominal, at, self)
        {
            return target;
        }
        match nominal {
            Symbol::Type(TypeTarget::External {
                module,
                import_line,
            }) if self
                .receiver_type(&owner.path, &module)
                .is_some_and(|receiver| receiver.generic) =>
            {
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
                if super::super::python::semantic_context::is_external_generic_base(
                    &module,
                    clean_base,
                    languages::by_id(&owner.language),
                ) =>
            {
                TypeTarget::Builtin(clean_base.to_string())
            }
            _ => TypeTarget::Unknown,
        }
    }
    fn resolve_payload(
        &self,
        owner: &'a Node,
        expression: &crate::core::semantic::TypeExpr,
        at: SourcePosition,
    ) -> TypeTarget<'a> {
        let crate::core::semantic::TypeExpr::Applied { base, args } = expression else {
            return TypeTarget::Unknown;
        };
        if args.is_empty() {
            return TypeTarget::Unknown;
        }
        let wrapper = self.resolve_applied(owner, base, args, at);
        if let TypeTarget::RustWrapper { ok, .. } = &wrapper {
            return (**ok).clone();
        }
        let admitted =
            super::super::rust::semantic_context::admits_payload(owner, &wrapper, args.len())
                || super::super::typescript::semantic_context::admits_payload(
                    owner,
                    &wrapper,
                    args.len(),
                );
        if !admitted {
            return TypeTarget::Unknown;
        }
        super::super::value_flow::resolve_type(
            owner,
            &args[0],
            at,
            self,
            super::super::value_flow::SemanticLimits::default(),
        )
    }
    fn resolve_member(
        &self,
        receiver: &TypeTarget<'a>,
        member: &str,
        owner: &'a Node,
        _at: SourcePosition,
    ) -> Symbol<'a> {
        match receiver {
            TypeTarget::ConfiguredReceiver {
                authority,
                fallback,
                ..
            } => {
                if let super::super::value_flow::ConfiguredReceiverAuthority::Nominal {
                    receiver,
                    ..
                } = authority.as_ref()
                {
                    if let Some(name) = receiver.rule.properties.get(member) {
                        return Symbol::Type(TypeTarget::Builtin(name.clone()));
                    }
                }
                fallback.as_deref().map_or(Symbol::Unknown, |fallback| {
                    self.resolve_member(fallback, member, owner, _at)
                })
            }
            TypeTarget::Builtin(receiver) => self
                .property_type_for_path(&owner.path, receiver, member)
                .map_or(Symbol::Unknown, |name| {
                    Symbol::Type(TypeTarget::Builtin(name))
                }),
            TypeTarget::Local(node) => self.member(node, member, false, owner),
            TypeTarget::RustReference { target, .. } => {
                self.resolve_member(target, member, owner, _at)
            }
            TypeTarget::RustDeref {
                wrapper, target, ..
            } => {
                if super::super::rust::semantic_context::wrapper_value_member(wrapper, member) {
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
        if let Some(symbol) = self.named_member(owner, name, at) {
            return symbol;
        }
        let Some(name) = super::super::receiver_type_name(name) else {
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
            let lookup = LanguageMemberLookup::new(self);
            if let Some(symbol) = super::super::rust::semantic_context::receiver_symbol(
                Some(&lookup),
                owner,
                &name,
                |receiver| self.resolve_symbol(owner, receiver, at, SymbolRole::Type),
            ) {
                return symbol;
            }
        }
        let (head, member) = name.split_once('.').unwrap_or((&name, ""));
        let receiver_access = super::super::python::semantic_context::receiver_access(owner, head)
            .or_else(|| super::super::javascript::semantic_context::receiver_access(owner, head));
        if let Some((associated, include_interface)) =
            receiver_access.filter(|_| !name_rebound(owner, head))
        {
            let mut scope = owner.qualname.as_str();
            while let Some((parent, _)) = scope.rsplit_once('.') {
                if let Some(class) = self
                    .nodes_in_path(parent, &owner.path)
                    .find(|n| n.kind == "class" || (include_interface && n.kind == "interface"))
                {
                    if member.is_empty() {
                        return Symbol::Type(TypeTarget::Local(class));
                    } else {
                        return self.member(class, member, associated, owner);
                    }
                }
                scope = parent;
            }
        }
        let mut scope = owner.qualname.as_str();
        let mut lookup_key = String::with_capacity(128);
        loop {
            let scope_nodes = self.by_qual.get(scope);
            if let Some(nodes) = scope_nodes {
                for node in nodes.iter().copied().filter(|node| node.path == owner.path) {
                    if self.commonjs.tracks(node, head)
                        && !self.admits_commonjs_binding(node, head, owner, at)
                    {
                        return Symbol::Unknown;
                    }
                    if (node.rebinding_names().any(|name| name == head)
                        || self.node_has_rebinding(node, head))
                        && !self.admits_commonjs_binding(node, head, owner, at)
                    {
                        return Symbol::Unknown;
                    }
                    if let Some(import) = self
                        .symbol_id(&node.id)
                        .and_then(|node_id| self.imports.get(&node_id))
                        .and_then(|aliases| {
                            self.symbol_id(head).and_then(|head| aliases.get(&head))
                        })
                    {
                        let imported_at = match import {
                            Imported::Nodes(_, position, _)
                            | Imported::External(_, position, _, _) => Some(*position),
                            Imported::Unknown => None,
                        };
                        if let Some(imported_at) = imported_at {
                            let local = self
                                .by_qual
                                .get(super::super::qualified(&mut lookup_key, scope, &name))
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
                                    || super::super::vue::semantic_context::template_import_visible(
                                        owner,
                                    ) =>
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
                                                    self.interner
                                                        .lookup_path(target.path.as_str())
                                                        .and_then(|path| exports.get(&path))
                                                })
                                                .and_then(|exports| {
                                                    self.interner
                                                        .lookup_symbol(export)
                                                        .and_then(|name| exports.get(&name))
                                                })
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
                                        nodes.extend(
                                            self.by_qual
                                                .get(super::super::qualified(
                                                    &mut lookup_key,
                                                    &target.qualname,
                                                    member,
                                                ))
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
                                if matches!(module.as_str(), "typing" | "typing_extensions")
                                    && (member == "Any" || head == "Any")
                                {
                                    Symbol::Unknown
                                } else if let Some(symbol) =
                                    super::super::python::semantic_context::external_import_type(
                                        owner,
                                        module,
                                        reference,
                                        head,
                                        member,
                                        role,
                                        position.line,
                                    )
                                {
                                    symbol
                                } else if let Some(receiver) = self
                                    .path_id(&owner.path)
                                    .and_then(|path| self.receiver_types.get(&path))
                                    .into_iter()
                                    .flat_map(|receivers| receivers.iter())
                                    .find(|receiver| {
                                        matches!(role, SymbolRole::Type)
                                            && receiver.module.as_deref() == Some(module)
                                            && receiver.export.as_deref().is_some_and(|export| {
                                                (reference.expression == export
                                                    && member.is_empty())
                                                    || (reference.expression == *module
                                                        && member == export)
                                            })
                                    })
                                {
                                    Symbol::Type(TypeTarget::External {
                                        module: receiver.name.clone(),
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
                if let Symbol::Type(TypeTarget::Local(receiver)) = self.classify(
                    self.by_qual
                        .get(super::super::qualified(&mut lookup_key, scope, head))
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
                .get(super::super::qualified(&mut lookup_key, scope, &name))
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
                    .any(|node| node.binding_names().any(|name| name == head))
            }) {
                return Symbol::Unknown;
            }
            let Some((parent, _)) = scope.rsplit_once('.') else {
                break;
            };
            scope = parent;
        }
        let builtin = languages::by_id(&owner.language).is_some_and(|profile| {
            profile.builtin_type(&name)
                || super::super::typescript::semantic_context::is_builtin_promise_type(
                    owner, profile, &name, role,
                )
        });
        if builtin
            || self
                .receiver_type(&owner.path, &name)
                .is_some_and(|receiver| receiver.module.is_none())
        {
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
            for node in self.nodes_in_path(scope, &owner.path) {
                if let Some(Imported::Nodes(targets, position, _)) = self
                    .symbol_id(&node.id)
                    .and_then(|node_id| self.imports.get(&node_id))
                    .and_then(|aliases| self.symbol_id(head).and_then(|head| aliases.get(&head)))
                {
                    if *position <= at
                        || super::super::vue::semantic_context::template_import_visible(owner)
                    {
                        if let [target] = targets.as_slice() {
                            if target.kind == "module" {
                                return Some(*target);
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
