use crate::{
    core::semantic::{BindingFact, FieldFact, SourcePosition, TypeExpr, ValueExpr, ValueFlowFacts},
    engine::ast::{field, text},
};
use std::collections::HashSet;
use tree_sitter::Node;

pub(super) fn unique_module_aliases(node: Node<'_>, source: &str) -> HashSet<String> {
    fn targets(
        node: Node<'_>,
        source: &str,
        alias: bool,
        writes: &mut std::collections::HashMap<String, (usize, bool)>,
    ) {
        if node.kind() == "identifier" {
            let entry = writes
                .entry(text(node, source).to_owned())
                .or_insert((0, true));
            entry.0 += 1;
            entry.1 &= alias;
        } else if matches!(
            node.kind(),
            "tuple_pattern"
                | "list_pattern"
                | "pattern_list"
                | "list_splat_pattern"
                | "dictionary_splat_pattern"
        ) {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                targets(child, source, false, writes);
            }
        }
    }
    fn visit(
        node: Node<'_>,
        source: &str,
        writes: &mut std::collections::HashMap<String, (usize, bool)>,
    ) {
        if matches!(
            node.kind(),
            "function_definition" | "class_definition" | "lambda"
        ) {
            return;
        }
        let target = match node.kind() {
            "assignment" | "augmented_assignment" | "for_statement" => {
                node.child_by_field_name("left")
            }
            "named_expression" => node.child_by_field_name("name"),
            "as_pattern" => node.child_by_field_name("alias"),
            _ => None,
        };
        if let Some(target) = target {
            targets(target, source, super::type_alias(node, source), writes);
        }
        if matches!(node.kind(), "import_statement" | "import_from_statement") {
            let mut cursor = node.walk();
            for child in node.children_by_field_name("name", &mut cursor) {
                let name = field(child, source, "alias")
                    .or_else(|| field(child, source, "name"))
                    .unwrap_or_else(|| text(child, source));
                let bound = if node.kind() == "import_statement" && !child.kind().contains("alias")
                {
                    name.split('.').next().unwrap_or(name)
                } else {
                    name
                };
                let entry = writes.entry(bound.to_owned()).or_insert((0, true));
                entry.0 += 1;
                entry.1 = false;
            }
        }
        if node.kind() == "delete_statement" {
            let mut cursor = node.walk();
            for target in node.named_children(&mut cursor) {
                targets(target, source, false, writes);
            }
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            visit(child, source, writes);
        }
    }
    let mut writes = std::collections::HashMap::new();
    visit(node, source, &mut writes);
    writes
        .into_iter()
        .filter_map(|(name, (count, alias))| (count == 1 && alias).then_some(name))
        .collect()
}

fn position(node: Node<'_>) -> SourcePosition {
    crate::engine::languages::source_end(node)
}

fn static_name(node: Node<'_>) -> bool {
    node.kind() == "identifier"
        || (node.kind() == "attribute"
            && node.child_by_field_name("object").is_some_and(static_name)
            && node
                .child_by_field_name("attribute")
                .is_some_and(|member| member.kind() == "identifier"))
}

fn annotation(node: Node<'_>, source: &str) -> TypeExpr {
    fn parse(node: Node<'_>, source: &str, depth: usize, remaining: &mut usize) -> TypeExpr {
        if depth >= crate::engine::languages::MAX_VALUE_FLOW_TYPE_DEPTH || *remaining == 0 {
            return TypeExpr::Unknown;
        }
        *remaining -= 1;
        if node.kind() == "type" {
            return node.named_child(0).map_or(TypeExpr::Unknown, |child| {
                parse(child, source, depth + 1, remaining)
            });
        }
        if matches!(node.kind(), "generic_type" | "subscript") {
            let base = if node.kind() == "subscript" {
                node.child_by_field_name("value")
            } else {
                node.named_child(0)
            };
            let Some(base) = base.filter(|base| static_name(*base)) else {
                return TypeExpr::Unknown;
            };
            let base = text(base, source);
            if base.len() > 256 {
                return TypeExpr::Unknown;
            }
            let mut cursor = node.walk();
            let arguments: Vec<_> = if node.kind() == "subscript" {
                let subs: Vec<_> = node.children_by_field_name("subscript", &mut cursor).collect();
                if subs.len() == 1 && subs[0].kind() == "tuple" {
                    let mut tuple_cursor = subs[0].walk();
                    subs[0].named_children(&mut tuple_cursor).collect()
                } else {
                    subs
                }
            } else {
                let Some(parameters) = node.named_child(1) else {
                    return TypeExpr::Unknown;
                };
                parameters.named_children(&mut cursor).collect()
            };
            if arguments.is_empty() || arguments.len() > *remaining {
                return TypeExpr::Unknown;
            }
            let args = arguments
                .into_iter()
                .map(|arg| parse(arg, source, depth + 1, remaining))
                .collect();
            return TypeExpr::Applied {
                base: base.to_owned(),
                args,
            };
        }
        let value = text(node, source).trim();
        let value = value
            .strip_prefix('\'')
            .and_then(|value| value.strip_suffix('\''))
            .or_else(|| {
                value
                    .strip_prefix('"')
                    .and_then(|value| value.strip_suffix('"'))
            })
            .unwrap_or(value);
        if !value.is_empty()
            && value.len() <= 256
            && value.split('.').all(|part| {
                !part.is_empty()
                    && part.chars().enumerate().all(|(i, c)| {
                        c == '_'
                            || if i == 0 {
                                c.is_alphabetic()
                            } else {
                                c.is_alphanumeric()
                            }
                    })
            })
        {
            TypeExpr::Named {
                name: value.to_owned(),
            }
        } else {
            TypeExpr::Unknown
        }
    }
    parse(node, source, 0, &mut 32)
}

fn literal_value(node: Node<'_>) -> Option<ValueExpr> {
    let kind = if node.kind() == "dictionary" { "dict" } else { node.kind() };
    crate::core::semantic::python_literal_constructor(kind)
        .map(|callee| ValueExpr::Construct { callee: callee.to_owned() })
}

fn value(node: Node<'_>, source: &str) -> ValueExpr {
    if let Some(literal) = literal_value(node) {
        return literal;
    }
    match node.kind() {
        "identifier" => ValueExpr::Alias {
            name: text(node, source).to_owned(),
        },
        "call" => node
            .child_by_field_name("function")
            .filter(|callee| static_name(*callee))
            .map_or(ValueExpr::Unknown, |callee| ValueExpr::Call {
                callee: text(callee, source).to_owned(),
            }),
        "attribute" => {
            let Some(receiver) = node
                .child_by_field_name("object")
                .filter(|receiver| static_name(*receiver))
            else {
                return ValueExpr::Unknown;
            };
            let Some(member) = field(node, source, "attribute") else {
                return ValueExpr::Unknown;
            };
            ValueExpr::Field {
                receiver: text(receiver, source).to_owned(),
                member: member.to_owned(),
            }
        }
        _ => ValueExpr::Unknown,
    }
}

struct Collector<'a> {
    source: &'a str,
    class_scope: bool,
    local_scope: bool,
    receiver: Option<&'a str>,
    init: bool,
    uncertain_callable: bool,
    nonlocal_names: HashSet<String>,
    facts: ValueFlowFacts,
}

impl Collector<'_> {
    fn bind(&mut self, target: Node<'_>, value: ValueExpr, at: SourcePosition, conditional: bool) {
        if target.kind() == "identifier" {
            let name = text(target, self.source).to_owned();
            let value = if self.nonlocal_names.contains(&name) {
                ValueExpr::Unknown
            } else {
                value
            };
            if self.class_scope {
                self.facts.fields.push(FieldFact {
                    name,
                    value,
                    position: at,
                    conditional,
                });
            } else {
                self.facts.bindings.push(BindingFact {
                    name,
                    value,
                    position: at,
                    conditional,
                });
            }
        } else if target.kind() == "attribute" {
            if target
                .child_by_field_name("object")
                .is_some_and(|receiver| {
                    receiver.kind() == "identifier"
                        && Some(text(receiver, self.source)) == self.receiver
                })
            {
                if let Some(member) = field(target, self.source, "attribute") {
                    self.facts.fields.push(FieldFact {
                        name: member.to_owned(),
                        value: if self.init && !self.uncertain_callable {
                            value
                        } else {
                            ValueExpr::Unknown
                        },
                        position: at,
                        conditional,
                    });
                }
            }
        } else if matches!(
            target.kind(),
            "tuple_pattern"
                | "list_pattern"
                | "pattern_list"
                | "list_splat_pattern"
                | "dictionary_splat_pattern"
        ) {
            let mut cursor = target.walk();
            for child in target.named_children(&mut cursor) {
                self.bind(child, ValueExpr::Unknown, at, conditional);
            }
        }
    }

    fn visit(&mut self, node: Node<'_>, conditional: bool) {
        if !self.local_scope && !self.class_scope && super::type_alias(node, self.source) {
            return;
        }
        if matches!(node.kind(), "function_definition" | "class_definition") {
            if self.local_scope {
                if let Some(name) = node.child_by_field_name("name") {
                    self.bind(name, ValueExpr::Unknown, position(node), conditional);
                }
            }
            return;
        }
        if node.kind() == "lambda" {
            return;
        }
        let conditional = conditional
            || matches!(
                node.kind(),
                "if_statement"
                    | "elif_clause"
                    | "else_clause"
                    | "for_statement"
                    | "while_statement"
                    | "try_statement"
                    | "except_clause"
                    | "finally_clause"
                    | "with_statement"
                    | "match_statement"
                    | "conditional_expression"
                    | "boolean_operator"
            );
        match node.kind() {
            "assignment" => {
                if let Some(left) = node.child_by_field_name("left") {
                    let literal = node.child_by_field_name("right").and_then(literal_value);
                    let assigned = if let Some(literal) = literal {
                        literal
                    } else if self.class_scope && node.child_by_field_name("type").is_some()
                    {
                        ValueExpr::Annotated {
                            type_expr: annotation(
                                node.child_by_field_name("type").unwrap(),
                                self.source,
                            ),
                        }
                    } else {
                        node.child_by_field_name("right")
                            .map_or(ValueExpr::Unknown, |right| {
                                node.child_by_field_name("type").map_or_else(
                                    || value(right, self.source),
                                    |ty| ValueExpr::Annotated {
                                        type_expr: annotation(ty, self.source),
                                    },
                                )
                            })
                    };
                    self.bind(left, assigned, position(node), conditional);
                }
            }
            "augmented_assignment" => {
                if let Some(left) = node.child_by_field_name("left") {
                    self.bind(left, ValueExpr::Unknown, position(node), conditional);
                }
            }
            "named_expression" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.bind(
                        name,
                        node.child_by_field_name("value")
                            .map_or(ValueExpr::Unknown, |right| value(right, self.source)),
                        position(node),
                        conditional,
                    );
                }
            }
            "for_statement" => {
                if let Some(left) = node.child_by_field_name("left") {
                    self.bind(left, ValueExpr::Unknown, position(left), true);
                }
            }
            "as_pattern" => {
                if let Some(alias) = node.child_by_field_name("alias") {
                    self.bind(alias, ValueExpr::Unknown, position(alias), true);
                }
            }
            "delete_statement" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.bind(child, ValueExpr::Unknown, position(node), conditional);
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.visit(child, conditional);
        }
    }
}

fn generator(node: Node<'_>) -> bool {
    if matches!(
        node.kind(),
        "function_definition" | "class_definition" | "lambda"
    ) {
        return false;
    }
    if node.kind() == "yield" {
        return true;
    }
    let mut cursor = node.walk();
    let found = node.named_children(&mut cursor).any(generator);
    found
}

fn nonlocal_names(node: Node<'_>, source: &str, names: &mut HashSet<String>) {
    if matches!(
        node.kind(),
        "function_definition" | "class_definition" | "lambda"
    ) {
        return;
    }
    if matches!(node.kind(), "global_statement" | "nonlocal_statement") {
        let mut cursor = node.walk();
        for name in node
            .named_children(&mut cursor)
            .filter(|name| name.kind() == "identifier")
        {
            let name = text(name, source).to_owned();
            names.insert(name);
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        nonlocal_names(child, source, names);
    }
}

fn returned_value(body: Node<'_>, source: &str) -> Option<(ValueExpr, SourcePosition)> {
    fn uncertain_expression(node: Node<'_>) -> bool {
        if matches!(
            node.kind(),
            "named_expression"
                | "conditional_expression"
                | "boolean_operator"
                | "await"
                | "yield"
                | "lambda"
        ) {
            return true;
        }
        let mut cursor = node.walk();
        let uncertain = node.named_children(&mut cursor).any(uncertain_expression);
        uncertain
    }
    if uncertain_expression(body) {
        return None;
    }
    let mut cursor = body.walk();
    let statements: Vec<_> = body
        .named_children(&mut cursor)
        .filter(|statement| statement.kind() != "comment")
        .collect();
    let (last, preceding) = statements.split_last()?;
    if last.kind() != "return_statement" {
        return None;
    }
    for statement in preceding {
        match statement.kind() {
            "pass_statement" | "import_statement" | "import_from_statement" => {}
            "expression_statement" => {
                let expression = statement.named_child(0)?;
                if expression.kind() == "string" {
                    continue;
                }
                if expression.kind() != "assignment"
                    || !expression
                        .child_by_field_name("left")
                        .is_some_and(|left| left.kind() == "identifier")
                {
                    return None;
                }
            }
            _ => return None,
        }
    }
    let expression = last.named_child(0)?;
    let returned = value(expression, source);
    (!matches!(returned, ValueExpr::Unknown)).then(|| (returned, position(expression)))
}

pub(super) fn extract(node: Node<'_>, source: &str) -> ValueFlowFacts {
    if super::type_alias(node, source) {
        let right = node.child_by_field_name("right").or_else(|| {
            (node.kind() == "type_alias_statement")
                .then(|| node.named_child(node.named_child_count().saturating_sub(1)))
                .flatten()
        });
        let alias_type = right
            .map(|right| {
                if right.kind() == "call" {
                    if field(right, source, "function")
                        .is_some_and(|callee| super::typing_name(callee) == "TypeAliasType")
                    {
                        right
                            .child_by_field_name("arguments")
                            .and_then(|arguments| arguments.named_child(1))
                            .map_or(TypeExpr::Unknown, |value| annotation(value, source))
                    } else {
                        TypeExpr::Unknown
                    }
                } else {
                    annotation(right, source)
                }
            })
            .unwrap_or(TypeExpr::Unknown);
        return ValueFlowFacts {
            alias_type: Some(alias_type),
            ..ValueFlowFacts::default()
        };
    }
    if !matches!(
        node.kind(),
        "module" | "class_definition" | "function_definition"
    ) {
        return ValueFlowFacts::default();
    }
    let callable = node.kind() == "function_definition";
    let body = if node.kind() == "module" {
        Some(node)
    } else {
        node.child_by_field_name("body")
    };
    let decorated = node
        .parent()
        .is_some_and(|parent| parent.kind() == "decorated_definition");
    let uncertain_callable = callable
        && (decorated
            || text(node, source).trim_start().starts_with("async ")
            || body.is_some_and(generator));
    let receiver = if callable {
        node.child_by_field_name("parameters")
            .and_then(|params| params.named_child(0))
            .and_then(|parameter| {
                if parameter.kind() == "identifier" {
                    Some(text(parameter, source))
                } else {
                    field(parameter, source, "name").or_else(|| {
                        parameter
                            .named_child(0)
                            .filter(|name| name.kind() == "identifier")
                            .map(|name| text(name, source))
                    })
                }
            })
            .filter(|name| matches!(*name, "self" | "cls"))
    } else {
        None
    };
    let mut scoped_names = HashSet::new();
    if callable {
        if let Some(body) = body {
            nonlocal_names(body, source, &mut scoped_names);
        }
    }
    let mut collector = Collector {
        source,
        class_scope: node.kind() == "class_definition",
        local_scope: callable,
        receiver,
        init: field(node, source, "name") == Some("__init__"),
        uncertain_callable,
        nonlocal_names: scoped_names,
        facts: ValueFlowFacts::default(),
    };
    let mut scoped_names: Vec<_> = collector.nonlocal_names.iter().collect();
    scoped_names.sort_unstable();
    for name in scoped_names {
        let start = node.start_position();
        collector.facts.bindings.push(BindingFact {
            name: name.clone(),
            position: SourcePosition {
                line: start.row + 1,
                column: start.column,
            },
            value: ValueExpr::Unknown,
            conditional: false,
        });
    }
    if callable && !uncertain_callable {
        collector.facts.return_type = node
            .child_by_field_name("return_type")
            .map(|ty| annotation(ty, source));
        if collector.nonlocal_names.is_empty() {
            if let Some((returned, at)) = body.and_then(|body| returned_value(body, source)) {
                collector.facts.return_value = Some(returned);
                collector.facts.return_position = Some(at);
            }
        }
    }
    if let Some(body) = body {
        collector.visit(body, false);
    }
    collector.facts.bindings.sort_by_key(|a| a.position);
    collector.facts.fields.sort_by_key(|a| a.position);
    collector.facts
}
