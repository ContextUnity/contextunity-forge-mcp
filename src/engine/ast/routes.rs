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

fn methods(call: Syntax<'_>, source: &str) -> Vec<String> {
    let Some(fun) = call.child_by_field_name("function") else {
        return Vec::new();
    };
    let callee = text(fun, source);
    let method = callee
        .rsplit('.')
        .next()
        .unwrap_or(callee)
        .to_ascii_lowercase();
    if matches!(
        method.as_str(),
        "get" | "post" | "put" | "patch" | "delete" | "head" | "options" | "all" | "route"
    ) && (callee.contains('.') || fun.kind() == "identifier")
    {
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
    if matches!(callee, "path" | "re_path")
        || callee.ends_with(".path")
        || callee.ends_with(".re_path")
    {
        return vec!["ANY".into()];
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
        for method in methods(call, source) {
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
        let mut cursor = args.walk();
        let handlers: Vec<_> = args
            .named_children(&mut cursor)
            .skip(1)
            .filter(|n| !matches!(n.kind(), "keyword_argument" | "comment"))
            .collect();
        for method in methods(node, source) {
            for handler in &handlers {
                let handler = unwrapped(*handler);
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
    } else if node.kind() == "object" {
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

fn unwrapped(mut node: Syntax<'_>) -> Syntax<'_> {
    while node.kind() == "parenthesized_expression" {
        let Some(inner) = node.named_child(0) else {
            break;
        };
        node = inner;
    }
    node
}
