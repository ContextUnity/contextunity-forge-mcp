use super::*;

pub(super) fn static_member_callee(function: Syntax<'_>, source: &str) -> Option<String> {
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
    if !bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$'))
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
    {
        return None;
    }
    Some(format!("{}.{}", text(receiver, source), member))
}

pub(super) fn supported_object(object: Syntax<'_>) -> Option<Syntax<'_>> {
    // The helper is called for each direct member. Keep the object-wide check
    // bounded, and decline unusually large objects rather than repeatedly
    // scanning an unbounded member list on the extraction hot path.
    let mut cursor = object.walk();
    let mut count = 0;
    for member in object.named_children(&mut cursor) {
        count += 1;
        if count > 128
            || member.kind() == "spread_element"
            || (member.kind() == "pair"
                && !member
                    .child_by_field_name("key")
                    .is_some_and(|key| matches!(key.kind(), "property_identifier" | "identifier")))
        {
            return None;
        }
    }
    Some(object)
}

pub(super) fn returned_object(object: Syntax<'_>) -> Option<Syntax<'_>> {
    if object.kind() != "object" {
        return None;
    }
    let parent = object.parent()?;
    let expression = if parent.kind() == "parenthesized_expression"
        && parent.named_child_count() == 1
        && parent.named_child(0) == Some(object)
    {
        parent
    } else {
        object
    };
    let enclosing = expression.parent()?;
    let function = if enclosing.kind() == "return_statement" {
        let body = enclosing.parent()?;
        if body.kind() != "statement_block" {
            return None;
        }
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

pub(super) fn assigned_or_alpine_object<'tree>(
    object: Syntax<'tree>,
    source: &str,
) -> Option<Syntax<'tree>> {
    if object.kind() != "object" {
        return None;
    }
    let parent = object.parent()?;
    let expression = if parent.kind() == "parenthesized_expression"
        && parent.named_child_count() == 1
        && parent.named_child(0) == Some(object)
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
                if callee.ends_with(".data")
                    || callee.ends_with(".mixin")
                    || callee == "Alpine.data"
                {
                    return supported_object(object);
                }
            }
        }
    }
    None
}

pub(super) fn same_object<'tree>(object: Syntax<'tree>, source: &str) -> Option<Syntax<'tree>> {
    returned_object(object).or_else(|| assigned_or_alpine_object(object, source))
}

pub(super) fn same_object_receiver<'tree>(
    node: Syntax<'tree>,
    source: &str,
) -> Option<Syntax<'tree>> {
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
                "function_declaration"
                | "function_expression"
                | "generator_function"
                | "generator_function_declaration"
                | "class_declaration"
                | "class" => return None,
                _ => current = parent.parent(),
            }
        }
    } else {
        return None;
    };
    same_object(member.parent()?, source)
}

pub(super) fn object_marker(object: Syntax<'_>, source: &str) -> String {
    let start = object.start_position();
    let is_alpine = object
        .parent()
        .and_then(|p| p.parent())
        .is_some_and(|enclosing| {
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

pub(super) fn nuxt_module_declared(node: Syntax<'_>, source: &str, module_name: &str) -> bool {
    if node.kind() != "call_expression"
        || node.parent().is_none_or(|parent| {
            parent.kind() != "export_statement"
                || !text(parent, source)
                    .trim_start()
                    .starts_with("export default")
        })
        || node
            .child_by_field_name("function")
            .is_none_or(|callee| text(callee, source) != "defineNuxtConfig")
    {
        return false;
    }
    let Some(args) = node.child_by_field_name("arguments") else {
        return false;
    };
    let mut cursor = args.walk();
    let mut values = args.named_children(&mut cursor);
    let Some(config) = values.next().filter(|value| value.kind() == "object") else {
        return false;
    };
    if values.next().is_some() {
        return false;
    }
    let mut modules = None;
    for pair in config.named_children(&mut config.walk()) {
        if pair.kind() == "comment" {
            continue;
        }
        if pair.kind() != "pair" {
            return false;
        }
        if pair
            .child_by_field_name("key")
            .is_some_and(|key| text(key, source) == "modules")
        {
            if modules.is_some() {
                return false;
            }
            modules = pair
                .child_by_field_name("value")
                .filter(|value| value.kind() == "array");
            if modules.is_none() {
                return false;
            }
        }
    }
    let Some(modules) = modules else {
        return false;
    };
    let mut found = false;
    for item in modules.named_children(&mut modules.walk()) {
        if item.kind() != "string" || text(item, source).contains('\\') {
            return false;
        }
        found |= text(item, source).trim_matches(['\'', '"']) == module_name;
    }
    found
}

pub(super) fn nuxt_default_components_enabled(node: Syntax<'_>, source: &str) -> Option<bool> {
    if node.kind() != "call_expression"
        || node.parent().is_none_or(|parent| {
            parent.kind() != "export_statement"
                || !text(parent, source)
                    .trim_start()
                    .starts_with("export default")
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
        if property.kind() == "comment" {
            continue;
        }
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
