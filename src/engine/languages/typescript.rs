use super::*;
use crate::core::models::ReceiverHint;
use crate::engine::ast::{relations, routes};
#[path = "typescript/linker.rs"]
pub(crate) mod linker;
#[path = "typescript/commonjs.rs"]
mod commonjs;
pub(crate) use commonjs::commonjs_bindings;
#[path = "typescript/value_flow.rs"]
mod value_flow;
/// Performs language.
pub fn language(path: &str, language: &str) -> tree_sitter::Language {
    if language == "javascript"
        || path.ends_with(".js")
        || path.ends_with(".jsx")
        || path.ends_with(".mjs")
        || path.ends_with(".cjs")
    {
        tree_sitter_javascript::language()
    } else if path.ends_with(".tsx") {
        tree_sitter_typescript::language_tsx()
    } else {
        tree_sitter_typescript::language_typescript()
    }
}
/// Performs kind.
pub fn kind(kind: &str) -> Option<&'static str> {
    match kind {
        "function_declaration"
        | "generator_function_declaration"
        | "arrow_function"
        | "function_expression" => Some("function"),
        "method_definition" | "method_signature" => Some("method"),
        "class_declaration" | "class" => Some("class"),
        "enum_declaration" => Some("enum"),
        "enum_assignment" => Some("field"),
        "interface_declaration" => Some("interface"),
        "field_definition" => Some("field"),
        "type_alias_declaration" => Some("type"),
        _ => None,
    }
}

fn method_owner<'tree>(node: Syntax<'tree>) -> Option<Syntax<'tree>> {
    let mut current = node.parent()?;
    while matches!(
        current.kind(),
        "class_body" | "interface_body" | "object_type"
    ) {
        if let Some(owner) = current.parent() {
            if matches!(
                owner.kind(),
                "class_declaration" | "class" | "interface_declaration"
            ) {
                return Some(owner);
            }
        }
        current = current.parent()?;
    }
    None
}

fn lexical_this_method<'tree>(node: Syntax<'tree>) -> Option<(Syntax<'tree>, Syntax<'tree>)> {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "arrow_function" => {}
            "method_definition" => return method_owner(parent).map(|class| (parent, class)),
            "function_declaration" | "generator_function_declaration" | "function_expression"
            | "generator_function" | "generator_function_expression" | "class_declaration"
            | "class" | "method_signature" => return None,
            _ => {}
        }
        current = parent.parent();
    }
    None
}

fn prototype_target<'a>(left: Syntax<'_>, source: &'a str) -> Option<(&'a str, &'a str)> {
    if left.kind() != "member_expression" {
        return None;
    }
    let member = left.child_by_field_name("property")
        .filter(|property| property.kind() == "property_identifier")?;
    let prototype = left.child_by_field_name("object")
        .filter(|object| object.kind() == "member_expression")?;
    let property = prototype.child_by_field_name("property")
        .filter(|property| property.kind() == "property_identifier")?;
    let owner = prototype.child_by_field_name("object")
        .filter(|object| object.kind() == "identifier")?;
    let owner = text(owner, source);
    let member = text(member, source);
    (text(property, source) == "prototype" && owner.len() + member.len() <= 128)
        .then_some((owner, member))
}

fn direct_prototype_owner<'a>(left: Syntax<'_>, source: &'a str) -> Option<&'a str> {
    if left.kind() != "member_expression" {
        return None;
    }
    let property = left.child_by_field_name("property")
        .filter(|property| property.kind() == "property_identifier")?;
    let owner = left.child_by_field_name("object")
        .filter(|object| object.kind() == "identifier")?;
    (text(property, source) == "prototype").then(|| text(owner, source))
}

pub(crate) fn commonjs_default_callable(node: Syntax<'_>, source: &str) -> bool {
    if !matches!(node.kind(), "function_expression" | "arrow_function") {
        return false;
    }
    let Some(assignment) = node.parent().filter(|parent| parent.kind() == "assignment_expression") else {
        return false;
    };
    if assignment.child_by_field_name("right").is_none_or(|right| right.id() != node.id())
        || !assignment.parent().is_some_and(|parent| {
            parent.kind() == "expression_statement"
                && parent.parent().is_some_and(|root| root.kind() == "program")
        })
    {
        return false;
    }
    let Some(left) = assignment.child_by_field_name("left")
        .filter(|left| left.kind() == "member_expression") else {
        return false;
    };
    left.child_by_field_name("object").is_some_and(|object| {
        object.kind() == "identifier" && text(object, source) == "module"
    }) && left.child_by_field_name("property").is_some_and(|property| {
        property.kind() == "property_identifier" && text(property, source) == "exports"
    })
}

fn prototype_assignment<'a>(node: Syntax<'_>, source: &'a str) -> Option<(&'a str, &'a str)> {
    if !matches!(node.kind(), "function_expression" | "arrow_function") {
        return None;
    }
    let assignment = node.parent().filter(|parent| parent.kind() == "assignment_expression")?;
    if assignment.child_by_field_name("right")?.id() != node.id()
        || assignment.child_by_field_name("operator").is_some_and(|operator| text(operator, source) != "=")
        || !assignment.parent().is_some_and(|parent| {
            parent.kind() == "expression_statement"
                && parent.parent().is_some_and(|root| root.kind() == "program")
        })
    {
        return None;
    }
    prototype_target(assignment.child_by_field_name("left")?, source)
}

fn prototype_identifier_assignment<'a>(node: Syntax<'_>, source: &'a str) -> Option<(&'a str, &'a str, &'a str)> {
    if node.kind() != "assignment_expression"
        || !node.parent().is_some_and(|parent| parent.kind() == "expression_statement"
            && parent.parent().is_some_and(|root| root.kind() == "program"))
        || node.child_by_field_name("operator").is_some_and(|operator| text(operator, source) != "=")
    {
        return None;
    }
    let (owner, member) = prototype_target(node.child_by_field_name("left")?, source)?;
    let implementation = node.child_by_field_name("right")
        .filter(|right| right.kind() == "identifier")?;
    Some((owner, member, text(implementation, source)))
}

fn lexical_prototype_owner<'a>(node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "arrow_function" => {}
            "function_expression" => return prototype_assignment(parent, source).map(|(owner, _)| owner),
            "function_declaration" | "generator_function_declaration" | "generator_function"
            | "generator_function_expression" | "method_definition" | "class_declaration"
            | "class" => return None,
            _ => {}
        }
        current = parent.parent();
    }
    None
}

fn static_member_callee(function: Syntax<'_>, source: &str) -> Option<String> {
    let receiver = function
        .child_by_field_name("object")
        .filter(|node| node.kind() == "identifier")?;
    let member = match function.kind() {
        "subscript_expression" => {
            let index = function
                .child_by_field_name("index")
                .filter(|node| node.kind() == "string")?;
            let literal = text(index, source);
            let quoted = literal.as_bytes();
            if quoted.len() < 3
                || !matches!(quoted[0], b'\'' | b'"')
                || quoted[0] != quoted[quoted.len() - 1]
            {
                return None;
            }
            &literal[1..literal.len() - 1]
        }
        "member_expression" if function.child_by_field_name("optional_chain").is_some() => {
            let property = function
                .child_by_field_name("property")
                .filter(|node| node.kind() == "property_identifier")?;
            text(property, source)
        }
        _ => return None,
    };
    let mut bytes = member.bytes();
    if !bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$'))
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
    {
        return None;
    }
    Some(format!("{}.{}", text(receiver, source), member))
}


fn supported_object(object: Syntax<'_>) -> Option<Syntax<'_>> {
    // The helper is called for each direct member. Keep the object-wide check
    // bounded, and decline unusually large objects rather than repeatedly
    // scanning an unbounded member list on the extraction hot path.
    let mut cursor = object.walk();
    let mut count = 0;
    for member in object.named_children(&mut cursor) {
        count += 1;
        if count > 128 || member.kind() == "spread_element"
            || (member.kind() == "pair"
                && !member.child_by_field_name("key")
                    .is_some_and(|key| matches!(key.kind(), "property_identifier" | "identifier")))
        {
            return None;
        }
    }
    Some(object)
}

fn returned_object(object: Syntax<'_>) -> Option<Syntax<'_>> {
    if object.kind() != "object" {
        return None;
    }
    let parent = object.parent()?;
    let expression = if parent.kind() == "parenthesized_expression"
        && parent.named_child_count() == 1 && parent.named_child(0) == Some(object)
    {
        parent
    } else {
        object
    };
    let enclosing = expression.parent()?;
    let function = if enclosing.kind() == "return_statement" {
        let body = enclosing.parent()?;
        if body.kind() != "statement_block" { return None; }
        body.parent()?
    } else if enclosing.kind() == "arrow_function" {
        enclosing
    } else {
        return None;
    };
    if value_flow::final_return(function) != Some(expression) {
        return None;
    }
    supported_object(object)
}

fn assigned_or_alpine_object<'tree>(object: Syntax<'tree>, source: &str) -> Option<Syntax<'tree>> {
    if object.kind() != "object" {
        return None;
    }
    let parent = object.parent()?;
    let expression = if parent.kind() == "parenthesized_expression"
        && parent.named_child_count() == 1 && parent.named_child(0) == Some(object)
    {
        parent
    } else {
        object
    };
    let enclosing = expression.parent()?;
    if enclosing.kind() == "arguments" {
        if let Some(call) = enclosing.parent().filter(|n| n.kind() == "call_expression") {
            if let Some(fun) = call.child_by_field_name("function") {
                let callee = text(fun, source);
                if callee.ends_with(".data") || callee.ends_with(".mixin") || callee == "Alpine.data" {
                    return supported_object(object);
                }
            }
        }
    }
    None
}

fn same_object<'tree>(object: Syntax<'tree>, source: &str) -> Option<Syntax<'tree>> {
    returned_object(object).or_else(|| assigned_or_alpine_object(object, source))
}

fn same_object_receiver<'tree>(node: Syntax<'tree>, source: &str) -> Option<Syntax<'tree>> {
    if node.kind() == "function_expression" {
        let pair = node.parent()?;
        if pair.kind() == "pair" && pair.child_by_field_name("value") == Some(node) {
            return same_object(pair.parent()?, source);
        }
        return None;
    }
    let member = if node.kind() == "method_definition" {
        node
    } else if node.kind() == "arrow_function" {
        let mut current = node.parent();
        loop {
            let parent = current?;
            match parent.kind() {
                "method_definition" => break parent,
                "function_declaration" | "function_expression" | "generator_function"
                | "generator_function_declaration" | "class_declaration" | "class" => return None,
                _ => current = parent.parent(),
            }
        }
    } else {
        return None;
    };
    same_object(member.parent()?, source)
}

fn object_marker(object: Syntax<'_>, source: &str) -> String {
    let start = object.start_position();
    let is_alpine = object.parent().and_then(|p| p.parent()).is_some_and(|enclosing| {
        if enclosing.kind() == "arguments" {
            if let Some(call) = enclosing.parent().filter(|n| n.kind() == "call_expression") {
                if let Some(fun) = call.child_by_field_name("function") {
                    let callee = text(fun, source);
                    return callee.ends_with(".data") || callee == "Alpine.data";
                }
            }
        }
        false
    });
    if is_alpine {
        format!("alpine_data:{}:{}", start.row + 1, start.column)
    } else {
        format!("object_return:{}:{}", start.row + 1, start.column)
    }
}

fn required_module(value: Syntax<'_>, source: &str) -> Option<String> {
    if value.kind() != "call_expression" {
        return None;
    }
    let callee = value.child_by_field_name("function")?;
    if text(callee, source).trim() != "require" {
        return None;
    }
    let argument = value.child_by_field_name("arguments")?.named_child(0)?;
    if argument.kind() != "string" {
        return None;
    }
    let raw = text(argument, source).trim();
    let quote = raw.chars().next()?;
    if !matches!(quote, '\'' | '"') || raw.chars().last()? != quote {
        return None;
    }
    Some(raw[1..raw.len() - 1].to_owned())
}

fn collect_require_bindings(
    pattern: Syntax<'_>,
    source: &str,
    imports: &mut Vec<(String, Option<String>)>,
) {
    match pattern.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => {
            let name = text(pattern, source).to_owned();
            imports.push((name.clone(), Some(name)));
        }
        "pair_pattern" => {
            let Some(key) = pattern.child_by_field_name("key") else {
                return;
            };
            let Some(value) = pattern.child_by_field_name("value") else {
                return;
            };
            let local = if value.kind() == "assignment_pattern" {
                value.child_by_field_name("left").unwrap_or(value)
            } else {
                value
            };
            if matches!(
                local.kind(),
                "identifier" | "shorthand_property_identifier_pattern"
            ) {
                imports.push((
                    text(key, source).to_owned(),
                    Some(text(local, source).to_owned()),
                ));
            }
        }
        "assignment_pattern" => {
            if let Some(left) = pattern.child_by_field_name("left") {
                collect_require_bindings(left, source, imports);
            }
        }
        "object_pattern" | "array_pattern" | "rest_pattern" => {
            let mut cursor = pattern.walk();
            for child in pattern.named_children(&mut cursor) {
                collect_require_bindings(child, source, imports);
            }
        }
        _ => {}
    }
}

fn require_is_shadowed(ctx: &SyntaxContext<'_, '_>) -> bool {
    let mut current = Some(ctx.node);
    while let Some(scope) = current {
        if ctx.shadowed_require_scopes.contains(&scope.id()) {
            return true;
        }
        current = scope.parent();
    }
    false
}

fn pattern_binds_name(pattern: Syntax<'_>, source: &str, name: &str) -> bool {
    match pattern.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => {
            text(pattern, source).trim() == name
        }
        "pair_pattern" => pattern
            .child_by_field_name("value")
            .is_some_and(|value| pattern_binds_name(value, source, name)),
        "assignment_pattern" => pattern
            .child_by_field_name("left")
            .is_some_and(|left| pattern_binds_name(left, source, name)),
        "formal_parameters" | "object_pattern" | "array_pattern" | "rest_pattern"
        | "required_parameter" | "optional_parameter" | "formal_parameter" => {
            let candidate = pattern
                .child_by_field_name("name")
                .or_else(|| pattern.child_by_field_name("pattern"));
            if let Some(candidate) = candidate {
                return pattern_binds_name(candidate, source, name);
            }
            let mut cursor = pattern.walk();
            let binds = pattern
                .named_children(&mut cursor)
                .any(|child| pattern_binds_name(child, source, name));
            binds
        }
        _ => false,
    }
}

fn pattern_binds_require(pattern: Syntax<'_>, source: &str) -> bool {
    pattern_binds_name(pattern, source, "require")
}

fn collect_pattern_binding_names(pattern: Syntax<'_>, source: &str, names: &mut Vec<String>) {
    match pattern.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => {
            names.push(text(pattern, source).to_owned());
        }
        "pair_pattern" => {
            if let Some(value) = pattern.child_by_field_name("value") {
                collect_pattern_binding_names(value, source, names);
            }
        }
        "assignment_pattern" => {
            if let Some(left) = pattern.child_by_field_name("left") {
                collect_pattern_binding_names(left, source, names);
            }
        }
        "formal_parameters" | "object_pattern" | "array_pattern" | "rest_pattern"
        | "required_parameter" | "optional_parameter" | "formal_parameter" => {
            if let Some(candidate) = pattern
                .child_by_field_name("name")
                .or_else(|| pattern.child_by_field_name("pattern"))
            {
                collect_pattern_binding_names(candidate, source, names);
                return;
            }
            let mut cursor = pattern.walk();
            for child in pattern.named_children(&mut cursor) {
                collect_pattern_binding_names(child, source, names);
            }
        }
        _ => {}
    }
}

fn dom_global_name(name: &str) -> Option<&'static str> {
    match name {
        "document" => Some("document"),
        "window" => Some("window"),
        "Event" => Some("Event"),
        "CustomEvent" => Some("CustomEvent"),
        "MouseEvent" => Some("MouseEvent"),
        "KeyboardEvent" => Some("KeyboardEvent"),
        "EventTarget" => Some("EventTarget"),
        "Element" => Some("Element"),
        "HTMLElement" => Some("HTMLElement"),
        "HTMLTableElement" => Some("HTMLTableElement"),
        "HTMLDivElement" => Some("HTMLDivElement"),
        "HTMLInputElement" => Some("HTMLInputElement"),
        "HTMLButtonElement" => Some("HTMLButtonElement"),
        "HTMLAnchorElement" => Some("HTMLAnchorElement"),
        "Document" => Some("Document"),
        "Window" => Some("Window"),
        _ => None,
    }
}

fn remember_dom_global(
    name: &str,
    names: &mut [Option<&'static str>; 16],
    count: &mut usize,
) {
    let Some(global) = dom_global_name(name.trim()) else {
        return;
    };
    if !names[..*count].contains(&Some(global)) {
        names[*count] = Some(global);
        *count += 1;
    }
}

fn collect_dom_pattern_globals(
    pattern: Syntax<'_>,
    source: &str,
    names: &mut [Option<&'static str>; 16],
    count: &mut usize,
) {
    match pattern.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => {
            remember_dom_global(text(pattern, source), names, count);
        }
        "pair_pattern" => {
            if let Some(value) = pattern.child_by_field_name("value") {
                collect_dom_pattern_globals(value, source, names, count);
            }
        }
        "assignment_pattern" => {
            if let Some(left) = pattern.child_by_field_name("left") {
                collect_dom_pattern_globals(left, source, names, count);
            }
        }
        "formal_parameters" | "object_pattern" | "array_pattern" | "rest_pattern"
        | "required_parameter" | "optional_parameter" | "formal_parameter" => {
            if let Some(candidate) = pattern
                .child_by_field_name("name")
                .or_else(|| pattern.child_by_field_name("pattern"))
            {
                collect_dom_pattern_globals(candidate, source, names, count);
                return;
            }
            let mut cursor = pattern.walk();
            for child in pattern.named_children(&mut cursor) {
                collect_dom_pattern_globals(child, source, names, count);
            }
        }
        _ => {}
    }
}

fn collect_dom_import_globals(
    node: Syntax<'_>,
    source: &str,
    names: &mut [Option<&'static str>; 16],
    count: &mut usize,
) {
    match node.kind() {
        "import_clause" | "namespace_import" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "identifier" {
                    remember_dom_global(text(child, source), names, count);
                }
            }
        }
        "import_specifier" => {
            if let Some(local) = node
                .child_by_field_name("alias")
                .or_else(|| node.child_by_field_name("name"))
            {
                remember_dom_global(text(local, source), names, count);
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_dom_import_globals(child, source, names, count);
    }
}

fn dom_scope_chain(node: Syntax<'_>) -> Vec<usize> {
    let mut scopes = Vec::new();
    let mut current = Some(node);
    while let Some(scope) = current {
        scopes.push(scope.id());
        current = scope.parent();
    }
    scopes
}

fn dom_listener_receiver_expression(
    node: Syntax<'_>,
    source: &str,
    depth: usize,
) -> DomListenerReceiverExpression {
    if depth > 4 {
        return DomListenerReceiverExpression::Unsupported;
    }
    match node.kind() {
        "identifier" => DomListenerReceiverExpression::Identifier {
            name: text(node, source).to_owned(),
            position: node.start_byte(),
        },
        "member_expression" => {
            let Some(object) = node.child_by_field_name("object") else {
                return DomListenerReceiverExpression::Unsupported;
            };
            let Some(name) = node
                .child_by_field_name("property")
                .filter(|property| property.kind() == "property_identifier")
            else {
                return DomListenerReceiverExpression::Unsupported;
            };
            if object.kind() != "this" {
                return DomListenerReceiverExpression::Unsupported;
            }
            let Some((method, class)) = lexical_this_method(node) else {
                return DomListenerReceiverExpression::Unsupported;
            };
            let static_method = (0..method.child_count())
                .any(|index| method.child(index).is_some_and(|child| child.kind() == "static"));
            DomListenerReceiverExpression::ThisField {
                name: text(name, source).to_owned(),
                class_scope: class.id(),
                static_method,
            }
        }
        "call_expression" => {
            let Some(callee) = node
                .child_by_field_name("function")
                .filter(|callee| callee.kind() == "member_expression")
            else {
                return DomListenerReceiverExpression::Unsupported;
            };
            let Some(receiver) = callee.child_by_field_name("object") else {
                return DomListenerReceiverExpression::Unsupported;
            };
            let Some(method) = callee
                .child_by_field_name("property")
                .filter(|property| property.kind() == "property_identifier")
            else {
                return DomListenerReceiverExpression::Unsupported;
            };
            let has_arguments = node
                .child_by_field_name("arguments")
                .is_some_and(|arguments| arguments.named_child_count() > 0);
            let type_arguments = node
                .child_by_field_name("type_arguments")
                .map(|arguments| {
                    arguments
                        .named_children(&mut arguments.walk())
                        .map(|argument| text(argument, source).to_owned())
                        .collect()
                })
                .unwrap_or_default();
            DomListenerReceiverExpression::MemberCall {
                receiver: Box::new(dom_listener_receiver_expression(receiver, source, depth + 1)),
                method: text(method, source).to_owned(),
                type_arguments,
                has_arguments,
            }
        }
        "parenthesized_expression" => node
            .named_child(0)
            .map(|inner| {
                DomListenerReceiverExpression::Parenthesized(Box::new(
                    dom_listener_receiver_expression(inner, source, depth + 1),
                ))
            })
            .unwrap_or_default(),
        _ => DomListenerReceiverExpression::Unsupported,
    }
}

fn index_dom_listener_declarator(node: Syntax<'_>, source: &str, file: &mut FileContext) {
    let Some(binding) = node.child_by_field_name("name") else {
        return;
    };
    let Some(statement) = node.parent().filter(|statement| {
        matches!(statement.kind(), "lexical_declaration" | "variable_declaration")
    }) else {
        return;
    };
    let Some(scope) = statement.parent().filter(|scope| {
        matches!(scope.kind(), "statement_block" | "program" | "source_file")
    }) else {
        return;
    };
    let mut names = Vec::new();
    collect_pattern_binding_names(binding, source, &mut names);
    names.sort_unstable();
    names.dedup();
    if names.is_empty() {
        return;
    }
    let is_const = statement.kind() == "lexical_declaration"
        && statement
            .child_by_field_name("kind")
            .is_some_and(|keyword| text(keyword, source) == "const");
    let initializer = if is_const {
        node.child_by_field_name("value")
            .map(|value| dom_listener_receiver_expression(value, source, 0))
    } else {
        None
    };
    let scope_chain = std::sync::Arc::new(dom_scope_chain(scope));
    let declarations = std::sync::Arc::make_mut(&mut file.dom_listener_local_declarations);
    let block = declarations.entry(scope.id()).or_default();
    for name in names {
        let declaration = block.entry(name).or_default();
        declaration.declaration_count += 1;
        if declaration.declaration_count == 1 {
            declaration.is_const = is_const;
            declaration.statement_end = statement.end_byte();
            declaration.initializer = initializer.clone();
            declaration.scope_chain = std::sync::Arc::clone(&scope_chain);
        } else {
            declaration.initializer = None;
        }
    }
}

fn nearest_binding_scope(node: Syntax<'_>, var_scoped: bool) -> Option<Syntax<'_>> {
    let mut current = Some(node);
    while let Some(scope) = current {
        if var_scoped {
            if matches!(
                scope.kind(),
                "program"
                    | "source_file"
                    | "function_declaration"
                    | "generator_function_declaration"
                    | "function_expression"
                    | "generator_function"
                    | "generator_function_expression"
                    | "arrow_function"
                    | "method_definition"
            ) {
                return Some(scope);
            }
        } else if matches!(
            scope.kind(),
            "statement_block"
                | "catch_clause"
                | "for_statement"
                | "for_in_statement"
                | "program"
                | "source_file"
        ) {
            return Some(scope);
        }
        current = scope.parent();
    }
    None
}

fn collect_array_shadow_fact(node: Syntax<'_>, source: &str, file: &mut FileContext) {
    let (array_binding, array_var_scoped) = match node.kind() {
        "variable_declarator" => (
            node.child_by_field_name("name")
                .is_some_and(|name| name.kind() == "identifier" && text(name, source) == "Array"),
            node.parent().is_some_and(|parent| parent.kind() == "variable_declaration"),
        ),
        "class_declaration" | "interface_declaration" | "type_alias_declaration"
        | "enum_declaration" | "function_declaration" => {
            (field(node, source, "name") == Some("Array"), false)
        }
        "type_parameter" => (field(node, source, "name") == Some("Array"), false),
        "required_parameter" | "optional_parameter" => {
            (field(node, source, "name") == Some("Array"), true)
        }
        "import_specifier" => (
            node.child_by_field_name("alias")
                .or_else(|| node.child_by_field_name("name"))
                .is_some_and(|name| text(name, source) == "Array"),
            false,
        ),
        "import_clause" => (
            node.named_child(0)
                .is_some_and(|name| name.kind() == "identifier" && text(name, source) == "Array"),
            false,
        ),
        "namespace_import" => (
            node.named_child(0)
                .is_some_and(|name| name.kind() == "identifier" && text(name, source) == "Array"),
            false,
        ),
        _ => (false, false),
    };
    if array_binding {
        let scope = if node.kind() == "type_parameter" {
            let mut current = node.parent();
            let mut owner = None;
            while let Some(candidate) = current {
                if matches!(
                    candidate.kind(),
                    "class_declaration"
                        | "class"
                        | "function_declaration"
                        | "function_expression"
                        | "arrow_function"
                        | "method_definition"
                ) {
                    owner = Some(candidate);
                    break;
                }
                current = candidate.parent();
            }
            owner
        } else {
            nearest_binding_scope(node, array_var_scoped)
        };
        if let Some(scope) = scope {
            std::sync::Arc::make_mut(&mut file.shadowed_array_type_scopes).insert(scope.id());
        }
    }
}

fn collect_rebound_parameter_fact(node: Syntax<'_>, source: &str, file: &mut FileContext) {
    let rebound = match node.kind() {
        "assignment_expression" | "augmented_assignment_expression" => {
            node.child_by_field_name("left")
        }
        "variable_declarator" => node.child_by_field_name("name"),
        "update_expression" => node.named_child(0),
        _ => None,
    };
    let Some(name) = rebound.filter(|name| name.kind() == "identifier") else {
        return;
    };
    let name = text(name, source);
    let mut current = node.parent();
    while let Some(scope) = current {
        if matches!(
            scope.kind(),
            "function_declaration" | "method_definition" | "function_expression" | "arrow_function"
        ) {
            std::sync::Arc::make_mut(&mut file.rebound_function_parameters)
                .entry(scope.id())
                .or_default()
                .insert(name.to_owned());
        }
        current = scope.parent();
    }
}

fn collect_array_callback_scope_facts(root: Syntax<'_>, source: &str, file: &mut FileContext) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "variable_declarator" {
            index_dom_listener_declarator(node, source, file);
        }
        collect_dom_global_shadow_fact(node, source, file);
        collect_array_shadow_fact(node, source, file);
        collect_rebound_parameter_fact(node, source, file);
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
}

fn has_member_property(source: &str, matches_property: impl Fn(&[u8]) -> bool) -> bool {
    let bytes = source.as_bytes();
    let mut dot = 0;
    while dot < bytes.len() {
        if bytes[dot] != b'.' {
            dot += 1;
            continue;
        }
        let mut property = dot + 1;
        loop {
            while bytes
                .get(property)
                .is_some_and(|byte| byte.is_ascii_whitespace())
            {
                property += 1;
            }
            if bytes.get(property..property + 2) == Some(b"//") {
                property += 2;
                while bytes
                    .get(property)
                    .is_some_and(|byte| !matches!(byte, b'\n' | b'\r'))
                {
                    property += 1;
                }
                continue;
            }
            if bytes.get(property..property + 2) == Some(b"/*") {
                property += 2;
                while property + 1 < bytes.len()
                    && bytes.get(property..property + 2) != Some(b"*/")
                {
                    property += 1;
                }
                property = (property + 2).min(bytes.len());
                continue;
            }
            break;
        }
        if matches_property(&bytes[property..]) {
            return true;
        }
        dot += 1;
    }
    false
}

fn has_member_method(source: &str, methods: &[&str]) -> bool {
    has_member_property(source, |property| {
        methods.iter().any(|method| {
            property.starts_with(method.as_bytes())
                && property
                    .get(method.len())
                    .is_none_or(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'_' | b'$') && *byte < 0x80)
        })
    })
}

fn has_dom_event_property(source: &str) -> bool {
    has_member_property(source, |property| {
        property.starts_with(b"on")
            && property.len() > 2
            && property
                .get(2)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$') || *byte >= 0x80)
    })
}

fn collect_dom_global_shadow_fact(node: Syntax<'_>, source: &str, file: &mut FileContext) {
    let mut shadowed_globals = [None; 16];
    let mut shadowed_count = 0;
    let mut var_scoped = false;
    match node.kind() {
        "variable_declarator" => {
            var_scoped = node
                .parent()
                .is_some_and(|parent| parent.kind() == "variable_declaration");
            if let Some(name) = node.child_by_field_name("name") {
                collect_dom_pattern_globals(
                    name,
                    source,
                    &mut shadowed_globals,
                    &mut shadowed_count,
                );
            }
        }
        "formal_parameters" | "required_parameter" | "optional_parameter" | "formal_parameter" => {
            var_scoped = true;
            collect_dom_pattern_globals(
                node,
                source,
                &mut shadowed_globals,
                &mut shadowed_count,
            );
        }
        "arrow_function" => {
            var_scoped = true;
            if let Some(parameter) = node.child_by_field_name("parameter") {
                collect_dom_pattern_globals(
                    parameter,
                    source,
                    &mut shadowed_globals,
                    &mut shadowed_count,
                );
            }
        }
        "function_declaration" | "class_declaration" | "enum_declaration"
        | "interface_declaration" | "type_alias_declaration" => {
            if let Some(name) = field(node, source, "name") {
                remember_dom_global(name, &mut shadowed_globals, &mut shadowed_count);
            }
        }
        "function_expression" => {
            var_scoped = true;
            if let Some(name) = field(node, source, "name") {
                remember_dom_global(name, &mut shadowed_globals, &mut shadowed_count);
            }
        }
        "import_statement" => collect_dom_import_globals(
            node,
            source,
            &mut shadowed_globals,
            &mut shadowed_count,
        ),
        "catch_clause" => {
            if let Some(parameter) = node.child_by_field_name("parameter") {
                collect_dom_pattern_globals(
                    parameter,
                    source,
                    &mut shadowed_globals,
                    &mut shadowed_count,
                );
            }
        }
        "assignment_expression" | "augmented_assignment_expression" => {
            var_scoped = true;
            if let Some(left) = node
                .child_by_field_name("left")
                .filter(|left| left.kind() == "identifier")
            {
                remember_dom_global(text(left, source), &mut shadowed_globals, &mut shadowed_count);
            }
        }
        _ => {}
    }
    if shadowed_count > 0 {
        if let Some(scope) = nearest_binding_scope(node, var_scoped) {
            let by_scope = std::sync::Arc::make_mut(&mut file.shadowed_dom_global_scopes);
            let globals = by_scope.entry(scope.id()).or_default();
            for global in shadowed_globals[..shadowed_count].iter().flatten() {
                globals.insert((*global).to_owned());
            }
        }
    }
}

fn collect_scope_facts(root: Syntax<'_>, source: &str, file: &mut FileContext, typescript: bool) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let (binding, var_scoped) = match node.kind() {
            "variable_declarator" => (
                node.child_by_field_name("name")
                    .is_some_and(|name| pattern_binds_require(name, source)),
                node.parent()
                    .is_some_and(|parent| parent.kind() == "variable_declaration"),
            ),
            "formal_parameters" | "required_parameter" | "optional_parameter"
            | "formal_parameter" => (pattern_binds_require(node, source), true),
            "arrow_function" => (
                node.child_by_field_name("parameter")
                    .is_some_and(|parameter| pattern_binds_require(parameter, source)),
                true,
            ),
            "function_expression" | "generator_function" | "generator_function_expression" => (
                field(node, source, "name").is_some_and(|name| name.trim() == "require"),
                true,
            ),
            "function_declaration" | "generator_function_declaration" | "class_declaration" => (
                field(node, source, "name").is_some_and(|name| name.trim() == "require"),
                false,
            ),
            "import_statement" => (import_binds_require(node, source), false),
            "catch_clause" => (
                node.child_by_field_name("parameter")
                    .is_some_and(|pattern| pattern_binds_require(pattern, source)),
                false,
            ),
            _ => (false, false),
        };
        if binding {
            if let Some(scope) = nearest_binding_scope(node, var_scoped) {
                std::sync::Arc::make_mut(&mut file.shadowed_require_scopes).insert(scope.id());
            }
        }
        if !typescript {
            let mut cursor = node.walk();
            stack.extend(node.named_children(&mut cursor));
            continue;
        }
        if node.kind() == "variable_declarator" {
            index_dom_listener_declarator(node, source, file);
        }
        if matches!(
            node.kind(),
            "function_declaration"
                | "generator_function_declaration"
                | "function_expression"
                | "generator_function"
                | "generator_function_expression"
                | "arrow_function"
                | "method_definition"
        ) {
            if let Some(parameters) = node
                .child_by_field_name("parameters")
                .or_else(|| node.child_by_field_name("parameter"))
            {
                let mut names = Vec::new();
                collect_pattern_binding_names(parameters, source, &mut names);
                if !names.is_empty() {
                    std::sync::Arc::make_mut(&mut file.dom_listener_parameter_bindings)
                        .entry(node.id())
                        .or_default()
                        .extend(names);
                }
            }
        }
        if node.kind() == "catch_clause" {
            if let Some(parameter) = node.child_by_field_name("parameter") {
                let mut names = Vec::new();
                collect_pattern_binding_names(parameter, source, &mut names);
                if !names.is_empty() {
                    std::sync::Arc::make_mut(&mut file.dom_listener_parameter_bindings)
                        .entry(node.id())
                        .or_default()
                        .extend(names);
                }
            }
        }
        let written = match node.kind() {
            "assignment_expression" | "augmented_assignment_expression" => {
                node.child_by_field_name("left")
            }
            "update_expression" => node.named_child(0),
            _ => None,
        };
        if let Some(name) = written.filter(|name| name.kind() == "identifier") {
            let name = text(name, source).to_owned();
            let local_writes = std::sync::Arc::make_mut(&mut file.dom_listener_local_writes);
            let mut parent = node.parent();
            while let Some(scope) = parent {
                if matches!(scope.kind(), "statement_block" | "program" | "source_file") {
                    local_writes
                        .entry(scope.id())
                        .or_default()
                        .entry(name.clone()).or_default().push(node.start_byte());
                }
                if matches!(scope.kind(), "function_declaration" | "function_expression" | "arrow_function" | "method_definition") {
                    break;
                }
                parent = scope.parent();
            }
        }
        collect_dom_global_shadow_fact(node, source, file);
        if matches!(node.kind(), "field_definition" | "public_field_definition") {
            if let Some(class) = node.parent()
                .filter(|body| body.kind() == "class_body")
                .and_then(|body| body.parent())
                .filter(|class| matches!(class.kind(), "class_declaration" | "class"))
            {
                if let Some(name) = node.child_by_field_name("name")
                    .or_else(|| node.child_by_field_name("property"))
                    .filter(|name| matches!(name.kind(), "property_identifier" | "identifier"))
                {
                    let static_field = (0..node.child_count()).any(|index| {
                        node.child(index).is_some_and(|child| child.kind() == "static")
                    });
                    let ty = (!static_field).then(|| node.child_by_field_name("type")
                        .and_then(|annotation| annotation.named_child(0))
                        .filter(|ty| ty.kind() == "type_identifier")
                        .map(|ty| text(ty, source).to_owned())).flatten();
                    let fields = std::sync::Arc::make_mut(&mut file.dom_listener_class_fields)
                        .entry(class.id()).or_default();
                    let name = text(name, source).to_owned();
                    match fields.entry(name) {
                        std::collections::hash_map::Entry::Occupied(mut entry) => {
                            entry.insert(None);
                        }
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            entry.insert(ty);
                        }
                    }
                }
            }
        }
        if matches!(node.kind(), "function_declaration" | "method_definition" | "function_expression" | "arrow_function") {
            let mut receivers = HashMap::new();
            let mut duplicates = HashSet::new();
            if let Some(parameters) = node.child_by_field_name("parameters") {
                for parameter in parameters.named_children(&mut parameters.walk()) {
                    let Some(name) = parameter.child_by_field_name("name")
                        .or_else(|| parameter.child_by_field_name("pattern"))
                        .filter(|name| name.kind() == "identifier") else { continue; };
                    let Some(annotation) = parameter.child_by_field_name("type")
                        .and_then(|ty| ty.named_child(0))
                        .filter(|ty| ty.kind() == "type_identifier") else { continue; };
                    let ty = text(annotation, source);
                    if !matches!(typed_dom_receiver(ty), Some("Element" | "Document" | "Window" | "EventTarget")) {
                        continue;
                    }
                    let name = text(name, source).to_owned();
                    if receivers.insert(name.clone(), ty.to_owned()).is_some() {
                        duplicates.insert(name);
                    }
                }
            }
            if node.kind() == "arrow_function" {
                let parameter = node.child_by_field_name("parameter")
                    .or_else(|| node.child_by_field_name("parameters")?.named_child(0))
                    .and_then(|p| {
                        if p.kind() == "identifier" {
                            Some(p)
                        } else {
                            p.child_by_field_name("name")
                                .or_else(|| p.child_by_field_name("pattern"))
                                .filter(|n| n.kind() == "identifier")
                        }
                    });
                if let (Some(parameter), Some(element)) = (
                    parameter,
                    array_callback_element(node, source, file),
                ) {
                    if matches!(typed_dom_receiver(element), Some("Element" | "Document" | "Window" | "EventTarget")) {
                        receivers.insert(text(parameter, source).to_owned(), element.to_owned());
                    }
                }
            }
            for name in duplicates { receivers.remove(&name); }
            if !receivers.is_empty() {
                std::sync::Arc::make_mut(&mut file.dom_listener_receivers)
                    .insert(node.id(), receivers);
            }
        }
        collect_array_shadow_fact(node, source, file);
        collect_rebound_parameter_fact(node, source, file);
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
}

fn import_binds_name(node: Syntax<'_>, source: &str, name: &str) -> bool {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        match current.kind() {
            "import_clause" | "namespace_import" => {
                let mut cursor = current.walk();
                if current.named_children(&mut cursor).any(|child| {
                    child.kind() == "identifier" && text(child, source).trim() == name
                }) {
                    return true;
                }
            }
            "import_specifier" => {
                let local =
                    field(current, source, "alias").or_else(|| field(current, source, "name"));
                if local.is_some_and(|local| local.trim() == name) {
                    return true;
                }
            }
            _ => {}
        }
        let mut cursor = current.walk();
        stack.extend(current.named_children(&mut cursor));
    }
    false
}

fn import_binds_require(node: Syntax<'_>, source: &str) -> bool {
    import_binds_name(node, source, "require")
}

fn has_type_import_modifier(node: Syntax<'_>) -> bool {
    (0..node.child_count()).any(|index| {
        node.child(index).is_some_and(|child| child.kind() == "type")
    })
}

fn browser_global_type(name: &str) -> bool {
    if typed_dom_receiver(name).is_some() {
        return true;
    }
    matches!(
        name,
        "Response"
            | "Request"
            | "Headers"
            | "URL"
            | "URLSearchParams"
            | "FormData"
            | "Blob"
            | "File"
    )
}

fn nuxt_module_declared(node: Syntax<'_>, source: &str, module_name: &str) -> bool {
    if node.kind() != "call_expression"
        || node.parent().is_none_or(|parent| {
            parent.kind() != "export_statement"
                || !text(parent, source).trim_start().starts_with("export default")
        })
        || node.child_by_field_name("function").is_none_or(|callee| text(callee, source) != "defineNuxtConfig")
    {
        return false;
    }
    let Some(args) = node.child_by_field_name("arguments") else { return false; };
    let mut cursor = args.walk();
    let mut values = args.named_children(&mut cursor);
    let Some(config) = values.next().filter(|value| value.kind() == "object") else { return false; };
    if values.next().is_some() { return false; }
    let mut modules = None;
    for pair in config.named_children(&mut config.walk()) {
        if pair.kind() == "comment" { continue; }
        if pair.kind() != "pair" { return false; }
        if pair.child_by_field_name("key").is_some_and(|key| text(key, source) == "modules") {
            if modules.is_some() { return false; }
            modules = pair.child_by_field_name("value").filter(|value| value.kind() == "array");
            if modules.is_none() { return false; }
        }
    }
    let Some(modules) = modules else { return false; };
    let mut found = false;
    for item in modules.named_children(&mut modules.walk()) {
        if item.kind() != "string" || text(item, source).contains('\\') { return false; }
        found |= text(item, source).trim_matches(['\'', '"']) == module_name;
    }
    found
}

fn nuxt_default_components_enabled(node: Syntax<'_>, source: &str) -> Option<bool> {
    if node.kind() != "call_expression"
        || node.parent().is_none_or(|parent| {
            parent.kind() != "export_statement"
                || !text(parent, source).trim_start().starts_with("export default")
        })
        || node
            .child_by_field_name("function")
            .is_none_or(|callee| text(callee, source) != "defineNuxtConfig")
    {
        return None;
    }
    let arguments = node.child_by_field_name("arguments")?;
    if arguments.named_child_count() != 1 {
        return Some(false);
    }
    let config = arguments.named_child(0)?;
    if config.kind() != "object" {
        return Some(false);
    }
    let mut enabled = true;
    let mut seen = false;
    for property in config.named_children(&mut config.walk()) {
        if property.kind() == "comment" { continue; }
        if property.kind() != "pair" {
            return Some(false);
        }
        let Some(key) = property.child_by_field_name("key") else {
            return Some(false);
        };
        let name = match key.kind() {
            "property_identifier" => text(key, source),
            "string" => text(key, source).trim_matches(['\'', '"']),
            _ => return Some(false),
        };
        if name != "components" {
            continue;
        }
        if seen {
            return Some(false);
        }
        seen = true;
        enabled = property
            .child_by_field_name("value")
            .is_some_and(|value| text(value, source) == "true");
    }
    Some(enabled)
}

pub(crate) fn typed_dom_receiver(name: &str) -> Option<&'static str> {
    match name {
        "Document" | "document" => Some("Document"),
        "Window" | "window" => Some("Window"),
        "EventTarget" => Some("EventTarget"),
        "Event" | "CustomEvent" | "MouseEvent" | "KeyboardEvent"
        | "DragEvent" | "ClipboardEvent" | "FocusEvent" => Some("Event"),
        "URL" => Some("URL"),
        "URLSearchParams" => Some("URLSearchParams"),
        "Headers" => Some("Headers"),
        "DOMTokenList" => Some("DOMTokenList"),
        "H3Event" => Some("H3Event"),
        "Storage" => Some("Storage"),
        "Element" | "HTMLElement" | "Node" => Some("Element"),
        "ParentNode" => Some("ParentNode"),
        "ChildNode" => Some("ChildNode"),
        "DocumentFragment" => Some("DocumentFragment"),
        _ if (name.starts_with("HTML") || name.starts_with("SVG")) && name.ends_with("Element") => {
            Some("Element")
        }
        _ => None,
    }
}

pub(crate) fn typed_dom_property(receiver: &str, property: &str) -> Option<&'static str> {
    match (typed_dom_receiver(receiver)?, property) {
        ("Element", "parentElement" | "firstElementChild" | "lastElementChild"
            | "nextElementSibling" | "previousElementSibling") => Some("Element"),
        ("Element", "ownerDocument") => Some("Document"),
        ("Element", "classList") => Some("DOMTokenList"),
        ("Document", "documentElement" | "body" | "head" | "activeElement") => Some("Element"),
        ("Document", "defaultView") => Some("Window"),
        ("Window", "document") => Some("Document"),
        ("Event", "target" | "currentTarget") => Some("Element"),
        ("URL", "searchParams") => Some("URLSearchParams"),
        ("H3Event", "headers") => Some("Headers"),
        _ => None,
    }
}

fn element_type_from_annotation<'a>(
    annotation: Syntax<'_>,
    arrow: Syntax<'_>,
    file: &FileContext,
    source: &'a str,
) -> Option<&'a str> {
    let element = match annotation.kind() {
        "array_type" => annotation.named_child(0)?,
        "generic_type" => {
            let name = annotation.child_by_field_name("name")?;
            let name_str = text(name, source);
            if name.kind() != "type_identifier"
                || !matches!(name_str, "Array" | "NodeListOf" | "HTMLCollectionOf" | "Set")
            {
                return None;
            }
            let arguments = annotation.child_by_field_name("type_arguments")?;
            if arguments.named_child_count() != 1 || array_type_shadowed(arrow, file) {
                return None;
            }
            arguments.named_child(0)?
        }
        _ => return None,
    };
    if !matches!(element.kind(), "type_identifier" | "identifier") {
        return None;
    }
    Some(text(element, source))
}

fn array_callback_element<'a>(arrow: Syntax<'_>, source: &'a str, file: &FileContext) -> Option<&'a str> {
    let call = arrow.parent().filter(|parent| parent.kind() == "arguments")?.parent()?;
    let function = call.child_by_field_name("function")
        .filter(|function| function.kind() == "member_expression")?;
    let method = function.child_by_field_name("property")?;
    if !matches!(text(method, source), "forEach" | "map" | "filter" | "find" | "some" | "every" | "flatMap") {
        return None;
    }
    let receiver = function.child_by_field_name("object")?;
    if receiver.kind() == "identifier" {
        let receiver_name = text(receiver, source);
        let mut parent = call.parent();
        let enclosing = loop {
            let node = parent?;
            if matches!(node.kind(), "function_declaration" | "method_definition" | "function_expression" | "arrow_function") {
                break node;
            }
            parent = node.parent();
        };
        if let Some(parameters) = enclosing.child_by_field_name("parameters") {
            let mut cursor = parameters.walk();
            let mut matches = parameters.named_children(&mut cursor).filter(|parameter| {
                parameter.child_by_field_name("name")
                    .or_else(|| parameter.child_by_field_name("pattern"))
                    .is_some_and(|name| name.kind() == "identifier" && text(name, source) == receiver_name)
            });
            if let Some(parameter) = matches.next() {
                if matches.next().is_none() {
                    let annotation = parameter.child_by_field_name("type")?.named_child(0)?;
                    if !file.rebound_function_parameters.get(&enclosing.id())
                        .is_some_and(|names| names.contains(receiver_name))
                    {
                        return element_type_from_annotation(annotation, arrow, file, source);
                    }
                }
            }
        }
        let mut current_scope = call.parent();
        while let Some(node) = current_scope {
            if matches!(node.kind(), "statement_block" | "program" | "source_file") {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.start_byte() >= call.start_byte() {
                        break;
                    }
                    if matches!(child.kind(), "lexical_declaration" | "variable_declaration") {
                        let mut decl_cursor = child.walk();
                        for declarator in child.named_children(&mut decl_cursor) {
                            if declarator.kind() == "variable_declarator" {
                                if let Some(var_name) = declarator.child_by_field_name("name") {
                                    if var_name.kind() == "identifier" && text(var_name, source) == receiver_name {
                                        if let Some(ty_node) = declarator.child_by_field_name("type") {
                                            let annotation = ty_node.named_child(0).unwrap_or(ty_node);
                                            if let Some(el) = element_type_from_annotation(annotation, arrow, file, source) {
                                                return Some(el);
                                            }
                                        }
                                        if let Some(init) = declarator.child_by_field_name("value") {
                                            if init.kind() == "call_expression" {
                                                if let Some(callee) = init.child_by_field_name("function").filter(|c| c.kind() == "member_expression") {
                                                    if let Some(method) = callee.child_by_field_name("property") {
                                                        if matches!(text(method, source), "querySelectorAll" | "getElementsByTagName" | "getElementsByClassName") {
                                                            if let Some(obj) = callee.child_by_field_name("object") {
                                                                if dom_receiver_from_ast(obj, source, file, 0).is_some() {
                                                                    return Some("Element");
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if matches!(node.kind(), "function_declaration" | "method_definition" | "function_expression" | "arrow_function") {
                break;
            }
            current_scope = node.parent();
        }
        None
    } else if receiver.kind() == "member_expression"
        && receiver.child_by_field_name("object").is_some_and(|obj| obj.kind() == "this")
    {
        let property = receiver.child_by_field_name("property")?;
        let field_name = text(property, source);
        let (_, class) = lexical_this_method(arrow)?;
        let body = class.child_by_field_name("body")?;
        let mut cursor = body.walk();
        for child in body.named_children(&mut cursor) {
            if matches!(child.kind(), "field_definition" | "public_field_definition") {
                if let Some(prop) = child.child_by_field_name("property").or_else(|| child.child_by_field_name("name")) {
                    if text(prop, source) == field_name {
                        let ty_node = child.child_by_field_name("type")?;
                        let annotation = ty_node.named_child(0).unwrap_or(ty_node);
                        return element_type_from_annotation(annotation, arrow, file, source);
                    }
                }
            }
        }
        None
    } else if receiver.kind() == "call_expression" {
        let callee = receiver.child_by_field_name("function")
            .filter(|callee| callee.kind() == "member_expression")?;
        let method = callee.child_by_field_name("property")?;
        let method_name = text(method, source);
        if matches!(method_name, "querySelectorAll" | "getElementsByTagName" | "getElementsByClassName") {
            let object = callee.child_by_field_name("object")?;
            if dom_receiver_from_ast(object, source, file, 0).is_some() {
                return Some("Element");
            }
        }
        None
    } else {
        None
    }
}

fn array_type_shadowed(node: Syntax<'_>, file: &FileContext) -> bool {
    let mut current = Some(node);
    while let Some(scope) = current {
        if file.shadowed_array_type_scopes.contains(&scope.id()) {
            return true;
        }
        current = scope.parent();
    }
    false
}

fn dom_global_shadowed(node: Syntax<'_>, file: &FileContext, name: &str) -> bool {
    let mut current = Some(node);
    while let Some(scope) = current {
        if file.shadowed_dom_global_scopes.get(&scope.id())
            .is_some_and(|names| names.contains(name)) {
            return true;
        }
        current = scope.parent();
    }
    false
}

fn dom_global_shadowed_in_scopes(scopes: &[usize], file: &FileContext, name: &str) -> bool {
    scopes.iter().any(|scope| {
        file.shadowed_dom_global_scopes
            .get(scope)
            .is_some_and(|names| names.contains(name))
    })
}

fn dom_indexed_const<'a>(
    file: &'a FileContext,
    name: &str,
    at: usize,
    scopes: &[usize],
) -> Option<Result<&'a DomListenerLocalDeclaration, usize>> {
    for scope in scopes {
        let declaration = file
            .dom_listener_local_declarations
            .get(scope)
            .and_then(|bindings| bindings.get(name));
        let rebound = file
            .dom_listener_local_writes
            .get(scope)
            .and_then(|names| names.get(name))
            .is_some_and(|writes| writes.iter().any(|position| *position < at));
        if declaration.is_some() || rebound {
            let valid = declaration.filter(|declaration| {
                declaration.declaration_count == 1
                    && declaration.is_const
                    && declaration.statement_end <= at
                    && declaration.initializer.is_some()
                    && !rebound
            });
            return Some(valid.ok_or(*scope));
        }
        if file
            .dom_listener_parameter_bindings
            .get(scope)
            .is_some_and(|names| names.contains(name))
        {
            return Some(Err(*scope));
        }
    }
    None
}

fn dom_receiver_from_indexed_expression(
    expression: &DomListenerReceiverExpression,
    file: &FileContext,
    scopes: &[usize],
    depth: usize,
) -> Option<&'static str> {
    if depth > 4 {
        return None;
    }
    match expression {
        DomListenerReceiverExpression::Identifier { name, position } => {
            if matches!(name.as_str(), "document" | "window")
                && !dom_global_shadowed_in_scopes(scopes, file, name)
            {
                return Some(if name == "document" { "Document" } else { "Window" });
            }
            match dom_indexed_const(file, name, *position, scopes)? {
                Ok(declaration) => dom_receiver_from_indexed_expression(
                    declaration.initializer.as_ref()?,
                    file,
                    &declaration.scope_chain,
                    depth + 1,
                ),
                Err(scope) => {
                    let rebound = file
                        .rebound_function_parameters
                        .get(&scope)
                        .is_some_and(|names| names.contains(name));
                    if rebound {
                        return None;
                    }
                    let ty = file.dom_listener_receivers.get(&scope)?.get(name)?;
                    typed_dom_receiver(ty)
                        .filter(|_| !dom_global_shadowed_in_scopes(scopes, file, ty))
                }
            }
        }
        DomListenerReceiverExpression::ThisField {
            name,
            class_scope,
            static_method,
        } => {
            if *static_method {
                return None;
            }
            let ty = file
                .dom_listener_class_fields
                .get(class_scope)?
                .get(name)?
                .as_deref()?;
            typed_dom_receiver(ty)
                .filter(|_| !dom_global_shadowed_in_scopes(scopes, file, ty))
        }
        DomListenerReceiverExpression::MemberCall {
            receiver,
            method,
            type_arguments,
            has_arguments,
        } => {
            if !has_arguments {
                return None;
            }
            if !type_arguments.is_empty()
                && (type_arguments.len() != 1
                    || typed_dom_receiver(&type_arguments[0]) != Some("Element")
                    || dom_global_shadowed_in_scopes(scopes, file, &type_arguments[0]))
            {
                return None;
            }
            let owner =
                dom_receiver_from_indexed_expression(receiver, file, scopes, depth + 1)?;
            let produces_element = match owner {
                "Document" => {
                    matches!(
                        method.as_str(),
                        "getElementById" | "createElement" | "querySelector"
                    )
                }
                "Element" => matches!(method.as_str(), "querySelector" | "closest"),
                _ => false,
            };
            produces_element.then_some("Element")
        }
        DomListenerReceiverExpression::Parenthesized(inner) => {
            dom_receiver_from_indexed_expression(inner, file, scopes, depth + 1)
        }
        DomListenerReceiverExpression::Unsupported => None,
    }
}

fn dom_const_initializer<'tree>(
    node: Syntax<'tree>,
    file: &FileContext,
    name: &str,
) -> Option<Result<Syntax<'tree>, Syntax<'tree>>> {
    let at = node.start_byte();
    let mut current = node.parent();
    while let Some(scope) = current {
        if matches!(scope.kind(), "statement_block" | "program" | "source_file") {
            let declaration = file
                .dom_listener_local_declarations
                .get(&scope.id())
                .and_then(|bindings| bindings.get(name));
            let rebound = file
                .dom_listener_local_writes
                .get(&scope.id())
                .and_then(|names| names.get(name))
                .is_some_and(|writes| writes.iter().any(|position| *position < at));
            if declaration.is_some() || rebound {
                return Some(if declaration.is_some_and(|declaration| {
                    declaration.declaration_count == 1
                        && declaration.is_const
                        && declaration.statement_end <= at
                        && declaration.initializer.is_some()
                        && !rebound
                }) {
                    Ok(scope)
                } else {
                    Err(scope)
                });
            }
        }
        if file
            .dom_listener_parameter_bindings
            .get(&scope.id())
            .is_some_and(|names| names.contains(name))
        {
            return Some(Err(scope));
        }
        current = scope.parent();
    }
    None
}

fn dom_receiver_from_ast(
    receiver: Syntax<'_>,
    source: &str,
    file: &FileContext,
    depth: usize,
) -> Option<&'static str> {
    if depth > 4 {
        return None;
    }
    match receiver.kind() {
        "identifier" => {
            let name = text(receiver, source);
            if matches!(name, "document" | "window") {
                return (!dom_global_shadowed(receiver, file, name))
                    .then_some(if name == "document" { "Document" } else { "Window" });
            }
            match dom_const_initializer(receiver, file, name)? {
                Ok(scope) => {
                    let declaration = file
                        .dom_listener_local_declarations
                        .get(&scope.id())?
                        .get(name)?;
                    dom_receiver_from_indexed_expression(
                        declaration.initializer.as_ref()?,
                        file,
                        &declaration.scope_chain,
                        depth + 1,
                    )
                }
                Err(scope) if matches!(scope.kind(), "function_declaration" | "function_expression" | "method_definition" | "arrow_function") => {
                        if file.rebound_function_parameters.get(&scope.id())
                            .is_some_and(|names| names.contains(name)) {
                            return None;
                        }
                        let ty = file.dom_listener_receivers.get(&scope.id())?.get(name)?;
                        typed_dom_receiver(ty)
                            .filter(|_| !dom_global_shadowed(receiver, file, ty))
                }
                Err(_) => None,
            }
        }
        "member_expression" => {
            let object = receiver.child_by_field_name("object")?;
            let property = receiver.child_by_field_name("property")
                .filter(|property| property.kind() == "property_identifier")?;
            if object.kind() == "this" {
                let (method, class) = lexical_this_method(receiver)?;
                if (0..method.child_count()).any(|index| {
                    method.child(index).is_some_and(|child| child.kind() == "static")
                }) {
                    return None;
                }
                let ty = file.dom_listener_class_fields.get(&class.id())?
                    .get(text(property, source))?.as_deref()?;
                return typed_dom_receiver(ty)
                    .filter(|_| !dom_global_shadowed(receiver, file, ty));
            }
            None
        }
        "call_expression" => {
            let callee = receiver.child_by_field_name("function")
                .filter(|callee| callee.kind() == "member_expression")?;
            let object = callee.child_by_field_name("object")?;
            let method = callee.child_by_field_name("property")
                .filter(|property| property.kind() == "property_identifier")?;
            let arguments = receiver.child_by_field_name("arguments")?;
            if arguments.named_child_count() == 0 {
                return None;
            }
            if let Some(type_arguments) = receiver.child_by_field_name("type_arguments") {
                let ty = type_arguments.named_child(0)
                    .filter(|_| type_arguments.named_child_count() == 1)?;
                let name = text(ty, source);
                if ty.kind() != "type_identifier"
                    || typed_dom_receiver(name) != Some("Element")
                    || dom_global_shadowed(receiver, file, name)
                {
                    return None;
                }
            }
            let owner = dom_receiver_from_ast(object, source, file, depth + 1)?;
            let method = text(method, source);
            let produces_element = match owner {
                "Document" => matches!(method, "getElementById" | "createElement" | "querySelector"),
                "Element" => matches!(method, "querySelector" | "closest"),
                _ => false,
            };
            produces_element.then_some("Element")
        }
        "parenthesized_expression" => receiver.named_child(0)
            .and_then(|inner| dom_receiver_from_ast(inner, source, file, depth + 1)),
        _ => None,
    }
}

fn dom_event_callback_parameter<'a>(callback: Syntax<'_>, source: &'a str, file: &FileContext) -> Option<&'a str> {
    if let Some(args) = callback.parent().filter(|parent| parent.kind() == "arguments") {
        if args.named_child(1) != Some(callback) || args.named_child_count() > 3 {
            return None;
        }
        let event = args.named_child(0).filter(|event| event.kind() == "string")?;
        let literal = text(event, source);
        if literal.len() < 2 || literal.contains('\\') || !matches!(literal.as_bytes()[0], b'\'' | b'"')
            || literal.as_bytes()[0] != *literal.as_bytes().last()? {
            return None;
        }
        let call = args.parent().filter(|parent| parent.kind() == "call_expression")?;
        let callee = call.child_by_field_name("function")
            .filter(|callee| callee.kind() == "member_expression")?;
        let member = callee.child_by_field_name("property")?;
        if member.kind() != "property_identifier" || text(member, source) != "addEventListener" {
            return None;
        }
        let receiver = callee.child_by_field_name("object")?;
        let verified_receiver = matches!(
            dom_receiver_from_ast(receiver, source, file, 0),
            Some("Element" | "Document" | "Window" | "EventTarget")
        );
        if !verified_receiver || dom_global_shadowed(callback, file, "Event")
            || dom_global_shadowed(callback, file, "EventTarget") {
            return None;
        }
    } else {
        let assignment = callback.parent().filter(|p| p.kind() == "assignment_expression")?;
        if assignment.child_by_field_name("right") != Some(callback) {
            return None;
        }
        let left = assignment.child_by_field_name("left")
            .filter(|left| left.kind() == "member_expression")?;
        let property = left.child_by_field_name("property")?;
        let prop_name = text(property, source);
        if !prop_name.starts_with("on") || prop_name.len() <= 2 {
            return None;
        }
        let receiver = left.child_by_field_name("object")?;
        let verified_receiver = matches!(
            dom_receiver_from_ast(receiver, source, file, 0),
            Some("Element" | "Document" | "Window" | "EventTarget")
        );
        if !verified_receiver || dom_global_shadowed(callback, file, "Event") {
            return None;
        }
    }
    let parameter = callback.child_by_field_name("parameter")
        .or_else(|| callback.child_by_field_name("parameters")?.named_child(0))?;
    let parameter = if parameter.kind() == "identifier" {
        parameter
    } else {
        parameter.child_by_field_name("name")
            .or_else(|| parameter.child_by_field_name("pattern"))?
    };
    (parameter.kind() == "identifier").then(|| text(parameter, source))
}

fn nitro_event_callback_parameter<'a>(callback: Syntax<'_>, source: &'a str) -> Option<&'a str> {
    let call = callback.parent().filter(|p| p.kind() == "arguments")?.parent()?;
    if call.kind() != "call_expression" {
        return None;
    }
    let callee = call.child_by_field_name("function")?;
    if callee.kind() != "identifier" || text(callee, source) != "defineEventHandler" {
        return None;
    }
    let parameter = callback.child_by_field_name("parameter")
        .or_else(|| callback.child_by_field_name("parameters")?.named_child(0))?;
    let parameter = if parameter.kind() == "identifier" {
        parameter
    } else {
        parameter.child_by_field_name("name")
            .or_else(|| parameter.child_by_field_name("pattern"))?
    };
    (parameter.kind() == "identifier").then(|| text(parameter, source))
}

/// TypeScript family profile that shares syntax rules with JavaScript.
pub struct TypeScript {
    /// Enables JavaScript syntax and file extensions when true.
    pub javascript: bool,
}
/// Shared typescript language profile.
pub static TYPESCRIPT: TypeScript = TypeScript { javascript: false };
/// Shared javascript language profile.
pub static JAVASCRIPT: TypeScript = TypeScript { javascript: true };

fn local_package_reference(value: &str) -> bool {
    ["workspace:", "link:", "file:", "portal:"].iter().any(|prefix| value.starts_with(prefix))
}

fn local_json_package(value: &serde_json::Value) -> bool {
    value.get("link").and_then(serde_json::Value::as_bool) == Some(true)
        || value.as_str().is_some_and(local_package_reference)
        || ["version", "specifier", "resolved"].iter().any(|field| {
            value.get(*field).and_then(serde_json::Value::as_str).is_some_and(local_package_reference)
        })
}

fn npm_dependency_keys(value: &serde_json::Value, output: &mut Vec<String>) {
    for section in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        if let Some(entries) = value.get(section).and_then(serde_json::Value::as_object) {
            output.extend(entries.iter().filter(|(_, descriptor)| !local_json_package(descriptor)).map(|(name, _)| name.clone()));
        }
    }
}

fn package_lock_dependencies(content: &str) -> Vec<String> {
    let Ok(lock) = serde_json::from_str::<serde_json::Value>(content) else {
        return Vec::new();
    };
    let mut dependencies = Vec::new();
    npm_dependency_keys(&lock, &mut dependencies);
    if let Some(entries) = lock.get("dependencies").and_then(serde_json::Value::as_object) {
        let mut pending: Vec<_> = entries.values().collect();
        while let Some(entry) = pending.pop() {
            if local_json_package(entry) {
                continue;
            }
            npm_dependency_keys(entry, &mut dependencies);
            if let Some(nested) = entry.get("dependencies").and_then(serde_json::Value::as_object) {
                pending.extend(nested.values());
            }
        }
    }
    if let Some(packages) = lock.get("packages").and_then(serde_json::Value::as_object) {
        for (path, package) in packages {
            if local_json_package(package) {
                continue;
            }
            if let Some((_, name)) = path.rsplit_once("node_modules/") {
                if !name.is_empty() {
                    dependencies.push(name.to_owned());
                }
            }
            npm_dependency_keys(package, &mut dependencies);
        }
    }
    dependencies
}

fn pnpm_package_name(locator: &str) -> Option<&str> {
    let locator = locator.trim_start_matches('/');
    let version = if let Some(scoped) = locator.strip_prefix('@') {
        let package = scoped.find('/').map(|index| index + 2)?;
        locator[package..].find(['@', '/']).map(|index| index + package)
    } else {
        locator.find(['@', '/'])
    }?;
    let descriptor = &locator[version + 1..];
    (version > 0 && !descriptor.is_empty() && !local_package_reference(descriptor))
        .then_some(&locator[..version])
}

fn local_yaml_package(value: &serde_yaml::Value) -> bool {
    value.as_str().is_some_and(local_package_reference)
        || ["specifier", "version"].iter().any(|field| {
            value.get(*field).and_then(serde_yaml::Value::as_str).is_some_and(local_package_reference)
        })
}

fn pnpm_lock_dependencies(content: &str) -> Vec<String> {
    let Ok(lock) = serde_yaml::from_str::<serde_yaml::Value>(content) else {
        return Vec::new();
    };
    let mut dependencies = Vec::new();
    for section in ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"] {
        if let Some(entries) = lock.get(section).and_then(serde_yaml::Value::as_mapping) {
            dependencies.extend(entries.iter().filter(|(_, descriptor)| !local_yaml_package(descriptor))
                .filter_map(|(name, _)| name.as_str()).map(str::to_owned));
        }
    }
    if let Some(importers) = lock.get("importers").and_then(serde_yaml::Value::as_mapping) {
        for importer in importers.values() {
            for section in ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"] {
                if let Some(entries) = importer.get(section).and_then(serde_yaml::Value::as_mapping) {
                    dependencies.extend(entries.iter().filter(|(_, descriptor)| !local_yaml_package(descriptor))
                        .filter_map(|(name, _)| name.as_str()).map(str::to_owned));
                }
            }
        }
    }
    for section in ["packages", "snapshots"] {
        if let Some(packages) = lock.get(section).and_then(serde_yaml::Value::as_mapping) {
            dependencies.extend(packages.keys().filter_map(serde_yaml::Value::as_str).filter_map(pnpm_package_name).map(str::to_owned));
        }
    }
    dependencies
}

fn yarn_package_name(selector: &str) -> Option<&str> {
    let selector = selector.trim().trim_matches(['\'', '"']);
    let version = if let Some(scoped) = selector.strip_prefix('@') {
        scoped.find('@').map(|index| index + 1)
    } else {
        selector.find('@')
    }?;
    let descriptor = &selector[version + 1..];
    (version > 0 && !descriptor.is_empty() && !local_package_reference(descriptor))
        .then_some(&selector[..version])
}

fn yarn_lock_dependencies(content: &str) -> Vec<String> {
    let mut dependencies = Vec::new();
    for line in content.lines() {
        if line.is_empty() || line.starts_with([' ', '\t', '#']) || !line.ends_with(':') {
            continue;
        }
        for selector in line.trim_end_matches(':').split(',') {
            if let Some(name) = yarn_package_name(selector) {
                dependencies.push(name.to_owned());
            }
        }
    }
    dependencies
}

#[derive(Default)]
struct TsCandidate {
    source: String,
    expression: String,
    line: usize,
    column: usize,
    is_member: bool,
}

#[derive(Default)]
struct JsPrototypeAlias {
    owner: String,
    member: String,
    implementation: String,
    line: usize,
    column: usize,
}

#[derive(Default)]
struct TypeScriptScratch {
    candidates: Vec<TsCandidate>,
    assigned: std::collections::HashSet<String>,
    assignment_overflow: bool,
    writes: std::collections::HashMap<String, u64>,
    write_overflow: bool,
    aliases: Vec<JsPrototypeAlias>,
    alias_overflow: bool,
}

impl TypeScriptScratch {
    fn reset(&mut self) {
        self.candidates.clear();
        self.assigned.clear();
        self.assignment_overflow = false;
        self.writes.clear();
        self.write_overflow = false;
        self.aliases.clear();
        self.alias_overflow = false;
    }
}

thread_local! {
    static TS_SCRATCH: std::cell::RefCell<TypeScriptScratch> = std::cell::RefCell::new(TypeScriptScratch::default());
}

impl LanguageProfile for TypeScript {
    fn id(&self) -> &'static str {
        if self.javascript {
            "javascript"
        } else {
            "typescript"
        }
    }
    fn receiver(&self, name: &str, owner: &Node) -> bool {
        name == "this" && owner.details["receiver_name"].as_str() == Some("this")
    }
    fn manifest_filenames(&self) -> &'static [&'static str] {
        &["package.json", "package-lock.json", "pnpm-lock.yaml", "pnpm-workspace.yaml", "yarn.lock", "tsconfig*.json"]
    }
    fn extract_manifest_dependencies(&self, filename: &str, content: &str) -> Vec<String> {
        match filename {
            "package-lock.json" => return package_lock_dependencies(content),
            "pnpm-lock.yaml" => return pnpm_lock_dependencies(content),
            "yarn.lock" => return yarn_lock_dependencies(content),
            _ => {}
        }
        if filename != "package.json" {
            return Vec::new();
        }
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(content) else {
            return Vec::new();
        };
        ["dependencies", "devDependencies", "peerDependencies"]
            .into_iter()
            .filter_map(|section| manifest.get(section)?.as_object())
            .flat_map(|dependencies| dependencies.iter().filter(|(_, value)| !local_json_package(value)).map(|(name, _)| name.clone()))
            .collect()
    }
    fn is_stdlib(&self, module: &str) -> bool {
        const NODE_BUILTINS: &[&str] = &[
            "assert",
            "assert/strict",
            "async_hooks",
            "buffer",
            "child_process",
            "cluster",
            "console",
            "constants",
            "crypto",
            "dgram",
            "diagnostics_channel",
            "dns",
            "dns/promises",
            "domain",
            "events",
            "fs",
            "fs/promises",
            "http",
            "http2",
            "https",
            "inspector",
            "inspector/promises",
            "module",
            "net",
            "os",
            "path",
            "path/posix",
            "path/win32",
            "perf_hooks",
            "process",
            "punycode",
            "querystring",
            "readline",
            "readline/promises",
            "repl",
            "stream",
            "stream/consumers",
            "stream/promises",
            "stream/web",
            "string_decoder",
            "sys",
            "timers",
            "timers/promises",
            "tls",
            "trace_events",
            "tty",
            "url",
            "util",
            "util/types",
            "v8",
            "vm",
            "wasi",
            "worker_threads",
            "zlib",
            "test",
            "test/reporters",
        ];
        if module.starts_with("node:") {
            return true;
        }
        let specifier = module.strip_prefix("node:").unwrap_or(module);
        NODE_BUILTINS.contains(&specifier)
    }
    fn extensions(&self) -> &'static [&'static str] {
        if self.javascript {
            &["js", "jsx", "mjs", "cjs"]
        } else {
            &["ts", "tsx", "mts", "cts"]
        }
    }
    fn grammar(&self, path: &str) -> tree_sitter::Language {
        language(path, self.id())
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("javascript")
    }
    fn symbol_kind(&self, k: &str) -> Option<&'static str> {
        kind(k)
    }
    fn symbol_with_source(&self, node: Syntax<'_>, source: &str) -> Option<&'static str> {
        if self.javascript && prototype_assignment(node, source).is_some() {
            return Some("method");
        }
        if !self.javascript {
            if node.kind() == "property_identifier"
                && node.parent().is_some_and(|parent| parent.kind() == "enum_body")
            {
                return Some("field");
            }
            if node.kind() == "enum_assignment"
                && !node.child_by_field_name("name")
                    .is_some_and(|name| name.kind() == "property_identifier")
            {
                return None;
            }
        }
        if self.javascript && node.kind() == "pair"
            && node.child_by_field_name("key")
                .is_some_and(|key| matches!(key.kind(), "property_identifier" | "identifier"))
            && node.child_by_field_name("value").is_some_and(|value| {
                !matches!(value.kind(), "arrow_function" | "function_expression")
            })
            && node.parent().and_then(|object| same_object(object, source)).is_some()
        {
            return Some("field");
        }
        if node.kind() == "field_definition"
            && !node.child_by_field_name("property")
                .is_some_and(|property| property.kind() == "property_identifier")
        {
            return None;
        }
        self.symbol(node)
    }
    fn symbol_name<'a>(&self, node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
        if self.javascript {
            if let Some((_, member)) = prototype_assignment(node, source) {
                return Some(member);
            }
        }
        if node.kind() == "property_identifier"
            && node.parent().is_some_and(|parent| parent.kind() == "enum_body")
        {
            return Some(text(node, source));
        }
        if node.kind() == "pair" {
            return field(node, source, "key");
        }
        if node.kind() == "field_definition" {
            return field(node, source, "property");
        }
        ast::symbol_name(node, source)
    }
    fn node_prefix(&self, _kind: &str) -> &'static str {
        "ts"
    }
    fn module_name(&self, path: &str) -> String {
        module_stem(path)
            .trim_end_matches("/index")
            .replace('/', ".")
    }
    fn extract_file(
        &self,
        path: &str,
        source: &str,
        module: &str,
        facts: &mut Facts,
    ) -> Result<()> {
        parse_file(self, path, source, module, facts)
    }
    fn extract_imports(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        let (node, source) = (ctx.node, ctx.source);
        if let Some(module) = facts.nodes.iter_mut().find(|node| node.kind == "module") {
            if module.path.ends_with("/nuxt.config.ts") || module.path == "nuxt.config.ts" {
                if nuxt_module_declared(node, source, "radix-vue/nuxt") {
                    module.details["nuxt_radix_vue_enabled"] = serde_json::json!(true);
                }
                if let Some(enabled) = nuxt_default_components_enabled(node, source) {
                    module.details["nuxt_default_components_enabled"] = serde_json::json!(enabled);
                }
            }
        }
        if node.kind() == "import_statement" {
            let before = facts.references.len();
            let module =
                field(node, source, "source").map(|s| s.trim_matches(['\'', '"']).to_owned());
            fn ids(node: Syntax<'_>, source: &str, values: &mut Vec<(String, Option<String>, bool)>) {
                match node.kind() {
                    "import_specifier" => {
                        let name =
                            field(node, source, "name").unwrap_or_else(|| text(node, source));
                        values.push((
                            name.into(),
                            field(node, source, "alias")
                                .map(str::to_owned)
                                .or_else(|| Some(name.into())),
                            has_type_import_modifier(node),
                        ));
                    }
                    "namespace_import" => {
                        if let Some(n) = node.named_child(0) {
                            values.push(("*".into(), Some(text(n, source).into()), false));
                        }
                    }
                    "identifier" => {
                        values.push(("default".into(), Some(text(node, source).into()), false))
                    }
                    _ => {
                        let mut c = node.walk();
                        for n in node.named_children(&mut c) {
                            ids(n, source, values);
                        }
                    }
                }
            }
            let mut names = Vec::new();
            let mut type_only_aliases = Vec::new();
            let mut c = node.walk();
            for n in node.named_children(&mut c) {
                if n.kind() == "import_clause" {
                    ids(n, source, &mut names);
                }
            }
            if names.is_empty() {
                ctx.import(facts, "*".into(), None, module);
            } else {
                for (index, (name, alias, type_only)) in names.into_iter().enumerate() {
                    ctx.import(facts, name, alias, module.clone());
                    if type_only {
                        type_only_aliases.push(before + index);
                    }
                }
            }
            let statement_type_only = has_type_import_modifier(node);
            for (index, reference) in facts.references.iter_mut().enumerate().skip(before) {
                if statement_type_only || type_only_aliases.contains(&index) {
                    reference.receiver_hint = Some(ReceiverHint::TypeOnlyImport);
                }
            }
            return;
        }
        let mut add = |expression, alias, module| ctx.import(facts, expression, alias, module);
        if node.kind() == "export_statement" {
            if let Some(module) = field(node, source, "source")
                .map(|module| module.trim_matches(['\'', '"']).to_owned())
            {
                add(module.clone(), None, Some(module));
            }
        } else if node.kind() == "assignment_expression" && !require_is_shadowed(ctx) {
            if let Some(module) = node
                .child_by_field_name("right")
                .and_then(|right| required_module(right, source))
            {
                if field(node, source, "left").is_some_and(|left| {
                    left == "module.exports"
                        || left.starts_with("exports.")
                        || left.starts_with("module.exports.")
                }) {
                    add(module.clone(), None, Some(module));
                }
            }
        } else if node.kind() == "variable_declarator" {
            let Some(value) = node.child_by_field_name("value") else {
                return;
            };
            let Some(module) = required_module(value, source) else {
                return;
            };
            if require_is_shadowed(ctx) {
                return;
            }
            let Some(pattern) = node.child_by_field_name("name") else {
                add(module.clone(), None, Some(module));
                return;
            };
            if pattern.kind() == "identifier" {
                add("*".into(), Some(text(pattern, source).into()), Some(module));
                return;
            }

            let mut imports = Vec::new();
            collect_require_bindings(pattern, source, &mut imports);
            if imports.is_empty() {
                add(module.clone(), None, Some(module));
            } else {
                for (name, alias) in imports {
                    add(name, alias, Some(module.clone()));
                }
            }
        }
    }
    fn extract_calls(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if ctx.node.kind() == "member_expression"
            && field(ctx.node, ctx.source, "object") == Some("process")
            && field(ctx.node, ctx.source, "property") == Some("env")
        {
            facts.references.push(Reference {
                source: ctx.owner.into(),
                dynamic: false,
                expression: "process.env".into(),
                kind: "references".into(),
                line: ctx.line(),
                column: ctx.node.start_position().column,
                alias: None,
                module: None,
                receiver_hint: None,
            });
        }
        if ctx.node.kind() == "new_expression" {
            if let Some(constructor) = field(ctx.node, ctx.source, "constructor")
                .filter(|name| matches!(*name, "URL" | "Response" | "Request" | "Headers"))
            {
                facts.references.push(Reference {
                    source: ctx.owner.into(),
                    dynamic: false,
                    expression: constructor.into(),
                    kind: "calls".into(),
                    line: ctx.line(),
                    column: ctx.node.start_position().column,
                    alias: None,
                    module: None,
                    receiver_hint: None,
                });
            }
        }
        if matches!(ctx.node.kind(), "call_expression") {
            if required_module(ctx.node, ctx.source).is_some() {
                if require_is_shadowed(ctx) {
                    call(ctx, facts, false);
                } else if !ctx
                    .node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "variable_declarator")
                {
                    call(ctx, facts, true);
                }
            } else {
                let reference_count = facts.references.len();
                call(ctx, facts, true);
                if self.javascript {
                    if let Some(function) = ctx.node.child_by_field_name("function") {
                        if let Some(expression) = static_member_callee(function, ctx.source) {
                            if let Some(reference) = facts.references.get_mut(reference_count).filter(|reference| {
                                reference.source == ctx.owner
                                    && reference.kind == "calls"
                                    && reference.line == ctx.line()
                            }) {
                                reference.expression = expression;
                                reference.dynamic = false;
                            }
                        }
                    }
                }
                if !self.javascript {
                    if let Some(reference) = facts.references.get_mut(reference_count) {
                        if reference.source == ctx.owner && reference.kind == "calls"
                            && reference.line == ctx.line() && reference.expression.contains("?.")
                        {
                            let normalized = reference.expression.replace("?.", ".");
                            if normalized.split('.').all(|part| {
                                !part.is_empty() && part.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                            }) {
                                reference.dynamic = false;
                            }
                        }
                    }
                }
                if let Some(function) = ctx
                    .node
                    .child_by_field_name("function")
                    .filter(|function| function.kind() == "member_expression")
                {
                    if let (Some(mut receiver), Some(member)) = (
                        function.child_by_field_name("object"),
                        function
                            .child_by_field_name("property")
                            .filter(|member| member.kind() == "property_identifier"),
                    ) {
                        for _ in 0..8 {
                            if receiver.kind() != "parenthesized_expression" {
                                break;
                            }
                            let Some(inner) = receiver.named_child(0) else {
                                break;
                            };
                            receiver = inner;
                        }
                        let mut curr_receiver = receiver;
                        let mut member_path = vec![text(member, ctx.source)];
                        while curr_receiver.kind() == "member_expression" {
                            if let Some(prop) = curr_receiver
                                .child_by_field_name("property")
                                .filter(|prop| prop.kind() == "property_identifier")
                            {
                                member_path.push(text(prop, ctx.source));
                            } else {
                                break;
                            }
                            let Some(obj) = curr_receiver.child_by_field_name("object") else {
                                break;
                            };
                            curr_receiver = obj;
                            for _ in 0..8 {
                                if curr_receiver.kind() != "parenthesized_expression" {
                                    break;
                                }
                                let Some(inner) = curr_receiver.named_child(0) else {
                                    break;
                                };
                                curr_receiver = inner;
                            }
                        }
                        member_path.reverse();
                        let full_member = member_path.join(".");
                        let hint =
                            match curr_receiver.kind() {
                                "call_expression" => {
                                    curr_receiver.child_by_field_name("function").map(|callee| {
                                        ReceiverHint::CallResult {
                                            callee: text(callee, ctx.source).to_owned(),
                                            member: full_member,
                                        }
                                    })
                                }
                                "new_expression" => curr_receiver
                                    .child_by_field_name("constructor")
                                    .map(|callee| ReceiverHint::ConstructorResult {
                                        callee: text(callee, ctx.source).to_owned(),
                                        member: full_member,
                                    }),
                                "string" => Some(ReceiverHint::StringLiteral {
                                    member: full_member,
                                }),
                                _ => None,
                            };
                        if let Some(reference) = facts.references.last_mut() {
                            reference.receiver_hint = hint;
                        }
                    }
                }
            }
        }
    }
    fn builtin(&self, symbol: &str) -> bool {
        if browser_global_type(symbol) {
            return true;
        }
        if matches!(
            symbol,
            "Object.assign"
                | "Object.hasOwn"
                | "Object.defineProperty"
                | "Object.defineProperties"
                | "Object.keys"
                | "Object.values"
                | "Object.entries"
                | "Object.fromEntries"
                | "Object.create"
                | "Object.freeze"
                | "Object.seal"
                | "Object.getPrototypeOf"
                | "Object.getOwnPropertyDescriptor"
                | "Object.getOwnPropertyDescriptors"
                | "Object.getOwnPropertyNames"
                | "Object.getOwnPropertySymbols"
                | "Object.is"
                | "Number.isFinite"
                | "Number.isNaN"
                | "Number.isInteger"
                | "Number.isSafeInteger"
                | "Number.parseInt"
                | "Number.parseFloat"
                | "Reflect.apply"
                | "Reflect.construct"
                | "Reflect.defineProperty"
                | "Reflect.deleteProperty"
                | "Reflect.get"
                | "Reflect.getOwnPropertyDescriptor"
                | "Reflect.getPrototypeOf"
                | "Reflect.has"
                | "Reflect.isExtensible"
                | "Reflect.ownKeys"
                | "Reflect.preventExtensions"
                | "Reflect.set"
                | "Reflect.setPrototypeOf"
                | "crypto.getRandomValues"
                | "crypto.randomUUID"
                | "Promise.all"
                | "Date.now"
                | "CSS.escape"
                | "JSON.parse"
                | "JSON.stringify"
                | "Math.abs"
                | "Math.ceil"
                | "Math.floor"
                | "Math.round"
                | "Math.max"
                | "Math.min"
                | "Math.pow"
                | "Math.sqrt"
                | "Math.trunc"
                | "Math.random"
                | "Math.sign"
                | "console.log"
                | "console.error"
                | "console.warn"
                | "console.info"
                | "console.debug"
                | "console.trace"
                | "console.table"
                | "Buffer.from"
                | "Buffer.alloc"
                | "Buffer.concat"
                | "Buffer.isBuffer"
                | "process.env"
                | "process.cwd"
                | "process.nextTick"
                | "document.querySelector"
                | "document.querySelectorAll"
                | "document.getElementById"
                | "document.getElementsByClassName"
                | "document.getElementsByTagName"
                | "document.createElement"
                | "document.createTextNode"
                | "document.addEventListener"
                | "document.removeEventListener"
                | "window.addEventListener"
                | "window.removeEventListener"
                | "window.dispatchEvent"
                | "window.setTimeout"
                | "window.clearTimeout"
                | "window.setInterval"
                | "window.clearInterval"
                | "window.requestAnimationFrame"
                | "window.cancelAnimationFrame"
                | "localStorage.getItem"
                | "localStorage.setItem"
                | "localStorage.removeItem"
                | "localStorage.clear"
                | "sessionStorage.getItem"
                | "sessionStorage.setItem"
                | "sessionStorage.removeItem"
                | "sessionStorage.clear"
                | "Array.isArray"
                | "Array.from"
                | "Array.of"
                | "getComputedStyle"
                | "Alpine"
                | "Alpine.data"
                | "Alpine.store"
                | "Alpine.start"
                | "Alpine.plugin"
                | "Alpine.directive"
                | "Alpine.magic"
                | "Alpine.$data"
                | "window.Alpine"
        ) {
            return true;
        }
        matches!(
            symbol,
            "parseInt"
                | "parseFloat"
                | "encodeURIComponent"
                | "fetch"
                | "alert"
                | "confirm"
                | "prompt"
                | "String"
                | "Number"
                | "Reflect"
                | "crypto"
                | "Boolean"
                | "setTimeout"
                | "clearTimeout"
                | "setInterval"
                | "clearInterval"
                | "string"
                | "number"
                | "boolean"
                | "any"
                | "unknown"
                | "never"
                | "void"
                | "undefined"
                | "null"
                | "Promise"
                | "Date"
                | "CSS"
                | "Array"
                | "Record"
                | "Map"
                | "Set"
                | "Object"
                | "Function"
                | "Symbol"
                | "Error"
                | "Uint8Array"
                | "Partial"
                | "Required"
                | "Readonly"
                | "Pick"
                | "Omit"
                | "Exclude"
                | "Extract"
                | "NonNullable"
                | "ReturnType"
                | "InstanceType"
                | "Buffer"
                | "__dirname"
                | "__filename"
                | "clearImmediate"
                | "exports"
                | "global"
                | "globalThis"
                | "module"
                | "process"
                | "require"
                | "setImmediate"
        )
    }
    fn builtin_type(&self, name: &str) -> bool {
        if browser_global_type(name) {
            return true;
        }
        matches!(
            name,
            "string"
                | "number"
                | "boolean"
                | "String"
                | "Number"
                | "Boolean"
                | "Array"
                | "Map"
                | "Set"
                | "Object"
        )
    }

    fn builtin_generic(&self, receiver: &str) -> bool {
        matches!(receiver, "Array" | "Map" | "Set")
    }
    fn builtin_member(&self, receiver: &str, member: &str) -> bool {
        match receiver {
            "Web.Response" => matches!(member, "json" | "text" | "arrayBuffer" | "blob" | "formData" | "clone"),
            "H3Event" => matches!(member, "headers" | "context" | "node"),
            "Document" => matches!(
                member,
                "querySelector" | "querySelectorAll" | "getElementById"
                    | "getElementsByClassName" | "getElementsByTagName" | "createElement"
                    | "createTextNode" | "addEventListener" | "removeEventListener"
                    | "body" | "head" | "documentElement"
            ),
            "ParentNode" | "DocumentFragment" => matches!(
                member,
                "querySelector" | "querySelectorAll" | "appendChild" | "removeChild"
            ),
            "ChildNode" => matches!(member, "appendChild" | "removeChild"),
            _ if typed_dom_receiver(receiver) == Some("Element") => matches!(
                member,
                "querySelector" | "querySelectorAll" | "getAttribute" | "setAttribute"
                    | "removeAttribute" | "closest" | "matches" | "addEventListener"
                    | "removeEventListener" | "appendChild" | "removeChild" | "classList"
            ),
            "Window" => matches!(member, "addEventListener" | "removeEventListener" | "dispatchEvent"),
            "Alpine" => matches!(
                member,
                "data" | "store" | "start" | "plugin" | "directive" | "magic" | "$data"
            ),
            "EventTarget" => matches!(member, "addEventListener" | "removeEventListener" | "dispatchEvent"),
            "Event" | "CustomEvent" | "MouseEvent" | "KeyboardEvent"
            | "DragEvent" | "ClipboardEvent" | "FocusEvent" => matches!(
                member,
                "stopPropagation" | "stopImmediatePropagation" | "preventDefault"
            ),
            "Storage" => matches!(
                member,
                "getItem" | "setItem" | "removeItem" | "clear" | "key" | "length"
            ),
            "URLSearchParams" => matches!(
                member,
                "append" | "delete" | "get" | "getAll" | "has" | "set" | "sort" | "forEach" | "entries" | "keys" | "values"
            ),
            "Headers" => matches!(
                member,
                "append" | "delete" | "get" | "getSetCookie" | "has" | "set" | "forEach" | "entries" | "keys" | "values"
            ),
            "DOMTokenList" => matches!(
                member,
                "add" | "remove" | "toggle" | "contains" | "replace" | "supports" | "entries" | "forEach" | "keys" | "values"
            ),
            "URL" => matches!(
                member,
                "hash" | "host" | "hostname" | "href" | "origin" | "password" | "pathname" | "port" | "protocol" | "search" | "searchParams" | "username" | "toString" | "toJSON"
            ),
            "string" | "String" => matches!(
                member,
                "charAt"
                    | "charCodeAt"
                    | "codePointAt"
                    | "concat"
                    | "endsWith"
                    | "includes"
                    | "indexOf"
                    | "lastIndexOf"
                    | "match"
                    | "matchAll"
                    | "normalize"
                    | "padEnd"
                    | "padStart"
                    | "repeat"
                    | "replace"
                    | "replaceAll"
                    | "search"
                    | "slice"
                    | "split"
                    | "startsWith"
                    | "substring"
                    | "toLowerCase"
                    | "toUpperCase"
                    | "trim"
                    | "trimEnd"
                    | "trimStart"
                    | "at"
            ),
            "Array" => matches!(
                member,
                "at" | "concat"
                    | "copyWithin"
                    | "entries"
                    | "every"
                    | "fill"
                    | "filter"
                    | "find"
                    | "findIndex"
                    | "findLast"
                    | "findLastIndex"
                    | "flat"
                    | "flatMap"
                    | "forEach"
                    | "includes"
                    | "indexOf"
                    | "join"
                    | "keys"
                    | "lastIndexOf"
                    | "map"
                    | "pop"
                    | "push"
                    | "reduce"
                    | "reduceRight"
                    | "reverse"
                    | "shift"
                    | "slice"
                    | "some"
                    | "sort"
                    | "splice"
                    | "unshift"
                    | "values"
            ),
            "Map" => matches!(
                member,
                "clear"
                    | "delete"
                    | "entries"
                    | "forEach"
                    | "get"
                    | "has"
                    | "keys"
                    | "set"
                    | "values"
            ),
            "Set" => matches!(
                member,
                "add" | "clear" | "delete" | "entries" | "forEach" | "has" | "keys" | "values"
            ),
            _ => false,
        }
    }

    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath> {
        self.normalize_import_with_root(None, owner, module)
    }

    fn normalize_import_with_root(
        &self,
        _root: Option<&Path>,
        owner: &str,
        module: &str,
    ) -> Option<ImportPath> {
        let path = Path::new(module);
        let module = if for_path(path).is_some_and(|p| p.family() == LanguageFamily("javascript")) {
            module_stem(module)
        } else {
            module
        };
        let module = module.trim_end_matches("/index");
        if module.starts_with("./") || module.starts_with("../") {
            Some(ImportPath {
                namespace: relative_namespace(owner, module)?,
                relative: true,
                symbol_path: false,
            })
        } else {
            Some(ImportPath::absolute(module.replace('/', ".")))
        }
    }
    fn external_import(&self, module: &str) -> Option<&'static str> {
        self.is_stdlib(module).then_some("Node.js standard library")
    }
    fn prepare(&self, root: Syntax<'_>, source: &str) -> FileContext {
        let mut file = FileContext {
            exports: value_flow::exports(root, source),
            ..Default::default()
        };
        let needs_dom_scope_facts =
            source.contains("addEventListener") || has_dom_event_property(source);
        let needs_array_callback_scope_facts = !self.javascript
            && has_member_method(
                source,
                &["forEach", "map", "filter", "find", "some", "every", "flatMap"],
            );
        let needs_dom_array_receiver_facts = needs_array_callback_scope_facts
            && ["querySelectorAll", "getElementsByTagName", "getElementsByClassName"]
                .iter()
                .any(|method| source.contains(method));
        let needs_full_scope_facts = needs_dom_scope_facts
            || needs_dom_array_receiver_facts
            || source.contains("require");
        if needs_full_scope_facts {
            collect_scope_facts(root, source, &mut file, !self.javascript);
        }
        let needs_partial_scope_facts =
            (self.javascript && (needs_dom_scope_facts || needs_array_callback_scope_facts))
                || (!needs_full_scope_facts
                    && needs_array_callback_scope_facts
                    && !needs_dom_array_receiver_facts);
        if needs_partial_scope_facts {
            collect_array_callback_scope_facts(root, source, &mut file);
        }
        let mut c = root.walk();
        for node in root
            .named_children(&mut c)
            .filter(|n| n.kind() == "export_statement" && n.child_by_field_name("source").is_none())
        {
            if text(node, source)
                .trim_start()
                .starts_with("export default")
            {
                if let Some(value) = node
                    .child_by_field_name("value")
                    .filter(|n| n.kind() == "identifier")
                {
                    file.default_exports.insert(text(value, source).into());
                }
            }
            let mut c = node.walk();
            for clause in node
                .named_children(&mut c)
                .filter(|n| n.kind() == "export_clause")
            {
                let mut c = clause.walk();
                for specifier in clause.named_children(&mut c) {
                    if field(specifier, source, "alias") == Some("default") {
                        if let Some(name) = field(specifier, source, "name") {
                            file.default_exports.insert(name.into());
                        }
                    }
                }
            }
        }
        file
    }

    fn value_flow(&self, node: Syntax<'_>, source: &str) -> crate::core::semantic::ValueFlowFacts {
        value_flow::extract(node, source)
    }

    fn metadata(
        &self,
        mut node: Syntax<'_>,
        source: &str,
        name: &str,
        file: &FileContext,
    ) -> SymbolMetadata {
        let mut param_types = BTreeMap::new();
        let documented = value_flow::jsdoc(node, source).0;
        if let Some(parameters) = node.child_by_field_name("parameters") {
            let mut cursor = parameters.walk();
            for parameter in parameters.named_children(&mut cursor) {
                let mut p_cursor = parameter.walk();
                let name = parameter
                    .child_by_field_name("pattern")
                    .or_else(|| parameter.child_by_field_name("name"))
                    .or_else(|| (parameter.kind() == "identifier").then_some(parameter))
                    .or_else(|| parameter.named_children(&mut p_cursor).find(|c| c.kind() == "identifier"))
                    .filter(|node| node.kind() == "identifier")
                    .map(|node| text(node, source).trim().to_owned());
                let ty = parameter.child_by_field_name("type").map(|node| {
                    let mut cursor = node.walk();
                    let inner = if let Some(union_node) = (node.kind() == "union_type")
                        .then_some(node)
                        .or_else(|| node.named_children(&mut cursor).find(|c| c.kind() == "union_type"))
                    {
                        let mut ucursor = union_node.walk();
                        let non_null: Vec<_> = union_node.named_children(&mut ucursor)
                            .filter(|c| {
                                let t = text(*c, source).trim();
                                !matches!(t, "null" | "undefined" | "void")
                                    && !matches!(c.kind(), "null_type" | "undefined_type" | "void_type")
                            })
                            .collect();
                        if non_null.len() == 1 {
                            non_null[0]
                        } else {
                            node
                        }
                    } else {
                        node
                    };
                    text(inner, source)
                        .trim()
                        .trim_start_matches(':')
                        .trim()
                        .to_owned()
                });
                let ty =
                    ty.or_else(|| name.as_ref().and_then(|name| documented.get(name).cloned()));
                if let (Some(name), Some(ty)) = (name, ty) {
                    if !name.is_empty() && !ty.is_empty() {
                        param_types.insert(name, ty);
                    }
                }
            }
        }
        if !self.javascript && matches!(node.kind(), "arrow_function" | "function_expression") {
            let parameter = node.child_by_field_name("parameter")
                .or_else(|| node.child_by_field_name("parameters")?.named_child(0))
                .and_then(|p| {
                    if p.kind() == "identifier" {
                        Some(p)
                    } else {
                        p.child_by_field_name("name")
                            .or_else(|| p.child_by_field_name("pattern"))
                            .filter(|n| n.kind() == "identifier")
                    }
                });
            if let (Some(parameter), Some(element)) = (
                parameter,
                array_callback_element(node, source, file),
            ) {
                param_types.insert(text(parameter, source).to_owned(), element.to_owned());
            }
        }
        if matches!(node.kind(), "arrow_function" | "function_expression") {
            if let Some(event) = dom_event_callback_parameter(node, source, file) {
                param_types.entry(event.to_owned()).or_insert_with(|| "Event".to_owned());
            } else if let Some(event) = nitro_event_callback_parameter(node, source) {
                param_types.entry(event.to_owned()).or_insert_with(|| "H3Event".to_owned());
            }
        }
        let is_async = text(node, source).trim_start().starts_with("async ");
        let is_method_node = matches!(node.kind(), "method_definition" | "method_signature");
        let local_object = if self.javascript {
            if node.kind() == "pair" {
                node.parent().and_then(|object| same_object(object, source))
            } else {
                same_object_receiver(node, source)
            }
        } else {
            None
        };
        let lexical_this = (node.kind() == "arrow_function")
            .then(|| lexical_this_method(node))
            .flatten();
        let prototype_method = self.javascript.then(|| prototype_assignment(node, source)).flatten();
        let lexical_prototype = (self.javascript && node.kind() == "arrow_function")
            .then(|| lexical_prototype_owner(node, source)).flatten();
        let (receiver_type, is_method, is_static) = if is_method_node {
            let owner = method_owner(node);
            let receiver_type = owner
                .and_then(|o| o.child_by_field_name("name"))
                .map(|n| text(n, source).trim().to_owned());
            let is_static = (0..node.child_count()).any(|i| {
                node.child(i)
                    .is_some_and(|c| c.kind() == "static" || text(c, source).trim() == "static")
            });
            (receiver_type, Some(true), Some(is_static))
        } else if let Some((owner, _)) = prototype_method {
            (Some(owner.to_owned()), Some(true), Some(false))
        } else if let Some((method, class)) = lexical_this {
            let receiver_type = class.child_by_field_name("name")
                .map(|name| text(name, source).trim().to_owned());
            let is_static = (0..method.child_count()).any(|index| {
                method.child(index).is_some_and(|child| child.kind() == "static")
            });
            (receiver_type, None, Some(is_static))
        } else if let Some(owner) = lexical_prototype {
            (Some(owner.to_owned()), None, Some(false))
        } else if node.kind() == "enum_assignment"
            || (node.kind() == "property_identifier"
                && node.parent().is_some_and(|parent| parent.kind() == "enum_body"))
        {
            (None, None, Some(true))
        } else if node.kind() == "field_definition" {
            let is_static = (0..node.child_count()).any(|index| {
                node.child(index).is_some_and(|child| child.kind() == "static")
            });
            (None, None, Some(is_static))
        } else {
            (None, None, None)
        };
        let is_ts_without_body = matches!(
            node.kind(),
            "function_declaration" | "method_definition" | "method_signature"
        ) && node.child_by_field_name("body").is_none();
        let is_overload = is_ts_without_body;
        let is_stub = is_ts_without_body;
        let mut default_export = false;
        while let Some(parent) = node.parent() {
            if parent.kind() == "export_statement"
                && text(parent, source)
                    .trim_start()
                    .starts_with("export default")
            {
                default_export = true;
                break;
            }
            if matches!(parent.kind(), "program" | "module") {
                default_export = file.default_exports.contains(name);
                break;
            }
            if !matches!(
                parent.kind(),
                "variable_declarator"
                    | "lexical_declaration"
                    | "variable_declaration"
                    | "export_statement"
            ) {
                break;
            }
            node = parent;
        }
        SymbolMetadata {
            param_types,
            default_export,
            is_async,
            receiver_name: if local_object.is_some() || lexical_this.is_some() || lexical_prototype.is_some() || (is_method == Some(true) && node.kind() != "arrow_function") {
                Some("this".into())
            } else {
                None
            },
            receiver: local_object.map(|object| object_marker(object, source)),
            receiver_type,
            is_method,
            is_static,
            is_overload,
            is_stub,
            ..Default::default()
        }
    }
    fn extract_relations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        let (node, source, id, line) = (ctx.node, ctx.source, ctx.owner, ctx.line());
        let mut c = node.walk();
        for child in node.named_children(&mut c) {
            if child.kind() == "class_heritage" {
                let mut h = child.walk();
                let clauses: Vec<_> = child.named_children(&mut h).collect();
                let has_clauses = clauses
                    .iter()
                    .any(|c| c.kind() == "extends_clause" || c.kind() == "implements_clause");
                if has_clauses {
                    for clause in clauses {
                        let kind = match clause.kind() {
                            "extends_clause" => "inherits",
                            "implements_clause" => "implements",
                            _ => continue,
                        };
                        let mut t = clause.walk();
                        for target in clause
                            .named_children(&mut t)
                            .filter(|n| n.kind() != "type_arguments")
                        {
                            relations::reference(
                                facts,
                                id,
                                relations::type_name(target, source),
                                kind,
                                line,
                            );
                        }
                    }
                } else {
                    for target in clauses {
                        relations::reference(
                            facts,
                            id,
                            relations::type_name(target, source),
                            "inherits",
                            line,
                        );
                    }
                }
            }
            if child.kind() == "extends_type_clause" {
                let mut t = child.walk();
                for target in child.named_children(&mut t) {
                    relations::reference(
                        facts,
                        id,
                        relations::type_name(target, source),
                        "inherits",
                        line,
                    );
                }
            }
        }

        relations::decorator_references(ctx, facts);
        routes::declaration(ctx.node, ctx.source, ctx.owner, facts, ctx.offset);

        if matches!(
            ctx.node.kind(),
            "function_declaration"
                | "function_signature"
                | "method_definition"
                | "method_signature"
                | "arrow_function"
                | "function"
        ) {
            if let Some(params) = ctx.node.child_by_field_name("parameters") {
                let mut c = params.walk();
                for param in params.named_children(&mut c) {
                    if let Some(ty) = param.child_by_field_name("type") {
                        relations::type_references(facts, ctx.owner, ty, ctx.source, ctx.line());
                    }
                }
            }
            if let Some(ret) = ctx.node.child_by_field_name("return_type") {
                relations::type_references(facts, ctx.owner, ret, ctx.source, ctx.line());
            }
        }
        if matches!(ctx.node.kind(), "property_signature" | "field_definition") {
            if let Some(ty) = ctx.node.child_by_field_name("type") {
                relations::type_references(facts, ctx.owner, ty, ctx.source, ctx.line());
            }
        }
        if ctx.node.kind() == "type_alias_declaration" {
            if let Some(val) = ctx.node.child_by_field_name("value") {
                relations::type_references(facts, ctx.owner, val, ctx.source, ctx.line());
            }
        }
    }

    fn extract_mutations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if !self.javascript && ctx.node.kind() == "identifier" {
            let runtime_value = ctx.node.parent().is_some_and(|parent| {
                (parent.kind() == "variable_declarator"
                    && parent.child_by_field_name("value") == Some(ctx.node))
                    || (parent.kind() == "assignment_expression"
                        && parent.child_by_field_name("right") == Some(ctx.node))
                    || (matches!(parent.kind(), "return_statement" | "arguments" | "array")
                        && (0..parent.named_child_count())
                            .any(|index| parent.named_child(index) == Some(ctx.node)))
            });
            if runtime_value {
                TS_SCRATCH.with(|s| {
                    s.borrow_mut().candidates.push(TsCandidate {
                        source: ctx.owner.into(),
                        expression: text(ctx.node, ctx.source).into(),
                        line: ctx.line(),
                        column: ctx.node.start_position().column,
                        is_member: false,
                    });
                });
            }
        }
        if !self.javascript && ctx.node.kind() == "member_expression" {
            let called = ctx.node.parent().is_some_and(|parent| {
                parent.kind() == "call_expression"
                    && parent.child_by_field_name("function") == Some(ctx.node)
            });
            if !called {
                if let (Some(object), Some(property)) = (
                    ctx.node.child_by_field_name("object")
                        .filter(|object| object.kind() == "identifier"),
                    ctx.node.child_by_field_name("property")
                        .filter(|property| property.kind() == "property_identifier"),
                ) {
                    TS_SCRATCH.with(|s| {
                        s.borrow_mut().candidates.push(TsCandidate {
                            source: ctx.owner.into(),
                            expression: format!(
                                "{}.{}",
                                text(object, ctx.source),
                                text(property, ctx.source)
                            ),
                            line: ctx.line(),
                            column: ctx.node.start_position().column,
                            is_member: true,
                        });
                    });
                }
            }
        }
        if self.javascript
            && (matches!(ctx.node.kind(), "assignment_expression" | "augmented_assignment_expression")
                || (ctx.node.kind() == "unary_expression"
                    && ctx.node.child(0).is_some_and(|operator| operator.kind() == "delete"))) {
            if let Some(left) = ctx.node.child_by_field_name("left")
                .or_else(|| ctx.node.child_by_field_name("argument")) {
                if facts.nodes.first().is_some_and(|node| node.kind == "module") {
                    TS_SCRATCH.with(|scratch| {
                        let mut s = scratch.borrow_mut();
                        if let Some(target) = (left.kind() == "identifier")
                            .then(|| text(left, ctx.source))
                            .or_else(|| direct_prototype_owner(left, ctx.source)) {
                            if !s.assigned.contains(target) {
                                if target.len() <= 128 && s.assigned.len() < 64 {
                                    s.assigned.insert(target.to_owned());
                                } else {
                                    s.assignment_overflow = true;
                                }
                            }
                        }
                        if let Some((owner, member)) = prototype_target(left, ctx.source) {
                            if !s.write_overflow {
                                let key = format!("{owner}.{member}");
                                if let Some(count) = s.writes.get_mut(&key) {
                                    *count = 2;
                                } else if s.writes.len() < 64 {
                                    s.writes.insert(key, 1);
                                } else {
                                    s.writes.clear();
                                    s.write_overflow = true;
                                }
                            }
                        }
                        if let Some((owner, member, implementation)) = prototype_identifier_assignment(ctx.node, ctx.source) {
                            if s.aliases.len() < 64 && owner.len() + member.len() + implementation.len() <= 192 {
                                s.aliases.push(JsPrototypeAlias {
                                    owner: owner.to_owned(),
                                    member: member.to_owned(),
                                    implementation: implementation.to_owned(),
                                    line: ctx.line(),
                                    column: ctx.node.start_position().column,
                                });
                            } else {
                                s.alias_overflow = true;
                            }
                        }
                    });
                }
            }
        }
        relations::mutation(
            ctx,
            facts,
            &["public_field_definition", "property_signature"],
            &[
                "assignment_expression",
                "augmented_assignment_expression",
                "update_expression",
            ],
            &["member_expression"],
        );
        if matches!(ctx.node.kind(), "public_field_definition" | "field_definition")
            && (0..ctx.node.child_count()).any(|index| {
                ctx.node.child(index).is_some_and(|child| child.kind() == "static")
            })
        {
            if let (Some(owner), Some(name)) = (
                facts.nodes.iter().find(|node| node.id == ctx.owner),
                ctx.node.child_by_field_name("name").map(|name| text(name, ctx.source).to_owned()),
            ) {
                let qualname = format!("{}.{}", owner.qualname, name);
                let path = owner.path.clone();
                if let Some(field) = facts.nodes.iter_mut().find(|node| {
                    node.path == path && node.kind == "field" && node.qualname == qualname
                }) {
                    field.details["is_static"] = serde_json::json!(true);
                }
            }
        }
        let before = facts.references.len();
        relations::member_access(ctx, facts, &["member_expression"]);
        if let Some(reference) = facts.references.get_mut(before) {
            reference.column = ctx.node.start_position().column;
        }
    }
    fn extract_routes(
        &self,
        ctx: &SyntaxContext<'_, '_>,
        facts: &mut Facts,
        symbols: &HashMap<usize, String>,
    ) {
        if matches!(ctx.node.kind(), "call_expression" | "object") {
            routes::registration(ctx.node, ctx.source, ctx.owner, facts, ctx.offset, symbols);
            routes::client_call(ctx.node, ctx.source, ctx.owner, facts, ctx.offset);
        }
    }
    fn finish(&self, facts: &mut Facts) {
        if !self.javascript {
            let enums: HashSet<String> = facts
                .nodes
                .iter()
                .filter(|node| node.kind == "enum")
                .map(|node| node.name.clone())
                .collect();
            let imports: HashSet<String> = facts
                .references
                .iter()
                .filter(|reference| reference.kind == "imports")
                .filter_map(|reference| reference.alias.clone())
                .collect();
            let type_imports: HashSet<String> = facts
                .references
                .iter()
                .filter(|reference| {
                    reference.kind == "imports"
                        && matches!(
                            reference.receiver_hint.as_ref(),
                            Some(ReceiverHint::TypeOnlyImport)
                        )
                })
                .filter_map(|reference| reference.alias.clone())
                .collect();
            let candidates = TS_SCRATCH.with(|s| std::mem::take(&mut s.borrow_mut().candidates));
            for candidate in candidates {
                if !candidate.is_member {
                    if type_imports.contains(&candidate.expression) {
                        facts.references.push(Reference {
                            source: candidate.source,
                            dynamic: false,
                            expression: candidate.expression,
                            kind: "references".into(),
                            line: candidate.line,
                            column: candidate.column,
                            alias: None,
                            module: None,
                            receiver_hint: Some(ReceiverHint::TypeOnlyImport),
                        });
                    }
                } else {
                    let Some((receiver, _)) = candidate.expression.split_once('.') else {
                        continue;
                    };
                    if enums.contains(receiver) || imports.contains(receiver) {
                        let hint = if type_imports.contains(receiver) {
                            Some(ReceiverHint::TypeOnlyImport)
                        } else {
                            None
                        };
                        facts.references.push(Reference {
                            source: candidate.source,
                            dynamic: false,
                            expression: candidate.expression,
                            kind: "references".into(),
                            line: candidate.line,
                            column: candidate.column,
                            alias: None,
                            module: None,
                            receiver_hint: hint,
                        });
                    }
                }
            }
        }
        if self.javascript {
            let (writes, write_overflow, prototype_aliases, alias_overflow, assigned, assignment_overflow) =
                TS_SCRATCH.with(|s| {
                    let mut b = s.borrow_mut();
                    let writes = std::mem::take(&mut b.writes);
                    let write_overflow = b.write_overflow;
                    let prototype_aliases = std::mem::take(&mut b.aliases);
                    let alias_overflow = b.alias_overflow;
                    let assigned = std::mem::take(&mut b.assigned);
                    let assignment_overflow = b.assignment_overflow;
                    b.reset();
                    (writes, write_overflow, prototype_aliases, alias_overflow, assigned, assignment_overflow)
                });
            let Some(module) = facts.nodes.iter_mut().find(|node| node.kind == "module") else {
                return;
            };
            let module_id = module.id.clone();
            let module_qualname = module.qualname.clone();
            let rebound: std::collections::HashSet<String> = module.details["rebindings"]
                .as_array().into_iter().flatten().filter_map(|name| name.as_str().map(str::to_owned)).collect();
            let declarations: std::collections::HashSet<&str> = facts.edges.iter()
                .filter(|edge| edge.src == module_id && edge.kind == "contains"
                    && matches!(edge.evidence.as_str(), "class_declaration" | "function_declaration"))
                .map(|edge| edge.dst.as_str()).collect();
            let mut direct_constructors: std::collections::HashMap<&str, Vec<&Node>> =
                std::collections::HashMap::new();
            for node in facts.nodes.iter().filter(|node| declarations.contains(node.id.as_str())) {
                direct_constructors.entry(node.qualname.as_str()).or_default().push(node);
            }
            let parents: std::collections::HashMap<&str, &str> = facts.edges.iter()
                .filter(|edge| edge.kind == "contains")
                .map(|edge| (edge.dst.as_str(), edge.src.as_str())).collect();
            let mut methods = std::collections::HashSet::new();
            let mut constructor_ids = std::collections::HashSet::new();
            let mut valid_aliases = Vec::new();
            for method in facts.nodes.iter().filter(|node| node.details["prototype"] == true) {
                let Some(owner) = method.details["receiver_type"].as_str() else { continue; };
                let key = format!("{owner}.{}", method.name);
                if write_overflow || assignment_overflow
                    || writes.get(key.as_str()).copied() != Some(1)
                    || rebound.contains(owner) || assigned.contains(owner)
                {
                    continue;
                }
                let qualname = format!("{module_qualname}.{owner}");
                let Some([constructor]) = direct_constructors.get(qualname.as_str()).map(Vec::as_slice) else {
                    continue;
                };
                if constructor.kind == "class" && (constructor.line > method.line
                    || (constructor.line == method.line
                        && constructor.details["column"].as_u64().unwrap_or(0)
                            >= method.details["column"].as_u64().unwrap_or(0))) {
                    continue;
                }
                methods.insert(method.id.clone());
                if constructor.kind == "function" {
                    constructor_ids.insert(constructor.id.clone());
                }
            }
            if !write_overflow && !assignment_overflow && !alias_overflow {
                let mut target_counts = std::collections::HashMap::<&str, usize>::new();
                for alias in &prototype_aliases {
                    *target_counts.entry(alias.implementation.as_str()).or_default() += 1;
                }
                for alias in &prototype_aliases {
                    let key = format!("{}.{}", alias.owner, alias.member);
                    if writes.get(key.as_str()).copied() != Some(1)
                        || rebound.contains(&alias.owner) || assigned.contains(&alias.owner)
                        || rebound.contains(&alias.implementation) || assigned.contains(&alias.implementation)
                        || target_counts.get(alias.implementation.as_str()) != Some(&1)
                    {
                        continue;
                    }
                    let owner_qualname = format!("{module_qualname}.{}", alias.owner);
                    let implementation_qualname = format!("{module_qualname}.{}", alias.implementation);
                    let (Some([constructor]), Some([function])) = (
                        direct_constructors.get(owner_qualname.as_str()).map(Vec::as_slice),
                        direct_constructors.get(implementation_qualname.as_str()).map(Vec::as_slice),
                    ) else { continue; };
                    if function.kind != "function"
                        || (function.line, function.details["column"].as_u64().unwrap_or(0)) >= (alias.line, alias.column as u64)
                        || (constructor.line, constructor.details["column"].as_u64().unwrap_or(0)) >= (alias.line, alias.column as u64)
                    {
                        continue;
                    }
                    methods.insert(function.id.clone());
                    if constructor.kind == "function" {
                        constructor_ids.insert(constructor.id.clone());
                    }
                    valid_aliases.push((function.id.clone(), alias.owner.clone(), alias.member.clone(), alias.line, alias.column));
                }
            }
            let valid_methods: std::collections::HashMap<String, String> = facts.nodes.iter()
                .filter(|node| methods.contains(&node.id))
                .filter_map(|node| node.details["receiver_type"].as_str()
                    .map(|owner| (node.id.clone(), owner.to_owned()))).collect();
            for node in &mut facts.nodes {
                if let Some((_, owner, member, line, column)) = valid_aliases.iter().find(|(id, ..)| *id == node.id) {
                    node.details["prototype"] = serde_json::json!(true);
                    node.details["prototype_member"] = serde_json::json!(member);
                    node.details["receiver_type"] = serde_json::json!(owner);
                    node.details["prototype_assignment_line"] = serde_json::json!(line);
                    node.details["prototype_assignment_column"] = serde_json::json!(column);
                }
                if methods.contains(&node.id) {
                    node.details["prototype_valid"] = serde_json::json!(true);
                }
                if constructor_ids.contains(&node.id) {
                    node.details["prototype_constructor"] = serde_json::json!(true);
                }
                if node.kind == "function" && node.details["receiver_name"] == "this" {
                    let receiver = node.details["receiver_type"].as_str();
                    let mut ancestor = node.id.as_str();
                    for _ in 0..16 {
                        let Some(parent) = parents.get(ancestor).copied() else { break; };
                        if valid_methods.get(parent).is_some_and(|owner| Some(owner.as_str()) == receiver) {
                            node.details["prototype_lexical_this"] = serde_json::json!(true);
                            break;
                        }
                        ancestor = parent;
                    }
                }
            }
        }
        relations::implicit_fields(facts);
    }
}

/// Language profiles provided by this module.
pub static PROFILES: &[&dyn LanguageProfile] = &[&TYPESCRIPT, &JAVASCRIPT];
