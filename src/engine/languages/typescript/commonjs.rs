use super::{collect_require_bindings, nearest_binding_scope, required_module};
use crate::core::semantic::SourcePosition;
use crate::engine::ast::text;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

#[derive(Serialize)]
pub(crate) struct CommonJsBinding {
    alias: String,
    module: String,
    declaration: SourcePosition,
    completed: SourcePosition,
    invalidated: bool,
    captured_safe: bool,
}

fn position(node: Node<'_>, offset: usize, completed: bool) -> SourcePosition {
    let point = if completed { node.end_position() } else { node.start_position() };
    SourcePosition { line: point.row + offset + 1, column: point.column }
}

fn names<'a>(node: Node<'_>, source: &'a str, output: &mut Vec<&'a str>) {
    match node.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => output.push(text(node, source)),
        "pair_pattern" => {
            if let Some(value) = node.child_by_field_name("value") { names(value, source, output); }
        }
        "assignment_pattern" => {
            if let Some(left) = node.child_by_field_name("left") { names(left, source, output); }
        }
        "required_parameter" | "optional_parameter" => {
            if let Some(name) = node.child_by_field_name("name").or_else(|| node.child_by_field_name("pattern")) {
                names(name, source, output);
            }
        }
        "formal_parameters" | "object_pattern" | "array_pattern" | "rest_pattern" | "parenthesized_expression" | "non_null_expression" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) { names(child, source, output); }
        }
        _ => {}
    }
}

fn target<'a>(node: Node<'_>, source: &'a str, output: &mut Vec<&'a str>) {
    match node.kind() {
        "member_expression" | "subscript_expression" => {
            if let Some(object) = node.child_by_field_name("object") { target(object, source, output); }
        }
        "pair_pattern" | "assignment_pattern" => {
            if let Some(value) = node.child_by_field_name("value").or_else(|| node.child_by_field_name("left")) { target(value, source, output); }
        }
        "object_pattern" | "array_pattern" | "rest_pattern" | "parenthesized_expression" | "non_null_expression" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) { target(child, source, output); }
        }
        _ => names(node, source, output),
    }
}

fn receiver_is_called(mut node: Node<'_>, source: &str) -> bool {
    while let Some(parent) = node.parent() {
        match parent.kind() {
            "member_expression" | "subscript_expression" if parent.child_by_field_name("object") == Some(node) => node = parent,
            "call_expression" => return parent.child_by_field_name("function") == Some(node),
            "new_expression" => return parent.child_by_field_name("constructor") == Some(node),
            "unary_expression" => return parent.child_by_field_name("operator").is_some_and(|operator| text(operator, source) == "typeof"),
            _ => return false,
        }
    }
    false
}

fn candidate_declaration_name(node: Node<'_>, declaration_ids: &[usize]) -> bool {
    let mut child = node;
    let mut parent = node.parent();
    while let Some(ancestor) = parent {
        if ancestor.kind() == "assignment_pattern"
            && ancestor.child_by_field_name("right") == Some(child)
        {
            return false;
        }
        if ancestor.kind() == "variable_declarator" {
            return declaration_ids.contains(&ancestor.id())
                && ancestor.child_by_field_name("name").is_some_and(|name| {
                    name.start_byte() <= node.start_byte() && node.end_byte() <= name.end_byte()
                });
        }
        child = ancestor;
        parent = ancestor.parent();
    }
    false
}

fn inert_initializer(node: Node<'_>, depth: usize) -> bool {
    if depth == 0 || node.named_child_count() > 64 {
        return false;
    }
    match node.kind() {
        "number" | "string" | "true" | "false" | "null" => true,
        "parenthesized_expression" => node.named_child_count() == 1
            && node.named_child(0).is_some_and(|child| inert_initializer(child, depth - 1)),
        "array" => {
            let mut cursor = node.walk();
            let inert = node.named_children(&mut cursor)
                .all(|child| inert_initializer(child, depth - 1));
            inert
        }
        "object" => {
            let mut cursor = node.walk();
            let inert = node.named_children(&mut cursor).all(|child| {
                child.kind() == "pair"
                    && child.child_by_field_name("key").is_some_and(|key| {
                        matches!(key.kind(), "property_identifier" | "string")
                    })
                    && child.child_by_field_name("value").is_some_and(|value| {
                        inert_initializer(value, depth - 1)
                    })
            });
            inert
        }
        _ => false,
    }
}

fn inert_require(node: Node<'_>, source: &str) -> bool {
    required_module(node, source).is_some()
        && node.child_by_field_name("arguments")
            .is_some_and(|arguments| arguments.named_child_count() == 1)
}

pub(crate) fn commonjs_bindings(
    root: Node<'_>,
    source: &str,
    offset: usize,
    shadowed_require_scopes: &HashSet<usize>,
) -> Vec<CommonJsBinding> {
    if !matches!(root.kind(), "program" | "source_file") || shadowed_require_scopes.contains(&root.id()) {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    let mut declaration_ids = Vec::new();
    let mut captured_safe = true;
    let mut cursor = root.walk();
    for statement in root.named_children(&mut cursor) {
        if matches!(statement.kind(), "comment" | "import_statement" | "function_declaration" | "generator_function_declaration" | "empty_statement") {
            continue;
        }
        if statement.kind() == "lexical_declaration" {
            let is_const = statement.child_by_field_name("kind").is_some_and(|kind| text(kind, source) == "const");
            let mut cursor = statement.walk();
            for declaration in statement.named_children(&mut cursor).filter(|node| node.kind() == "variable_declarator") {
                let value = declaration.child_by_field_name("value");
                if is_const {
                    if let Some((name, module)) = declaration.child_by_field_name("name")
                        .filter(|name| matches!(name.kind(), "identifier" | "object_pattern"))
                        .zip(value.filter(|value| inert_require(*value, source))
                            .and_then(|value| required_module(value, source)))
                    {
                        let mut imports = Vec::new();
                        collect_require_bindings(name, source, &mut imports);
                        for (_, alias) in imports {
                            if let Some(alias) = alias {
                                candidates.push(CommonJsBinding {
                                    alias, module: module.clone(),
                                    declaration: position(declaration, offset, false),
                                    completed: position(declaration, offset, true),
                                    invalidated: false, captured_safe,
                                });
                                declaration_ids.push(declaration.id());
                            }
                        }
                    }
                }
                if !is_const || value.is_some_and(|value| {
                    !inert_initializer(value, 8) && !inert_require(value, source)
                }) {
                    captured_safe = false;
                }
            }
        } else { captured_safe = false; }
    }
    if candidates.is_empty() { return candidates; }
    let mut aliases: HashSet<&str> = candidates.iter().map(|binding| binding.alias.as_str()).collect();
    aliases.insert("require");
    let mut pending = vec![root];
    let mut nodes = Vec::new();
    while let Some(node) = pending.pop() {
        nodes.push(node);
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
    let mut bindings: HashMap<usize, HashSet<&str>> = HashMap::new();
    let mut declaration_scopes = HashMap::new();
    let mut bound = Vec::new();
    for &node in &nodes {
        let (pattern, scope) = match node.kind() {
            "variable_declarator" => (node.child_by_field_name("name"), nearest_binding_scope(node, node.parent().is_some_and(|parent| parent.kind() == "variable_declaration"))),
            "formal_parameters" => (Some(node), nearest_binding_scope(node, true)),
            "arrow_function" => (node.child_by_field_name("parameter"), Some(node)),
            "function_expression" | "generator_function" | "generator_function_expression" => (node.child_by_field_name("name"), Some(node)),
            "catch_clause" => (node.child_by_field_name("parameter"), Some(node)),
            "for_in_statement" if node.child_by_field_name("kind").is_some() => (node.child_by_field_name("left"), nearest_binding_scope(node, node.child_by_field_name("kind").is_some_and(|kind| text(kind, source) == "var"))),
            "function_declaration" | "generator_function_declaration" | "class_declaration" => (node.child_by_field_name("name"), node.parent().and_then(|parent| nearest_binding_scope(parent, false))),
            _ => (None, None),
        };
        if let Some((pattern, scope)) = pattern.zip(scope) {
            bound.clear();
            names(pattern, source, &mut bound);
            for &name in bound.iter().filter(|name| aliases.contains(**name)) {
                bindings.entry(scope.id()).or_default().insert(name);
                declaration_scopes.entry(node.id()).or_insert(scope.id());
            }
        }
    }
    let module_binding = |node: Node<'_>, alias: &str| {
        let mut current = Some(node);
        while let Some(scope) = current {
            if bindings.get(&scope.id()).is_some_and(|names| names.contains(alias)) { return scope.id() == root.id(); }
            current = scope.parent();
        }
        true
    };
    let mut invalidated = HashSet::new();
    for node in nodes {
        if declaration_scopes.get(&node.id()) == Some(&root.id()) {
            bound.clear();
            if let Some(pattern) = node.child_by_field_name("name") { names(pattern, source, &mut bound); }
            for name in &bound {
                if candidates.iter().zip(&declaration_ids).any(|(binding, id)| binding.alias == *name && *id != node.id()) {
                    invalidated.insert(*name);
                }
            }
        }
        let written = match node.kind() {
            "assignment_expression" | "augmented_assignment_expression" | "for_in_statement" => node.child_by_field_name("left"),
            "update_expression" => node.child_by_field_name("argument"),
            "unary_expression" if node.child_by_field_name("operator").is_some_and(|operator| text(operator, source) == "delete") => node.child_by_field_name("argument"),
            _ => None,
        };
        if let Some(written) = written {
            bound.clear();
            target(written, source, &mut bound);
            for &name in &bound {
                if aliases.contains(name) && module_binding(node, name) { invalidated.insert(name); }
            }
        }
        if matches!(node.kind(), "identifier" | "shorthand_property_identifier") && aliases.contains(text(node, source)) {
            let alias = text(node, source);
            let safe_use = candidate_declaration_name(node, &declaration_ids)
                || node.kind() == "identifier" && node.parent().is_some_and(|parent| match parent.kind() {
                "variable_declarator" => parent.child_by_field_name("name") == Some(node),
                "member_expression" | "subscript_expression" => receiver_is_called(node, source),
                "call_expression" => parent.child_by_field_name("function") == Some(node),
                "new_expression" => parent.child_by_field_name("constructor") == Some(node),
                "unary_expression" => parent.child_by_field_name("operator").is_some_and(|operator| text(operator, source) == "typeof"),
                _ => false,
            });
            if !safe_use && module_binding(node, alias) { invalidated.insert(alias); }
        }
    }
    let require_invalidated = invalidated.contains("require");
    for binding in &mut candidates { binding.invalidated = require_invalidated || invalidated.contains(binding.alias.as_str()); }
    candidates
}
