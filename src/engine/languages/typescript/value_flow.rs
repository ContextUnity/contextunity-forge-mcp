use super::*;
use crate::core::semantic::{
    BindingFact, ExportBinding, FieldFact, ObjectMember, SourcePosition, TypeExpr, ValueExpr,
    ValueFlowFacts,
};

fn position(node: Syntax<'_>) -> SourcePosition {
    SourcePosition {
        line: node.start_position().row + 1,
        column: node.start_position().column,
    }
}

fn completed(node: Syntax<'_>) -> SourcePosition {
    SourcePosition {
        line: node.end_position().row + 1,
        column: node.end_position().column,
    }
}

fn named_type(node: Syntax<'_>, source: &str) -> TypeExpr {
    type_expr(node, source, 0)
}

fn type_expr(node: Syntax<'_>, source: &str, depth: usize) -> TypeExpr {
    if depth >= 8 {
        return TypeExpr::Unknown;
    }
    if node.kind() == "type_annotation" {
        let mut cursor = node.walk();
        return node
            .named_children(&mut cursor)
            .next()
            .map_or(TypeExpr::Unknown, |child| {
                type_expr(child, source, depth + 1)
            });
    }
    if node.kind() == "generic_type" {
        let (Some(base), Some(arguments)) = (
            node.child_by_field_name("name"),
            node.child_by_field_name("type_arguments"),
        ) else {
            return TypeExpr::Unknown;
        };
        let TypeExpr::Named { name: base } = type_expr(base, source, depth + 1) else {
            return TypeExpr::Unknown;
        };
        let mut cursor = arguments.walk();
        let mut args = Vec::new();
        for argument in arguments.named_children(&mut cursor) {
            if args.len() >= 16 {
                return TypeExpr::Unknown;
            }
            args.push(type_expr(argument, source, depth + 1));
        }
        return if args.is_empty() {
            TypeExpr::Unknown
        } else {
            TypeExpr::Applied { base, args }
        };
    }
    if node.kind() == "array_type" {
        let mut cursor = node.walk();
        return node
            .named_children(&mut cursor)
            .next()
            .map_or(TypeExpr::Unknown, |element| TypeExpr::Applied {
                base: "Array".into(),
                args: vec![type_expr(element, source, depth + 1)],
            });
    }
    let value = text(node, source).trim().trim_start_matches(':').trim();
    if !value.is_empty()
        && value.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        })
    {
        TypeExpr::Named {
            name: value.to_owned(),
        }
    } else {
        TypeExpr::Unknown
    }
}

pub(super) fn jsdoc(
    node: Syntax<'_>,
    source: &str,
) -> (BTreeMap<String, String>, Option<TypeExpr>, Option<TypeExpr>) {
    let mut anchor = node;
    while let Some(parent) = anchor.parent().filter(|parent| {
        matches!(
            parent.kind(),
            "export_statement"
                | "variable_declarator"
                | "lexical_declaration"
                | "variable_declaration"
        )
    }) {
        anchor = parent;
    }
    let Some(comment) = anchor
        .prev_named_sibling()
        .filter(|comment| comment.kind() == "comment")
    else {
        return (BTreeMap::new(), None, None);
    };
    let comment = text(comment, source);
    if !comment.starts_with("/**") || comment.len() > 8192 {
        return (BTreeMap::new(), None, None);
    }
    let mut parameters = BTreeMap::new();
    let mut duplicates = HashSet::new();
    let mut returned = None;
    let mut declared = None;
    let mut returns = 0;
    let mut types = 0;
    let mut invalid_parameters = false;
    for line in comment.lines() {
        let line = line
            .trim()
            .trim_start_matches("/**")
            .trim_start_matches('*')
            .trim();
        let (tag, rest) = if let Some(rest) = line.strip_prefix("@param ") {
            ("param", rest)
        } else if let Some(rest) = line
            .strip_prefix("@returns ")
            .or_else(|| line.strip_prefix("@return "))
        {
            ("return", rest)
        } else if let Some(rest) = line.strip_prefix("@type ") {
            ("type", rest)
        } else {
            continue;
        };
        if tag == "return" {
            returns += 1;
            if returns > 1 {
                returned = Some(TypeExpr::Unknown);
            }
        }
        if tag == "type" {
            types += 1;
            if types > 1 {
                declared = Some(TypeExpr::Unknown);
            }
        }
        let Some(rest) = rest.trim_start().strip_prefix('{') else {
            if tag == "param" {
                invalid_parameters = true;
            }
            continue;
        };
        let Some((ty, tail)) = rest.split_once('}') else {
            if tag == "param" {
                invalid_parameters = true;
            }
            continue;
        };
        let ty = ty.trim();
        if ty.is_empty()
            || !ty.split('.').all(|part| {
                !part.is_empty()
                    && part
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
            })
        {
            match tag {
                "return" => returned = Some(TypeExpr::Unknown),
                "type" => declared = Some(TypeExpr::Unknown),
                _ => invalid_parameters = true,
            }
            continue;
        }
        if tag == "param" {
            let Some(name) = tail.split_whitespace().next().filter(|name| {
                name.chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
            }) else {
                continue;
            };
            if parameters.insert(name.to_owned(), ty.to_owned()).is_some() {
                duplicates.insert(name.to_owned());
            }
        } else if tag == "return" {
            if returns != 1 {
                returned = Some(TypeExpr::Unknown);
            } else {
                returned = Some(TypeExpr::Named {
                    name: ty.to_owned(),
                });
            }
        } else if types != 1 {
            declared = Some(TypeExpr::Unknown);
        } else {
            declared = Some(TypeExpr::Named {
                name: ty.to_owned(),
            });
        }
    }
    for duplicate in duplicates {
        parameters.remove(&duplicate);
    }
    if invalid_parameters {
        parameters.clear();
    }
    (parameters, returned, declared)
}

fn value(node: Syntax<'_>, source: &str) -> ValueExpr {
    match node.kind() {
        "identifier" => ValueExpr::Alias {
            name: text(node, source).to_owned(),
        },
        "new_expression" => node
            .child_by_field_name("constructor")
            .map(|callee| ValueExpr::Construct {
                callee: text(callee, source).to_owned(),
            })
            .unwrap_or(ValueExpr::Unknown),
        "call_expression" => node
            .child_by_field_name("function")
            .map(|callee| ValueExpr::Call {
                callee: text(callee, source).to_owned(),
            })
            .unwrap_or(ValueExpr::Unknown),
        "member_expression" => match (
            node.child_by_field_name("object"),
            node.child_by_field_name("property"),
        ) {
            (Some(receiver), Some(member))
                if receiver.kind() == "identifier" && member.kind() == "property_identifier" =>
            {
                ValueExpr::Field {
                    receiver: text(receiver, source).to_owned(),
                    member: text(member, source).to_owned(),
                }
            }
            _ => ValueExpr::Unknown,
        },
        "object" => {
            let mut members = Vec::new();
            let mut names = hashbrown::HashSet::new();
            let mut cursor = node.walk();
            for item in node.named_children(&mut cursor) {
                let (name, declaration) = if item.kind() == "method_definition" {
                    if text(item, source).trim_start().starts_with("get ")
                        || text(item, source).trim_start().starts_with("set ")
                    {
                        return ValueExpr::Unknown;
                    }
                    let Some(name) = item
                        .child_by_field_name("name")
                        .filter(|name| matches!(name.kind(), "property_identifier" | "identifier"))
                    else {
                        return ValueExpr::Unknown;
                    };
                    (text(name, source), item)
                } else if item.kind() == "pair" {
                    let (Some(name), Some(declaration)) = (
                        item.child_by_field_name("key"),
                        item.child_by_field_name("value"),
                    ) else {
                        return ValueExpr::Unknown;
                    };
                    if !matches!(name.kind(), "property_identifier" | "identifier")
                        || !matches!(declaration.kind(), "arrow_function" | "function_expression")
                    {
                        return ValueExpr::Unknown;
                    }
                    (text(name, source), declaration)
                } else {
                    return ValueExpr::Unknown;
                };
                if !names.insert(name) {
                    return ValueExpr::Unknown;
                }
                members.push(ObjectMember {
                    name: name.to_owned(),
                    position: position(declaration),
                });
            }
            if members.is_empty() {
                ValueExpr::Unknown
            } else {
                ValueExpr::Object { members }
            }
        }
        _ => ValueExpr::Unknown,
    }
}

pub(super) fn extract(root: Syntax<'_>, source: &str) -> ValueFlowFacts {
    let mut cursor = root.walk();
    let wrapped_return = root.kind().contains("generator_function")
        || root
            .children(&mut cursor)
            .any(|child| matches!(child.kind(), "async" | "*"));
    let mut result = ValueFlowFacts {
        return_type: if wrapped_return {
            Some(TypeExpr::Unknown)
        } else {
            root.child_by_field_name("return_type")
                .map(|ty| named_type(ty, source))
                .or_else(|| jsdoc(root, source).1)
        },
        ..Default::default()
    };
    if result.return_type.is_none() {
        if let Some(expression) = final_return(root) {
            let returned = value(expression, source);
            if !matches!(returned, ValueExpr::Unknown) {
                result.return_value = Some(returned);
                result.return_position = Some(position(expression));
            }
        }
    }
    let mut pending = vec![(root, false)];
    let mut object_aliases = std::collections::BTreeSet::new();
    while let Some((node, conditional)) = pending.pop() {
        if node.id() != root.id()
            && matches!(
                node.kind(),
                "function_declaration"
                    | "function_expression"
                    | "generator_function_declaration"
                    | "generator_function"
                    | "generator_function_expression"
                    | "arrow_function"
                    | "method_definition"
                    | "class_declaration"
                    | "class"
            )
        {
            continue;
        }
        let conditional = conditional
            || matches!(
                node.kind(),
                "if_statement"
                    | "switch_statement"
                    | "for_statement"
                    | "for_in_statement"
                    | "while_statement"
                    | "do_statement"
                    | "try_statement"
                    | "ternary_expression"
            );
        if node.kind() == "variable_declarator" {
            if let Some(name) = node
                .child_by_field_name("name")
                .filter(|name| name.kind() == "identifier")
            {
                let initializer = node.child_by_field_name("value");
                let expression = initializer
                    .map(|initializer| {
                        node.child_by_field_name("type")
                            .map(|ty| named_type(ty, source))
                            .or_else(|| jsdoc(node, source).2)
                            .map(|type_expr| ValueExpr::Annotated { type_expr })
                            .unwrap_or_else(|| value(initializer, source))
                    })
                    .unwrap_or(ValueExpr::Unknown);
                let local = text(name, source);
                object_aliases.remove(local);
                if matches!(
                    &expression,
                    ValueExpr::Object { .. } | ValueExpr::Alias { .. }
                ) {
                    object_aliases.insert(local);
                }
                result.bindings.push(BindingFact {
                    name: text(name, source).to_owned(),
                    position: position(node),
                    value: ValueExpr::Unknown,
                    conditional,
                });
                if !matches!(&expression, ValueExpr::Unknown) {
                    result.bindings.push(BindingFact {
                        name: text(name, source).to_owned(),
                        position: completed(node),
                        value: expression,
                        conditional,
                    });
                }
            } else if let (Some(pattern), Some(initializer)) = (
                node.child_by_field_name("name"),
                node.child_by_field_name("value"),
            ) {
                if pattern.kind() == "object_pattern" && initializer.kind() == "identifier" {
                    let mut cursor = pattern.walk();
                    for item in pattern.named_children(&mut cursor) {
                        let pair = if item.kind() == "shorthand_property_identifier_pattern" {
                            Some((text(item, source), text(item, source)))
                        } else {
                            match (
                                item.child_by_field_name("key"),
                                item.child_by_field_name("value"),
                            ) {
                                (Some(key), Some(local))
                                    if local.kind() == "identifier"
                                        && matches!(
                                            key.kind(),
                                            "property_identifier" | "identifier"
                                        ) =>
                                {
                                    Some((text(key, source), text(local, source)))
                                }
                                _ => None,
                            }
                        };
                        if let Some((member, local)) = pair {
                            result.bindings.push(BindingFact {
                                name: local.to_owned(),
                                position: position(node),
                                value: ValueExpr::Unknown,
                                conditional,
                            });
                            result.bindings.push(BindingFact {
                                name: local.to_owned(),
                                position: completed(node),
                                value: ValueExpr::Field {
                                    receiver: text(initializer, source).to_owned(),
                                    member: member.to_owned(),
                                },
                                conditional,
                            });
                        }
                    }
                }
            }
        }
        if matches!(
            node.kind(),
            "public_field_definition" | "field_definition" | "property_signature"
        ) {
            if let Some(name) = node.child_by_field_name("name") {
                let value = node
                    .child_by_field_name("type")
                    .map(|ty| named_type(ty, source))
                    .or_else(|| jsdoc(node, source).2)
                    .map(|type_expr| ValueExpr::Annotated { type_expr })
                    .or_else(|| {
                        node.child_by_field_name("value")
                            .map(|initializer| value(initializer, source))
                    })
                    .unwrap_or(ValueExpr::Unknown);
                result.fields.push(FieldFact {
                    name: text(name, source).to_owned(),
                    value,
                    position: position(node),
                    conditional,
                });
            }
        }
        if matches!(
            node.kind(),
            "assignment_expression" | "augmented_assignment_expression" | "update_expression"
        ) {
            let left = node
                .child_by_field_name("left")
                .or_else(|| node.child_by_field_name("argument"));
            if let Some(receiver) = left
                .filter(|left| left.kind() == "member_expression")
                .and_then(|left| left.child_by_field_name("object"))
                .filter(|receiver| receiver.kind() == "identifier")
            {
                let name = text(receiver, source);
                if object_aliases.contains(name) {
                    for name in std::mem::take(&mut object_aliases) {
                        result.bindings.push(BindingFact {
                            name: name.to_owned(),
                            position: completed(node),
                            value: ValueExpr::Unknown,
                            conditional,
                        });
                    }
                }
            }
        }
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();
        pending.extend(children.into_iter().rev().map(|child| (child, conditional)));
    }
    result
}

fn final_return(root: Syntax<'_>) -> Option<Syntax<'_>> {
    if !matches!(
        root.kind(),
        "function_declaration" | "function_expression" | "arrow_function" | "method_definition"
    ) {
        return None;
    }
    let body = root.child_by_field_name("body")?;
    if body.kind() != "statement_block" {
        return Some(body);
    }
    let mut cursor = body.walk();
    let last = body
        .named_children(&mut cursor)
        .filter(|node| node.kind() != "comment")
        .last()?;
    if last.kind() != "return_statement" {
        return None;
    }
    let mut pending = vec![body];
    while let Some(node) = pending.pop() {
        if node.id() != body.id()
            && matches!(
                node.kind(),
                "function_declaration"
                    | "function_expression"
                    | "generator_function_declaration"
                    | "generator_function"
                    | "generator_function_expression"
                    | "arrow_function"
                    | "method_definition"
                    | "class_declaration"
                    | "class"
            )
        {
            continue;
        }
        if matches!(
            node.kind(),
            "if_statement"
                | "switch_statement"
                | "for_statement"
                | "for_in_statement"
                | "while_statement"
                | "do_statement"
                | "try_statement"
                | "throw_statement"
                | "ternary_expression"
        ) || (node.kind() == "return_statement" && node.id() != last.id())
        {
            return None;
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
    let mut cursor = last.walk();
    let expression = last.named_children(&mut cursor).next();
    expression
}

fn binding(
    name: &str,
    local: Option<&str>,
    module: Option<&str>,
    type_only: bool,
    star: bool,
) -> ExportBinding {
    ExportBinding {
        name: name.to_owned(),
        local: local.map(str::to_owned),
        module: module.map(str::to_owned),
        type_only,
        star,
    }
}

pub(super) fn exports(root: Syntax<'_>, source: &str) -> Vec<ExportBinding> {
    let mut result = Vec::new();
    let commonjs_shadowed = TYPESCRIPT
        .bindings(root, source)
        .all
        .iter()
        .any(|name| matches!(name.as_str(), "module" | "exports"));
    let mut commonjs_invalidated = false;
    let mut module_replacements = 0;
    let mut mutations = vec![root];
    while let Some(node) = mutations.pop() {
        if node.id() != root.id()
            && matches!(
                node.kind(),
                "function_declaration"
                    | "function_expression"
                    | "arrow_function"
                    | "method_definition"
                    | "class_declaration"
            )
        {
            continue;
        }
        if node.kind() == "assignment_expression"
            && field(node, source, "left").is_some_and(|left| {
                left == "module.exports"
                    || left.starts_with("exports.")
                    || left.starts_with("module.exports.")
            })
            && !node
                .parent()
                .and_then(|parent| parent.parent())
                .is_some_and(|parent| parent.id() == root.id())
        {
            commonjs_invalidated = true;
        }
        let mut cursor = node.walk();
        mutations.extend(node.named_children(&mut cursor));
    }
    let mut type_imports = HashSet::new();
    let mut pending = vec![(root, false)];
    while let Some((node, type_only)) = pending.pop() {
        if node.id() != root.id()
            && !matches!(
                node.kind(),
                "import_statement" | "import_clause" | "named_imports" | "import_specifier"
            )
        {
            continue;
        }
        let type_only = type_only
            || (node.kind() == "import_statement"
                && text(node, source).trim_start().starts_with("import type "))
            || (node.kind() == "import_specifier"
                && text(node, source).trim_start().starts_with("type "));
        if type_only && node.kind() == "import_specifier" {
            if let Some(name) = field(node, source, "alias").or_else(|| field(node, source, "name"))
            {
                type_imports.insert(name.to_owned());
            }
        }
        let mut cursor = node.walk();
        pending.extend(
            node.named_children(&mut cursor)
                .map(|child| (child, type_only)),
        );
    }
    let mut cursor = root.walk();
    for node in root.named_children(&mut cursor) {
        if node.kind() == "export_statement" {
            let module =
                field(node, source, "source").map(|module| module.trim_matches(['\'', '"']));
            let type_only = text(node, source).trim_start().starts_with("export type ");
            let default = text(node, source)
                .trim_start()
                .starts_with("export default ");
            if default {
                if let Some(local) = node
                    .child_by_field_name("value")
                    .filter(|value| value.kind() == "identifier")
                {
                    result.push(binding(
                        "default",
                        Some(text(local, source)),
                        None,
                        false,
                        false,
                    ));
                }
            }
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                match child.kind() {
                    "export_clause" => {
                        let mut cursor = child.walk();
                        for item in child.named_children(&mut cursor) {
                            if let Some(local) = field(item, source, "name") {
                                result.push(binding(
                                    field(item, source, "alias").unwrap_or(local),
                                    Some(local),
                                    module,
                                    type_only
                                        || text(item, source).trim_start().starts_with("type "),
                                    false,
                                ));
                            }
                        }
                    }
                    "namespace_export" => {
                        if let Some(name) = child.named_child(0) {
                            result.push(binding(
                                text(name, source),
                                None,
                                module,
                                type_only,
                                false,
                            ));
                        }
                    }
                    "function_declaration"
                    | "class_declaration"
                    | "interface_declaration"
                    | "type_alias_declaration" => {
                        if let Some(name) = field(child, source, "name") {
                            result.push(binding(
                                if default { "default" } else { name },
                                Some(name),
                                None,
                                type_only
                                    || matches!(
                                        child.kind(),
                                        "interface_declaration" | "type_alias_declaration"
                                    ),
                                false,
                            ));
                        }
                    }
                    "lexical_declaration" | "variable_declaration" => {
                        let mut cursor = child.walk();
                        for declaration in child.named_children(&mut cursor) {
                            if let Some(name) = declaration
                                .child_by_field_name("name")
                                .filter(|name| name.kind() == "identifier")
                            {
                                result.push(binding(
                                    text(name, source),
                                    Some(text(name, source)),
                                    None,
                                    type_only,
                                    false,
                                ));
                            }
                        }
                    }
                    _ => {}
                }
            }
            if module.is_some() && text(node, source).trim_start().starts_with("export * from") {
                result.push(binding("*", None, module, type_only, true));
            }
        } else if node.kind() == "expression_statement" && !commonjs_shadowed {
            let Some(assignment) = node
                .named_child(0)
                .filter(|item| item.kind() == "assignment_expression")
            else {
                continue;
            };
            let (Some(left), Some(right)) = (
                assignment.child_by_field_name("left"),
                assignment.child_by_field_name("right"),
            ) else {
                continue;
            };
            let left = text(left, source).trim();
            if left == "module.exports" {
                module_replacements += 1;
            }
            if let Some(name) = left
                .strip_prefix("exports.")
                .or_else(|| left.strip_prefix("module.exports."))
            {
                if right.kind() == "identifier" {
                    result.push(binding(name, Some(text(right, source)), None, false, false));
                } else if let Some(module) = required_module(right, source) {
                    result.push(binding(name, None, Some(&module), false, false));
                } else {
                    result.push(binding(name, None, None, false, false));
                }
            } else if left == "module.exports" && required_module(right, source).is_some() {
                let module = required_module(right, source).expect("checked literal require");
                result.push(binding("*", None, Some(&module), false, true));
            } else if left == "module.exports" && right.kind() == "object" {
                let mut cursor = right.walk();
                for item in right.named_children(&mut cursor) {
                    if item.kind() == "spread_element" {
                        commonjs_invalidated = true;
                    }
                    if item.kind() == "shorthand_property_identifier" {
                        result.push(binding(
                            text(item, source),
                            Some(text(item, source)),
                            None,
                            false,
                            false,
                        ));
                    } else if let (Some(key), Some(value)) = (
                        item.child_by_field_name("key"),
                        item.child_by_field_name("value"),
                    ) {
                        if !matches!(key.kind(), "property_identifier" | "identifier" | "string") {
                            commonjs_invalidated = true;
                        }
                        if value.kind() == "identifier" {
                            result.push(binding(
                                text(key, source).trim_matches(['\'', '"']),
                                Some(text(value, source)),
                                None,
                                false,
                                false,
                            ));
                        }
                    }
                }
            } else if left == "module.exports" {
                commonjs_invalidated = true;
            }
        }
    }
    if commonjs_invalidated || module_replacements > 1 {
        result.retain(|export| export.type_only);
    }
    for export in &mut result {
        if export.module.is_none()
            && export
                .local
                .as_ref()
                .is_some_and(|name| type_imports.contains(name))
        {
            export.type_only = true;
        }
    }
    result
}
