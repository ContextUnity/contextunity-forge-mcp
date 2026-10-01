use crate::core::semantic::{
    BindingFact, FieldFact, SourcePosition, TypeExpr, ValueExpr, ValueFlowFacts,
};
use crate::{
    core::models::Facts,
    engine::ast::{relations, text},
};
use std::{borrow::Cow, collections::HashSet};
use tree_sitter::Node;

pub(super) fn type_expr(node: Node<'_>, source: &str) -> TypeExpr {
    let generic_names = generic_names(node, source);
    type_expr_inner(node, source, &generic_names, 0)
}

fn type_expr_inner(
    node: Node<'_>,
    source: &str,
    generic_names: &HashSet<&str>,
    depth: usize,
) -> TypeExpr {
    if depth >= 8 {
        return TypeExpr::Unknown;
    }
    let mut node = node;
    while node.kind() == "reference_type" {
        let Some(inner) = node.child_by_field_name("type") else {
            return TypeExpr::Unknown;
        };
        node = inner;
    }
    if matches!(
        node.kind(),
        "type_identifier" | "scoped_type_identifier" | "primitive_type"
    ) {
        let name = text(node, source).trim();
        if name != "_" && !generic_root(node, source, generic_names) {
            return TypeExpr::Named {
                name: name.to_owned(),
            };
        }
    }
    if node.kind() == "generic_type" {
        let Some(base) = node
            .child_by_field_name("type")
            .and_then(|node| static_name(node, source))
        else {
            return TypeExpr::Unknown;
        };
        if generic_root(node, source, generic_names) {
            return TypeExpr::Unknown;
        }
        let Some(arguments) = node.child_by_field_name("type_arguments") else {
            return TypeExpr::Unknown;
        };
        let mut cursor = arguments.walk();
        let mut args = Vec::new();
        let mut has_lifetime = false;
        for argument in arguments.named_children(&mut cursor) {
            if argument.kind() == "lifetime" {
                has_lifetime = true;
                continue;
            }
            if args.len() == 4 {
                return TypeExpr::Unknown;
            }
            args.push(type_expr_inner(argument, source, generic_names, depth + 1));
        }
        if !args.is_empty() {
            return TypeExpr::Applied { base, args };
        }
        if has_lifetime {
            return TypeExpr::Named { name: base };
        }
    }
    TypeExpr::Unknown
}

fn generic_root(mut node: Node<'_>, source: &str, names: &HashSet<&str>) -> bool {
    if names.is_empty() {
        return false;
    }
    for _ in 0..8 {
        node = match node.kind() {
            "generic_type" => match node.child_by_field_name("type") {
                Some(base) => base,
                None => return true,
            },
            "scoped_type_identifier" | "scoped_identifier" => {
                match node.child_by_field_name("path") {
                    Some(path) => path,
                    None => return false,
                }
            }
            "type_identifier" | "identifier" | "primitive_type" => {
                return names.contains(text(node, source));
            }
            _ => return false,
        };
    }
    true
}

fn static_name(node: Node<'_>, source: &str) -> Option<String> {
    matches!(
        node.kind(),
        "identifier" | "scoped_identifier" | "type_identifier" | "scoped_type_identifier"
    )
    .then(|| text(node, source).trim().to_owned())
}

pub(super) fn callee_name<'a>(mut node: Node<'_>, source: &'a str) -> Option<Cow<'a, str>> {
    const MAX_SEGMENTS: usize = 8;
    const MAX_BYTES: usize = 512;

    struct Parts<'a> {
        names: [Option<(&'static str, &'a str)>; MAX_SEGMENTS],
        len: usize,
        bytes: usize,
    }

    impl<'a> Parts<'a> {
        fn push(&mut self, separator: &'static str, name: &'a str) -> Option<()> {
            if self.len == MAX_SEGMENTS || name.is_empty() {
                return None;
            }
            self.bytes = self
                .bytes
                .checked_add(separator.len())?
                .checked_add(name.len())?;
            if self.bytes > MAX_BYTES
                || !name
                    .chars()
                    .all(|character| character.is_alphanumeric() || character == '_')
            {
                return None;
            }
            self.names[self.len] = Some((separator, name));
            self.len += 1;
            Some(())
        }

        fn collect(&mut self, node: Node<'_>, source: &'a str, depth: usize) -> Option<()> {
            if depth >= MAX_SEGMENTS || node.has_error() || node.is_missing() {
                return None;
            }
            match node.kind() {
                "identifier" | "type_identifier" | "self" | "super" | "crate" => {
                    self.push("", text(node, source))
                }
                "scoped_identifier" => {
                    if let Some(path) = node.child_by_field_name("path") {
                        self.collect(path, source, depth + 1)?;
                    }
                    let name = node.child_by_field_name("name")?;
                    if !matches!(name.kind(), "identifier" | "super") {
                        return None;
                    }
                    self.push("::", text(name, source))
                }
                "field_expression" => {
                    self.collect(node.child_by_field_name("value")?, source, depth + 1)?;
                    let field = node.child_by_field_name("field")?;
                    if field.kind() != "field_identifier" {
                        return None;
                    }
                    self.push(".", text(field, source))
                }
                _ => None,
            }
        }
    }

    if node.has_error() || node.is_missing() {
        return None;
    }
    if node.kind() == "generic_function" {
        node = node.child_by_field_name("function")?;
    }
    let mut parts = Parts {
        names: [None; MAX_SEGMENTS],
        len: 0,
        bytes: 0,
    };
    parts.collect(node, source, 0)?;
    let original = text(node, source);
    if original.len() == parts.bytes {
        return Some(Cow::Borrowed(original));
    }
    let mut canonical = String::with_capacity(parts.bytes);
    for (separator, name) in parts.names.into_iter().take(parts.len).flatten() {
        canonical.push_str(separator);
        canonical.push_str(name);
    }
    Some(Cow::Owned(canonical))
}

fn value(node: Node<'_>, source: &str) -> ValueExpr {
    match node.kind() {
        "struct_expression" => node
            .child_by_field_name("name")
            .and_then(|name| static_name(name, source))
            .map_or(ValueExpr::Unknown, |callee| ValueExpr::Construct { callee }),
        "call_expression" => node
            .child_by_field_name("function")
            .and_then(|name| callee_name(name, source))
            .map_or(ValueExpr::Unknown, |callee| ValueExpr::Call {
                callee: callee.into_owned(),
            }),
        "identifier" | "scoped_identifier" => ValueExpr::Alias {
            name: text(node, source).to_owned(),
        },
        _ => ValueExpr::Unknown,
    }
}

pub(super) fn extract(node: Node<'_>, source: &str) -> ValueFlowFacts {
    let mut facts = ValueFlowFacts::default();
    let mut generics = None;
    let mut resolve = |ty: Node<'_>| {
        let names = generics.get_or_insert_with(|| generic_names(node, source));
        type_expr_inner(ty, source, names, 0)
    };
    if node.kind() == "type_item" {
        facts.alias_type = node.child_by_field_name("type").map(&mut resolve);
    }
    if matches!(node.kind(), "function_item" | "function_signature_item") {
        facts.return_type = node.child_by_field_name("return_type").map(|ty| {
            if is_async(node, source) {
                TypeExpr::Unknown
            } else {
                resolve(ty)
            }
        });
        if let Some(parameters) = node.child_by_field_name("parameters") {
            let mut cursor = parameters.walk();
            for parameter in parameters.named_children(&mut cursor) {
                let (Some(pattern), Some(annotation)) = (
                    parameter.child_by_field_name("pattern"),
                    parameter.child_by_field_name("type"),
                ) else {
                    continue;
                };
                let resolved = resolve(annotation);
                match &resolved {
                    TypeExpr::Applied { .. } => {}
                    TypeExpr::Named { .. } => {
                        let mut nominal = annotation;
                        while nominal.kind() == "reference_type" {
                            let Some(inner) = nominal.child_by_field_name("type") else {
                                break;
                            };
                            nominal = inner;
                        }
                        if nominal.kind() == "primitive_type" && text(nominal, source) != "str" {
                            continue;
                        }
                    }
                    TypeExpr::Unknown => continue,
                }
                let pattern = if pattern.kind() == "mut_pattern" {
                    pattern.named_child(0).unwrap_or(pattern)
                } else {
                    pattern
                };
                if pattern.kind() != "identifier" {
                    continue;
                }
                let position = parameter.end_position();
                facts.bindings.push(BindingFact {
                    name: text(pattern, source).to_owned(),
                    position: SourcePosition {
                        line: position.row + 1,
                        column: position.column,
                    },
                    value: ValueExpr::Annotated {
                        type_expr: resolved,
                    },
                    conditional: false,
                });
            }
        }
    }
    let Some(body) = node.child_by_field_name("body") else {
        return facts;
    };
    if node.kind() == "struct_item" {
        let mut push_field = |name, at: Node<'_>, ty| {
            let position = at.start_position();
            facts.fields.push(FieldFact {
                name,
                position: SourcePosition {
                    line: position.row + 1,
                    column: position.column,
                },
                value: ValueExpr::Annotated {
                    type_expr: resolve(ty),
                },
                conditional: false,
            });
        };
        let mut cursor = body.walk();
        if body.kind() == "ordered_field_declaration_list" {
            for (ordinal, ty) in body.children_by_field_name("type", &mut cursor).enumerate() {
                push_field(ordinal.to_string(), ty, ty);
            }
        } else {
            for field in body.named_children(&mut cursor) {
                if field.kind() != "field_declaration" {
                    continue;
                }
                if let (Some(name), Some(ty)) = (
                    field.child_by_field_name("name"),
                    field.child_by_field_name("type"),
                ) {
                    push_field(text(name, source).to_owned(), field, ty);
                }
            }
        }
        return facts;
    }
    let mut stack = vec![(body, false)];
    while let Some((current, conditional)) = stack.pop() {
        if current != body
            && matches!(
                current.kind(),
                "function_item" | "closure_expression" | "impl_item" | "trait_item" | "mod_item"
            )
        {
            continue;
        }
        let conditional = conditional
            || (current != body
                && matches!(
                    current.kind(),
                    "block"
                        | "if_expression"
                        | "match_expression"
                        | "while_expression"
                        | "for_expression"
                        | "loop_expression"
                ));
        if matches!(
            current.kind(),
            "let_declaration" | "assignment_expression" | "compound_assignment_expr"
        ) {
            let pattern = current
                .child_by_field_name("pattern")
                .or_else(|| current.child_by_field_name("left"));
            if let Some(pattern) = pattern {
                let mut emit = |pattern: Node<'_>| {
                    let position = current.end_position();
                    let inferred = if current.kind() == "let_declaration"
                        && current.child_by_field_name("value").is_some()
                        && current
                            .child_by_field_name("pattern")
                            .is_some_and(|pattern| {
                                matches!(pattern.kind(), "identifier" | "mut_pattern")
                            }) {
                        current
                            .child_by_field_name("type")
                            .map(|ty| ValueExpr::Annotated {
                                type_expr: resolve(ty),
                            })
                            .unwrap_or_else(|| {
                                value(
                                    current.child_by_field_name("value").expect("value checked"),
                                    source,
                                )
                            })
                    } else {
                        ValueExpr::Unknown
                    };
                    facts.bindings.push(BindingFact {
                        name: text(pattern, source).to_owned(),
                        position: SourcePosition {
                            line: position.row + 1,
                            column: position.column,
                        },
                        value: inferred,
                        conditional,
                    });
                };
                if current.kind() == "let_declaration" {
                    super::patterns::bindings(pattern, emit);
                } else {
                    let mut names = vec![pattern];
                    while let Some(pattern) = names.pop() {
                        if pattern.kind() == "identifier" {
                            emit(pattern);
                        } else {
                            let mut cursor = pattern.walk();
                            names.extend(pattern.named_children(&mut cursor));
                        }
                    }
                }
            }
        }
        let mut cursor = current.walk();
        stack.extend(
            current
                .named_children(&mut cursor)
                .map(|child| (child, conditional)),
        );
    }
    facts.bindings.sort_by(|left, right| {
        left.position
            .cmp(&right.position)
            .then_with(|| left.name.cmp(&right.name))
    });
    facts.fields.sort_by(|left, right| {
        left.position
            .cmp(&right.position)
            .then_with(|| left.name.cmp(&right.name))
    });
    facts
}

pub(super) fn is_async(node: Node<'_>, source: &str) -> bool {
    let mut cursor = node.walk();
    let is_async = node.named_children(&mut cursor).any(|child| {
        child.kind() == "function_modifiers"
            && text(child, source)
                .split_whitespace()
                .any(|modifier| modifier == "async")
    });
    is_async
}

fn generic_names<'a>(node: Node<'_>, source: &'a str) -> HashSet<&'a str> {
    let mut generic_names = HashSet::new();
    let mut ancestor = Some(node);
    while let Some(scope) = ancestor {
        if let Some(parameters) = scope.child_by_field_name("type_parameters") {
            let mut cursor = parameters.walk();
            for parameter in parameters.named_children(&mut cursor) {
                let name = match parameter.kind() {
                    "type_identifier" => Some(parameter),
                    "type_parameter" | "const_parameter" => parameter.child_by_field_name("name"),
                    "constrained_type_parameter" | "optional_type_parameter" => parameter
                        .child_by_field_name("left")
                        .or_else(|| parameter.child_by_field_name("name")),
                    _ => None,
                };
                if let Some(name) = name {
                    let name = if name.kind() == "constrained_type_parameter" {
                        name.child_by_field_name("left")
                    } else {
                        Some(name)
                    };
                    if let Some(name) = name {
                        generic_names.insert(text(name, source));
                    }
                }
            }
        }
        ancestor = scope.parent();
    }
    generic_names
}

pub(super) fn type_references(
    facts: &mut Facts,
    owner: &str,
    node: Node<'_>,
    source: &str,
    line: usize,
) {
    let generic_names = generic_names(node, source);
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "lifetime" | "lifetime_parameter" => {}
            "scoped_type_identifier" => {
                relations::reference(facts, owner, text(node, source), "references", line)
            }
            "type_identifier" | "identifier" => {
                let name = text(node, source);
                if name != "_" && !name.starts_with('\'') && !generic_names.contains(name) {
                    relations::reference(facts, owner, name, "references", line);
                }
            }
            _ => {
                let mut cursor = node.walk();
                stack.extend(node.named_children(&mut cursor));
            }
        }
    }
}
