use super::*;
use crate::core::models::{Edge, Facts, Node};
use serde_json::json;

fn literal_name<'a>(node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
    if node.kind() != "string" {
        return None;
    }
    let raw = text(node, source);
    let quote = raw.as_bytes().first().copied()?;
    if !matches!(quote, b'\'' | b'"') || raw.as_bytes().last() != Some(&quote) || raw.len() < 2 {
        return None;
    }
    let inner = &raw[1..raw.len() - 1];
    (!inner.contains('\\') && !inner.is_empty()).then_some(inner)
}

pub(super) fn decorator_candidates(ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
    if ctx.node.kind() != "function_definition" {
        return;
    }
    let mut candidates = Vec::new();
    for decorator in relations::decorators(ctx.node) {
        let Some(expression) = decorator.named_child(0) else {
            continue;
        };
        let (callee, arguments) = if expression.kind() == "call" {
            (
                expression.child_by_field_name("function"),
                expression.child_by_field_name("arguments"),
            )
        } else {
            (Some(expression), None)
        };
        let Some(callee) = callee else {
            continue;
        };
        let Some(receiver) = text(callee, ctx.source).strip_suffix(".tool") else {
            continue;
        };
        if receiver.is_empty()
            || !receiver
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            continue;
        }
        let mut name = None;
        let mut name_status = "default";
        if let Some(arguments) = arguments {
            let mut cursor = arguments.walk();
            for argument in arguments.named_children(&mut cursor) {
                if argument.kind() == "keyword_argument" {
                    if argument
                        .child_by_field_name("name")
                        .is_some_and(|node| text(node, ctx.source) == "name")
                    {
                        name = argument
                            .child_by_field_name("value")
                            .and_then(|node| literal_name(node, ctx.source));
                        name_status = if name.is_some() {
                            "explicit"
                        } else {
                            "dynamic"
                        };
                    }
                } else {
                    name_status = "dynamic";
                }
            }
        }
        candidates.push(json!({"receiver":receiver,"line":decorator.start_position().row + ctx.offset + 1,"name":name,"name_status":name_status}));
    }
    if !candidates.is_empty() {
        if let Some(function) = facts.nodes.iter_mut().find(|node| node.id == ctx.owner) {
            function.details["tool_decorators"] = json!(candidates);
        }
    }
}

pub(super) fn module_assignment(ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
    if ctx.node.kind() != "assignment" || !ctx.owner.starts_with("module:") {
        return;
    }
    if !ctx
        .node
        .parent()
        .and_then(|node| node.parent())
        .is_some_and(|node| node.kind() == "module")
    {
        return;
    }
    let Some(left) = ctx
        .node
        .child_by_field_name("left")
        .filter(|node| node.kind() == "identifier")
    else {
        return;
    };
    let callee = ctx
        .node
        .child_by_field_name("right")
        .filter(|node| node.kind() == "call")
        .and_then(|node| node.child_by_field_name("function"))
        .map(|node| text(node, ctx.source));
    let name = text(left, ctx.source);
    let tracked = facts
        .nodes
        .iter()
        .find(|node| node.id == ctx.owner)
        .and_then(|module| module.details["assignment_events"].as_array())
        .is_some_and(|events| events.iter().any(|event| event["name"] == name));
    let imported = facts.references.iter().any(|reference| {
        reference.kind == "imports"
            && reference.source == ctx.owner
            && (reference.alias.as_deref() == callee || reference.alias.as_deref() == Some(name))
            && matches!(
                reference.module.as_deref(),
                Some("fastmcp" | "mcp.server.fastmcp")
            )
    });
    if !tracked
        && !imported
        && !callee.is_some_and(|callee| callee == "FastMCP" || callee.ends_with(".FastMCP"))
    {
        return;
    }
    if let Some(module) = facts.nodes.iter_mut().find(|node| node.id == ctx.owner) {
        let events = module.details["assignment_events"].as_array_mut();
        if let Some(events) = events {
            events.push(json!({"name":name,"callee":callee,"line":ctx.line()}));
        } else {
            module.details["assignment_events"] =
                json!([{"name":name,"callee":callee,"line":ctx.line()}]);
        }
    }
}

fn imported_constructor(
    facts: &Facts,
    module_id: &str,
    callee: &str,
    assignment_line: usize,
) -> bool {
    let (alias, direct) = if let Some(alias) = callee.strip_suffix(".FastMCP") {
        (alias, false)
    } else if callee
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        (callee, true)
    } else {
        return false;
    };
    let mut imports = facts.references.iter().filter(|reference| {
        reference.kind == "imports"
            && reference.source == module_id
            && reference.alias.as_deref() == Some(alias)
            && reference.line < assignment_line
            && if direct {
                reference.expression == "FastMCP"
                    && matches!(
                        reference.module.as_deref(),
                        Some("fastmcp" | "mcp.server.fastmcp")
                    )
            } else {
                reference.expression == "fastmcp" && reference.module.as_deref() == Some("fastmcp")
            }
    });
    let Some(import) = imports.next() else {
        return false;
    };
    if imports.next().is_some() {
        return false;
    }
    !facts
        .nodes
        .iter()
        .find(|node| node.id == module_id)
        .and_then(|module| module.details["assignment_events"].as_array())
        .is_some_and(|events| {
            events.iter().any(|event| {
                event["name"] == alias
                    && event["line"].as_u64().is_some_and(|line| {
                        line > import.line as u64 && line < assignment_line as u64
                    })
            })
        })
}

fn confirmed_instance(facts: &Facts, module: &Node, receiver: &str, decorator_line: usize) -> bool {
    let Some(events) = module.details["assignment_events"].as_array() else {
        return false;
    };
    let mut bindings = events.iter().filter(|event| {
        event["name"] == receiver
            && event["line"]
                .as_u64()
                .is_some_and(|line| line < decorator_line as u64)
    });
    let Some(first) = bindings.next() else {
        return false;
    };
    let mut latest = first;
    for event in bindings {
        if event["line"].as_u64() >= latest["line"].as_u64() {
            latest = event;
        }
    }
    let (Some(callee), Some(line)) = (latest["callee"].as_str(), latest["line"].as_u64()) else {
        return false;
    };
    imported_constructor(facts, &module.id, callee, line as usize)
}

pub(super) fn registrations(facts: &mut Facts) {
    let Some(module) = facts
        .nodes
        .iter()
        .find(|node| node.kind == "module")
        .cloned()
    else {
        return;
    };
    let functions: Vec<_> = facts
        .nodes
        .iter()
        .filter(|node| node.kind == "function" && node.details["tool_decorators"].is_array())
        .cloned()
        .collect();
    for function in functions {
        if facts.edges.iter().any(|edge| {
            edge.dst == function.id
                && edge.kind == "contains"
                && facts
                    .nodes
                    .iter()
                    .any(|node| node.id == edge.src && node.kind == "class")
        }) {
            continue;
        }
        let Some(candidates) = function.details["tool_decorators"].as_array() else {
            continue;
        };
        for candidate in candidates {
            let (Some(receiver), Some(line)) =
                (candidate["receiver"].as_str(), candidate["line"].as_u64())
            else {
                continue;
            };
            let line = line as usize;
            if !confirmed_instance(facts, &module, receiver, line) {
                continue;
            }
            let name_status = candidate["name_status"].as_str().unwrap_or("dynamic");
            let advertised_name = if name_status == "explicit" {
                candidate["name"].as_str()
            } else if name_status == "default" {
                Some(function.name.as_str())
            } else {
                None
            };
            let name = advertised_name
                .map(str::to_owned)
                .unwrap_or_else(|| format!("tool@{line}"));
            let id = format!("mcp-tool:{}:{line}:{}", function.path, function.name);
            facts.nodes.push(Node {
                id: id.clone(), kind: "tool_registration".into(), name: name.clone(),
                qualname: format!("{}.tool.{name}", module.qualname), path: function.path.clone(),
                line, end_line: line, is_test: function.is_test, language: "python".into(), generated: false,
                details: json!({"framework":"FastMCP","receiver":receiver,"advertised_name":advertised_name,"name_status":name_status,"handler":function.id}),
            });
            facts.edges.push(Edge {
                src: module.id.clone(),
                dst: id.clone(),
                kind: "contains".into(),
                path: function.path.clone(),
                line,
                evidence: "FastMCP tool decorator".into(),
                confidence: "exact".into(),
            });
            facts.edges.push(Edge {
                src: id,
                dst: function.id.clone(),
                kind: "handles".into(),
                path: function.path.clone(),
                line,
                evidence: "decorated function".into(),
                confidence: "exact".into(),
            });
        }
    }
}
