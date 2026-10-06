use crate::{
    core::semantic::{
        AnnotationReferenceFact, BindingFact, CollectionElementFact, FieldFact, GuardImportFact,
        SourcePosition, TypeExpr, TypeOnlyImportFact, ValueExpr, ValueFlowFacts,
    },
    engine::ast::{field, text},
};
use std::collections::{HashMap, HashSet};
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
                | "expression_list"
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
        if node.kind() == "delete_statement" {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                targets(child, source, false, writes);
            }
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

fn annotation_reference_positions(
    node: Node<'_>,
    source: &str,
    output: &mut Vec<AnnotationReferenceFact>,
) {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        let is_reference = match current.kind() {
            "scoped_type_identifier" | "attribute" => true,
            "type_identifier" | "identifier" => {
                !matches!(
                    text(current, source),
                    "_" | "self" | "cls" | "this" | "true" | "false" | "None" | "nil" | "null"
                ) && !text(current, source).starts_with('\'')
            }
            _ => false,
        };
        if is_reference {
            let start = current.start_position();
            output.push(AnnotationReferenceFact {
                expression: text(current, source).to_owned(),
                position: SourcePosition {
                    line: start.row + 1,
                    column: start.column,
                },
            });
        } else {
            let mut cursor = current.walk();
            for child in current.named_children(&mut cursor) {
                stack.push(child);
            }
        }
    }
}

fn type_only_imports(module: Node<'_>, source: &str) -> Vec<TypeOnlyImportFact> {
    fn target_names(node: Node<'_>, source: &str, names: &mut HashSet<String>) {
        if matches!(node.kind(), "identifier" | "attribute") {
            names.insert(text(node, source).to_owned());
        } else if matches!(
            node.kind(),
            "tuple_pattern" | "list_pattern" | "pattern_list"
        ) {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                target_names(child, source, names);
            }
        }
    }

    fn written_names(node: Node<'_>, source: &str, names: &mut HashSet<String>) {
        match node.kind() {
            "function_definition" | "class_definition" => {
                if let Some(name) = field(node, source, "name") {
                    names.insert(name.to_owned());
                }
                return;
            }
            "import_from_statement" | "import_statement" => {
                let mut cursor = node.walk();
                for child in node.children_by_field_name("name", &mut cursor) {
                    let name = field(child, source, "name").unwrap_or_else(|| text(child, source));
                    let alias = field(child, source, "alias").unwrap_or_else(|| {
                        if node.kind() == "import_statement" {
                            name.split('.').next().unwrap_or(name)
                        } else {
                            name
                        }
                    });
                    names.insert(alias.to_owned());
                }
                return;
            }
            "assignment" | "augmented_assignment" | "for_statement" => {
                if let Some(target) = node.child_by_field_name("left") {
                    target_names(target, source, names);
                }
            }
            "named_expression" => {
                if let Some(target) = node.child_by_field_name("name") {
                    target_names(target, source, names);
                }
            }
            "as_pattern" => {
                if let Some(target) = node.child_by_field_name("alias") {
                    target_names(target, source, names);
                }
            }
            "delete_statement" => {
                let mut cursor = node.walk();
                for target in node.named_children(&mut cursor) {
                    target_names(target, source, names);
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            written_names(child, source, names);
        }
    }

    fn guarded_imports(
        node: Node<'_>,
        source: &str,
        guard: Option<&GuardImportFact>,
        output: &mut Vec<TypeOnlyImportFact>,
    ) {
        match node.kind() {
            "import_from_statement" | "import_statement" => {
                let mut cursor = node.walk();
                for child in node.children_by_field_name("name", &mut cursor) {
                    let name = field(child, source, "name").unwrap_or_else(|| text(child, source));
                    let alias = field(child, source, "alias").unwrap_or_else(|| {
                        if node.kind() == "import_statement" {
                            name.split('.').next().unwrap_or(name)
                        } else {
                            name
                        }
                    });
                    output.push(TypeOnlyImportFact {
                        alias: alias.to_owned(),
                        position: SourcePosition {
                            line: node.start_position().row + 1,
                            column: node.start_position().column,
                        },
                        guard_import: guard.cloned(),
                    });
                }
            }
            "function_definition" | "class_definition" => {}
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    guarded_imports(
                        child,
                        source,
                        guard.filter(|_| node.kind() == "block"),
                        output,
                    );
                }
            }
        }
    }

    fn nested_guards(
        node: Node<'_>,
        source: &str,
        known: &HashSet<String>,
        output: &mut Vec<TypeOnlyImportFact>,
    ) {
        if node.kind() == "if_statement" {
            if let Some(condition) = field(node, source, "condition") {
                let condition = condition.trim();
                if condition == "TYPE_CHECKING"
                    || condition.ends_with(".TYPE_CHECKING")
                    || known.contains(condition)
                {
                    if let Some(consequence) = node.child_by_field_name("consequence") {
                        guarded_imports(consequence, source, None, output);
                    }
                    return;
                }
            }
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            nested_guards(child, source, known, output);
        }
    }

    let mut known_guards = HashSet::new();
    let mut safe_guards: HashMap<String, GuardImportFact> = HashMap::new();
    let mut output = Vec::new();
    let mut cursor = module.walk();
    for statement in module.named_children(&mut cursor) {
        if !safe_guards.is_empty() {
            let mut writes = HashSet::new();
            written_names(statement, source, &mut writes);
            safe_guards.retain(|guard, _| {
                !writes.contains(guard)
                    && !writes.contains(guard.split('.').next().unwrap_or(guard.as_str()))
            });
        }
        match statement.kind() {
            "import_from_statement"
                if matches!(
                    field(statement, source, "module_name"),
                    Some("typing" | "typing_extensions")
                ) =>
            {
                let mut names = statement.walk();
                for child in statement.children_by_field_name("name", &mut names) {
                    if field(child, source, "name").unwrap_or_else(|| text(child, source))
                        == "TYPE_CHECKING"
                    {
                        let alias = field(child, source, "alias").unwrap_or("TYPE_CHECKING");
                        known_guards.insert(alias.to_owned());
                        safe_guards.insert(
                            alias.to_owned(),
                            GuardImportFact {
                                alias: alias.to_owned(),
                                position: SourcePosition {
                                    line: statement.start_position().row + 1,
                                    column: statement.start_position().column,
                                },
                            },
                        );
                    }
                }
            }
            "import_statement" => {
                let mut names = statement.walk();
                for child in statement.children_by_field_name("name", &mut names) {
                    let name = field(child, source, "name").unwrap_or_else(|| text(child, source));
                    if matches!(name, "typing" | "typing_extensions") {
                        let alias = field(child, source, "alias").unwrap_or(name);
                        let guard = format!("{alias}.TYPE_CHECKING");
                        known_guards.insert(guard.clone());
                        safe_guards.insert(
                            guard,
                            GuardImportFact {
                                alias: alias.to_owned(),
                                position: SourcePosition {
                                    line: statement.start_position().row + 1,
                                    column: statement.start_position().column,
                                },
                            },
                        );
                    }
                }
            }
            "if_statement" => {
                if let Some(condition) = field(statement, source, "condition") {
                    let condition = condition.trim();
                    if condition == "TYPE_CHECKING"
                        || condition.ends_with(".TYPE_CHECKING")
                        || known_guards.contains(condition)
                    {
                        if let Some(consequence) = statement.child_by_field_name("consequence") {
                            guarded_imports(
                                consequence,
                                source,
                                safe_guards.get(condition),
                                &mut output,
                            );
                        }
                    } else {
                        nested_guards(statement, source, &known_guards, &mut output);
                    }
                }
            }
            "function_definition"
            | "class_definition"
            | "decorated_definition"
            | "try_statement"
            | "with_statement"
            | "for_statement"
            | "while_statement"
            | "match_statement" => {
                nested_guards(statement, source, &known_guards, &mut output);
            }
            _ => {}
        }
    }
    output
}

fn direct_branch_end(node: Node<'_>) -> Option<SourcePosition> {
    let statement = node.parent()?;
    let body = if statement.kind() == "expression_statement" {
        statement.parent()?
    } else {
        statement
    };
    let body = (body.kind() == "block").then_some(body)?;
    let branch = body.parent()?;
    let conditional = match branch.kind() {
        "if_statement" => branch,
        "elif_clause" | "else_clause" => branch
            .parent()
            .filter(|parent| parent.kind() == "if_statement")?,
        _ => return None,
    };
    let parent = conditional.parent()?;
    let enclosing = if parent.kind() == "block" {
        parent.parent()?
    } else {
        parent
    };
    matches!(enclosing.kind(), "function_definition" | "module").then(|| position(body))
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
                let subs: Vec<_> = node
                    .children_by_field_name("subscript", &mut cursor)
                    .collect();
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
    let kind = if node.kind() == "dictionary" {
        "dict"
    } else {
        node.kind()
    };
    crate::core::semantic::python_literal_constructor(kind).map(|callee| ValueExpr::Construct {
        callee: callee.to_owned(),
    })
}

fn value(node: Node<'_>, source: &str) -> ValueExpr {
    if let Some(literal) = literal_value(node) {
        return literal;
    }
    match node.kind() {
        "identifier" => ValueExpr::Alias {
            name: text(node, source).to_owned(),
        },
        "await" => node.named_child(0).map_or(ValueExpr::Unknown, |expression| {
            ValueExpr::Await {
                value: Box::new(value(expression, source)),
            }
        }),
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
    has_django_shortcuts_import: bool,
    class_scope: bool,
    local_scope: bool,
    receiver: Option<&'a str>,
    init: bool,
    uncertain_callable: bool,
    collect_summary: bool,
    collect_return_summary: bool,
    summary: BodySummary,
    facts: ValueFlowFacts,
}

impl Collector<'_> {
    fn bind(&mut self, target: Node<'_>, value: ValueExpr, at: SourcePosition, conditional: bool) {
        if target.kind() == "identifier" {
            let name = text(target, self.source).to_owned();
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

    fn visit(
        &mut self,
        node: Node<'_>,
        conditional: bool,
        collect_facts: bool,
        lexical_scope: bool,
    ) {
        if self.collect_summary {
            if self.collect_return_summary
                && matches!(
                node.kind(),
                "named_expression"
                    | "conditional_expression"
                    | "boolean_operator"
                    | "await"
                    | "yield"
                    | "lambda"
            ) {
                self.summary.has_uncertain_expression = true;
            }
            if lexical_scope {
                if node.kind() == "yield" {
                    self.summary.has_yield = true;
                }
                if matches!(node.kind(), "global_statement" | "nonlocal_statement") {
                    let mut cursor = node.walk();
                    self.summary.nonlocal_names.extend(
                        node.named_children(&mut cursor)
                            .filter(|name| name.kind() == "identifier")
                            .map(|name| text(name, self.source).to_owned()),
                    );
                }
            }
        }

        let nested_scope = matches!(
            node.kind(),
            "function_definition" | "class_definition" | "lambda"
        );
        if !self.local_scope && !self.class_scope && super::type_alias(node, self.source) {
            return;
        }
        if collect_facts
            && self.local_scope
            && matches!(node.kind(), "function_definition" | "class_definition")
        {
            if let Some(name) = node.child_by_field_name("name") {
                self.bind(name, ValueExpr::Unknown, position(node), conditional);
            }
        }
        if collect_facts && node.kind() == "lambda" {
            if self.collect_summary && self.collect_return_summary {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.visit(child, conditional, false, false);
                }
            }
            return;
        }
        if collect_facts && matches!(node.kind(), "function_definition" | "class_definition") {
            if self.collect_summary && self.collect_return_summary {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.visit(child, conditional, false, false);
                }
            }
            return;
        }
        if !collect_facts && !self.collect_summary {
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
                    | "match_statement"
                    | "conditional_expression"
                    | "boolean_operator"
            );
        if collect_facts {
            match node.kind() {
            "call" => {
                if let Some(context) = super::render_context::extract(
                    node,
                    self.source,
                    self.has_django_shortcuts_import,
                ) {
                    self.facts.template_contexts.push(context);
                }
            }
            "assignment" => {
                if let Some(left) = node.child_by_field_name("left") {
                    let right = node.child_by_field_name("right");
                    let declared = node
                        .child_by_field_name("type")
                        .map(|ty| annotation(ty, self.source));
                    if !conditional && left.kind() == "identifier" {
                        if let Some(TypeExpr::Applied { base, args }) = &declared {
                            if matches!(base.as_str(), "list" | "List" | "Sequence" | "Iterable") && args.len() == 1 {
                                self.facts.collection_elements.push(CollectionElementFact {
                                    name: text(left, self.source).to_owned(),
                                    position: position(node),
                                    element_type: args[0].clone(),
                                });
                            }
                        }
                    }
                    let literal = right.and_then(literal_value);
                    let assigned = if let Some(literal) = literal {
                        literal
                    } else if self.class_scope && node.child_by_field_name("type").is_some() {
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
                    let branch_end = conditional.then(|| direct_branch_end(node)).flatten();
                    let assigned = if let Some(body_end) = branch_end {
                        ValueExpr::Scoped {
                            value: Box::new(assigned),
                            body_end,
                        }
                    } else {
                        assigned
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
                    let value = match (
                        node.child_by_field_name("right"),
                        node.child_by_field_name("body"),
                    ) {
                        (Some(iterable), Some(body))
                            if left.kind() == "identifier" && iterable.kind() == "identifier" =>
                        {
                            ValueExpr::LoopElement {
                                iterable: text(iterable, self.source).to_owned(),
                                body_end: position(body),
                            }
                        }
                        _ => ValueExpr::Unknown,
                    };
                    let bounded = matches!(value, ValueExpr::LoopElement { .. });
                    self.bind(left, value, position(left), !bounded);
                }
            }
            "as_pattern" => {
                if let Some(alias) = node.child_by_field_name("alias") {
                    let name = if alias.kind() == "identifier" {
                        alias
                    } else {
                        alias
                            .named_child(0)
                            .filter(|child| child.kind() == "identifier")
                            .unwrap_or(alias)
                    };
                    // `with expr as name` binds name to expr. __enter__ that
                    // returns self is that value; except-as stays unknown.
                    let in_with = node
                        .parent()
                        .is_some_and(|parent| parent.kind() == "with_item");
                    let bound = if in_with {
                        node.named_child(0)
                            .filter(|child| child.id() != alias.id())
                            .map(|expr| value(expr, self.source))
                            .unwrap_or(ValueExpr::Unknown)
                    } else {
                        ValueExpr::Unknown
                    };
                    self.bind(name, bound, position(name), !in_with || conditional);
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
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.visit(
                child,
                conditional,
                collect_facts && !nested_scope,
                lexical_scope && !nested_scope,
            );
        }
    }
}

#[derive(Default)]
struct BodySummary {
    has_yield: bool,
    has_uncertain_expression: bool,
    nonlocal_names: HashSet<String>,
}

fn simple_return_statement(body: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = body.walk();
    let mut last: Option<Node<'_>> = None;
    let mut valid_prefix = true;
    for statement in body
        .named_children(&mut cursor)
        .filter(|statement| statement.kind() != "comment")
    {
        if let Some(previous) = last {
            valid_prefix &= match previous.kind() {
                "pass_statement" | "import_statement" | "import_from_statement" => true,
                "expression_statement" => previous
                    .named_child(0)
                    .is_some_and(|expression| {
                        expression.kind() == "string"
                            || (expression.kind() == "assignment"
                                && expression
                                    .child_by_field_name("left")
                                    .is_some_and(|left| left.kind() == "identifier"))
                    }),
                _ => false,
            };
        }
        last = Some(statement);
    }
    if !valid_prefix {
        return None;
    }
    let last = last?;
    (last.kind() == "return_statement").then_some(last)
}

fn returned_value(statement: Node<'_>, source: &str) -> Option<(ValueExpr, SourcePosition)> {
    if statement.kind() != "return_statement" {
        return None;
    }
    let expression = statement.named_child(0)?;
    let returned = value(expression, source);
    (!matches!(returned, ValueExpr::Unknown)).then(|| (returned, position(expression)))
}

pub(super) fn extract(
    node: Node<'_>,
    source: &str,
    has_django_shortcuts_import: bool,
) -> ValueFlowFacts {
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
    let initially_uncertain = callable
        && (decorated || text(node, source).trim_start().starts_with("async "));
    let return_statement = body.and_then(simple_return_statement);
    let collect_return_summary = callable && !initially_uncertain && return_statement.is_some();
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
    let mut collector = Collector {
        source,
        has_django_shortcuts_import,
        class_scope: node.kind() == "class_definition",
        local_scope: callable,
        receiver,
        init: field(node, source, "name") == Some("__init__"),
        uncertain_callable: initially_uncertain,
        collect_summary: callable,
        collect_return_summary,
        summary: BodySummary::default(),
        facts: ValueFlowFacts::default(),
    };
    if callable {
        if let Some(parameters) = node.child_by_field_name("parameters") {
            let mut cursor = parameters.walk();
            for parameter in parameters.named_children(&mut cursor) {
                if let Some(ty) = parameter.child_by_field_name("type") {
                    annotation_reference_positions(
                        ty,
                        source,
                        &mut collector.facts.annotation_references,
                    );
                    let param_name = if parameter.kind() == "identifier" {
                        Some(text(parameter, source))
                    } else {
                        field(parameter, source, "name").or_else(|| {
                            parameter
                                .named_child(0)
                                .filter(|name| name.kind() == "identifier")
                                .map(|name| text(name, source))
                        })
                    };
                    if let Some(name) = param_name {
                        let parsed = annotation(ty, source);
                        if let TypeExpr::Applied { base, args } = &parsed {
                            if matches!(base.as_str(), "list" | "List" | "Sequence" | "Iterable")
                                && args.len() == 1
                            {
                                collector.facts.collection_elements.push(CollectionElementFact {
                                    name: name.to_owned(),
                                    position: crate::engine::languages::source_start(node),
                                    element_type: args[0].clone(),
                                });
                            }
                        }
                    }
                }
            }
        }
        if let Some(ret) = node.child_by_field_name("return_type") {
            annotation_reference_positions(ret, source, &mut collector.facts.annotation_references);
        }
    }
    if let Some(body) = body {
        collector.visit(body, false, true, true);
    }
    collector.uncertain_callable |= callable && collector.summary.has_yield;
    if collector.uncertain_callable && collector.init {
        for field in &mut collector.facts.fields {
            field.value = ValueExpr::Unknown;
        }
    }
    for name in &collector.summary.nonlocal_names {
        for binding in &mut collector.facts.bindings {
            if binding.name == *name {
                binding.value = ValueExpr::Unknown;
            }
        }
    }
    let mut scoped_names: Vec<_> = collector.summary.nonlocal_names.iter().collect();
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
    if callable {
        let async_callable = text(node, source).trim_start().starts_with("async ");
        if async_callable {
            if !decorated && !collector.summary.has_yield {
                // An async function's declared type describes the awaited
                // result, not the ordinary call result (a coroutine).
                collector.facts.async_return_type = node
                    .child_by_field_name("return_type")
                    .map(|ty| annotation(ty, source));
            }
        } else if !collector.uncertain_callable {
            collector.facts.return_type = node
                .child_by_field_name("return_type")
                .map(|ty| annotation(ty, source));
        }
    }
    if callable
        && !collector.uncertain_callable
        && collector.summary.nonlocal_names.is_empty()
        && collector.collect_return_summary
        && !collector.summary.has_uncertain_expression
    {
        if let Some((returned, at)) =
            return_statement.and_then(|statement| returned_value(statement, source))
        {
            collector.facts.return_value = Some(returned);
            collector.facts.return_position = Some(at);
        }
    }
    if node.kind() == "module" {
        collector.facts.type_only_imports = type_only_imports(node, source);
    }
    collector.facts.bindings.sort_by_key(|a| a.position);
    collector.facts.fields.sort_by_key(|a| a.position);
    collector.facts
}
