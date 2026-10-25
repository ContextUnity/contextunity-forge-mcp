use super::Syntax;
use crate::core::models::Facts;
use crate::core::semantic::ExportBinding;
use crate::engine::ast::text;
use std::collections::HashSet;

fn source_key(node: Syntax<'_>, line_offset: usize) -> (usize, usize) {
    let position = node.start_position();
    (position.row + 1 + line_offset, position.column)
}

fn collect_module_declaration<'tree>(
    node: Syntax<'tree>,
    source: &str,
    declarations: &mut Vec<(String, Syntax<'tree>)>,
) {
    match node.kind() {
        "function_declaration"
        | "class_declaration"
        | "enum_declaration"
        | "interface_declaration"
        | "type_alias_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                declarations.push((text(name, source).to_owned(), node));
            }
        }
        "lexical_declaration" | "variable_declaration" => {
            let mut cursor = node.walk();
            for declaration in node.named_children(&mut cursor) {
                if declaration.kind() != "variable_declarator" {
                    continue;
                }
                let Some(name) = declaration
                    .child_by_field_name("name")
                    .filter(|name| name.kind() == "identifier")
                else {
                    continue;
                };
                let name = text(name, source).to_owned();
                declarations.push((name.clone(), declaration));
                if let Some(value) = declaration.child_by_field_name("value") {
                    if matches!(
                        value.kind(),
                        "function_expression" | "arrow_function" | "class"
                    ) {
                        declarations.push((name, value));
                    }
                }
            }
        }
        _ => {}
    }
}

fn module_declarations<'tree>(root: Syntax<'tree>, source: &str) -> Vec<(String, Syntax<'tree>)> {
    let mut declarations = Vec::new();
    let mut cursor = root.walk();
    // Only visit declarations owned by the module. In particular, do not enter statement blocks.
    for statement in root.named_children(&mut cursor) {
        if statement.kind() == "export_statement" {
            let mut export_cursor = statement.walk();
            for declaration in statement.named_children(&mut export_cursor) {
                collect_module_declaration(declaration, source, &mut declarations);
            }
        } else {
            collect_module_declaration(statement, source, &mut declarations);
            if statement.kind() == "expression_statement" {
                if let Some(value) = statement
                    .named_child(0)
                    .filter(|node| node.kind() == "assignment_expression")
                    .and_then(|assignment| assignment.child_by_field_name("right"))
                    .filter(|value| matches!(value.kind(), "function_expression" | "class"))
                {
                    if let Some(name) = value.child_by_field_name("name") {
                        declarations.push((text(name, source).to_owned(), value));
                    }
                }
            }
        }
    }
    declarations
}

pub(super) fn finalize_file_visibility(
    javascript: bool,
    root: Syntax<'_>,
    source: &str,
    facts: &mut Facts,
    exports: &[ExportBinding],
) {
    if javascript {
        return;
    }
    let exported_locals = exports
        .iter()
        .filter(|export| !export.type_only && export.module.is_none())
        .filter_map(|export| export.local.as_deref())
        .collect::<HashSet<_>>();
    if exported_locals.is_empty() {
        for node in &mut facts.nodes {
            node.details["is_public"] = serde_json::json!(false);
        }
        return;
    }
    let module_line = facts
        .nodes
        .iter()
        .find(|node| node.kind == "module")
        .map(|node| node.line)
        .unwrap_or(1);
    let line_offset = module_line.saturating_sub(root.start_position().row + 1);
    let mut exported_declarations = HashSet::new();
    let mut exported_class_members = HashSet::new();
    for (name, declaration) in module_declarations(root, source) {
        if !exported_locals.contains(name.as_str()) {
            continue;
        }
        exported_declarations.insert(source_key(declaration, line_offset));
        if matches!(declaration.kind(), "class_declaration" | "class") {
            // The class declaration's actual members carry its owner identity, even when a
            // block-local class happens to share its name and qualified name.
            if let Some(body) = declaration.child_by_field_name("body").or_else(|| {
                let mut cursor = declaration.walk();
                let body = declaration
                    .named_children(&mut cursor)
                    .find(|child| child.kind() == "class_body");
                body
            }) {
                let mut cursor = body.walk();
                exported_class_members.extend(
                    body.named_children(&mut cursor)
                        .map(|member| source_key(member, line_offset)),
                );
            }
        }
    }
    if exported_declarations.is_empty() {
        for node in &mut facts.nodes {
            node.details["is_public"] = serde_json::json!(false);
        }
        return;
    }
    for node in &mut facts.nodes {
        let position = node
            .details
            .get("column")
            .and_then(serde_json::Value::as_u64)
            .map(|column| (node.line, column as usize));
        let is_public = match node.kind.as_str() {
            "class" => position.is_some_and(|position| exported_declarations.contains(&position)),
            "method" | "field" => node
                .details
                .get("column")
                .and_then(serde_json::Value::as_u64)
                .map(|column| (node.line, column as usize))
                .is_some_and(|position| {
                    exported_class_members.contains(&position)
                        && node.details["is_public"] != serde_json::Value::Bool(false)
                }),
            _ => position.is_some_and(|position| exported_declarations.contains(&position)),
        };
        node.details["is_public"] = serde_json::json!(is_public);
    }
}
