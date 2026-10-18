use crate::{
    core::semantic::{
        AnnotationReferenceFact, BindingFact, BranchJoinFact, CollectionElementFact, FieldFact,
        GuardImportFact, SourcePosition, TypeExpr, TypeOnlyImportFact, ValueExpr, ValueFlowFacts,
    },
    engine::ast::{field, text},
};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(crate) struct AliasWrites {
    writes: HashMap<String, (usize, bool)>,
    active: crate::engine::ast::visitor::BitStack,
}
impl AliasWrites {
    pub(crate) fn new() -> Self {
        Self {
            writes: Default::default(),
            active: crate::engine::ast::visitor::BitStack::new(),
        }
    }
    fn targets(
        node: Node<'_>,
        source: &str,
        alias: bool,
        writes: &mut std::collections::HashMap<String, (usize, bool)>,
    ) {
        if node.kind() == "identifier" {
            let name = text(node, source);
            if let Some(entry) = writes.get_mut(name) {
                entry.0 += 1;
                entry.1 &= alias;
            } else {
                writes.insert(name.to_owned(), (1, alias));
            }
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
                Self::targets(child, source, false, writes);
            }
        }
    }
    pub(crate) fn enter(&mut self, node: Node<'_>, source: &str) {
        let active = self.active.last().unwrap_or(true)
            && !matches!(
                node.kind(),
                "function_definition" | "class_definition" | "lambda"
            );
        self.active.push(active);
        if !active {
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
            Self::targets(
                target,
                source,
                super::type_alias(node, source),
                &mut self.writes,
            );
        }
        if node.kind() == "delete_statement" {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                Self::targets(child, source, false, &mut self.writes);
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
                let entry = self.writes.entry(bound.to_owned()).or_insert((0, true));
                entry.0 += 1;
                entry.1 = false;
            }
        }
    }
    pub(crate) fn exit(&mut self) {
        self.active.pop();
    }
    pub(crate) fn aliases(&self) -> HashSet<String> {
        self.writes
            .iter()
            .filter(|(_, (count, alias))| *count == 1 && *alias)
            .map(|(name, _)| name.clone())
            .collect()
    }
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
                        position: crate::engine::languages::source_start(node),
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
                                position: crate::engine::languages::source_start(statement),
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
                                position: crate::engine::languages::source_start(statement),
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

fn strip_outer_parentheses(s: &str) -> &str {
    let trimmed = s.trim();
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        let mut depth = 0_usize;
        let mut closed_at_end = false;
        for (i, c) in trimmed.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 && i == trimmed.len() - 1 {
                        closed_at_end = true;
                    }
                }
                _ => {}
            }
        }
        if closed_at_end {
            trimmed[1..trimmed.len() - 1].trim()
        } else {
            trimmed
        }
    } else {
        trimmed
    }
}

fn split_python_pipe_types(annotation: &str) -> Option<Vec<&str>> {
    let mut depth = 0_usize;
    let mut start = 0_usize;
    let mut parts = Vec::new();
    for (index, character) in annotation.char_indices() {
        match character {
            '[' | '(' => depth = depth.checked_add(1)?,
            ']' | ')' => depth = depth.checked_sub(1)?,
            '|' if depth == 0 => {
                let part = annotation[start..index].trim();
                if part.is_empty() {
                    return None;
                }
                parts.push(part);
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    if depth != 0 || parts.is_empty() {
        return None;
    }
    let last = annotation[start..].trim();
    if last.is_empty() {
        return None;
    }
    parts.push(last);
    Some(parts)
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
        if node.kind() == "parenthesized_expression" {
            if let Some(inner) = node.named_child(0) {
                return parse(inner, source, depth, remaining);
            }
        }
        if node.kind() == "binary_operator"
            && node
                .child_by_field_name("operator")
                .is_some_and(|op| text(op, source) == "|")
        {
            let mut operands = Vec::new();
            let mut stack = vec![node];
            while let Some(curr) = stack.pop() {
                if curr.kind() == "binary_operator"
                    && curr
                        .child_by_field_name("operator")
                        .is_some_and(|op| text(op, source) == "|")
                {
                    if let Some(right) = curr.child_by_field_name("right") {
                        stack.push(right);
                    }
                    if let Some(left) = curr.child_by_field_name("left") {
                        stack.push(left);
                    }
                } else if curr.kind() == "parenthesized_expression" {
                    if let Some(inner) = curr.named_child(0) {
                        stack.push(inner);
                    } else {
                        operands.push(curr);
                    }
                } else {
                    operands.push(curr);
                }
            }
            if !operands.is_empty() && operands.len() <= *remaining {
                let args = operands
                    .into_iter()
                    .map(|arg| parse(arg, source, depth + 1, remaining))
                    .collect();
                return TypeExpr::Applied {
                    base: "Union".to_owned(),
                    args,
                };
            }
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
        let value = strip_outer_parentheses(value);
        if let Some(parts) = split_python_pipe_types(value) {
            if parts.len() >= 2 && parts.len() <= *remaining {
                let args = parts
                    .into_iter()
                    .map(|part| TypeExpr::Named {
                        name: strip_outer_parentheses(part).to_owned(),
                    })
                    .collect();
                return TypeExpr::Applied {
                    base: "Union".to_owned(),
                    args,
                };
            }
        }
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
        "await" => node
            .named_child(0)
            .map_or(ValueExpr::Unknown, |expression| ValueExpr::Await {
                value: Box::new(value(expression, source)),
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

fn direct_assignment<'tree>(
    block: Node<'tree>,
    source: &str,
) -> Result<Option<(String, Node<'tree>)>, ()> {
    let mut assignment = None;
    let mut cursor = block.walk();
    for statement in block.named_children(&mut cursor) {
        let statement = if statement.kind() == "expression_statement" {
            let Some(expression) = statement.named_child(0) else {
                return Err(());
            };
            expression
        } else {
            statement
        };
        match statement.kind() {
            "comment" | "pass_statement" => {}
            "assignment" => {
                let Some(left) = statement
                    .child_by_field_name("left")
                    .filter(|left| left.kind() == "identifier")
                else {
                    return Err(());
                };
                if assignment.is_some() {
                    return Err(());
                }
                assignment = Some((text(left, source).to_owned(), statement));
            }
            _ => return Err(()),
        }
    }
    Ok(assignment)
}

fn branch_blocks<'tree>(node: Node<'tree>) -> Option<(Vec<Node<'tree>>, bool)> {
    let consequence = node.child_by_field_name("consequence")?;
    let mut branches = vec![consequence];
    let mut has_else = false;
    let mut cursor = node.walk();
    for alternative in node.children_by_field_name("alternative", &mut cursor) {
        match alternative.kind() {
            "elif_clause" => branches.push(alternative.child_by_field_name("consequence")?),
            "else_clause" => {
                branches.push(alternative.child_by_field_name("body")?);
                has_else = true;
            }
            _ => return None,
        }
    }
    Some((branches, has_else))
}

pub(crate) struct Collector<'a, 'tree> {
    root: Node<'tree>,
    module_export_facts: Option<super::module_exports::Collector<'tree>>,
    body: Option<Node<'tree>>,
    body_depth: Option<usize>,
    states: Vec<(bool, bool, bool, bool)>,
    return_last: Option<Node<'tree>>,
    return_prefix_valid: bool,
    aliases: Option<AliasWrites>,
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

impl<'a, 'tree> Collector<'a, 'tree> {
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

    fn bind_if_join(&mut self, node: Node<'_>, inherited_conditional: bool) {
        let Some((branches, has_else)) = branch_blocks(node) else {
            return;
        };
        let mut branch_assignments = Vec::with_capacity(branches.len());
        let mut joined_name: Option<String> = None;
        for branch in branches {
            let assignment = match direct_assignment(branch, self.source) {
                Ok(assignment) => assignment,
                Err(()) => return,
            };
            if let Some((name, _)) = &assignment {
                if joined_name.as_ref().is_some_and(|joined| joined != name) {
                    return;
                }
                joined_name.get_or_insert_with(|| name.clone());
            }
            branch_assignments.push(assignment);
        }
        let Some(name) = joined_name else {
            return;
        };

        let start = node.start_position();
        let prior_binding = self
            .facts
            .bindings
            .iter()
            .filter(|binding| {
                binding.name == name
                    && !binding.conditional
                    && (binding.position.line, binding.position.column)
                        < (start.row + 1, start.column)
            })
            .map(|binding| (binding.position, &binding.value));
        let prior_join = self
            .facts
            .branch_joins
            .iter()
            .filter(|join| {
                join.name == name
                    && !join.conditional
                    && (join.position.line, join.position.column) < (start.row + 1, start.column)
            })
            .map(|join| (join.position, &join.value));
        let incoming = prior_binding
            .into_iter()
            .chain(prior_join)
            .max_by_key(|(position, _)| (position.line, position.column))
            .map(|(_, value)| value.clone())
            .unwrap_or(ValueExpr::Unknown);
        let mut alternatives = branch_assignments
            .into_iter()
            .map(|assignment| match assignment {
                Some((_, statement)) => {
                    let right = statement.child_by_field_name("right");
                    let literal = right.and_then(literal_value);
                    if let Some(literal) = literal {
                        literal
                    } else {
                        right.map_or(ValueExpr::Unknown, |right| value(right, self.source))
                    }
                }
                None => incoming.clone(),
            })
            .collect::<Vec<_>>();
        if !has_else {
            alternatives.push(incoming);
        }

        let mut joined = ValueExpr::BranchJoin { alternatives };
        if inherited_conditional {
            let Some(body_end) = direct_branch_end(node) else {
                return;
            };
            joined = ValueExpr::Scoped {
                value: Box::new(joined),
                body_end,
            };
        }
        let mut at = position(node);
        at.column = at.column.saturating_add(1);
        self.facts.branch_joins.push(BranchJoinFact {
            name,
            value: joined,
            position: at,
            conditional: inherited_conditional,
        });
    }

    pub(crate) fn enter(&mut self, node: Node<'tree>) {
        if let Some(aliases) = &mut self.aliases {
            aliases.enter(node, self.source);
        }
        let (parent_active, inherited, inherited_facts, inherited_lexical) =
            self.states.last().copied().unwrap_or_default();
        let active = self.body == Some(node) || parent_active;
        let conditional = if self.body == Some(node) {
            false
        } else {
            inherited
        };
        let collect_facts = self.body == Some(node) || inherited_facts;
        let lexical_scope = self.body == Some(node) || inherited_lexical;
        if !active {
            self.states.push((false, false, false, false));
            return;
        }
        if self.body == Some(node) {
            self.body_depth = Some(self.states.len());
        }
        let is_direct_child = self.body_depth.is_some_and(|d| self.states.len() == d + 1);
        if is_direct_child && node.kind() != "comment" {
            if let Some(previous) = self.return_last {
                self.return_prefix_valid &= valid_return_prefix(previous);
            }
            self.return_last = Some(node);
        }
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
                )
            {
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
            self.states.push((false, conditional, false, false));
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
        if collect_facts && nested_scope {
            self.states.push((
                self.collect_summary && self.collect_return_summary,
                conditional,
                false,
                false,
            ));
            return;
        }
        if !collect_facts && !self.collect_summary {
            self.states.push((false, conditional, false, false));
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
                                if matches!(
                                    base.as_str(),
                                    "list" | "List" | "Sequence" | "Iterable"
                                ) && args.len() == 1
                                {
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
                                if left.kind() == "identifier"
                                    && iterable.kind() == "identifier" =>
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
        self.states.push((
            true,
            conditional,
            collect_facts && !nested_scope,
            lexical_scope && !nested_scope,
        ));
    }

    pub(crate) fn exit(&mut self, node: Node<'tree>) {
        if self.body == Some(node) {
            self.body_depth = None;
        }
        let (_, _, collect_facts, _) = self.states.pop().unwrap_or_default();
        let inherited_conditional = self.states.last().is_some_and(|state| state.1);
        if collect_facts && self.local_scope && !self.class_scope && node.kind() == "if_statement" {
            self.bind_if_join(node, inherited_conditional);
        }
        if let Some(aliases) = &mut self.aliases {
            aliases.exit();
        }
    }
}

#[derive(Default)]
struct BodySummary {
    has_yield: bool,
    has_uncertain_expression: bool,
    nonlocal_names: HashSet<String>,
}

fn valid_return_prefix(previous: Node<'_>) -> bool {
    match previous.kind() {
        "pass_statement" | "import_statement" | "import_from_statement" => true,
        "expression_statement" => previous.named_child(0).is_some_and(|expression| {
            expression.kind() == "string"
                || (expression.kind() == "assignment"
                    && expression
                        .child_by_field_name("left")
                        .is_some_and(|left| left.kind() == "identifier"))
        }),
        _ => false,
    }
}

fn returned_value(statement: Node<'_>, source: &str) -> Option<(ValueExpr, SourcePosition)> {
    if statement.kind() != "return_statement" {
        return None;
    }
    let expression = statement.named_child(0)?;
    let returned = value(expression, source);
    (!matches!(returned, ValueExpr::Unknown)).then(|| (returned, position(expression)))
}

impl<'a, 'tree> Collector<'a, 'tree> {
    fn empty(
        node: Node<'tree>,
        source: &'a str,
        has_django_shortcuts_import: bool,
        collect_aliases: bool,
        facts: ValueFlowFacts,
    ) -> Self {
        Self {
            root: node,
            module_export_facts: (node.kind() == "module")
                .then(|| super::module_exports::Collector::new(node)),
            body: None,
            body_depth: None,
            states: Vec::with_capacity(32),
            return_last: None,
            return_prefix_valid: true,
            aliases: collect_aliases.then(AliasWrites::new),
            source,
            has_django_shortcuts_import,
            class_scope: false,
            local_scope: false,
            receiver: None,
            init: false,
            uncertain_callable: false,
            collect_summary: false,
            collect_return_summary: false,
            summary: BodySummary::default(),
            facts,
        }
    }
    pub(crate) fn new(
        node: Node<'tree>,
        source: &'a str,
        has_django_shortcuts_import: bool,
        collect_aliases: bool,
    ) -> Self {
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
            return Self::empty(
                node,
                source,
                has_django_shortcuts_import,
                collect_aliases,
                ValueFlowFacts {
                    alias_type: Some(alias_type),
                    ..ValueFlowFacts::default()
                },
            );
        }
        if !matches!(
            node.kind(),
            "module" | "class_definition" | "function_definition"
        ) {
            return Self::empty(
                node,
                source,
                has_django_shortcuts_import,
                collect_aliases,
                ValueFlowFacts::default(),
            );
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
        let initially_uncertain =
            callable && (decorated || text(node, source).trim_start().starts_with("async "));
        let collect_return_summary = callable && !initially_uncertain;
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
            root: node,
            module_export_facts: (node.kind() == "module")
                .then(|| super::module_exports::Collector::new(node)),
            body,
            body_depth: None,
            states: Vec::with_capacity(32),
            return_last: None,
            return_prefix_valid: true,
            aliases: collect_aliases.then(AliasWrites::new),
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
                                if matches!(
                                    base.as_str(),
                                    "list" | "List" | "Sequence" | "Iterable"
                                ) && args.len() == 1
                                {
                                    collector.facts.collection_elements.push(
                                        CollectionElementFact {
                                            name: name.to_owned(),
                                            position: crate::engine::languages::source_start(node),
                                            element_type: args[0].clone(),
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
            }
            if let Some(ret) = node.child_by_field_name("return_type") {
                annotation_reference_positions(
                    ret,
                    source,
                    &mut collector.facts.annotation_references,
                );
            }
        }
        collector
    }
    pub(crate) fn module_aliases(&self) -> HashSet<String> {
        self.aliases
            .as_ref()
            .map_or_else(HashSet::new, AliasWrites::aliases)
    }
    pub(crate) fn observe_module_event(&mut self, node: Node<'tree>, entering: bool) {
        if let Some(exports) = &mut self.module_export_facts {
            exports.observe(node, entering, self.source);
        }
    }
    pub(crate) fn module_exports(
        &self,
        bindings: &crate::engine::ast::ScopeBindings,
    ) -> Option<(
        Vec<crate::core::semantic::ExportBinding>,
        Vec<super::LazyExport>,
    )> {
        self.module_export_facts
            .as_ref()
            .map(|exports| exports.outputs(self.source, bindings))
    }
    pub(crate) fn finish(mut self) -> ValueFlowFacts {
        let node = self.root;
        let source = self.source;
        let callable = self.local_scope;
        let decorated = node
            .parent()
            .is_some_and(|parent| parent.kind() == "decorated_definition");
        let return_statement = self
            .return_last
            .filter(|last| self.return_prefix_valid && last.kind() == "return_statement");
        self.uncertain_callable |= callable && self.summary.has_yield;
        if self.uncertain_callable && self.init {
            for field in &mut self.facts.fields {
                field.value = ValueExpr::Unknown;
            }
        }
        for name in &self.summary.nonlocal_names {
            for binding in &mut self.facts.bindings {
                if binding.name == *name {
                    binding.value = ValueExpr::Unknown;
                }
            }
        }
        let mut scoped_names: Vec<_> = self.summary.nonlocal_names.iter().collect();
        scoped_names.sort_unstable();
        for name in scoped_names {
            let start = node.start_position();
            self.facts.bindings.push(BindingFact {
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
                if !decorated && !self.summary.has_yield {
                    // An async function's declared type describes the awaited
                    // result, not the ordinary call result (a coroutine).
                    self.facts.async_return_type = node
                        .child_by_field_name("return_type")
                        .map(|ty| annotation(ty, source));
                }
            } else if !self.uncertain_callable {
                self.facts.return_type = node
                    .child_by_field_name("return_type")
                    .map(|ty| annotation(ty, source));
            }
        }
        if callable
            && !self.uncertain_callable
            && self.summary.nonlocal_names.is_empty()
            && self.collect_return_summary
            && !self.summary.has_uncertain_expression
        {
            if let Some((returned, at)) =
                return_statement.and_then(|statement| returned_value(statement, source))
            {
                self.facts.return_value = Some(returned);
                self.facts.return_position = Some(at);
            }
        }
        if node.kind() == "module" {
            self.facts.type_only_imports = type_only_imports(node, source);
        }
        self.facts.bindings.sort_by_key(|a| a.position);
        self.facts.fields.sort_by_key(|a| a.position);
        self.facts
    }
}

pub(super) fn extract(
    node: Node<'_>,
    source: &str,
    has_django_shortcuts_import: bool,
) -> ValueFlowFacts {
    let mut collector = Collector::new(node, source, has_django_shortcuts_import, false);
    crate::engine::ast::visitor::walk(node, &mut |node, entering| {
        if entering {
            collector.enter(node);
        } else {
            collector.exit(node);
        }
    });
    collector.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_persistence_type_only_imports() {
        let source = "from typing import TYPE_CHECKING\n\nclass PersistenceMixin:\n    if TYPE_CHECKING:\n        from contextunity.router.service.payloads import GraphEntry\n";
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_python::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let facts = extract(tree.root_node(), source, false);
        assert_eq!(facts.type_only_imports.len(), 1);
        assert_eq!(facts.type_only_imports[0].alias, "GraphEntry");
        let ast_facts = crate::engine::ast::extract("persistence.py", "python", source).unwrap();
        let module = ast_facts
            .nodes
            .iter()
            .find(|n| n.id == "module:persistence.py")
            .unwrap();
        let type_only = &module.details["value_flow"]["type_only_imports"];
        assert_eq!(type_only[0]["alias"], "GraphEntry");
    }
}

