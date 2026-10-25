use super::{relations, text, Syntax};
use crate::core::models::{Edge, Facts, Node, Reference};
use crate::engine::languages;
use languages::manifests::framework::RoutePrefixRule;
use serde_json::json;
use std::collections::{HashMap, HashSet};

fn literal<'a>(node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
    if !matches!(node.kind(), "string" | "string_literal") {
        return None;
    }
    let value = text(node, source);
    let quote = value.as_bytes().first().copied()?;
    if !matches!(quote, b'\'' | b'"') || value.len() < 2 || value.as_bytes().last() != Some(&quote)
    {
        return None;
    }
    Some(&value[1..value.len() - 1])
}

fn callee<'source>(node: Syntax<'_>, source: &'source str) -> Option<&'source str> {
    node.child_by_field_name("function")
        .map(|function| text(function, source))
}

fn callee_matches(actual: &str, configured: &str) -> bool {
    actual == configured || (configured.starts_with('.') && actual.ends_with(configured))
}

fn positional_argument<'tree>(
    node: Syntax<'tree>,
    source: &str,
    index: usize,
) -> Option<Syntax<'tree>> {
    let arguments = node.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let result = arguments
        .named_children(&mut cursor)
        .filter(|child| !matches!(child.kind(), "keyword_argument" | "comment"))
        .nth(index)
        .filter(|argument| !text(*argument, source).is_empty());
    result
}

fn positional_arguments<'tree>(node: Syntax<'tree>) -> Vec<Syntax<'tree>> {
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let mut cursor = arguments.walk();
    arguments
        .named_children(&mut cursor)
        .filter(|child| !matches!(child.kind(), "keyword_argument" | "comment"))
        .collect()
}

fn router_prefix<'source>(
    value: Syntax<'_>,
    source: &'source str,
    configured: &str,
    keyword: &str,
) -> Option<Option<&'source str>> {
    if !matches!(value.kind(), "call" | "call_expression")
        || !callee_matches(callee(value, source)?, configured)
    {
        return None;
    }
    let Some(arguments) = value.child_by_field_name("arguments") else {
        return Some(None);
    };
    let mut cursor = arguments.walk();
    for argument in arguments.named_children(&mut cursor) {
        if argument.kind() == "keyword_argument"
            && argument
                .child_by_field_name("name")
                .is_some_and(|name| text(name, source) == keyword)
        {
            return Some(
                argument
                    .child_by_field_name("value")
                    .and_then(|value| literal(value, source)),
            );
        }
    }
    Some(Some(""))
}

fn controller_prefix<'source>(
    node: Syntax<'_>,
    source: &'source str,
    configured: &str,
    argument: usize,
) -> Option<Option<&'source str>> {
    let mut matches = relations::decorators(node)
        .into_iter()
        .filter_map(|decorator| {
            let call = decorator
                .named_child(0)
                .filter(|child| matches!(child.kind(), "call" | "call_expression"))?;
            if !callee_matches(callee(call, source)?, configured) {
                return None;
            }
            Some(match positional_argument(call, source, argument) {
                Some(value) => literal(value, source),
                None => Some(""),
            })
        });
    let first = matches.next()?;
    Some(if matches.next().is_some() {
        None
    } else {
        first
    })
}

/// Per-file route prefixes discovered from framework constructors and class decorators.
#[doc(hidden)]
#[derive(Debug, Default)]
pub struct RouteContext {
    main_walk: bool,
    root: usize,
    providers: Vec<(RoutePrefixRule, PrefixBindings)>,
    lexical: LexicalBindings,
    prepared_bindings: HashMap<usize, super::ScopeBindings>,
}

#[derive(Debug, Default)]
struct LexicalBindings {
    root: usize,
    #[cfg(feature = "lang-python")]
    python_module: bool,
    scopes: HashMap<usize, HashSet<String>>,
    #[cfg(feature = "lang-python")]
    python_writes: HashMap<usize, languages::python::BindingWrites>,
}

impl LexicalBindings {
    #[cfg(feature = "lang-python")]
    fn python_scope<'tree>(&self, node: Syntax<'tree>, name: &str) -> Option<Syntax<'tree>> {
        if self.scopes.is_empty() {
            return None;
        }
        languages::python::lexical_binding_owner_with(node, |scope| {
            self.scopes
                .get(&scope.id())
                .is_some_and(|names| names.contains(name))
                || self
                    .python_writes
                    .get(&scope.id())
                    .is_some_and(|writes| writes.count(name) != 0)
        })
    }

    fn owner(&self, node: Syntax<'_>, name: &str) -> Option<usize> {
        if self.scopes.is_empty() {
            return None;
        }
        #[cfg(feature = "lang-python")]
        if self.python_module {
            return self.python_scope(node, name).map(|owner| owner.id());
        }
        let mut parent = node.parent();
        while let Some(scope) = parent {
            if super::visitor::binding_scope(scope)
                && self
                    .scopes
                    .get(&scope.id())
                    .is_some_and(|names| names.contains(name))
            {
                return Some(scope.id());
            }
            parent = scope.parent();
        }
        Some(self.root)
    }
}

#[derive(Debug, Default)]
struct PrefixBindings {
    routers: HashMap<ReceiverBinding, Option<String>>,
    controllers: HashMap<usize, Option<String>>,
    attachments: HashMap<ReceiverBinding, Vec<(ReceiverBinding, Option<String>)>>,
    writes: HashMap<ReceiverBinding, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReceiverBinding {
    owner: usize,
    name: String,
}

fn receiver_binding(
    lexical: &LexicalBindings,
    node: Syntax<'_>,
    name: &str,
) -> Option<ReceiverBinding> {
    let head = name.split('.').next()?;
    Some(ReceiverBinding {
        owner: lexical.owner(node, head)?,
        name: name.into(),
    })
}

#[cfg(feature = "lang-python")]
fn python_summary_candidates(source: &str, rules: &languages::LanguageContext) -> bool {
    use languages::manifests::{
        projections::{Api, NamedBindingForm},
        syntax::SyntaxOperation,
        SemanticOperation,
    };
    let api_candidate = |api: &Api| match api {
        Api::Imported { module, export } => source.contains(module) && source.contains(export),
        Api::ConstructedMember {
            module,
            member,
            constructors,
        } => {
            source.contains(module)
                && source.contains(member)
                && constructors.iter().any(|export| source.contains(export))
        }
        Api::Global { .. } | Api::GlobalMember { .. } => true,
    };
    (source.contains('@')
        && rules.route_decorators.iter().any(|rule| {
            source.contains(&rule.callee)
                && (rule.callee.contains('.') || !rule.prefixes.is_empty())
        }))
        || rules.route_calls.iter().any(|rule| {
            source.contains(&rule.callee)
                && (rule.callee.contains('.') || !rule.prefixes.is_empty())
        })
        || rules
            .semantic_rules
            .iter()
            .filter(|rule| rule.active)
            .any(|rule| match &rule.rule.operation {
                SemanticOperation::SymbolAttachment {
                    language, callee, ..
                } if language == "python" => api_candidate(callee),
                SemanticOperation::EndpointProjection {
                    language,
                    callee,
                    attachment,
                    ..
                } if language == "python" => {
                    api_candidate(callee) && api_candidate(&attachment.callee)
                }
                _ => false,
            })
        || rules
            .syntax_rules
            .iter()
            .filter(|rule| rule.language == "python")
            .any(|rule| match &rule.operation {
                SyntaxOperation::NamedBinding { binding, .. } => match binding {
                    NamedBindingForm::CallReference { callee, .. } => api_candidate(callee),
                    NamedBindingForm::AssignmentDefinition { member, .. } => {
                        source.contains(member)
                    }
                    _ => true,
                },
                _ => false,
            })
}

impl RouteContext {
    pub(crate) fn prepare(
        root: Syntax<'_>,
        source: &str,
        rules: &languages::LanguageContext,
    ) -> Self {
        #[cfg(feature = "lang-python")]
        if root.kind() == "module"
            && *root.language() == languages::python::language()
            && root.parent().is_none()
            && !root.has_error()
        {
            return if python_summary_candidates(source, rules) {
                Self {
                    main_walk: true,
                    ..Self::default()
                }
            } else {
                Self::build_with_python_admission(root, source, rules, Some(false))
            };
        }
        Self::build(root, source, rules)
    }

    pub(crate) fn uses_main_walk(&self) -> bool {
        self.main_walk
    }

    fn observe(&mut self, node: Syntax<'_>, source: &str, track_bare_writes: bool) {
        let assignment = matches!(node.kind(), "assignment" | "variable_declarator")
            .then(|| {
                let name = node
                    .child_by_field_name("left")
                    .or_else(|| node.child_by_field_name("name"))?;
                let value = node
                    .child_by_field_name("right")
                    .or_else(|| node.child_by_field_name("value"))?;
                Some((text(name, source), value, std::cell::OnceCell::new()))
            })
            .flatten();
        for (rule, bindings) in &mut self.providers {
            if let Some((name, value, binding)) = &assignment {
                let track_writes = track_bare_writes || name.contains('.');
                let prefix = if let RoutePrefixRule::Constructor { callee, keyword } = rule {
                    router_prefix(*value, source, callee, keyword)
                } else {
                    None
                };
                if track_writes || prefix.is_some() {
                    if let Some(binding) =
                        binding.get_or_init(|| receiver_binding(&self.lexical, node, name))
                    {
                        if track_writes {
                            *bindings.writes.entry(binding.clone()).or_default() += 1;
                        }
                        if let Some(prefix) = prefix {
                            bindings
                                .routers
                                .insert(binding.clone(), prefix.map(str::to_owned));
                        }
                    }
                }
            }
            if let RoutePrefixRule::Attachment {
                callee: configured,
                child_argument,
                prefix_keyword,
            } = rule
            {
                if matches!(node.kind(), "call" | "call_expression") {
                    if let Some(actual) =
                        callee(node, source).filter(|actual| callee_matches(actual, configured))
                    {
                        if let (Some((parent, _)), Some(child)) = (
                            actual.rsplit_once('.'),
                            positional_argument(node, source, *child_argument)
                                .filter(|child| child.kind() == "identifier"),
                        ) {
                            let (Some(child), Some(parent)) = (
                                receiver_binding(&self.lexical, node, text(child, source)),
                                receiver_binding(&self.lexical, node, parent),
                            ) else {
                                continue;
                            };
                            let prefix = router_prefix(node, source, configured, prefix_keyword)
                                .flatten()
                                .map(str::to_owned);
                            bindings
                                .attachments
                                .entry(child)
                                .or_default()
                                .push((parent, prefix));
                        }
                    }
                }
            }
            if matches!(node.kind(), "class_definition" | "class_declaration") {
                if let RoutePrefixRule::ClassDecorator { callee, argument } = rule {
                    if let Some(prefix) = controller_prefix(node, source, callee, *argument) {
                        bindings
                            .controllers
                            .insert(node.id(), prefix.map(str::to_owned));
                    }
                }
            }
        }
    }

    pub(crate) fn build(
        root: Syntax<'_>,
        source: &str,
        rules: &languages::LanguageContext,
    ) -> Self {
        Self::build_with_python_admission(root, source, rules, None)
    }

    fn build_with_python_admission(
        root: Syntax<'_>,
        source: &str,
        rules: &languages::LanguageContext,
        python_admission: Option<bool>,
    ) -> Self {
        let mut builder = match python_admission {
            None => RouteContextBuilder::new(root, source, rules),
            Some(admitted) => {
                RouteContextBuilder::new_with_python_admission(root, source, rules, Some(admitted))
            }
        };
        if builder.active {
            super::visitor::walk(root, &mut |node, entering| {
                let _ = builder.observe(node, entering);
            });
        }
        builder.finish()
    }

    /// Resolves an identifier using completed summaries and language lexical boundaries.
    pub(crate) fn lexical_owner(&self, node: Syntax<'_>, name: &str) -> Option<usize> {
        self.lexical.owner(node, name)
    }

    #[cfg(feature = "lang-python")]
    pub(crate) fn lexical_scope<'tree>(
        &self,
        node: Syntax<'tree>,
        name: &str,
    ) -> Option<Syntax<'tree>> {
        if self.lexical.python_module {
            return self.lexical.python_scope(node, name);
        }
        let owner = self.lexical.owner(node, name)?;
        let mut ancestor = node.parent();
        while let Some(scope) = ancestor {
            if scope.id() == owner {
                return Some(scope);
            }
            ancestor = scope.parent();
        }
        None
    }

    /// Moves raw Python summaries into the consuming traversal without retaining a copy.
    pub(super) fn take_python_bindings(&mut self) -> HashMap<usize, super::ScopeBindings> {
        std::mem::take(&mut self.prepared_bindings)
    }

    /// Returns completed Python write multiplicity, including wildcard uncertainty.
    #[cfg(feature = "lang-python")]
    pub(crate) fn lexical_write_count(&self, owner: usize, name: &str) -> usize {
        self.lexical
            .python_writes
            .get(&owner)
            .map_or(0, |writes| writes.count(name))
    }

    fn prefix_for(
        &self,
        node: Syntax<'_>,
        _source: &str,
        route_callee: &str,
        allowed: &[RoutePrefixRule],
    ) -> Result<Option<String>, ()> {
        let mut ancestor = self
            .providers
            .iter()
            .any(|(rule, bindings)| allowed.contains(rule) && !bindings.controllers.is_empty())
            .then_some(node);
        while let Some(current) = ancestor {
            let prefixes: Vec<_> = self
                .providers
                .iter()
                .filter(|(rule, _)| allowed.contains(rule))
                .filter_map(|(_, bindings)| bindings.controllers.get(&current.id()))
                .collect();
            if !prefixes.is_empty() {
                return match prefixes.as_slice() {
                    [Some(prefix)] => Ok(Some(prefix.clone())),
                    _ => Err(()),
                };
            }
            ancestor = current.parent();
        }
        let Some((receiver, _)) = route_callee.rsplit_once('.') else {
            return Ok(None);
        };
        let Some(binding) = receiver_binding(&self.lexical, node, receiver) else {
            return Err(());
        };
        if !self
            .providers
            .iter()
            .filter(|(rule, _)| allowed.contains(rule))
            .any(|(_, bindings)| bindings.routers.contains_key(&binding))
        {
            if binding.owner != self.root
                || self
                    .providers
                    .iter()
                    .filter(|(rule, _)| allowed.contains(rule))
                    .any(|(_, bindings)| bindings.routers.keys().any(|key| key.name == receiver))
            {
                return Err(());
            }
            return Ok(None);
        }
        self.receiver_prefix(&binding, allowed, &mut Vec::new())
            .map(Some)
    }

    fn receiver_prefix(
        &self,
        receiver: &ReceiverBinding,
        allowed: &[RoutePrefixRule],
        visited: &mut Vec<ReceiverBinding>,
    ) -> Result<String, ()> {
        if visited.len() >= 32 || visited.iter().any(|name| name == receiver) {
            return Err(());
        }
        visited.push(receiver.clone());
        let prefixes: Vec<_> = self
            .providers
            .iter()
            .filter(|(rule, _)| allowed.contains(rule))
            .filter_map(|(_, bindings)| bindings.routers.get(receiver))
            .collect();
        let prefix = match prefixes.as_slice() {
            [Some(prefix)] => prefix.clone(),
            _ => return Err(()),
        };
        let attachments: Vec<_> = self
            .providers
            .iter()
            .filter(|(rule, _)| allowed.contains(rule))
            .flat_map(|(_, bindings)| bindings.attachments.get(receiver).into_iter().flatten())
            .collect();
        match attachments.as_slice() {
            [] => Ok(prefix),
            [(parent, Some(attached))] => {
                let root = self.receiver_prefix(parent, allowed, visited)?;
                Ok(prefixed_path(
                    Some(&prefixed_path(Some(&root), attached)),
                    &prefix,
                ))
            }
            _ => Err(()),
        }
    }

    fn is_router_receiver(
        &self,
        node: Syntax<'_>,
        _source: &str,
        route_callee: &str,
        allowed: &[RoutePrefixRule],
    ) -> bool {
        let Some((receiver, _)) = route_callee.rsplit_once('.') else {
            return false;
        };
        let Some(binding) = receiver_binding(&self.lexical, node, receiver) else {
            return false;
        };
        self.providers
            .iter()
            .filter(|(rule, _)| allowed.contains(rule))
            .any(|(_, bindings)| bindings.routers.contains_key(&binding))
    }
}

pub(super) fn prefixed_path(prefix: Option<&str>, path: &str) -> String {
    let Some(prefix) = prefix.filter(|prefix| !prefix.is_empty()) else {
        return path.to_owned();
    };
    let leading_slash = prefix.starts_with('/') || path.starts_with('/');
    let trailing_slash = path.ends_with('/');
    let mut joined = format!("{}/{}", prefix.trim_matches('/'), path.trim_matches('/'));
    if leading_slash {
        joined.insert(0, '/');
    }
    if trailing_slash && !joined.ends_with('/') {
        joined.push('/');
    }
    joined
}

fn in_routes_collection(node: Syntax<'_>, source: &str, collection: &str) -> bool {
    let Some(array) = node.parent().filter(|parent| parent.kind() == "array") else {
        return false;
    };
    let Some(container) = array.parent() else {
        return false;
    };
    let name = match container.kind() {
        "variable_declarator" => container.child_by_field_name("name"),
        "pair" => container.child_by_field_name("key"),
        _ => None,
    };
    name.is_some_and(|name| text(name, source).trim_matches(['\'', '"']) == collection)
}

fn rule_methods(
    call: Syntax<'_>,
    source: &str,
    keyword: Option<&str>,
    method: &str,
) -> Vec<String> {
    if let Some(keyword_name) = keyword {
        if let Some(arguments) = call.child_by_field_name("arguments") {
            let mut cursor = arguments.walk();
            for keyword in arguments.named_children(&mut cursor) {
                if keyword.kind() == "keyword_argument"
                    && keyword
                        .child_by_field_name("name")
                        .is_some_and(|name| text(name, source) == keyword_name)
                {
                    if let Some(values) = keyword.child_by_field_name("value") {
                        let mut cursor = values.walk();
                        return values
                            .named_children(&mut cursor)
                            .filter_map(|value| literal(value, source))
                            .map(str::to_ascii_uppercase)
                            .collect();
                    }
                }
            }
        }
    }
    vec![method.to_owned()]
}

fn route_receiver_matches(
    node: Syntax<'_>,
    source: &str,
    actual_callee: &str,
    configured_receivers: &[String],
    route_context: &RouteContext,
    prefixes: &[RoutePrefixRule],
) -> bool {
    if configured_receivers.is_empty() {
        return true;
    }
    let Some((receiver, _)) = actual_callee.rsplit_once('.') else {
        return false;
    };
    configured_receivers.iter().any(|name| {
        receiver == name || receiver.rsplit('.').next().is_some_and(|last| last == name)
    }) || route_context.is_router_receiver(node, source, actual_callee, prefixes)
}

fn route_path_is_allowed(path: &str, allow_relative_path: bool) -> bool {
    allow_relative_path || path.starts_with(['/', '^'])
}

struct Route<'a> {
    origins: Vec<crate::core::semantic::ExtractionRuleOrigin>,
    syntax: Syntax<'a>,
    path: &'a str,
    method: String,
    handler: Option<&'a str>,
    direct: bool,
    route_name: Option<String>,
    include_module: Option<String>,
    include_namespace: Option<String>,
}

fn route_origins(
    origin: Option<crate::core::semantic::ExtractionRuleOrigin>,
    guard: Option<&str>,
) -> Vec<crate::core::semantic::ExtractionRuleOrigin> {
    let mut origins: Vec<_> = origin.into_iter().collect();
    if let Some((pack, rule)) =
        guard.and_then(|key| serde_json::from_str::<(String, String)>(key).ok())
    {
        origins.push(crate::core::semantic::ExtractionRuleOrigin { pack, rule });
    }
    origins
}

fn add(route: Route<'_>, owner: &str, facts: &mut Facts, offset: usize) {
    let Some(parent) = facts.nodes.iter().find(|n| n.id == owner).cloned() else {
        return;
    };
    let line = route.syntax.start_position().row + offset + 1;
    let name = format!("{} {}", route.method, route.path);
    let id = format!(
        "route:{}:{line}:{}:{}",
        parent.path,
        route.syntax.start_position().column,
        route.method
    );
    if let Some(existing) = facts.nodes.iter_mut().find(|n| n.id == id) {
        if let (Some(handler), Some(handlers)) =
            (route.handler, existing.details["handlers"].as_array_mut())
        {
            handlers.push(json!(handler));
        }
    } else {
        facts.pending_flow_nodes.push(facts.nodes.len());
        facts.nodes.push(Node {
    native_semantic: None,
        id: id.clone(),
        kind: "route".into(),
        name: name.clone(),
        qualname: format!("{}.{name}", parent.qualname),
        path: parent.path.clone(),
        line,
        end_line: route.syntax.end_position().row + offset + 1,
        is_test: parent.is_test,
        language: parent.language.clone(),
        generated: false,
        details: json!({"method":route.method,"route_path":route.path,"handler":route.handler,"handlers":route.handler.into_iter().collect::<Vec<_>>(),"route_name":route.route_name,"include_module":route.include_module,"include_namespace":route.include_namespace,"value_flow":crate::core::semantic::ValueFlowFacts { framework: Some(Box::new(crate::core::semantic::FrameworkScopeFacts { facts: route.origins.into_iter().map(crate::core::semantic::FrameworkFact::RuleOrigin).collect(), ..Default::default() })), ..Default::default() }}),
    });
        facts.edges.push(Edge {
            src: format!("module:{}", parent.path),
            dst: id.clone(),
            kind: "contains".into(),
            path: parent.path.clone(),
            line,
            evidence: "route registration".into(),
            confidence: "exact".into(),
        });
    }
    if let Some(handler) = route.handler {
        if route.direct {
            facts.edges.push(Edge {
                src: id,
                dst: handler.into(),
                kind: "handles".into(),
                path: parent.path,
                line,
                evidence: "handler syntax identity".into(),
                confidence: "exact".into(),
            });
        } else {
            relations::reference(facts, &id, handler, "handles", line);
        }
    }
}

#[cfg(feature = "lang-python")]
pub(crate) fn declaration_candidate(
    ctx: &languages::SyntaxContext<'_, '_>,
    rule_ir: &languages::manifests::RuleIRHandle,
) -> bool {
    relations::decorators(ctx.node)
        .into_iter()
        .any(|decorator| {
            let Some(call) = decorator
                .named_child(0)
                .filter(|call| call.kind() == "call")
            else {
                return false;
            };
            let Some(actual) = callee(call, ctx.source) else {
                return false;
            };
            rule_ir
                .route_decorators_for_kind(call.kind_id())
                .iter()
                .any(|rule| {
                    callee_matches(actual, &rule.callee)
                        && positional_argument(call, ctx.source, rule.path_argument)
                            .and_then(|argument| literal(argument, ctx.source))
                            .is_some_and(|path| {
                                route_path_is_allowed(path, rule.allow_relative_path)
                            })
                })
        })
}

pub(crate) fn declaration_with_context(
    ctx: &languages::SyntaxContext<'_, '_>,
    facts: &mut Facts,
    rule_ir: &languages::manifests::RuleIRHandle,
    route_context: &RouteContext,
) {
    let (node, source, owner, offset) = (ctx.node, ctx.source, ctx.owner, ctx.offset);
    for decorator in relations::decorators(node) {
        let Some(call) = decorator
            .named_child(0)
            .filter(|n| matches!(n.kind(), "call" | "call_expression"))
        else {
            continue;
        };
        let rules = rule_ir.route_decorators_for_kind(call.kind_id());
        if rules.is_empty() {
            continue;
        }
        let Some(actual_callee) = callee(call, source) else {
            continue;
        };
        for rule in rules
            .iter()
            .filter(|rule| callee_matches(actual_callee, &rule.callee))
        {
            let Ok(prefix) = route_context.prefix_for(node, source, actual_callee, &rule.prefixes)
            else {
                continue;
            };
            if !route_receiver_matches(
                node,
                source,
                actual_callee,
                &rule.receiver_names,
                route_context,
                &rule.prefixes,
            ) {
                continue;
            }
            let Some(path) = positional_argument(call, source, rule.path_argument)
                .and_then(|argument| literal(argument, source))
            else {
                continue;
            };
            if !route_path_is_allowed(path, rule.allow_relative_path) {
                continue;
            }
            let path = prefixed_path(prefix.as_deref(), path);
            for method in rule_methods(call, source, rule.methods_keyword.as_deref(), &rule.method)
            {
                add(
                    Route {
                        syntax: decorator,
                        origins: route_origins(rule.origin.clone(), None),
                        path: &path,
                        method,
                        handler: Some(owner),
                        direct: true,
                        route_name: None,
                        include_module: None,
                        include_namespace: None,
                    },
                    owner,
                    facts,
                    offset,
                );
            }
        }
    }
}

pub(crate) fn registration_with_context(
    ctx: &languages::SyntaxContext<'_, '_>,
    facts: &mut Facts,
    symbols: &HashMap<usize, String>,
    rule_ir: &languages::manifests::RuleIRHandle,
    route_context: &RouteContext,
) {
    registration_with_context_filtered(ctx, facts, symbols, rule_ir, route_context, |_, _| true);
}

pub(crate) fn registration_with_context_filtered(
    ctx: &languages::SyntaxContext<'_, '_>,
    facts: &mut Facts,
    symbols: &HashMap<usize, String>,
    rule_ir: &languages::manifests::RuleIRHandle,
    route_context: &RouteContext,
    admit: impl Fn(&languages::manifests::RouteCallRule, &Facts) -> bool,
) {
    let (node, source, owner, offset) = (ctx.node, ctx.source, ctx.owner, ctx.offset);
    if matches!(node.kind(), "call" | "call_expression") {
        let rules = rule_ir.route_calls_for_kind(ctx.node.kind_id());
        if rules.is_empty() {
            return;
        }
        let Some(actual_callee) = callee(node, source) else {
            return;
        };
        let mut matching = rules
            .iter()
            .filter(|rule| callee_matches(actual_callee, &rule.callee))
            .peekable();
        if matching.peek().is_none() || node.parent().is_some_and(|p| p.kind() == "decorator") {
            return;
        }
        for rule in matching {
            let Some(path) = positional_argument(node, source, rule.path_argument)
                .and_then(|argument| literal(argument, source))
            else {
                continue;
            };
            if !route_path_is_allowed(path, rule.allow_relative_path) || !admit(rule, facts) {
                continue;
            }
            let Ok(prefix) = route_context.prefix_for(node, source, actual_callee, &rule.prefixes)
            else {
                continue;
            };
            if !route_receiver_matches(
                node,
                source,
                actual_callee,
                &rule.receiver_names,
                route_context,
                &rule.prefixes,
            ) {
                continue;
            }
            let path = prefixed_path(prefix.as_deref(), path);
            let methods = rule_methods(node, source, rule.methods_keyword.as_deref(), &rule.method);
            let handlers = rule
                .handler_argument
                .map(|first| {
                    positional_arguments(node)
                        .into_iter()
                        .skip(first)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if rule.handler_argument.is_some() && handlers.is_empty() {
                continue;
            }
            let route_name = node
                .child_by_field_name("arguments")
                .and_then(|arguments| {
                    let mut cursor = arguments.walk();
                    let argument = arguments.named_children(&mut cursor).find(|argument| {
                        argument.kind() == "keyword_argument"
                            && argument.child_by_field_name("name").is_some_and(|name| {
                                Some(text(name, source)) == rule.name_keyword.as_deref()
                            })
                    });
                    argument
                })
                .and_then(|argument| argument.child_by_field_name("value"))
                .and_then(|value| literal(value, source))
                .map(str::to_owned);
            if handlers.is_empty() {
                for method in methods {
                    add(
                        Route {
                            syntax: node,
                            origins: route_origins(
                                rule.origin.clone(),
                                rule.registration_guard.as_deref(),
                            ),
                            path: &path,
                            method,
                            handler: None,
                            direct: false,
                            route_name: route_name.clone(),
                            include_module: None,
                            include_namespace: None,
                        },
                        owner,
                        facts,
                        offset,
                    );
                }
            } else {
                for method in methods {
                    for handler in &handlers {
                        let handler = unwrapped(*handler);
                        if literal_handler(handler) {
                            continue;
                        }
                        let direct = symbols.get(&handler.id());
                        let (include_module, include_namespace) =
                            include_target(handler, source, rule.include.as_ref());
                        add(
                            Route {
                                syntax: node,
                                origins: route_origins(
                                    rule.origin.clone(),
                                    rule.registration_guard.as_deref(),
                                ),
                                path: &path,
                                method: method.clone(),
                                handler: Some(
                                    direct
                                        .map(String::as_str)
                                        .unwrap_or_else(|| text(handler, source)),
                                ),
                                direct: direct.is_some(),
                                route_name: route_name.clone(),
                                include_module,
                                include_namespace,
                            },
                            owner,
                            facts,
                            offset,
                        );
                    }
                }
            }
        }
    }
}

pub(crate) fn object_registration(
    ctx: &languages::SyntaxContext<'_, '_>,
    facts: &mut Facts,
    symbols: &HashMap<usize, String>,
    rules: &languages::LanguageContext,
    language: &str,
) {
    let (node, source, owner, offset) = (ctx.node, ctx.source, ctx.owner, ctx.offset);
    for rule in &rules.syntax_rules {
        if rule.language != language {
            continue;
        }
        let languages::manifests::syntax::SyntaxOperation::RouteObject {
            collection,
            path_key,
            handler_key,
            method,
        } = &rule.operation
        else {
            continue;
        };
        if node.kind() == "object" && in_routes_collection(node, source, collection) {
            let mut path = None;
            let mut handler = None;
            let mut c = node.walk();
            for pair in node.named_children(&mut c).filter(|n| n.kind() == "pair") {
                let (Some(key), Some(value)) = (
                    pair.child_by_field_name("key"),
                    pair.child_by_field_name("value"),
                ) else {
                    continue;
                };
                let key = text(key, source).trim_matches(['\'', '"']);
                if key == path_key {
                    path = literal(value, source);
                }
                if key == handler_key {
                    handler = Some(unwrapped(value));
                }
            }
            if let (Some(path), Some(handler)) = (path, handler) {
                if literal_handler(handler) {
                    return;
                }
                add(
                    Route {
                        syntax: node,
                        origins: vec![crate::core::semantic::ExtractionRuleOrigin {
                            pack: rule.pack.clone(),
                            rule: rule.id.clone(),
                        }],
                        path,
                        method: method.clone(),
                        handler: Some(
                            symbols
                                .get(&handler.id())
                                .map(String::as_str)
                                .unwrap_or_else(|| text(handler, source)),
                        ),
                        direct: symbols.contains_key(&handler.id()),
                        route_name: None,
                        include_module: None,
                        include_namespace: None,
                    },
                    owner,
                    facts,
                    offset,
                );
            }
        }
    }
}

fn include_target(
    node: Syntax<'_>,
    source: &str,
    rule: Option<&languages::manifests::framework::RouteIncludeRule>,
) -> (Option<String>, Option<String>) {
    let Some(rule) = rule else {
        return (None, None);
    };
    if !matches!(node.kind(), "call" | "call_expression")
        || callee(node, source).is_none_or(|callee| {
            !callee_matches(callee, &rule.callee) && callee != rule.callee.trim_start_matches('.')
        })
    {
        return (None, None);
    }
    let module =
        positional_argument(node, source, rule.argument).and_then(|value| literal(value, source));
    let namespace = node
        .child_by_field_name("arguments")
        .and_then(|arguments| {
            let mut cursor = arguments.walk();
            let argument = arguments.named_children(&mut cursor).find(|argument| {
                argument.kind() == "keyword_argument"
                    && argument
                        .child_by_field_name("name")
                        .is_some_and(|name| text(name, source) == rule.namespace_keyword)
            });
            argument
        })
        .and_then(|argument| argument.child_by_field_name("value"))
        .and_then(|value| literal(value, source));
    (module.map(str::to_owned), namespace.map(str::to_owned))
}

pub(crate) fn dependency_injection(
    ctx: &languages::SyntaxContext<'_, '_>,
    rule_ir: &languages::manifests::RuleIRHandle,
    facts: &mut Facts,
) {
    if !matches!(ctx.node.kind(), "call" | "call_expression") {
        return;
    }
    let di_rules = rule_ir.di_calls_for_kind(ctx.node.kind_id());
    if di_rules.is_empty() {
        return;
    }
    let Some(actual_callee) = callee(ctx.node, ctx.source) else {
        return;
    };
    for rule in di_rules
        .iter()
        .filter(|rule| callee_matches(actual_callee, &rule.callee))
    {
        let Some(argument) = positional_argument(ctx.node, ctx.source, rule.argument) else {
            continue;
        };
        let expression = text(argument, ctx.source).trim();
        if expression.is_empty() {
            continue;
        }
        facts.references.push(Reference {
            source: ctx.owner.to_owned(),
            dynamic: false,
            expression: expression.to_owned(),
            kind: "injects".to_owned(),
            line: ctx.line(),
            column: argument.start_position().column,
            alias: None,
            module: None,
            receiver_hint: rule
                .origin
                .clone()
                .map(|origin| crate::core::models::ReceiverHint::ConfiguredReference { origin }),
        });
    }
}

fn literal_handler(node: Syntax<'_>) -> bool {
    matches!(
        node.kind(),
        "string"
            | "string_literal"
            | "concatenated_string"
            | "template_string"
            | "integer"
            | "float"
            | "number"
            | "true"
            | "false"
            | "none"
            | "null"
            | "array"
            | "list"
            | "tuple"
            | "set"
            | "dictionary"
            | "object"
    ) || (matches!(node.kind(), "unary_operator" | "unary_expression")
        && node
            .named_child(0)
            .is_some_and(|child| literal_handler(unwrapped(child))))
}

fn unwrapped(mut node: Syntax<'_>) -> Syntax<'_> {
    while matches!(
        node.kind(),
        "parenthesized_expression" | "as_expression" | "type_assertion" | "non_null_expression"
    ) {
        let mut cursor = node.walk();
        let inner = if node.kind() == "type_assertion" {
            node.child_by_field_name("expression").or_else(|| {
                node.named_children(&mut cursor)
                    .find(|child| child.kind() != "type_arguments")
            })
        } else {
            node.child_by_field_name("expression")
                .or_else(|| node.named_child(0))
        };
        let Some(inner) = inner else {
            break;
        };
        node = inner;
    }
    node
}

pub(crate) fn client_call(
    ctx: &languages::SyntaxContext<'_, '_>,
    facts: &mut Facts,
    rules: &languages::LanguageContext,
    language: &str,
) {
    use languages::manifests::syntax::SyntaxOperation;
    let (node, source) = (ctx.node, ctx.source);
    if !matches!(node.kind(), "call" | "call_expression") {
        return;
    }
    let Some(actual) = callee(node, source) else {
        return;
    };
    for rule in &rules.syntax_rules {
        if rule.language != language {
            continue;
        }
        let SyntaxOperation::HttpClientCall {
            callee,
            receivers,
            methods,
            default_method,
            url_key,
            method_key,
            relative_prefixes,
            ..
        } = &rule.operation
        else {
            continue;
        };
        let direct = !callee.is_empty() && actual == callee;
        let default = if direct {
            default_method.as_str()
        } else {
            let Some((receiver, member)) = actual.rsplit_once('.') else {
                continue;
            };
            let name = receiver.rsplit('.').next().unwrap_or(receiver).trim();
            if !receivers.iter().any(|allowed| allowed == name) {
                continue;
            }
            let Some(method) = methods.get(member.trim()) else {
                continue;
            };
            method.as_str()
        };
        let Some(args) = node.child_by_field_name("arguments") else {
            continue;
        };
        let Some(first) = args.named_child(0) else {
            continue;
        };
        let url = literal(first, source).or_else(|| object_literal(first, source, url_key, false));
        let Some(url) = url.filter(|url| {
            relative_prefixes
                .iter()
                .any(|prefix| url.starts_with(prefix))
        }) else {
            continue;
        };
        let method = if direct {
            args.named_child(1)
                .and_then(|options| object_literal(options, source, method_key, true))
                .map(str::to_ascii_uppercase)
                .filter(|value| methods.values().any(|allowed| allowed == value))
                .unwrap_or_else(|| default.to_owned())
        } else {
            default.to_owned()
        };
        let reference = Reference {
            source: ctx.owner.into(),
            expression: format!("{method} {url}"),
            kind: "calls_endpoint".into(),
            line: ctx.line(),
            column: node.start_position().column,
            alias: None,
            module: None,
            dynamic: false,
            receiver_hint: Some(crate::core::models::ReceiverHint::ConfiguredReference {
                origin: crate::core::semantic::ExtractionRuleOrigin {
                    pack: rule.pack.clone(),
                    rule: rule.id.clone(),
                },
            }),
        };
        if !facts.references.iter().any(|existing| {
            existing.source == reference.source
                && existing.line == reference.line
                && existing.column == reference.column
                && existing.kind == reference.kind
                && existing.expression == reference.expression
        }) {
            facts.references.push(reference);
        }
    }
}

fn object_literal<'a>(
    node: Syntax<'_>,
    source: &'a str,
    key: &str,
    ignore_case: bool,
) -> Option<&'a str> {
    if node.kind() != "object" {
        return None;
    }
    for pair in node
        .named_children(&mut node.walk())
        .filter(|node| node.kind() == "pair")
    {
        let name = text(pair.child_by_field_name("key")?, source).trim_matches(['\'', '"']);
        if name == key || (ignore_case && name.eq_ignore_ascii_case(key)) {
            return literal(pair.child_by_field_name("value")?, source);
        }
    }
    None
}

/// The same completed-summary observer can be driven by the existing main walk.
pub(crate) struct RouteContextBuilder<'source, 'tree> {
    root: Syntax<'tree>,
    source: &'source str,
    context: RouteContext,
    active: bool,
    #[cfg(feature = "lang-python")]
    python_root: bool,
    candidates: Vec<Syntax<'tree>>,
    collectors: Vec<(
        Syntax<'tree>,
        super::visitor::BindingCollector<'source, 'tree>,
    )>,
    #[cfg(feature = "lang-python")]
    python_collectors: Vec<(
        Syntax<'tree>,
        languages::python::BindingWriteCollector<'source, 'tree>,
    )>,
}

impl<'source, 'tree> RouteContextBuilder<'source, 'tree> {
    fn inactive(context: RouteContext, root: Syntax<'tree>, source: &'source str) -> Self {
        Self {
            root,
            source,
            context,
            active: false,
            #[cfg(feature = "lang-python")]
            python_root: root.kind() == "module",
            candidates: Vec::new(),
            collectors: Vec::new(),
            #[cfg(feature = "lang-python")]
            python_collectors: Vec::new(),
        }
    }

    pub(crate) fn new(
        root: Syntax<'tree>,
        source: &'source str,
        rules: &languages::LanguageContext,
    ) -> Self {
        Self::new_with_python_admission(root, source, rules, None)
    }

    /// Reuses the Python admission already established by RouteContext::prepare.
    pub(crate) fn for_main_walk(
        root: Syntax<'tree>,
        source: &'source str,
        rules: &languages::LanguageContext,
    ) -> Self {
        Self::new_with_python_admission(root, source, rules, Some(true))
    }

    fn new_with_python_admission(
        root: Syntax<'tree>,
        source: &'source str,
        rules: &languages::LanguageContext,
        python_admission: Option<bool>,
    ) -> Self {
        let mut tree_root = root;
        while let Some(parent) = tree_root.parent() {
            tree_root = parent;
        }
        let mut context = RouteContext {
            root: root.id(),
            lexical: LexicalBindings {
                root: tree_root.id(),
                #[cfg(feature = "lang-python")]
                python_module: tree_root.kind() == "module",
                ..LexicalBindings::default()
            },
            ..RouteContext::default()
        };
        if python_admission == Some(false) {
            return Self::inactive(context, root, source);
        }
        #[cfg(feature = "lang-typescript")]
        if root.kind() == "program"
            && root.parent().is_none()
            && !root.has_error()
            && [
                languages::typescript::language("", "javascript"),
                languages::typescript::language("", "typescript"),
                languages::typescript::language(".tsx", "typescript"),
            ]
            .iter()
            .any(|language| *root.language() == *language)
            && !rules
                .route_decorators
                .iter()
                .map(|rule| (&rule.callee, &rule.prefixes))
                .chain(
                    rules
                        .route_calls
                        .iter()
                        .map(|rule| (&rule.callee, &rule.prefixes)),
                )
                .any(|(callee, prefixes)| {
                    source.contains(callee) && (callee.contains('.') || !prefixes.is_empty())
                })
        {
            // Only matching dotted or prefix-bearing route rows can observe
            // these summaries. Other semantic producers use their own facts.
            return Self::inactive(context, root, source);
        }
        #[cfg(feature = "lang-python")]
        if python_admission.is_none()
            && root.kind() == "module"
            && *root.language() == languages::python::language()
            && root.parent().is_none()
            && !root.has_error()
            && !python_summary_candidates(source, rules)
        {
            // No configured consumer can observe these summaries. The main
            // extractor retains its normal binding collector fallback.
            return Self::inactive(context, root, source);
        }
        for rule in rules
            .route_decorators
            .iter()
            .flat_map(|rule| &rule.prefixes)
            .chain(rules.route_calls.iter().flat_map(|rule| &rule.prefixes))
        {
            if !context
                .providers
                .iter()
                .any(|(existing, _)| existing == rule)
            {
                context
                    .providers
                    .push((rule.clone(), PrefixBindings::default()));
            }
        }
        let semantic_bindings =
            rules.semantic_rules.iter().any(|rule| {
                rule.active
                    && matches!(
                rule.rule.operation,
                languages::manifests::semantic::SemanticOperation::SymbolAttachment { .. }
                    | languages::manifests::semantic::SemanticOperation::EndpointProjection { .. }
            )
            }) || rules.syntax_rules.iter().any(|rule| {
                rule.language == "python"
                    && matches!(
                        rule.operation,
                        languages::manifests::syntax::SyntaxOperation::NamedBinding { .. }
                    )
            });
        if rules.route_decorators.is_empty() && rules.route_calls.is_empty() && !semantic_bindings {
            return Self::inactive(context, root, source);
        }

        let mut builder = Self::inactive(context, root, source);
        builder.active = true;
        builder
    }

    pub(crate) fn observe(&mut self, node: Syntax<'tree>, entering: bool) -> bool {
        if !self.active {
            return false;
        }

        if entering {
            let kind = node.kind();
            let scope = node == self.root || super::visitor::binding_scope_kind(kind);
            if let Some((_, collector)) = self.collectors.last_mut() {
                collector.enter_with_kind(node, kind);
                if scope {
                    collector.exit();
                }
            }
            if scope {
                let mut collector =
                    super::visitor::BindingCollector::new_with_kind(node, self.source, kind);
                collector.enter_with_kind(node, kind);
                self.collectors.push((node, collector));
            }
            #[cfg(feature = "lang-python")]
            if self.python_root {
                let python_scope =
                    matches!(kind, "module" | "function_definition" | "class_definition");
                if let Some((_, collector)) = self.python_collectors.last_mut() {
                    collector.enter(node, kind);
                    if python_scope {
                        collector.exit();
                    }
                }
                if python_scope {
                    let mut collector =
                        languages::python::BindingWriteCollector::new(node, self.source);
                    collector.enter(node, kind);
                    self.python_collectors.push((node, collector));
                }
            }
            if !self.context.providers.is_empty()
                && matches!(
                    kind,
                    "assignment"
                        | "variable_declarator"
                        | "call"
                        | "call_expression"
                        | "class_definition"
                        | "class_declaration"
                )
            {
                self.candidates.push(node);
            }
        } else {
            #[cfg(feature = "lang-python")]
            if self.python_root {
                if self
                    .python_collectors
                    .last()
                    .is_some_and(|(scope_node, _)| *scope_node == node)
                {
                    let (scope_node, collector) = self
                        .python_collectors
                        .pop()
                        .expect("Python scope entry precedes exit");
                    self.context
                        .lexical
                        .python_writes
                        .insert(scope_node.id(), collector.finish());
                } else if let Some((_, collector)) = self.python_collectors.last_mut() {
                    collector.exit();
                }
            }
            if self
                .collectors
                .last()
                .is_some_and(|(scope_node, _)| *scope_node == node)
            {
                let (scope_node, collector) =
                    self.collectors.pop().expect("scope entry precedes exit");
                let bindings = collector.finish();
                #[cfg(feature = "lang-python")]
                let (names, prepared_bindings): (HashSet<_>, bool) =
                    if self.context.lexical.python_module {
                        let writes = self.context.lexical.python_writes.get(&scope_node.id());
                        let names = bindings
                            .all
                            .iter()
                            .filter(|name| writes.is_none_or(|writes| writes.count(name) == 0))
                            .cloned()
                            .collect();
                        self.context
                            .prepared_bindings
                            .insert(scope_node.id(), bindings);
                        (names, true)
                    } else {
                        (bindings.all.into_iter().collect(), false)
                    };
                #[cfg(not(feature = "lang-python"))]
                let (names, prepared_bindings): (HashSet<_>, bool) =
                    (bindings.all.into_iter().collect(), false);
                #[cfg(feature = "lang-python")]
                let names = {
                    let mut names = names;
                    if self.python_root {
                        let writes = self.context.lexical.python_writes.get(&scope_node.id());
                        names.extend(
                            languages::python::lexical_type_parameters(scope_node, self.source)
                                .into_iter()
                                .filter(|name| {
                                    !self.context.lexical.python_module
                                        || writes.is_none_or(|writes| writes.count(name) == 0)
                                }),
                        );
                    }
                    names
                };
                self.context.lexical.scopes.insert(scope_node.id(), names);
                return prepared_bindings;
            } else if let Some((_, collector)) = self.collectors.last_mut() {
                collector.exit();
            }
        }
        false
    }

    pub(crate) fn take_bindings(&mut self, node: Syntax<'_>) -> Option<super::ScopeBindings> {
        self.context.prepared_bindings.remove(&node.id())
    }

    pub(crate) fn finish(mut self) -> RouteContext {
        #[cfg(feature = "lang-python")]
        let track_bare_writes = !self.python_root;
        #[cfg(not(feature = "lang-python"))]
        let track_bare_writes = true;
        for candidate in self.candidates {
            self.context
                .observe(candidate, self.source, track_bare_writes);
        }
        for (_, bindings) in &mut self.context.providers {
            for (name, prefix) in &mut bindings.routers {
                #[cfg(feature = "lang-python")]
                let writes = if self.python_root && !name.name.contains('.') {
                    self.context
                        .lexical
                        .python_writes
                        .get(&name.owner)
                        .map_or(0, |writes| writes.count(&name.name))
                } else {
                    bindings.writes.get(name).copied().unwrap_or_default()
                };
                #[cfg(not(feature = "lang-python"))]
                let writes = bindings.writes.get(name).copied().unwrap_or_default();
                if writes != 1 {
                    *prefix = None;
                }
            }
        }

        self.context
    }
}
