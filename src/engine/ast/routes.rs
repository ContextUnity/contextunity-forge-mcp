use super::{relations, text, Syntax};
use crate::core::models::{Edge, Facts, Node};
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

fn django_path(call: Syntax<'_>, source: &str) -> bool {
    if call.kind() != "call" {
        return false;
    }
    let Some(function) = call.child_by_field_name("function") else {
        return false;
    };
    matches!(
        text(function, source),
        "path"
            | "re_path"
            | "urls.path"
            | "urls.re_path"
            | "django.urls.path"
            | "django.urls.re_path"
    )
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

fn methods(call: Syntax<'_>, source: &str, decorated: bool) -> Vec<String> {
    if django_path(call, source) {
        return vec!["ANY".into()];
    }
    let Some(fun) = call.child_by_field_name("function") else {
        return Vec::new();
    };
    let callee = text(fun, source);
    let receiver = callee
        .rsplit_once('.')
        .map(|(receiver, _)| receiver.rsplit('.').next().unwrap_or(receiver));
    let registration_receiver = receiver.is_some_and(|receiver| {
        matches!(
            receiver,
            "app" | "router" | "server" | "api" | "bp" | "blueprint" | "route"
        )
    });
    let framework_decorator = decorated
        && fun.kind() == "identifier"
        && matches!(
            callee,
            "Get" | "Post" | "Put" | "Patch" | "Delete" | "Head" | "Options" | "All" | "Route"
        );
    if !registration_receiver && !framework_decorator {
        return Vec::new();
    }
    let method = callee
        .rsplit('.')
        .next()
        .unwrap_or(callee)
        .to_ascii_lowercase();
    if matches!(
        method.as_str(),
        "get" | "post" | "put" | "patch" | "delete" | "head" | "options" | "all" | "route"
    ) {
        if method == "route" {
            if let Some(args) = call.child_by_field_name("arguments") {
                let mut c = args.walk();
                for kw in args.named_children(&mut c) {
                    if kw.kind() == "keyword_argument"
                        && kw
                            .child_by_field_name("name")
                            .is_some_and(|n| text(n, source) == "methods")
                    {
                        if let Some(values) = kw.child_by_field_name("value") {
                            let mut c = values.walk();
                            return values
                                .named_children(&mut c)
                                .filter_map(|n| literal(n, source))
                                .map(str::to_ascii_uppercase)
                                .collect();
                        }
                    }
                }
            }
        }
        return vec![if matches!(method.as_str(), "all" | "route") {
            "ANY".into()
        } else {
            method.to_ascii_uppercase()
        }];
    }
    Vec::new()
}

struct Route<'a> {
    syntax: Syntax<'a>,
    path: &'a str,
    method: String,
    handler: &'a str,
    direct: bool,
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
        if let Some(handlers) = existing.details["handlers"].as_array_mut() {
            handlers.push(json!(route.handler));
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
        details: json!({"method":route.method,"route_path":route.path,"handler":route.handler,"handlers":[route.handler]}),
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
    if route.direct {
        facts.edges.push(Edge {
            src: id,
            dst: route.handler.into(),
            kind: "handles".into(),
            path: parent.path,
            line,
            evidence: "handler syntax identity".into(),
            confidence: "exact".into(),
        });
    } else {
        relations::reference(facts, &id, route.handler, "handles", line);
    }
}

pub(crate) fn declaration(
    node: Syntax<'_>,
    source: &str,
    owner: &str,
    facts: &mut Facts,
    offset: usize,
) {
    for decorator in relations::decorators(node) {
        let Some(call) = decorator
            .named_child(0)
            .filter(|n| matches!(n.kind(), "call" | "call_expression"))
        else {
            continue;
        };
        let Some(path) = call
            .child_by_field_name("arguments")
            .and_then(|a| a.named_child(0))
            .and_then(|n| literal(n, source))
        else {
            continue;
        };
        if !path.starts_with(['/', '^']) && !django_path(call, source) {
            continue;
        }
        for method in methods(call, source, true) {
            add(
                Route {
                    syntax: decorator,
                    path,
                    method,
                    handler: owner,
                    direct: true,
                },
                owner,
                facts,
                offset,
            );
        }
    }
}

pub(crate) fn registration(
    node: Syntax<'_>,
    source: &str,
    owner: &str,
    facts: &mut Facts,
    offset: usize,
    symbols: &HashMap<usize, String>,
) {
    if matches!(node.kind(), "call" | "call_expression") {
        if node.parent().is_some_and(|p| p.kind() == "decorator") {
            return;
        }
        let Some(args) = node.child_by_field_name("arguments") else {
            return;
        };
        let Some(path) = args.named_child(0).and_then(|n| literal(n, source)) else {
            return;
        };
        if !path.starts_with(['/', '^']) && !django_path(node, source) {
            return;
        }
        let methods = methods(node, source, false);
        if methods.is_empty() {
            return;
        }
        let mut cursor = args.walk();
        let handlers: Vec<_> = args
            .named_children(&mut cursor)
            .skip(1)
            .filter(|n| !matches!(n.kind(), "keyword_argument" | "comment"))
            .collect();
        for method in methods {
            for handler in &handlers {
                let handler = unwrapped(*handler);
                if literal_handler(handler) {
                    continue;
                }
                let direct = symbols.get(&handler.id());
                add(
                    Route {
                        syntax: node,
                        path,
                        method: method.clone(),
                        handler: direct
                            .map(String::as_str)
                            .unwrap_or_else(|| text(handler, source)),
                        direct: direct.is_some(),
                    },
                    owner,
                    facts,
                    offset,
                );
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
                    handler: symbols
                        .get(&handler.id())
                        .map(String::as_str)
                        .unwrap_or_else(|| text(handler, source)),
                    direct: symbols.contains_key(&handler.id()),
                },
                owner,
                facts,
                offset,
            );
        }
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
