use super::*;

/// Incremental language body analysis driven exclusively by shared traversal events.
pub struct ScopeAnalysis<'source, 'tree> {
    state: Option<LanguageAnalysis<'source, 'tree>>,
    marker: std::marker::PhantomData<(&'source str, Syntax<'tree>)>,
}
enum LanguageAnalysis<'source, 'tree> {
    #[cfg(feature = "lang-python")]
    Python(Box<languages::python::value_flow::Collector<'source, 'tree>>),
    #[cfg(feature = "lang-rust")]
    Rust(Box<languages::rust::value_flow::Collector<'source, 'tree>>),
    #[cfg(feature = "lang-typescript")]
    TypeScript(Box<languages::typescript::value_flow::Collector<'source, 'tree>>),
    #[cfg(all(test, feature = "lang-python"))]
    Traced(Box<TracedAnalysis<'source, 'tree>>),
    // Lifetime ownership remains explicit in document-only feature builds.
    Marker(std::marker::PhantomData<(&'source str, Syntax<'tree>)>),
}
#[cfg(all(test, feature = "lang-python"))]
struct TracedAnalysis<'source, 'tree> {
    inner: ScopeAnalysis<'source, 'tree>,
    trace: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    name: String,
}
impl Default for ScopeAnalysis<'_, '_> {
    fn default() -> Self {
        Self {
            state: Some(LanguageAnalysis::Marker(std::marker::PhantomData)),
            marker: std::marker::PhantomData,
        }
    }
}
impl<'source, 'tree> ScopeAnalysis<'source, 'tree> {
    #[cfg(feature = "lang-python")]
    pub(crate) fn python(root: Syntax<'tree>, source: &'source str) -> Self {
        Self {
            state: Some(LanguageAnalysis::Python(Box::new(
                languages::python::value_flow::Collector::new(
                    root,
                    source,
                    root.kind() == "module",
                ),
            ))),
            marker: std::marker::PhantomData,
        }
    }
    #[cfg(feature = "lang-rust")]
    pub(crate) fn rust(root: Syntax<'tree>, source: &'source str) -> Self {
        Self {
            state: Some(LanguageAnalysis::Rust(Box::new(
                languages::rust::value_flow::Collector::new(root, source),
            ))),
            marker: std::marker::PhantomData,
        }
    }
    #[cfg(feature = "lang-typescript")]
    pub(crate) fn typescript(root: Syntax<'tree>, source: &'source str) -> Self {
        Self {
            state: Some(LanguageAnalysis::TypeScript(Box::new(
                languages::typescript::value_flow::Collector::new(root, source),
            ))),
            marker: std::marker::PhantomData,
        }
    }
    fn enter(&mut self, node: Syntax<'tree>) {
        match self.state.as_mut() {
            #[cfg(feature = "lang-python")]
            Some(LanguageAnalysis::Python(state)) => state.enter(node),
            #[cfg(feature = "lang-rust")]
            Some(LanguageAnalysis::Rust(state)) => state.enter(node),
            #[cfg(feature = "lang-typescript")]
            Some(LanguageAnalysis::TypeScript(state)) => state.enter(node),
            #[cfg(all(test, feature = "lang-python"))]
            Some(LanguageAnalysis::Traced(state)) => state.inner.enter(node),
            _ => {}
        }
    }
    fn observe_module_event(&mut self, node: Syntax<'tree>, entering: bool) {
        match self.state.as_mut() {
            #[cfg(feature = "lang-python")]
            Some(LanguageAnalysis::Python(state)) => state.observe_module_event(node, entering),
            #[cfg(all(test, feature = "lang-python"))]
            Some(LanguageAnalysis::Traced(state)) => {
                state.inner.observe_module_event(node, entering)
            }
            _ => {}
        }
    }
    fn module_exports(
        &self,
        bindings: &ScopeBindings,
    ) -> Option<(Vec<crate::core::semantic::ExportBinding>, serde_json::Value)> {
        match self.state.as_ref() {
            #[cfg(feature = "lang-python")]
            Some(LanguageAnalysis::Python(state)) => state
                .module_exports(bindings)
                .map(|(exports, lazy)| (exports, json!(lazy))),
            #[cfg(all(test, feature = "lang-python"))]
            Some(LanguageAnalysis::Traced(state)) => state.inner.module_exports(bindings),
            _ => None,
        }
    }
    fn exit(&mut self, node: Syntax<'tree>) {
        match self.state.as_mut() {
            #[cfg(feature = "lang-python")]
            Some(LanguageAnalysis::Python(state)) => state.exit(node),
            #[cfg(feature = "lang-rust")]
            Some(LanguageAnalysis::Rust(state)) => state.exit(node),
            #[cfg(feature = "lang-typescript")]
            Some(LanguageAnalysis::TypeScript(state)) => state.exit(node),
            #[cfg(all(test, feature = "lang-python"))]
            Some(LanguageAnalysis::Traced(state)) => state.inner.exit(node),
            _ => {
                let _ = node;
            }
        }
    }
    fn enter_suspended(&mut self, node: Syntax<'tree>) {
        match self.state.as_mut() {
            #[cfg(feature = "lang-typescript")]
            Some(LanguageAnalysis::TypeScript(state)) => state.enter_suspended(node),
            _ => {
                let _ = node;
            }
        }
    }
    fn finish(
        self,
        collect_module_aliases: bool,
    ) -> (
        crate::core::semantic::ValueFlowFacts,
        Option<HashSet<String>>,
    ) {
        match self.state {
            #[cfg(feature = "lang-python")]
            Some(LanguageAnalysis::Python(state)) => {
                let aliases = collect_module_aliases.then(|| state.module_aliases());
                ((*state).finish(), aliases)
            }
            #[cfg(feature = "lang-rust")]
            Some(LanguageAnalysis::Rust(state)) => ((*state).finish(), None),
            #[cfg(feature = "lang-typescript")]
            Some(LanguageAnalysis::TypeScript(state)) => ((*state).finish(), None),
            #[cfg(all(test, feature = "lang-python"))]
            Some(LanguageAnalysis::Traced(state)) => {
                let result = state.inner.finish(collect_module_aliases);
                state
                    .trace
                    .lock()
                    .unwrap()
                    .push(format!("flow:{}", state.name));
                result
            }
            _ => (Default::default(), None),
        }
    }
}

/// Delivers balanced enter/exit events in source order for standalone profile hooks.
pub(crate) fn walk<'tree>(root: Syntax<'tree>, visitor: &mut impl FnMut(Syntax<'tree>, bool)) {
    walk_in_order(root, false, visitor);
}

#[cfg(feature = "lang-vue")]
pub(crate) fn walk_reversed<'tree>(
    root: Syntax<'tree>,
    visitor: &mut impl FnMut(Syntax<'tree>, bool),
) {
    walk_in_order(root, true, visitor);
}

fn walk_in_order<'tree>(
    root: Syntax<'tree>,
    reversed: bool,
    visitor: &mut impl FnMut(Syntax<'tree>, bool),
) {
    fn first_named_child(cursor: &mut tree_sitter::TreeCursor<'_>, reversed: bool) -> bool {
        if !cursor.goto_first_child() {
            return false;
        }
        if reversed {
            while cursor.goto_next_sibling() {}
            loop {
                if cursor.node().is_named() {
                    return true;
                }
                if !cursor.goto_previous_sibling() {
                    cursor.goto_parent();
                    return false;
                }
            }
        }
        loop {
            if cursor.node().is_named() {
                return true;
            }
            if !cursor.goto_next_sibling() {
                cursor.goto_parent();
                return false;
            }
        }
    }

    fn next_named_sibling(cursor: &mut tree_sitter::TreeCursor<'_>, reversed: bool) -> bool {
        let mut moved = if reversed {
            cursor.goto_previous_sibling()
        } else {
            cursor.goto_next_sibling()
        };
        while moved && !cursor.node().is_named() {
            moved = if reversed {
                cursor.goto_previous_sibling()
            } else {
                cursor.goto_next_sibling()
            };
        }
        moved
    }

    let mut cursor = root.walk();
    visitor(root, true);
    if !first_named_child(&mut cursor, reversed) {
        visitor(root, false);
        return;
    }

    loop {
        let node = cursor.node();
        visitor(node, true);
        if first_named_child(&mut cursor, reversed) {
            continue;
        }

        visitor(node, false);
        loop {
            if next_named_sibling(&mut cursor, reversed) {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
            let parent = cursor.node();
            visitor(parent, false);
            if parent.id() == root.id() {
                return;
            }
        }
    }
}

#[cfg(feature = "lang-python")]
pub(crate) fn python_bindings(node: Syntax<'_>, source: &str) -> ScopeBindings {
    let mut bindings = BindingCollector::new(node, source);
    let mut aliases = languages::python::value_flow::AliasWrites::new();
    walk(node, &mut |node, entering| {
        if entering {
            bindings.enter(node);
            aliases.enter(node, source);
        } else {
            bindings.exit();
            aliases.exit();
        }
    });
    let aliases = aliases.aliases();
    let mut bindings = bindings.finish();
    if node.kind() == "module" {
        bindings.rebindings.retain(|name| !aliases.contains(name));
    }
    bindings
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BitStack {
    words: [u64; 4],
    depth: u16,
}

impl BitStack {
    #[inline(always)]
    pub(crate) fn new() -> Self {
        Self {
            words: [0; 4],
            depth: 0,
        }
    }

    #[inline(always)]
    pub(crate) fn push(&mut self, bit: bool) {
        let d = self.depth as usize;
        if d < 256 {
            let idx = d / 64;
            let offset = d % 64;
            if bit {
                self.words[idx] |= 1 << offset;
            } else {
                self.words[idx] &= !(1 << offset);
            }
        }
        self.depth += 1;
    }

    #[inline(always)]
    pub(crate) fn pop(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
        }
    }

    #[inline(always)]
    pub(crate) fn last(&self) -> Option<bool> {
        if self.depth == 0 {
            None
        } else {
            let d = (self.depth - 1) as usize;
            if d < 256 {
                let idx = d / 64;
                let offset = d % 64;
                Some((self.words[idx] & (1 << offset)) != 0)
            } else {
                Some(false)
            }
        }
    }
}

pub(super) struct BindingCollector<'source, 'tree> {
    source: &'source str,
    body: Option<Syntax<'tree>>,
    states: BitStack,
    params: HashSet<&'source str>,
    names: HashSet<&'source str>,
}
fn binding_names<'a>(node: Syntax<'_>, source: &'a str, out: &mut HashSet<&'a str>) {
    match node.kind() {
        #[cfg(feature = "lang-rust")]
        "tuple_struct_pattern" | "struct_pattern" | "ref_pattern" | "captured_pattern" => {
            languages::rust::patterns::bindings(node, |binding| {
                out.insert(text(binding, source));
            });
        }
        "identifier" | "shorthand_property_identifier_pattern" => {
            out.insert(text(node, source));
        }
        "default_parameter"
        | "typed_default_parameter"
        | "required_parameter"
        | "optional_parameter"
        | "parameter"
        | "parameter_declaration"
        | "variadic_parameter_declaration" => {
            for field in ["name", "pattern"] {
                let mut c = node.walk();
                for child in node.children_by_field_name(field, &mut c) {
                    binding_names(child, source, out);
                }
            }
        }
        "assignment_pattern" | "object_assignment_pattern" => {
            if let Some(n) = node.child_by_field_name("left") {
                binding_names(n, source, out);
            }
        }
        "pair_pattern" => {
            if let Some(n) = node.child_by_field_name("value") {
                binding_names(n, source, out);
            }
        }
        "typed_parameter" => {
            if let Some(n) = node.named_child(0) {
                if Some(n) != node.child_by_field_name("type") {
                    binding_names(n, source, out);
                }
            }
        }
        "parameters"
        | "lambda_parameters"
        | "as_pattern_target"
        | "formal_parameters"
        | "parameter_list"
        | "tuple_pattern"
        | "list_pattern"
        | "pattern_list"
        | "expression_list"
        | "array_pattern"
        | "object_pattern"
        | "rest_pattern"
        | "list_splat_pattern"
        | "dictionary_splat_pattern"
        | "slice_pattern"
        | "reference_pattern"
        | "mut_pattern" => {
            let mut c = node.walk();
            for child in node.named_children(&mut c) {
                binding_names(child, source, out);
            }
        }
        _ => {}
    }
}

impl<'source, 'tree> BindingCollector<'source, 'tree> {
    pub(super) fn new(node: Syntax<'tree>, source: &'source str) -> Self {
        Self::new_with_kind(node, source, node.kind())
    }
    pub(super) fn new_with_kind(node: Syntax<'tree>, source: &'source str, kind: &str) -> Self {
        let is_callable = matches!(
            kind,
            "function_definition"
                | "function_declaration"
                | "function_item"
                | "method_definition"
                | "method_declaration"
                | "arrow_function"
                | "function_expression"
                | "lambda"
                | "closure_expression"
                | "func_literal"
        );
        let is_module = matches!(
            kind,
            "module" | "program" | "source_file" | "class_definition"
        );
        if !is_callable && !is_module {
            return Self {
                source,
                body: None,
                states: BitStack::new(),
                params: Default::default(),
                names: Default::default(),
            };
        }
        let mut param_names = HashSet::new();
        if is_callable {
            for field in ["parameters", "parameter"] {
                if let Some(parameters) = node.child_by_field_name(field) {
                    binding_names(parameters, source, &mut param_names);
                }
            }
        }
        let body = node
            .child_by_field_name("body")
            .or_else(|| is_module.then_some(node));
        Self {
            source,
            body,
            states: BitStack::new(),
            params: param_names,
            names: Default::default(),
        }
    }
    pub(super) fn enter(&mut self, n: Syntax<'tree>) {
        self.enter_with_kind(n, n.kind());
    }
    pub(super) fn enter_with_kind(&mut self, n: Syntax<'tree>, kind: &str) {
        let source = self.source;
        let active = self.body == Some(n)
            || (self.states.last().unwrap_or(false) && !binding_scope_kind(kind));
        self.states.push(active);
        if !active {
            return;
        }
        let lhs = match kind {
            "assignment"
            | "augmented_assignment"
            | "assignment_expression"
            | "augmented_assignment_expression"
            | "for_statement"
            | "for_in_statement"
            | "for_in_clause"
            | "short_var_declaration" => n.child_by_field_name("left"),
            "as_pattern" => n.child_by_field_name("alias"),
            "variable_declarator" | "let_declaration" | "named_expression" => n
                .child_by_field_name("name")
                .or_else(|| n.child_by_field_name("pattern")),
            _ => None,
        };
        if let Some(lhs) = lhs {
            let callable = n
                .child_by_field_name("value")
                .or_else(|| n.child_by_field_name("right"))
                .is_some_and(|n| {
                    matches!(
                        n.kind(),
                        "arrow_function" | "function_expression" | "lambda"
                    )
                });
            if !callable {
                binding_names(lhs, source, &mut self.names);
            }
        }
        if kind == "delete_statement" {
            let mut c = n.walk();
            for child in n.named_children(&mut c) {
                binding_names(child, source, &mut self.names);
            }
        }
    }
    pub(super) fn exit(&mut self) {
        self.states.pop();
    }
    pub(super) fn finish(self) -> ScopeBindings {
        let rebindings: std::collections::BTreeSet<_> =
            self.names.into_iter().map(str::to_owned).collect();
        let all: std::collections::BTreeSet<_> = self
            .params
            .into_iter()
            .map(str::to_owned)
            .chain(rebindings.iter().cloned())
            .collect();
        ScopeBindings {
            all: all.into_iter().collect(),
            rebindings: rebindings.into_iter().collect(),
        }
    }
}
pub(super) fn binding_scope(n: Syntax<'_>) -> bool {
    binding_scope_kind(n.kind())
}
pub(super) fn binding_scope_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition"
            | "lambda"
            | "class_definition"
            | "function_declaration"
            | "function_item"
            | "arrow_function"
            | "function_expression"
            | "method_definition"
            | "method_declaration"
            | "closure_expression"
            | "func_literal"
    )
}
pub(super) fn bindings(node: Syntax<'_>, source: &str) -> ScopeBindings {
    let mut collector = BindingCollector::new(node, source);
    walk(node, &mut |node, entering| {
        if entering {
            collector.enter(node);
        } else {
            collector.exit();
        }
    });
    collector.finish()
}

pub(super) struct SinglePassExtractor<'a> {
    pub(super) source: &'a str,
    pub(super) path: &'a str,
    pub(super) profile: &'a dyn LanguageProfile,
    pub(super) language_context: &'a languages::LanguageContext,
    pub(super) rule_ir: languages::manifests::RuleIRHandle,
    pub(super) route_context: routes::RouteContext,
    pub(super) file: FileContext,
    pub(super) module: &'a str,
    pub(super) offset: usize,
    pub(super) symbols: HashMap<usize, String>,
    pub(super) ids: HashSet<String>,
    pub(super) flows: Option<&'a mut FlowStore>,
}
struct VisitContext<'a> {
    root_id: usize,
    source: &'a str,
    path: &'a str,
    profile: &'a dyn LanguageProfile,
    language: &'static str,
    route_extraction_enabled: bool,
    language_context: &'a languages::LanguageContext,
    rule_ir: &'a languages::manifests::RuleIRHandle,
    route_context: &'a routes::RouteContext,
    deferred_python: bool,
    syntax: Option<std::sync::Arc<languages::CompiledSyntaxGrammar>>,
    file: &'a FileContext,
    module: &'a str,
    offset: usize,
}
struct VisitState<'a, 'flow> {
    symbols: &'a mut HashMap<usize, String>,
    ids: &'a mut HashSet<String>,
    flows: &'a mut Option<&'flow mut FlowStore>,
    root_summary: Option<ScopeBindings>,
    prepared_root_bindings: Option<ScopeBindings>,
    prepared_scope_bindings: HashMap<usize, ScopeBindings>,
    projections: HashMap<String, crate::core::semantic::ValueFlowFacts>,
}
struct ScopeTraversal<'scope, 'source, 'tree> {
    frames: Vec<ScopeFrame<'source, 'tree>>,
    scopes: &'scope mut Vec<String>,
    #[cfg(feature = "lang-python")]
    deferred: Vec<PythonAction<'tree>>,
}
impl SinglePassExtractor<'_> {
    pub(super) fn visit(
        &mut self,
        node: Syntax<'_>,
        scopes: &mut Vec<String>,
        owner: &str,
        facts: &mut Facts,
    ) -> ScopeBindings {
        let deferred_python = self.profile.id() == "python" && self.route_context.uses_main_walk();
        let mut summary_builder = deferred_python.then(|| {
            routes::RouteContextBuilder::for_main_walk(node, self.source, self.language_context)
        });
        let prepared_root_bindings = self
            .file
            .root_bindings
            .take()
            .filter(|(root, _)| *root == node.id())
            .map(|(_, bindings)| bindings);
        let prepared_scope_bindings = if self.profile.id() == "python" {
            self.route_context.take_python_bindings()
        } else {
            HashMap::new()
        };
        let context = VisitContext {
            root_id: node.id(),
            source: self.source,
            path: self.path,
            profile: self.profile,
            language: self.profile.id(),
            route_extraction_enabled: self.profile.route_extraction_enabled(),
            language_context: self.language_context,
            rule_ir: &self.rule_ir,
            route_context: &self.route_context,
            deferred_python,
            syntax: self.profile.syntax_grammar_spec().map(|spec| {
                spec.compile_cached(&node.language())
                    .expect("syntax spec validated before parsing")
            }),
            file: &self.file,
            module: self.module,
            offset: self.offset,
        };
        let mut state = VisitState {
            symbols: &mut self.symbols,
            ids: &mut self.ids,
            flows: &mut self.flows,
            root_summary: None,
            prepared_root_bindings,
            prepared_scope_bindings,
            projections: HashMap::new(),
        };
        let mut scope_traversal = ScopeTraversal {
            frames: Vec::new(),
            scopes,
            #[cfg(feature = "lang-python")]
            deferred: Vec::new(),
        };
        walk_in_order(node, false, &mut |node, entering| {
            if let Some(builder) = &mut summary_builder {
                let prepared_bindings = builder.observe(node, entering);
                if !entering && prepared_bindings {
                    if let Some(bindings) = builder.take_bindings(node) {
                        state.prepared_scope_bindings.insert(node.id(), bindings);
                    }
                }
            }
            Self::visit_event(
                &context,
                &mut state,
                node,
                &mut scope_traversal,
                owner,
                facts,
                entering,
            );
        });
        #[cfg(feature = "lang-python")]
        if let Some(builder) = summary_builder {
            let lexical = builder.finish();
            finish_python_actions(
                &context,
                &mut state,
                facts,
                scope_traversal.deferred,
                &lexical,
            );
        }
        #[cfg(feature = "lang-typescript")]
        if matches!(context.language, "typescript" | "javascript") {
            languages::typescript::finalize_configured_projections(
                facts,
                state.flows.as_deref_mut(),
                context.file,
            );
        }
        state.root_summary.unwrap_or(ScopeBindings {
            all: Vec::new(),
            rebindings: Vec::new(),
        })
    }

    fn visit_event<'source, 'tree>(
        context: &VisitContext<'source>,
        state: &mut VisitState<'_, '_>,
        node: Syntax<'tree>,
        traversal: &mut ScopeTraversal<'_, 'source, 'tree>,
        owner: &str,
        facts: &mut Facts,
        entering: bool,
    ) {
        if !entering {
            if context.language == "python"
                && traversal
                    .frames
                    .first()
                    .is_some_and(|root| root.suspended_at == Some(node.id()))
                && module_export_exit_event(node)
            {
                if let Some(root) = traversal.frames.first_mut() {
                    root.observe_module_event(node, false);
                }
            }
            if route_candidate(context.route_extraction_enabled, context.language, node) {
                let node_owner = traversal
                    .frames
                    .last()
                    .map_or(owner, |frame| frame.owner.as_str());
                let ctx = SyntaxContext {
                    node,
                    source: context.source,
                    owner: node_owner,
                    offset: context.offset,
                    shadowed_require_scopes: &context.file.shadowed_require_scopes,
                    type_checking_aliases: Some(&context.file.type_checking_aliases),
                };
                #[cfg(feature = "lang-python")]
                if context.deferred_python {
                    if let Some(event) = languages::python::prepare_deferred_event(
                        &ctx,
                        facts,
                        state.symbols,
                        context.language_context,
                        context.rule_ir,
                    ) {
                        traversal.deferred.push(PythonAction::Candidate(event));
                    }
                } else {
                    context.profile.extract_routes_with_typed_projections(
                        &ctx,
                        facts,
                        state.symbols,
                        context.language_context,
                        context.rule_ir,
                        context.route_context,
                        &mut state.projections,
                    );
                }
                #[cfg(not(feature = "lang-python"))]
                context.profile.extract_routes_with_typed_projections(
                    &ctx,
                    facts,
                    state.symbols,
                    context.language_context,
                    context.rule_ir,
                    context.route_context,
                    &mut state.projections,
                );
            }
            if context.language == "python" {
                let mut first_effective_frame = traversal.frames.len();
                for (index, frame) in traversal.frames.iter().enumerate().rev() {
                    if frame
                        .suspended_at
                        .is_some_and(|suspended_at| suspended_at != node.id())
                    {
                        break;
                    }
                    first_effective_frame = index;
                }
                for frame in traversal.frames.iter_mut().skip(first_effective_frame) {
                    frame.exit(node);
                }
            } else {
                for frame in traversal.frames.iter_mut() {
                    frame.exit(node);
                }
            }
            if let Some(frame) = traversal
                .frames
                .pop_if(|frame| frame.root.id() == node.id())
            {
                let named_scope = frame.name.is_some();
                #[cfg(feature = "lang-python")]
                if context.deferred_python {
                    traversal.deferred.push(PythonAction::Publication(
                        frame.prepare_publication(context, state),
                    ));
                } else {
                    frame.publish(context, state, facts);
                }
                #[cfg(not(feature = "lang-python"))]
                frame.publish(context, state, facts);
                if named_scope {
                    traversal.scopes.pop();
                }
            }
            return;
        }
        let (source, path, language, module, offset) = (
            context.source,
            context.path,
            context.language,
            context.module,
            context.offset,
        );
        let node_kind = node.kind();
        let is_boundary = scope_boundary_kind(language, node_kind);
        let syntax_kind = context
            .syntax
            .as_ref()
            .and_then(|syntax| syntax.classify(node.kind_id()));
        for frame in traversal.frames.iter_mut() {
            if frame.suspended_at.is_some() {
                frame.enter_suspended(node);
            } else {
                frame.enter(node, node_kind, is_boundary);
            }
        }
        if language == "python"
            && traversal
                .frames
                .first()
                .is_some_and(|root| root.suspended_at.is_none())
            && module_export_enter_kind(node_kind)
        {
            if let Some(root) = traversal.frames.first_mut() {
                root.observe_module_event(node, true);
            }
        }
        let module_root = node.id() == context.root_id;
        if module_root {
            let prepared_bindings = state
                .prepared_scope_bindings
                .remove(&node.id())
                .filter(|_| state.prepared_root_bindings.is_none());
            let mut frame = ScopeFrame::new(
                node,
                owner.to_owned(),
                None,
                state.prepared_root_bindings.is_none(),
                context,
                prepared_bindings,
            );
            frame.enter(node, node_kind, is_boundary);
            traversal.frames.push(frame);
        }
        if module_root {
            if let Some(module_index) = facts.nodes.iter().position(|n| n.id == owner) {
                traversal
                    .frames
                    .last_mut()
                    .expect("module frame entered")
                    .node_index = Some(module_index);
                let module = &mut facts.nodes[module_index];
                #[cfg(feature = "lang-python")]
                if language == "python" {
                    let factories =
                        crate::engine::languages::python::logging_factories(node, source, offset);
                    if factories
                        .as_array()
                        .is_some_and(|entries| !entries.is_empty())
                    {
                        module.details["python_logging_factories"] = factories;
                    }
                }
                #[cfg(feature = "lang-typescript")]
                if matches!(language, "javascript" | "typescript" | "vue") {
                    module.details["commonjs_bindings"] =
                        json!(crate::engine::languages::typescript::commonjs_bindings(
                            node,
                            source,
                            offset,
                            &context.file.shadowed_require_scopes,
                        ));
                }
                if !context.file.lazy_exports.is_empty() {
                    module.details["lazy_exports"] = json!(context.file.lazy_exports);
                } else if language == "python" {
                    // The Python module collector finalizes this slot after the
                    // shared traversal, in the same position used by prepared data.
                    module.details["lazy_exports"] = serde_json::Value::Null;
                }
                if !context.file.exports.is_empty() {
                    module.details["exports"] = json!(context.file.exports);
                } else {
                    // Preserve the original root-detail slot order when a profile
                    // finalizes exports from bindings collected by this traversal.
                    module.details["exports"] = serde_json::Value::Null;
                }
                let details = module
                    .details
                    .as_object_mut()
                    .expect("module details object");
                let frame = traversal.frames.last_mut().expect("module frame entered");
                frame.reserved_flow_slot = !details.contains_key("value_flow");
                details
                    .entry("value_flow")
                    .or_insert(serde_json::Value::Null);
                details.entry("bindings").or_insert_with(|| json!([]));
                details.entry("rebindings").or_insert_with(|| json!([]));
            }
        }
        if node.is_error() || node.is_missing() {
            let is_ts_grammar_gap = if language == "typescript" {
                let line_text = source.lines().nth(node.start_position().row).unwrap_or("");
                line_text.contains("import(")
                    || line_text.contains("readonly [")
                    || line_text.contains("readonly (")
            } else {
                false
            };
            if !is_ts_grammar_gap {
                facts.errors.push(Diagnostic {
                    path: path.into(),
                    line: node.start_position().row + offset + 1,
                    message: format!(
                        "syntax {} at column {}",
                        node.kind(),
                        node.start_position().column + 1
                    ),
                });
            }
        }
        let parent_owner = traversal
            .frames
            .last()
            .map_or(owner, |frame| frame.owner.as_str());
        let kind = match context.syntax.as_ref() {
            Some(_) => syntax_kind.and_then(languages::CompiledSyntaxKind::declaration),
            None => context
                .profile
                .symbol_with_context(node, source, context.file),
        };
        if let Some(kind) = kind {
            let name = context
                .profile
                .symbol_name(node, source)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    format!(
                        "anonymous@{}:{}",
                        node.start_position().row + offset + 1,
                        node.start_position().column + 1
                    )
                });
            let prefix = context.profile.node_prefix(kind);
            let line = node.start_position().row + offset + 1;
            let mut id = format!("{prefix}:{path}:{line}:{name}");
            if !state.ids.insert(id.clone()) {
                id.push_str(&format!(":{}", node.start_position().column));
                state.ids.insert(id.clone());
            }
            state.symbols.insert(node.id(), id.clone());
            let prepared_bindings = state.prepared_scope_bindings.remove(&node.id());
            let mut frame = ScopeFrame::new(
                node,
                id.clone(),
                Some(name.clone()),
                true,
                context,
                prepared_bindings,
            );
            let mut metadata = if frame.analysis.is_some() {
                context.profile.metadata(node, source, &name, context.file)
            } else {
                let (metadata, flow) =
                    context
                        .profile
                        .declaration(node, source, &name, context.file);
                frame.legacy_flow = Some(flow);
                metadata
            };
            let test_role = context.profile.test_role(node, source, path, context.file);
            let prototype_owner = (language == "javascript"
                && matches!(node.kind(), "function_expression" | "arrow_function")
                && metadata.is_method == Some(true))
            .then(|| metadata.receiver_type.clone())
            .flatten();
            let qualname = if let Some(receiver) = prototype_owner.as_deref() {
                qualified(module, traversal.scopes, &format!("{receiver}.{name}"))
            } else {
                qualified(module, traversal.scopes, &name)
            };
            let mut details = serde_json::Map::new();
            if let Some(receiver_name) = metadata.receiver_name.filter(|value| !value.is_empty()) {
                details.insert("receiver_name".into(), json!(receiver_name));
            }
            if metadata.default_export {
                details.insert("default_export".into(), json!(true));
            }
            let doc = syntax_doc(context, node);
            if !doc.is_empty() {
                details.insert("doc".into(), json!(doc));
            }
            if !metadata.decorators.is_empty() {
                details.insert("decorators".into(), json!(metadata.decorators));
            }
            if metadata
                .bases
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            {
                details.insert("bases".into(), json!(metadata.bases));
            }
            if metadata
                .receiver
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            {
                details.insert("receiver".into(), json!(metadata.receiver));
            }
            if let Some(signature) = declaration_signature_if_informative(node, source) {
                details.insert("signature".into(), json!(signature));
            }
            if metadata.is_async {
                details.insert("async".into(), json!(true));
            }
            if let Some(test_role) = test_role {
                details.insert("test_role".into(), json!(test_role));
            }
            if let Some(is_public) = metadata.is_public {
                details.insert("is_public".into(), json!(is_public));
            }
            if let Some(receiver_type) = metadata.receiver_type.as_ref() {
                details.insert("receiver_type".into(), json!(receiver_type));
            }
            if prototype_owner.is_some() {
                details.insert("prototype".into(), json!(true));
            }
            if language == "javascript" && commonjs_default_callable(node, source) {
                details.insert("commonjs_default_callable".into(), json!(true));
            }
            if metadata.is_method == Some(true) && kind != "method" {
                details.insert("is_method".into(), json!(true));
            }
            if metadata.is_static == Some(true) {
                details.insert("is_static".into(), json!(true));
            }
            let native_semantic = if !metadata.param_types.is_empty() {
                if state.flows.is_some() {
                    details.insert("param_types".into(), serde_json::Value::Null);
                    Some(Box::new(
                        crate::core::typed_facts::NodeSemanticEntry::with_parameters(
                            std::mem::take(&mut metadata.param_types),
                        ),
                    ))
                } else {
                    details.insert("param_types".into(), json!(metadata.param_types));
                    None
                }
            } else {
                None
            };
            details.insert("column".into(), json!(node.start_position().column));
            if metadata.is_overload {
                details.insert("is_overload".into(), json!(true));
            }
            if metadata.is_stub {
                details.insert("is_stub".into(), json!(true));
            }
            if metadata.is_conditional {
                details.insert("is_conditional".into(), json!(true));
            }
            let details = if details.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::Value::Object(details)
            };
            let node_index = facts.nodes.len();
            facts.nodes.push(Node {
                native_semantic,
                id: id.clone(),
                kind: kind.into(),
                name: name.clone(),
                qualname,
                path: path.into(),
                line,
                end_line: node.end_position().row + offset + 1,
                is_test: is_test(path)
                    || test_role.is_some()
                    || context.profile.test_attribute(node, source),
                language: language.into(),
                generated: false,
                details,
            });
            frame.node_index = Some(node_index);
            facts.edges.push(Edge {
                src: parent_owner.into(),
                dst: id.clone(),
                kind: "contains".into(),
                path: path.into(),
                line,
                evidence: node.kind().into(),
                confidence: "exact".into(),
            });
            context.profile.extract_relations(
                &SyntaxContext {
                    node,
                    source,
                    owner: &id,
                    offset,
                    shadowed_require_scopes: &context.file.shadowed_require_scopes,
                    type_checking_aliases: Some(&context.file.type_checking_aliases),
                },
                facts,
            );
            frame.reserved_flow_slot = true;
            frame.metadata_return = metadata.return_type;
            frame.metadata_ownership = metadata.object_ownership;
            frame.enter(node, node_kind, is_boundary);
            traversal.frames.push(frame);
            traversal.scopes.push(name);
        }
        let node_owner = traversal
            .frames
            .last()
            .map_or(owner, |frame| frame.owner.as_str());
        let ctx = SyntaxContext {
            node,
            source,
            owner: node_owner,
            offset,
            shadowed_require_scopes: &context.file.shadowed_require_scopes,
            type_checking_aliases: Some(&context.file.type_checking_aliases),
        };
        if context
            .syntax
            .as_ref()
            .is_none_or(|_| syntax_kind.is_some_and(|kind| kind.is_import()))
        {
            context.profile.extract_imports(&ctx, facts);
        }
        if context
            .syntax
            .as_ref()
            .is_none_or(|_| syntax_kind.is_some_and(|kind| kind.is_reference()))
        {
            context.profile.extract_calls(&ctx, facts);
        }
        if context
            .syntax
            .as_ref()
            .is_some_and(|_| syntax_kind.is_some_and(|kind| kind.is_member_access()))
        {
            facts.references.push(crate::core::models::Reference {
                source: node_owner.to_owned(),
                expression: text(node, source).into(),
                kind: "reads".into(),
                line: ctx.line(),
                column: node.start_position().column,
                alias: None,
                module: None,
                dynamic: false,
                receiver_hint: None,
            });
        }
        if matches!(node_kind, "call" | "call_expression") {
            routes::dependency_injection(&ctx, context.rule_ir, facts);
        }
        context.profile.extract_mutations(&ctx, facts);
    }
}

fn route_candidate(enabled: bool, language: &str, node: Syntax<'_>) -> bool {
    if !enabled {
        return false;
    }
    match language {
        "python" => matches!(
            node.kind(),
            "function_definition"
                | "class_definition"
                | "call"
                | "assignment"
                | "delete_statement"
                | "augmented_assignment"
        ),
        "javascript" | "typescript" | "vue" => matches!(
            node.kind(),
            "method_definition"
                | "method_declaration"
                | "call_expression"
                | "object"
                | "member_expression"
        ),
        _ => true,
    }
}

#[cfg(feature = "lang-python")]
enum PythonAction<'tree> {
    Candidate(languages::python::DeferredEvent<'tree>),
    Publication(ScopePublication<'tree>),
}

#[cfg(feature = "lang-python")]
fn finish_python_actions(
    context: &VisitContext<'_>,
    state: &mut VisitState<'_, '_>,
    facts: &mut Facts,
    actions: Vec<PythonAction<'_>>,
    lexical: &routes::RouteContext,
) {
    let base = (facts.nodes.len(), facts.edges.len(), facts.references.len());
    let mut node_batches = Vec::new();
    let mut edge_batches = Vec::new();
    let mut reference_batches = Vec::new();
    for action in actions {
        match action {
            PythonAction::Publication(publication) => publication.publish(context, state, facts),
            PythonAction::Candidate(event) => {
                let before = (facts.nodes.len(), facts.edges.len(), facts.references.len());
                let ctx = SyntaxContext {
                    node: event.node,
                    source: context.source,
                    owner: &event.owner,
                    offset: context.offset,
                    shadowed_require_scopes: &context.file.shadowed_require_scopes,
                    type_checking_aliases: Some(&context.file.type_checking_aliases),
                };
                languages::python::finish_deferred_event(
                    &event,
                    &ctx,
                    facts,
                    state.symbols,
                    context.language_context,
                    context.rule_ir,
                    lexical,
                    &mut state.projections,
                );
                node_batches.push((event.frontiers.0, facts.nodes.len() - before.0));
                edge_batches.push((event.frontiers.1, facts.edges.len() - before.1));
                reference_batches.push((event.frontiers.2, facts.references.len() - before.2));
            }
        }
    }
    facts.remap_deferred_nodes(base.0, &node_batches);
    restore_append_order(&mut facts.nodes, base.0, node_batches);
    restore_append_order(&mut facts.edges, base.1, edge_batches);
    restore_append_order(&mut facts.references, base.2, reference_batches);
}

#[cfg(feature = "lang-python")]
fn restore_append_order<T>(values: &mut Vec<T>, base_len: usize, batches: Vec<(usize, usize)>) {
    if values.len() == base_len {
        return;
    }
    let append_only =
        batches
            .iter()
            .try_fold((0usize, 0usize), |(total, previous), &(frontier, count)| {
                if frontier < previous
                    || frontier > base_len
                    || (count != 0 && frontier != base_len)
                {
                    return None;
                }
                total.checked_add(count).map(|total| (total, frontier))
            });
    if append_only.is_some_and(|(total, _)| total == values.len() - base_len) {
        return;
    }
    let mut original = std::mem::take(values);
    let appended = original.split_off(base_len);
    let mut original = original.into_iter();
    let mut appended = appended.into_iter();
    let mut cursor = 0;
    for (frontier, count) in batches {
        values.extend(original.by_ref().take(frontier - cursor));
        cursor = frontier;
        values.extend(appended.by_ref().take(count));
    }
    values.extend(original);
    debug_assert!(appended.next().is_none());
}

struct ScopeFrame<'source, 'tree> {
    root: Syntax<'tree>,
    owner: String,
    node_index: Option<usize>,
    name: Option<String>,
    metadata_return: Option<crate::core::semantic::TypeExpr>,
    metadata_ownership: Option<crate::core::semantic::ObjectOwnershipFact>,
    bindings: Option<BindingCollector<'source, 'tree>>,
    prepared_bindings: Option<ScopeBindings>,
    analysis: Option<ScopeAnalysis<'source, 'tree>>,
    legacy_flow: Option<crate::core::semantic::ValueFlowFacts>,
    reserved_flow_slot: bool,
    suspended_at: Option<usize>,
}
impl<'source, 'tree> ScopeFrame<'source, 'tree> {
    fn new(
        root: Syntax<'tree>,
        owner: String,
        name: Option<String>,
        collect_bindings: bool,
        context: &VisitContext<'source>,
        prepared_bindings: Option<ScopeBindings>,
    ) -> Self {
        Self {
            root,
            owner,
            node_index: None,
            name,
            metadata_return: None,
            metadata_ownership: None,
            bindings: (collect_bindings
                && prepared_bindings.is_none()
                && !(context.deferred_python
                    && (root.id() == context.root_id || binding_scope(root))))
            .then(|| BindingCollector::new_with_kind(root, context.source, root.kind())),
            prepared_bindings,
            analysis: context
                .profile
                .scope_analysis(root, context.source, context.file),
            legacy_flow: None,
            reserved_flow_slot: false,
            suspended_at: None,
        }
    }
    fn enter(&mut self, node: Syntax<'tree>, node_kind: &str, is_boundary: bool) {
        if let Some(bindings) = &mut self.bindings {
            bindings.enter_with_kind(node, node_kind);
        }
        if let Some(analysis) = &mut self.analysis {
            analysis.enter(node);
        }
        if is_boundary && node.id() != self.root.id() {
            self.suspended_at = Some(node.id());
        }
    }
    fn observe_module_event(&mut self, node: Syntax<'tree>, entering: bool) {
        if self.root.kind() == "module" {
            if let Some(analysis) = &mut self.analysis {
                analysis.observe_module_event(node, entering);
            }
        }
    }
    fn enter_suspended(&mut self, node: Syntax<'tree>) {
        if let Some(analysis) = &mut self.analysis {
            analysis.enter_suspended(node);
        }
    }
    fn exit(&mut self, node: Syntax<'tree>) {
        if let Some(suspended_at) = self.suspended_at {
            if suspended_at != node.id() {
                return;
            }
            self.suspended_at = None;
        }
        if let Some(bindings) = &mut self.bindings {
            bindings.exit();
        }
        if let Some(analysis) = &mut self.analysis {
            analysis.exit(node);
        }
    }
    fn prepare_publication(
        self,
        context: &VisitContext<'_>,
        state: &mut VisitState<'_, '_>,
    ) -> ScopePublication<'tree> {
        let collected_bindings = state
            .prepared_scope_bindings
            .remove(&self.root.id())
            .or(self.prepared_bindings);
        let prepared_bindings = if self.name.is_none() {
            state.prepared_root_bindings.take().or(collected_bindings)
        } else {
            collected_bindings
        };
        let mut bindings = if let Some(bindings) = prepared_bindings {
            bindings
        } else if context.syntax.as_ref().is_some_and(|syntax| {
            !syntax
                .classify(self.root.kind_id())
                .is_some_and(|kind| kind.is_scope())
        }) {
            ScopeBindings {
                all: Vec::new(),
                rebindings: Vec::new(),
            }
        } else if self.analysis.is_some() || context.syntax.is_some() {
            self.bindings
                .expect("unprepared scope collects bindings")
                .finish()
        } else {
            context.profile.bindings(self.root, context.source)
        };
        let mut module_outputs = None;
        let mut flow = if let Some(analysis) = self.analysis {
            if self.name.is_none() {
                module_outputs = analysis.module_exports(&bindings);
            }
            let (flow, aliases) = analysis.finish(self.root.kind() == "module");
            if self.root.kind() == "module" {
                if let Some(aliases) = aliases {
                    bindings.rebindings.retain(|name| !aliases.contains(name));
                }
            }
            flow
        } else if let Some(flow) = self.legacy_flow {
            flow
        } else {
            context
                .profile
                .value_flow_with_context(self.root, context.source, context.file)
        };
        if flow.return_type.is_none() {
            flow.return_type = self.metadata_return;
        }
        if let Some(ownership) = self.metadata_ownership {
            flow.object_ownership = Some(ownership);
        }
        flow.offset_lines(context.offset);
        #[cfg(feature = "lang-html")]
        if let Some(framework) = flow.framework.as_mut() {
            for template in framework.template_contexts_mut() {
                template.target_path = languages::html::local_template_target(
                    context.path,
                    &template.target,
                    context.language_context,
                );
            }
        }
        ScopePublication {
            root: self.root,
            owner: self.owner,
            node_index: self.node_index,
            name: self.name,
            reserved_flow_slot: self.reserved_flow_slot,
            flow: (!(flow.is_empty() && flow.return_position.is_none())).then(|| Box::new(flow)),
            bindings,
            module_outputs,
        }
    }

    fn publish(
        self,
        context: &VisitContext<'_>,
        state: &mut VisitState<'_, '_>,
        facts: &mut Facts,
    ) {
        self.prepare_publication(context, state)
            .publish(context, state, facts);
    }
}

struct ScopePublication<'tree> {
    root: Syntax<'tree>,
    owner: String,
    node_index: Option<usize>,
    name: Option<String>,
    reserved_flow_slot: bool,
    flow: Option<Box<crate::core::semantic::ValueFlowFacts>>,
    bindings: ScopeBindings,
    module_outputs: Option<(Vec<crate::core::semantic::ExportBinding>, serde_json::Value)>,
}

impl ScopePublication<'_> {
    fn publish(
        self,
        context: &VisitContext<'_>,
        state: &mut VisitState<'_, '_>,
        facts: &mut Facts,
    ) {
        let mut prepared_flow = self.flow;
        let mut empty_flow = crate::core::semantic::ValueFlowFacts::default();
        let flow = prepared_flow.as_deref_mut().unwrap_or(&mut empty_flow);
        let mut bindings = self.bindings;
        let module_outputs = self.module_outputs;
        let mut preserve_invalid = false;
        let Some(node) = self
            .node_index
            .and_then(|index| facts.nodes.get_mut(index))
            .filter(|node| node.id == self.owner)
        else {
            return;
        };
        if let Some(projections) = state.projections.remove(&self.owner) {
            if projections.fact_limit_exceeded {
                flow.fact_limit_exceeded = true;
            }
            if let Some(projections) = projections.framework {
                let mut projections = *projections;
                projections.facts.retain(|fact| {
                    matches!(
                        fact,
                        crate::core::semantic::FrameworkFact::ComponentConfiguration(_)
                            | crate::core::semantic::FrameworkFact::SymbolAttachment(_)
                            | crate::core::semantic::FrameworkFact::NamedBinding(_)
                            | crate::core::semantic::FrameworkFact::EndpointRequest(_)
                    )
                });
                if !projections.facts.is_empty() {
                    flow.framework
                        .get_or_insert_with(Default::default)
                        .facts
                        .append(&mut projections.facts);
                }
            }
        }
        if !context.profile.typed_scope_projections() {
            if let Some(configurations) = node.details["value_flow"].get("component_configurations")
            {
                flow.framework
                    .get_or_insert_with(Default::default)
                    .facts
                    .retain(|fact| {
                        !matches!(
                            fact,
                            crate::core::semantic::FrameworkFact::ComponentConfiguration(_)
                        )
                    });
                let configurations: Vec<crate::core::semantic::ComponentConfigurationFact> =
                    serde_json::from_value(configurations.clone())
                        .expect("parser-owned typed component configurations");
                for fact in configurations {
                    flow.push_component_configuration(fact);
                }
            }
            if let Some(value) = node.details["value_flow"].get("symbol_attachments") {
                flow.framework
                    .get_or_insert_with(Default::default)
                    .facts
                    .retain(|fact| {
                        !matches!(
                            fact,
                            crate::core::semantic::FrameworkFact::SymbolAttachment(_)
                        )
                    });
                let attachments: Vec<crate::core::semantic::SymbolAttachmentFact> =
                    serde_json::from_value(value.clone()).expect("parser-owned symbol attachments");
                for fact in attachments {
                    flow.push_symbol_attachment(fact);
                }
            }
            if let Some(value) = node.details["value_flow"].get("named_bindings") {
                flow.framework
                    .get_or_insert_with(Default::default)
                    .facts
                    .retain(|fact| {
                        !matches!(fact, crate::core::semantic::FrameworkFact::NamedBinding(_))
                    });
                let bindings: Vec<crate::core::semantic::NamedBindingFact> =
                    serde_json::from_value(value.clone()).expect("parser-owned named bindings");
                for fact in bindings {
                    flow.push_named_binding(fact);
                }
            }
            if let Some(value) = node.details["value_flow"].get("endpoint_requests") {
                flow.framework
                    .get_or_insert_with(Default::default)
                    .facts
                    .retain(|fact| {
                        !matches!(
                            fact,
                            crate::core::semantic::FrameworkFact::EndpointRequest(_)
                        )
                    });
                let requests: Vec<crate::core::semantic::EndpointRequestFact> =
                    serde_json::from_value(value.clone()).expect("parser-owned endpoint requests");
                for fact in requests {
                    flow.push_endpoint_request(fact);
                }
            }
            if node.details["value_flow"]["fact_limit_exceeded"] == true {
                flow.fact_limit_exceeded = true;
            }
        }
        let mut visibility_exports = None;
        if self.name.is_none() {
            if let Some((exports, lazy_exports)) = module_outputs {
                visibility_exports = Some(exports.clone());
                if lazy_exports
                    .as_array()
                    .is_some_and(|entries| !entries.is_empty())
                {
                    node.details["lazy_exports"] = lazy_exports;
                } else {
                    node.details
                        .as_object_mut()
                        .expect("module details object")
                        .shift_remove("lazy_exports");
                }
                if exports.is_empty() {
                    node.details
                        .as_object_mut()
                        .expect("module details object")
                        .shift_remove("exports");
                } else {
                    node.details["exports"] = json!(exports);
                }
            } else {
                if let Some(exports) = context.profile.module_exports_with_bindings(
                    self.root,
                    context.source,
                    &bindings,
                ) {
                    visibility_exports = Some(exports.clone());
                    if exports.is_empty() {
                        node.details
                            .as_object_mut()
                            .expect("module details object")
                            .shift_remove("exports");
                    } else {
                        node.details["exports"] = json!(exports);
                    }
                } else if context.file.exports.is_empty() {
                    node.details
                        .as_object_mut()
                        .expect("module details object")
                        .shift_remove("exports");
                }
            }
            state.root_summary = Some(ScopeBindings {
                all: bindings.all.clone(),
                rebindings: bindings.rebindings.clone(),
            });
            bindings.all.extend(node.binding_names().map(str::to_owned));
            bindings
                .rebindings
                .extend(node.rebinding_names().map(str::to_owned));
            bindings.all.sort();
            bindings.all.dedup();
            bindings.rebindings.sort();
            bindings.rebindings.dedup();
            if !flow.is_empty() {
                let previous = if let Some(flows) = state.flows.as_mut() {
                    if matches!(
                        flows.state(node),
                        Some(crate::core::typed_facts::FlowState::Invalid(_))
                    ) {
                        preserve_invalid = true;
                        None
                    } else {
                        flows.take(&node.id)
                    }
                } else {
                    let raw = &node.details["value_flow"];
                    if raw.is_null() {
                        None
                    } else if raw["fact_limit_exceeded"] == true
                        || crate::core::typed_facts::encoded_scope_fact_count(raw)
                            > crate::core::semantic::DEFAULT_FACTS_PER_SCOPE
                    {
                        preserve_invalid = true;
                        None
                    } else {
                        match crate::core::semantic::ValueFlowFacts::deserialize(raw) {
                            Ok(previous) => Some(previous),
                            Err(_) => {
                                preserve_invalid = true;
                                None
                            }
                        }
                    }
                };
                if let Some(mut previous) = previous {
                    if context.profile.typed_scope_projections() {
                        if let Some(mut projections) = previous.framework.take() {
                            projections.facts.retain(|fact| {
                                matches!(
                                    fact,
                                    crate::core::semantic::FrameworkFact::ComponentConfiguration(_)
                                        | crate::core::semantic::FrameworkFact::SymbolAttachment(_)
                                        | crate::core::semantic::FrameworkFact::NamedBinding(_)
                                        | crate::core::semantic::FrameworkFact::EndpointRequest(_)
                                )
                            });
                            if !projections.facts.is_empty() {
                                let mut current = flow.framework.take().unwrap_or_default();
                                projections.facts.append(&mut current.facts);
                                current.facts = projections.facts;
                                flow.framework = Some(current);
                            }
                        }
                        if previous.fact_limit_exceeded {
                            flow.fact_limit_exceeded = true;
                        }
                    }
                    flow.bindings.extend(previous.bindings);
                    flow.immutable_initializers
                        .extend(previous.immutable_initializers);
                    flow.fields.extend(previous.fields);
                }
                flow.bindings.sort_by_key(|v| v.position);
                flow.immutable_initializers.sort_unstable();
                flow.fields.sort_by_key(|v| v.position);
            }
        }
        if state.flows.is_some() {
            crate::core::typed_facts::NodeSemanticEntry::set_bindings(
                node,
                bindings.all,
                bindings.rebindings,
            );
        } else {
            if bindings.all.is_empty() {
                if let Some(details) = node.details.as_object_mut() {
                    details.remove("bindings");
                }
            } else {
                node.details["bindings"] = json!(bindings.all);
            }
            if bindings.rebindings.is_empty() {
                if let Some(details) = node.details.as_object_mut() {
                    details.remove("rebindings");
                }
            } else {
                node.details["rebindings"] = json!(bindings.rebindings);
            }
        }
        // Keep opaque prior evidence; a later island cannot restore exact facts.
        if !preserve_invalid && !flow.is_empty() {
            if let Some(flows) = state.flows.as_mut() {
                if let Some(details) = node.details.as_object_mut() {
                    details.shift_remove("value_flow");
                }
                flows.insert_boxed(
                    node.id.clone(),
                    prepared_flow.unwrap_or_else(|| Box::new(empty_flow)),
                );
            } else if context.profile.typed_scope_projections() {
                let mut validated = FlowStore::default();
                validated.insert_boxed(
                    node.id.clone(),
                    prepared_flow.unwrap_or_else(|| Box::new(empty_flow)),
                );
                node.details["value_flow"] =
                    json!(validated.state(node).expect("published scope flow").view());
            } else {
                node.details["value_flow"] = json!(flow);
            }
        } else if !preserve_invalid && self.reserved_flow_slot {
            if let Some(details) = node.details.as_object_mut() {
                details.shift_remove("value_flow");
            }
        }
        if let Some(exports) = visibility_exports.as_deref() {
            context
                .profile
                .finalize_file_visibility(self.root, context.source, facts, exports);
        }
    }
}

fn module_export_enter_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition"
            | "class_definition"
            | "lambda"
            | "assignment"
            | "augmented_assignment"
            | "delete_statement"
            | "import_statement"
            | "import_from_statement"
            | "for_statement"
            | "named_expression"
            | "as_pattern"
    )
}

fn module_export_exit_event(node: Syntax<'_>) -> bool {
    matches!(
        node.kind(),
        "function_definition" | "class_definition" | "lambda"
    )
}

fn scope_boundary_kind(language: &str, kind: &str) -> bool {
    match language {
        "python" => matches!(kind, "function_definition" | "class_definition" | "lambda"),
        "rust" => matches!(
            kind,
            "function_item" | "closure_expression" | "impl_item" | "trait_item" | "mod_item"
        ),
        "javascript" | "typescript" | "vue" => matches!(
            kind,
            "function_declaration"
                | "function_expression"
                | "generator_function_declaration"
                | "generator_function"
                | "generator_function_expression"
                | "arrow_function"
                | "method_definition"
                | "method_declaration"
                | "class_declaration"
                | "class"
        ),
        _ => binding_scope_kind(kind),
    }
}

fn syntax_doc(context: &VisitContext<'_>, node: Syntax<'_>) -> String {
    if let Some(syntax) = &context.syntax {
        node.prev_named_sibling()
            .filter(|comment| {
                syntax
                    .classify(comment.kind_id())
                    .is_some_and(|kind| kind.is_comment())
            })
            .map(|comment| text(comment, context.source).to_owned())
            .unwrap_or_default()
    } else {
        context.profile.doc_comment(node, context.source)
    }
}

#[cfg(all(test, feature = "lang-python"))]
mod legacy_hook_tests {
    use super::*;
    use crate::core::{
        semantic::{BindingFact, SourcePosition, TypeExpr, ValueExpr, ValueFlowFacts},
        typed_facts::TypedFacts,
    };

    struct Legacy;
    impl LanguageProfile for Legacy {
        fn id(&self) -> &'static str {
            "legacy-typed"
        }
        fn extensions(&self) -> &'static [&'static str] {
            &["py"]
        }
        fn grammar(&self, path: &str) -> tree_sitter::Language {
            languages::require("python").unwrap().grammar(path)
        }
        fn symbol_kind(&self, kind: &str) -> Option<&'static str> {
            (kind == "function_definition").then_some("function")
        }
        fn node_prefix(&self, _: &str) -> &'static str {
            "legacy"
        }
        fn normalize_import(&self, _: &str, _: &str) -> Option<languages::ImportPath> {
            None
        }
        fn prepare(&self, root: Syntax<'_>, _: &str) -> FileContext {
            FileContext {
                root_bindings: Some((
                    root.id(),
                    ScopeBindings {
                        all: vec!["prepared".into()],
                        rebindings: vec!["rebound".into()],
                    },
                )),
                ..Default::default()
            }
        }
        fn declaration(
            &self,
            _: Syntax<'_>,
            _: &str,
            _: &str,
            _: &FileContext,
        ) -> (languages::SymbolMetadata, ValueFlowFacts) {
            (
                languages::SymbolMetadata {
                    is_static: Some(true),
                    return_type: Some(TypeExpr::Named {
                        name: "MetadataReturn".into(),
                    }),
                    ..Default::default()
                },
                {
                    ValueFlowFacts {
                        framework: Some(Box::new(crate::core::semantic::FrameworkScopeFacts {
                            alias_type: Some(TypeExpr::Named {
                                name: "Alias".into(),
                            }),
                            ..Default::default()
                        })),
                        bindings: vec![BindingFact {
                            name: "local".into(),
                            position: SourcePosition { line: 2, column: 4 },
                            value: ValueExpr::Alias {
                                name: "argument".into(),
                            },
                            conditional: false,
                        }],
                        ..Default::default()
                    }
                },
            )
        }
        fn extract_file(
            &self,
            path: &str,
            source: &str,
            module: &str,
            facts: &mut Facts,
        ) -> anyhow::Result<()> {
            languages::parse_file(self, path, source, module, facts)
        }
    }

    struct Incremental(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
    impl LanguageProfile for Incremental {
        fn id(&self) -> &'static str {
            "scope-order"
        }
        fn extensions(&self) -> &'static [&'static str] {
            &["py"]
        }
        fn grammar(&self, path: &str) -> tree_sitter::Language {
            Legacy.grammar(path)
        }
        fn symbol_kind(&self, kind: &str) -> Option<&'static str> {
            Legacy.symbol_kind(kind)
        }
        fn node_prefix(&self, _: &str) -> &'static str {
            "scope-order"
        }
        fn normalize_import(&self, _: &str, _: &str) -> Option<languages::ImportPath> {
            None
        }
        fn scope_analysis<'source, 'tree>(
            &self,
            node: Syntax<'tree>,
            source: &'source str,
            file: &FileContext,
        ) -> Option<ScopeAnalysis<'source, 'tree>> {
            let inner = languages::require("python")
                .unwrap()
                .scope_analysis(node, source, file)?;
            if node.kind() != "function_definition" {
                return Some(inner);
            }
            let name = node
                .child_by_field_name("name")
                .unwrap()
                .utf8_text(source.as_bytes())
                .unwrap()
                .to_owned();
            Some(ScopeAnalysis {
                state: Some(LanguageAnalysis::Traced(Box::new(TracedAnalysis {
                    inner,
                    trace: self.0.clone(),
                    name,
                }))),
                marker: std::marker::PhantomData,
            })
        }
        fn value_flow(&self, _: Syntax<'_>, _: &str) -> ValueFlowFacts {
            panic!("incremental profile requested legacy body summary")
        }
        fn extract_calls(&self, ctx: &SyntaxContext<'_, '_>, _: &mut Facts) {
            if ctx.node.kind() == "call" {
                self.0.lock().unwrap().push(format!(
                    "call:{}",
                    ctx.node.utf8_text(ctx.source.as_bytes()).unwrap()
                ));
            }
        }
        fn extract_file(
            &self,
            path: &str,
            source: &str,
            module: &str,
            facts: &mut Facts,
        ) -> anyhow::Result<()> {
            languages::parse_file(self, path, source, module, facts)
        }
    }

    #[test]
    fn incremental_analysis_finishes_after_its_descendants() {
        let trace = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut facts = Facts::default();
        languages::parse_file(
            &Incremental(trace.clone()),
            "probe.py",
            "def outer():\n    first()\n    def inner():\n        nested()\n    last()\n",
            "probe",
            &mut facts,
        )
        .unwrap();
        assert!(facts.errors.is_empty());
        assert_eq!(
            *trace.lock().unwrap(),
            [
                "call:first()",
                "call:nested()",
                "flow:inner",
                "call:last()",
                "flow:outer"
            ]
        );
    }

    #[test]
    fn legacy_hooks_preserve_typed_flow_and_offset_composition() {
        let path = "typed.py";
        let source = "def sample(argument):\n    local = argument\n    return local\n";
        let mut failures = Vec::new();
        for offset in [0, 7] {
            let mut typed =
                TypedFacts::from_public(super::super::extract(path, "python", "").unwrap());
            if offset == 0 {
                languages::parse_file_typed(
                    &Legacy,
                    path,
                    source,
                    "typed",
                    &mut typed,
                    &std::sync::Arc::new(languages::LanguageContext::default()),
                )
                .unwrap();
            } else {
                let mut parser = tree_sitter::Parser::new();
                parser.set_language(&Legacy.grammar(path)).unwrap();
                let tree = parser.parse(source, None).unwrap();
                let root = tree.root_node();
                let language_context = languages::LanguageContext::default();
                let rule_ir = language_context.bind_rule_ir(&root.language());
                let summary = SinglePassExtractor {
                    source,
                    path,
                    profile: &Legacy,
                    language_context: &language_context,
                    rule_ir,
                    route_context: routes::RouteContext::build(root, source, &language_context),
                    file: Legacy.prepare(root, source),
                    module: "typed",
                    offset,
                    symbols: HashMap::new(),
                    ids: typed.facts.nodes.iter().map(|n| n.id.clone()).collect(),
                    flows: Some(&mut typed.flows),
                }
                .visit(
                    root,
                    &mut Vec::new(),
                    &format!("module:{path}"),
                    &mut typed.facts,
                );
                if summary.all != ["prepared"] || summary.rebindings != ["rebound"] {
                    failures.push(format!("offset={offset}: exact-root returned summary"));
                }
            }
            let symbol = typed
                .facts
                .nodes
                .iter()
                .find(|n| n.name == "sample")
                .unwrap();
            if symbol.details["is_static"] != true || symbol.line != offset + 1 {
                failures.push(format!("offset={offset}: metadata and owner position"));
            }
            let flow = typed
                .flows
                .get(symbol)
                .expect("actual typed FlowStore entry");
            if flow.return_type
                != Some(TypeExpr::Named {
                    name: "MetadataReturn".into(),
                })
                || flow.alias_type
                    != Some(TypeExpr::Named {
                        name: "Alias".into(),
                    })
                || flow.bindings[0].position.line != offset + 2
            {
                failures.push(format!("offset={offset}: typed flow/return/offset"));
            }
            if !symbol.details["value_flow"].is_null() {
                failures.push(format!("offset={offset}: typed composition must own flow"));
            }
            let module = typed
                .facts
                .nodes
                .iter()
                .find(|n| n.kind == "module")
                .unwrap();
            if module.binding_names().collect::<Vec<_>>() != ["prepared"]
                || module.rebinding_names().collect::<Vec<_>>() != ["rebound"]
            {
                failures.push(format!("offset={offset}: typed root summary publication"));
            }
            let public = serde_json::to_value(&typed).unwrap();
            let symbol = public["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|n| n["name"] == "sample")
                .unwrap();
            let decoded: ValueFlowFacts =
                serde_json::from_value(symbol["details"]["value_flow"].clone()).unwrap();
            if decoded.return_type
                != Some(TypeExpr::Named {
                    name: "MetadataReturn".into(),
                })
                || decoded.bindings[0].position.line != offset + 2
            {
                failures.push(format!("offset={offset}: public serialized typed flow"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

#[cfg(all(test, feature = "lang-python"))]
mod tree_cursor_walk_tests {
    use super::*;

    fn reference_walk<'tree>(
        node: Syntax<'tree>,
        reversed: bool,
        events: &mut Vec<(String, usize, usize, bool)>,
    ) {
        events.push((
            node.kind().to_owned(),
            node.start_byte(),
            node.end_byte(),
            true,
        ));
        let mut children: Vec<_> = node.named_children(&mut node.walk()).collect();
        if reversed {
            children.reverse();
        }
        for child in children {
            reference_walk(child, reversed, events);
        }
        events.push((
            node.kind().to_owned(),
            node.start_byte(),
            node.end_byte(),
            false,
        ));
    }

    #[test]
    fn tree_cursor_walk_preserves_named_node_events_in_both_directions() {
        let source =
            "import os\nx = call(1, [2, 3])\ndef f(a):\n    if a:\n        return {'x': a}\n";
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&languages::python::language())
            .expect("Python grammar");
        let tree = parser.parse(source, None).expect("Python syntax tree");
        let root = tree.root_node();

        for reversed in [false, true] {
            let mut expected = Vec::new();
            reference_walk(root, reversed, &mut expected);
            let mut actual = Vec::new();
            walk_in_order(root, reversed, &mut |node, entering| {
                actual.push((
                    node.kind().to_owned(),
                    node.start_byte(),
                    node.end_byte(),
                    entering,
                ));
            });
            assert_eq!(actual, expected, "reversed={reversed}");
        }
    }
}
