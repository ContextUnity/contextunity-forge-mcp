use super::{relations, text, Syntax};
use crate::core::models::{Edge, Facts, Node, Reference};
use crate::engine::languages::{self, LanguageContext};
use serde_json::json;
use std::collections::HashMap;

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
    positional_arguments(node)
        .into_iter()
        .nth(index)
        .filter(|argument| !text(*argument, source).is_empty())
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

/// Per-file route prefixes discovered from framework constructors and class decorators.
#[doc(hidden)]
#[derive(Debug, Default)]
pub struct RouteContext {
    routers: HashMap<String, Option<String>>,
    controllers: HashMap<usize, String>,
}

impl RouteContext {
    pub(crate) fn build(root: Syntax<'_>, source: &str) -> Self {
        fn visit(node: Syntax<'_>, source: &str, context: &mut RouteContext) {
            if matches!(node.kind(), "assignment" | "variable_declarator") {
                let name = node
                    .child_by_field_name("left")
                    .or_else(|| node.child_by_field_name("name"));
                let value = node
                    .child_by_field_name("right")
                    .or_else(|| node.child_by_field_name("value"));
                if let (Some(name), Some(value)) = (name, value) {
                    if let Some(prefix) = router_prefix(value, source) {
                        context
                            .routers
                            .insert(text(name, source).to_owned(), prefix.map(str::to_owned));
                    }
                }
            }
            if matches!(node.kind(), "class_definition" | "class_declaration") {
                if let Some(prefix) = controller_prefix(node, source) {
                    context.controllers.insert(node.id(), prefix.to_owned());
                }
            }
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                visit(child, source, context);
            }
        }

        fn router_prefix<'source>(
            value: Syntax<'_>,
            source: &'source str,
        ) -> Option<Option<&'source str>> {
            if !matches!(value.kind(), "call" | "call_expression")
                || callee(value, source)? != "APIRouter"
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
                        .is_some_and(|name| text(name, source) == "prefix")
                {
                    return Some(
                        argument
                            .child_by_field_name("value")
                            .and_then(|value| literal(value, source)),
                    );
                }
            }
            Some(None)
        }

        fn controller_prefix<'source>(
            node: Syntax<'_>,
            source: &'source str,
        ) -> Option<&'source str> {
            relations::decorators(node)
                .into_iter()
                .find_map(|decorator| {
                    let call = decorator
                        .named_child(0)
                        .filter(|child| matches!(child.kind(), "call" | "call_expression"))?;
                    (callee(call, source)? == "Controller")
                        .then(|| {
                            positional_argument(call, source, 0)
                                .and_then(|value| literal(value, source))
                        })
                        .flatten()
                })
        }

        let mut context = Self::default();
        visit(root, source, &mut context);
        context
    }

    fn prefix_for(&self, node: Syntax<'_>, route_callee: &str) -> Option<&str> {
        let mut ancestor = Some(node);
        while let Some(current) = ancestor {
            if let Some(prefix) = self.controllers.get(&current.id()) {
                return Some(prefix);
            }
            ancestor = current.parent();
        }
        let (receiver, _) = route_callee.rsplit_once('.')?;
        self.routers
            .get(receiver)
            .or_else(|| {
                receiver
                    .rsplit('.')
                    .next()
                    .and_then(|name| self.routers.get(name))
            })
            .and_then(Option::as_deref)
    }

    fn is_router_receiver(&self, route_callee: &str) -> bool {
        let Some((receiver, _)) = route_callee.rsplit_once('.') else {
            return false;
        };
        self.routers.contains_key(receiver)
            || receiver
                .rsplit('.')
                .next()
                .is_some_and(|name| self.routers.contains_key(name))
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

fn in_routes_collection(node: Syntax<'_>, source: &str) -> bool {
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
    name.is_some_and(|name| text(name, source).trim_matches(['\'', '"']) == "routes")
}

fn rule_methods(call: Syntax<'_>, source: &str, configured: &str, method: &str) -> Vec<String> {
    let is_route = configured.rsplit('.').next() == Some("route");
    if is_route && method == "ANY" {
        if let Some(arguments) = call.child_by_field_name("arguments") {
            let mut cursor = arguments.walk();
            for keyword in arguments.named_children(&mut cursor) {
                if keyword.kind() == "keyword_argument"
                    && keyword
                        .child_by_field_name("name")
                        .is_some_and(|name| text(name, source) == "methods")
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
    actual_callee: &str,
    configured_receivers: &[String],
    route_context: &RouteContext,
) -> bool {
    if configured_receivers.is_empty() {
        return true;
    }
    let Some((receiver, _)) = actual_callee.rsplit_once('.') else {
        return false;
    };
    configured_receivers.iter().any(|name| {
        receiver == name || receiver.rsplit('.').next().is_some_and(|last| last == name)
    }) || route_context.is_router_receiver(actual_callee)
}

fn route_path_is_allowed(path: &str, allow_relative_path: bool) -> bool {
    allow_relative_path || path.starts_with(['/', '^'])
}

struct Route<'a> {
    syntax: Syntax<'a>,
    path: &'a str,
    method: String,
    handler: Option<&'a str>,
    direct: bool,
    route_name: Option<String>,
    include_module: Option<String>,
    include_namespace: Option<String>,
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
        facts.nodes.push(Node {
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
        details: json!({"method":route.method,"route_path":route.path,"handler":route.handler,"handlers":route.handler.into_iter().collect::<Vec<_>>(),"route_name":route.route_name,"include_module":route.include_module,"include_namespace":route.include_namespace}),
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

pub(crate) fn declaration_with_context(
    ctx: &languages::SyntaxContext<'_, '_>,
    facts: &mut Facts,
    rules: &LanguageContext,
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
        let Some(actual_callee) = callee(call, source) else {
            continue;
        };
        for rule in rules
            .route_decorators
            .iter()
            .filter(|rule| callee_matches(actual_callee, &rule.callee))
        {
            if !route_receiver_matches(actual_callee, &rule.receiver_names, route_context) {
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
            let path = prefixed_path(route_context.prefix_for(node, actual_callee), path);
            for method in rule_methods(call, source, &rule.callee, &rule.method) {
                add(
                    Route {
                        syntax: decorator,
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
    rules: &LanguageContext,
    route_context: &RouteContext,
) {
    let (node, source, owner, offset) = (ctx.node, ctx.source, ctx.owner, ctx.offset);
    if matches!(node.kind(), "call" | "call_expression") {
        if node.parent().is_some_and(|p| p.kind() == "decorator") {
            return;
        }
        let Some(actual_callee) = callee(node, source) else {
            return;
        };
        for rule in rules
            .route_calls
            .iter()
            .filter(|rule| callee_matches(actual_callee, &rule.callee))
        {
            if !route_receiver_matches(actual_callee, &rule.receiver_names, route_context) {
                continue;
            }
            let Some(path) = positional_argument(node, source, rule.path_argument)
                .and_then(|argument| literal(argument, source))
            else {
                continue;
            };
            if !route_path_is_allowed(path, rule.allow_relative_path) {
                continue;
            }
            let path = prefixed_path(route_context.prefix_for(node, actual_callee), path);
            let methods = rule_methods(node, source, &rule.callee, &rule.method);
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
                            && argument
                                .child_by_field_name("name")
                                .is_some_and(|name| text(name, source) == "name")
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
                        let (include_module, include_namespace) = include_target(handler, source);
                        add(
                            Route {
                                syntax: node,
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
    } else if node.kind() == "object" && in_routes_collection(node, source) {
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
            match text(key, source).trim_matches(['\'', '"']) {
                "path" => path = literal(value, source),
                "component" => handler = Some(unwrapped(value)),
                _ => {}
            }
        }
        if let (Some(path), Some(handler)) = (path, handler) {
            if literal_handler(handler) {
                return;
            }
            add(
                Route {
                    syntax: node,
                    path,
                    method: "ANY".into(),
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

fn include_target(node: Syntax<'_>, source: &str) -> (Option<String>, Option<String>) {
    if !matches!(node.kind(), "call" | "call_expression")
        || callee(node, source)
            .is_none_or(|callee| callee != "include" && !callee.ends_with(".include"))
    {
        return (None, None);
    }
    let module = positional_argument(node, source, 0).and_then(|value| literal(value, source));
    let namespace = node
        .child_by_field_name("arguments")
        .and_then(|arguments| {
            let mut cursor = arguments.walk();
            let argument = arguments.named_children(&mut cursor).find(|argument| {
                argument.kind() == "keyword_argument"
                    && argument
                        .child_by_field_name("name")
                        .is_some_and(|name| text(name, source) == "namespace")
            });
            argument
        })
        .and_then(|argument| argument.child_by_field_name("value"))
        .and_then(|value| literal(value, source));
    (module.map(str::to_owned), namespace.map(str::to_owned))
}

pub(crate) fn dependency_injection(
    ctx: &languages::SyntaxContext<'_, '_>,
    rules: &LanguageContext,
    facts: &mut Facts,
) {
    if !matches!(ctx.node.kind(), "call" | "call_expression") {
        return;
    }
    let Some(actual_callee) = callee(ctx.node, ctx.source) else {
        return;
    };
    for rule in rules
        .di_calls
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
            receiver_hint: None,
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
    node: Syntax<'_>,
    source: &str,
    owner: &str,
    facts: &mut Facts,
    offset: usize,
) {
    if !matches!(node.kind(), "call" | "call_expression") {
        return;
    }
    let Some(fun) = node.child_by_field_name("function") else {
        return;
    };
    let callee = text(fun, source);
    let receiver_method = if callee.contains('.') {
        let Some((receiver, method)) = callee.rsplit_once('.') else {
            return;
        };
        let receiver = receiver.trim();
        let method = method.trim();
        let receiver_name = receiver
            .rsplit_once('.')
            .map_or(receiver, |(_, last)| last)
            .trim();
        if !matches!(
            receiver_name,
            "axios" | "http" | "client" | "$" | "jQuery" | "requests"
        ) {
            return;
        }
        Some(method)
    } else {
        if callee != "fetch" {
            return;
        }
        None
    };
    let (is_http_client, default_method) = if callee == "fetch" {
        (true, "GET")
    } else if let Some(member) = receiver_method {
        let m = match member {
            "get" => "GET",
            "post" => "POST",
            "put" => "PUT",
            "patch" => "PATCH",
            "delete" => "DELETE",
            "head" => "HEAD",
            "options" => "OPTIONS",
            "ajax" => "ANY",
            _ => return,
        };
        (true, m)
    } else {
        return;
    };
    if !is_http_client {
        return;
    }
    let Some(args) = node.child_by_field_name("arguments") else {
        return;
    };
    let Some(first_arg) = args.named_child(0) else {
        return;
    };
    let url = if let Some(url_str) = literal(first_arg, source) {
        url_str
    } else if first_arg.kind() == "object" {
        let mut u = None;
        let mut c = first_arg.walk();
        for pair in first_arg
            .named_children(&mut c)
            .filter(|n| n.kind() == "pair")
        {
            if let (Some(k), Some(v)) = (
                pair.child_by_field_name("key"),
                pair.child_by_field_name("value"),
            ) {
                if text(k, source).trim_matches(['\'', '"']) == "url" {
                    u = literal(v, source);
                    break;
                }
            }
        }
        let Some(u) = u else {
            return;
        };
        u
    } else {
        return;
    };
    if !url.starts_with(['/', '^']) && !url.starts_with("api/") {
        return;
    }
    let method = if default_method == "GET" && callee == "fetch" {
        if let Some(opts) = args.named_child(1).filter(|n| n.kind() == "object") {
            let mut m = "GET";
            let mut c = opts.walk();
            for pair in opts.named_children(&mut c).filter(|n| n.kind() == "pair") {
                if let (Some(k), Some(v)) = (
                    pair.child_by_field_name("key"),
                    pair.child_by_field_name("value"),
                ) {
                    if text(k, source)
                        .trim_matches(['\'', '"'])
                        .eq_ignore_ascii_case("method")
                    {
                        if let Some(val) = literal(v, source) {
                            m = match val.to_ascii_uppercase().as_str() {
                                "POST" => "POST",
                                "PUT" => "PUT",
                                "PATCH" => "PATCH",
                                "DELETE" => "DELETE",
                                "HEAD" => "HEAD",
                                "OPTIONS" => "OPTIONS",
                                _ => "GET",
                            };
                            break;
                        }
                    }
                }
            }
            m
        } else {
            "GET"
        }
    } else {
        default_method
    };
    let line = node.start_position().row + offset + 1;
    facts.references.push(crate::core::models::Reference {
        source: owner.into(),
        expression: format!("{method} {url}"),
        kind: "calls_endpoint".into(),
        line,
        column: node.start_position().column,
        alias: None,
        module: None,
        dynamic: false,
        receiver_hint: None,
    });
}
