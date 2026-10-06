use super::{direct_module_assignment, lazy_exports, Syntax};
use crate::engine::ast::{self, field, text};
use std::collections::{HashMap, HashSet};

pub(super) fn collect<'tree>(
    root: Syntax<'tree>,
    source: &str,
    statements: &[Syntax<'tree>],
    type_checking_aliases: &mut HashMap<String, Vec<(usize, bool)>>,
    has_django_shortcuts_import: &mut bool,
    lazy_getters: &mut Vec<Syntax<'tree>>,
) -> Vec<crate::core::semantic::ExportBinding> {
    fn mark_write(node: Syntax<'_>, source: &str, writes: &mut HashSet<String>) {
        match node.kind() {
            "identifier" => {
                writes.insert(text(node, source).to_owned());
            }
            "attribute" | "subscript" => {}
            _ => {
                let mut children = node.walk();
                for child in node.named_children(&mut children) {
                    mark_write(child, source, writes);
                }
            }
        }
    }

    fn collect_type_checking_aliases(
        statement: Syntax<'_>,
        source: &str,
        aliases: &mut HashMap<String, Vec<(usize, bool)>>,
    ) {
        let position = statement.start_byte();
        if statement.kind() == "import_from_statement"
            && matches!(
                field(statement, source, "module_name"),
                Some("typing" | "typing_extensions")
            )
        {
            let mut names = statement.walk();
            for child in statement.children_by_field_name("name", &mut names) {
                if field(child, source, "name").unwrap_or_else(|| text(child, source))
                    == "TYPE_CHECKING"
                {
                    let alias = field(child, source, "alias").unwrap_or("TYPE_CHECKING");
                    aliases
                        .entry(alias.to_owned())
                        .or_default()
                        .push((position, true));
                }
            }
            return;
        }

        let rebinding = if statement.kind() == "expression_statement" {
            statement.named_child(0).filter(|expression| {
                matches!(expression.kind(), "assignment" | "augmented_assignment")
            })
        } else {
            Some(statement)
        };
        if let Some(rebinding) = rebinding {
            match rebinding.kind() {
                "assignment" | "augmented_assignment" => {
                    if let Some(left) = rebinding.child_by_field_name("left") {
                        if let Some(events) = aliases.get_mut(text(left, source)) {
                            events.push((position, false));
                        }
                    }
                }
                "delete_statement" => {
                    for name in text(rebinding, source)
                        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                        .filter(|part| !part.is_empty())
                    {
                        if let Some(events) = aliases.get_mut(name) {
                            events.push((position, false));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let mut imports = HashMap::new();
    let mut import_positions = HashMap::new();
    let mut conditional_imports = HashSet::new();
    let mut conditional_writes = HashSet::new();
    let mut assignments = HashMap::<String, usize>::new();
    let mut local_functions = HashMap::<String, Vec<usize>>::new();
    let mut local_aliases = Vec::new();
    let mut aliases = Vec::new();
    let mut imports_valid = true;
    for &statement in statements {
        collect_type_checking_aliases(statement, source, type_checking_aliases);
        if statement.kind() == "function_definition"
            && field(statement, source, "name") == Some("__getattr__")
        {
            lazy_getters.push(statement);
        }
        if statement.kind() == "function_definition" {
            if let Some(name) = field(statement, source, "name") {
                local_functions
                    .entry(name.to_owned())
                    .or_default()
                    .push(statement.start_byte());
            }
        }
        if matches!(statement.kind(), "import_statement" | "import_from_statement") {
            *has_django_shortcuts_import |= statement.kind() == "import_from_statement"
                && field(statement, source, "module_name") == Some("django.shortcuts");
            let mut names = HashMap::new();
            imports_valid &= lazy_exports::imported_binding(statement, source, &mut names);
            for (name, target) in names {
                import_positions.insert(name.clone(), statement.start_byte());
                imports_valid &= imports.insert(name, target).is_none();
            }
        }
        let Some(assignment) = statement
            .named_child(0)
            .filter(|node| direct_module_assignment(*node))
        else {
            continue;
        };
        let Some(name) = field(assignment, source, "left") else {
            continue;
        };
        *assignments.entry(name.to_owned()).or_default() += 1;
        if let Some(target) = assignment
            .child_by_field_name("right")
            .filter(|right| right.kind() == "identifier")
        {
            local_aliases.push((
                name.to_owned(),
                text(target, source).to_owned(),
                assignment.start_byte(),
            ));
        }
        let Some(right) = assignment
            .child_by_field_name("right")
            .filter(|right| right.kind() == "attribute")
        else {
            continue;
        };
        let Some(receiver) = right
            .child_by_field_name("object")
            .filter(|object| object.kind() == "identifier")
        else {
            continue;
        };
        let Some(member) = field(right, source, "attribute") else {
            continue;
        };
        aliases.push((
            name.to_owned(),
            text(receiver, source).to_owned(),
            member.to_owned(),
            assignment.start_byte(),
        ));
    }
    if aliases.is_empty() && local_aliases.is_empty() {
        return Vec::new();
    }
    let mut nested = vec![root];
    while let Some(node) = nested.pop() {
        if node.kind() == "function_definition" && node.parent() != Some(root) {
            if let Some(name) = field(node, source, "name") {
                conditional_writes.insert(name.to_owned());
            }
        }
        if node != root && matches!(node.kind(), "function_definition" | "class_definition" | "lambda") {
            continue;
        }
        if matches!(
            node.kind(),
            "assignment" | "augmented_assignment" | "for_statement" | "named_expression"
        ) && !direct_module_assignment(node)
        {
            if let Some(target) = node
                .child_by_field_name("left")
                .or_else(|| node.child_by_field_name("name"))
            {
                mark_write(target, source, &mut conditional_writes);
            }
        }
        if matches!(node.kind(), "as_pattern" | "delete_statement") {
            let mut targets = node.walk();
            for target in node.named_children(&mut targets) {
                mark_write(target, source, &mut conditional_writes);
            }
        }
        if node.parent() != Some(root)
            && matches!(node.kind(), "import_statement" | "import_from_statement")
        {
            let mut bindings = HashMap::new();
            let _ = lazy_exports::imported_binding(node, source, &mut bindings);
            conditional_imports.extend(bindings.into_keys());
        }
        let mut children = node.walk();
        nested.extend(node.named_children(&mut children));
    }
    let rebound = ast::scope_bindings(root, source).rebindings;
    let mut exports: Vec<_> = aliases
        .into_iter()
        .filter(|(_, receiver, _, _)| {
            imports.contains_key(receiver) || conditional_imports.contains(receiver)
        })
        .map(|(name, receiver, member, assignment_position)| {
            let provider = imports_valid
                .then(|| imports.get(&receiver))
                .flatten()
                .filter(|(_, imported_member)| imported_member.is_none())
                .filter(|_| assignments.get(&name) == Some(&1))
                .filter(|_| {
                    import_positions
                        .get(&receiver)
                        .is_some_and(|position| *position < assignment_position)
                })
                .filter(|_| !rebound.contains(&receiver));
            crate::core::semantic::ExportBinding {
                name,
                local: provider.map(|_| member),
                module: provider.map(|(module, _)| module.clone()),
                type_only: false,
                star: false,
            }
        })
        .collect();
    exports.extend(local_aliases.into_iter().filter_map(|(name, target, position)| {
        let [declared_at] = local_functions.get(&target)?.as_slice() else {
            return None;
        };
        (declared_at < &position
            && assignments.get(&name) == Some(&1)
            && !assignments.contains_key(&target)
            && !rebound.contains(&target)
            && !local_functions.contains_key(&name)
            && !imports.contains_key(&name)
            && !imports.contains_key(&target)
            && !conditional_imports.contains(&name)
            && !conditional_imports.contains(&target)
            && !conditional_writes.contains(&name)
            && !conditional_writes.contains(&target))
            .then_some(crate::core::semantic::ExportBinding {
                name,
                local: Some(target),
                module: None,
                type_only: false,
                star: false,
            })
    }));
    exports
}
