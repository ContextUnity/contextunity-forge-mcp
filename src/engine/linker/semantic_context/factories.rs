use super::*;

impl<'a> Context<'a> {
    pub(super) fn prepare_factory_bindings<F: AsRef<Facts>>(
        &mut self,
        all: &'a BTreeMap<String, F>,
    ) {
        use crate::core::semantic::ValueExpr;
        for owner in all
            .values()
            .flat_map(|facts| &facts.as_ref().nodes)
            .filter(|owner| {
                matches!(owner.language.as_str(), "typescript" | "javascript" | "vue")
                    && (matches!(owner.kind.as_str(), "function" | "method" | "module")
                        || owner.binding_count() > 0
                        || owner.rebinding_count() > 0)
            })
        {
            let owner_id = self
                .interner
                .lookup_symbol(&owner.id)
                .expect("factory declaration owner is interned");
            let binding_count = owner.binding_count();
            let rebinding_count = owner.rebinding_count();
            let inherited_names = |count: usize, index: &super::super::NodeBindingIndex| {
                count == 0 && index.get(&owner_id).is_some_and(|names| !names.is_empty())
            };
            let declarations = if self
                .by_id
                .get(&owner_id)
                .is_none_or(|canonical| !std::ptr::eq(*canonical, owner))
                || inherited_names(binding_count, self.node_bindings)
                || inherited_names(rebinding_count, self.node_rebindings)
            {
                FactoryDeclarations::Raw(owner)
            } else {
                FactoryDeclarations::Indexed {
                    bindings: self.node_bindings.get(&owner_id).map(Vec::as_slice),
                    rebindings: self.node_rebindings.get(&owner_id).map(Vec::as_slice),
                }
            };
            self.factory_declarations.insert(owner_id, declarations);
            let Some(flow) = self
                .value_flow(owner)
                .filter(|flow| !flow.fact_limit_exceeded)
            else {
                continue;
            };
            for binding in &flow.bindings {
                let binding_id = self
                    .interner
                    .lookup_symbol(&binding.name)
                    .expect("flow binding is interned");
                self.factory_bindings
                    .entry((owner_id, binding_id))
                    .and_modify(|state| {
                        *state = match *state {
                            FactoryBinding::Declaration(declaration)
                                if !declaration.conditional
                                    && !binding.conditional
                                    && declaration.position < binding.position
                                    && !matches!(binding.value, ValueExpr::Unknown) =>
                            {
                                FactoryBinding::Initializer(binding)
                            }
                            _ => FactoryBinding::Invalid,
                        }
                    })
                    .or_insert_with(|| {
                        if matches!(binding.value, ValueExpr::Unknown) {
                            FactoryBinding::Declaration(binding)
                        } else {
                            FactoryBinding::Invalid
                        }
                    });
            }
        }
    }

    /// Check deferred read admission through borrowed, once-prepared lexical
    /// summaries and the existing effective factory/import indexes. No reduction
    /// or full binding scan occurs for a read; its proven receiver is reused.
    pub(in crate::engine::linker) fn factory_member_receiver(
        &self,
        owner: &'a Node,
        name: &str,
        at: SourcePosition,
    ) -> bool {
        self.factory_receiver(owner, name, at, 0)
    }

    fn factory_receiver(
        &self,
        owner: &'a Node,
        name: &str,
        at: SourcePosition,
        depth: usize,
    ) -> bool {
        let limit = super::super::value_flow::SemanticLimits::default().alias_depth;
        if depth >= limit {
            return false;
        }
        let Some(name_id) = self.interner.lookup_symbol(name) else {
            return false;
        };
        let mut scope = owner.qualname.as_str();
        for _ in 0..limit {
            for lexical in self.nodes_in_path(scope, &owner.path) {
                let Some(lexical_id) = self.interner.lookup_symbol(&lexical.id) else {
                    return false;
                };
                if let Some(binding) = self.factory_bindings.get(&(lexical_id, name_id)) {
                    let FactoryBinding::Initializer(binding) = binding else {
                        return false;
                    };
                    if binding.position >= at {
                        return false;
                    }
                    return match &binding.value {
                        crate::core::semantic::ValueExpr::Call { callee, .. } => {
                            self.factory_callee(lexical, callee, binding.position)
                        }
                        crate::core::semantic::ValueExpr::Alias { name } => {
                            self.factory_receiver(lexical, name, binding.position, depth + 1)
                        }
                        _ => false,
                    };
                }
                if self.factory_declares(lexical, name) {
                    return false;
                }
            }
            let Some((parent, _)) = scope.rsplit_once('.') else {
                return false;
            };
            scope = parent;
        }
        false
    }

    fn factory_callee(&self, owner: &'a Node, callee: &str, at: SourcePosition) -> bool {
        if callee.contains('.') {
            return false;
        }
        let mut scope = owner.qualname.as_str();
        for _ in 0..super::super::value_flow::SemanticLimits::default().alias_depth {
            for lexical in self.nodes_in_path(scope, &owner.path) {
                let Some(lexical_id) = self.interner.lookup_symbol(&lexical.id) else {
                    return false;
                };
                let Some(callee_id) = self.interner.lookup_symbol(callee) else {
                    return false;
                };
                let imported = self.factory_imports.get(&(lexical_id, callee_id));
                if imported.is_some()
                    && self.factory_bindings.contains_key(&(lexical_id, callee_id))
                {
                    return false;
                }
                let (provider, binding) = match imported {
                    Some(Some((provider, binding, position))) if *position < at => {
                        (*provider, *binding)
                    }
                    Some(_) => return false,
                    None => (lexical, callee_id),
                };
                if self
                    .interner
                    .lookup_symbol(&provider.id)
                    .is_some_and(|provider| self.named.factories.contains_key(&(provider, binding)))
                {
                    return true;
                }
                if imported.is_some() || self.factory_declares(lexical, callee) {
                    return false;
                }
            }
            let Some((parent, _)) = scope.rsplit_once('.') else {
                break;
            };
            scope = parent;
        }
        matches!(
            self.implicit_factory_result(owner, callee),
            Some(TypeTarget::Object(_) | TypeTarget::Ambiguous)
        )
    }
}
