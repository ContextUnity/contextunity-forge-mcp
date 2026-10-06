use crate::engine::ast::{field, text};
use tree_sitter::Node as Syntax;

/// Sparse eligibility for finite logging factory contracts. Nested same-named
/// shadows conservatively block inference when their namespace escapes.
pub(super) fn collect(root: Syntax<'_>, source: &str, offset: usize) -> serde_json::Value {
    let mut imports = Vec::new();
    let mut setters = Vec::new();
    let mut cursor = root.walk();
    for statement in root.named_children(&mut cursor) {
        if !matches!(statement.kind(), "import_statement" | "import_from_statement") {
            continue;
        }
        let from = statement.kind() == "import_from_statement";
        if from && field(statement, source, "module_name") != Some("logging") {
            continue;
        }
        let mut names = statement.walk();
        for child in statement.children_by_field_name("name", &mut names) {
            let name = field(child, source, "name").unwrap_or_else(|| text(child, source));
            if from && name == "setLoggerClass" {
                setters.push(field(child, source, "alias").unwrap_or(name));
            }
            if name != if from { "getLogger" } else { "logging" } {
                continue;
            }
            let alias = field(child, source, "alias").unwrap_or(name);
            imports.push((alias, statement.start_position(), true));
        }
    }
    if imports.is_empty() {
        return serde_json::Value::Null;
    }
    let mut cursor = root.walk();
    let mut visited = 0usize;
    loop {
        let node = cursor.node();
        visited += 1;
        if visited > 65_536 {
            for (_, _, safe) in &mut imports { *safe = false; }
            break;
        }
        if node.kind() == "identifier" {
            let name = text(node, source);
            if imports.iter().any(|(alias, _, _)| *alias == name) || setters.contains(&name) {
                let mut value = node;
                let mut factory_changed = setters.contains(&name);
                while let Some(parent) = value.parent().filter(|parent| {
                    (parent.kind() == "attribute"
                        && parent.child_by_field_name("object") == Some(value))
                        || parent.kind() == "parenthesized_expression"
                }) {
                    factory_changed |= field(parent, source, "attribute") == Some("setLoggerClass");
                    value = parent;
                }
                let level_constant = value.kind() == "attribute"
                    && matches!(
                        field(value, source, "attribute"),
                        Some("CRITICAL" | "ERROR" | "WARNING" | "INFO" | "DEBUG" | "NOTSET")
                    )
                    && value.parent().is_some_and(|parent| {
                        matches!(parent.kind(), "argument_list" | "keyword_argument")
                    });
                let escaped = !level_constant && !value.parent().is_some_and(|parent| {
                    matches!(parent.kind(), "dotted_name" | "aliased_import"
                        | "import_statement" | "import_from_statement")
                        || (parent.kind() == "call"
                            && parent.child_by_field_name("function") == Some(value)
                            && !factory_changed)
                });
                if escaped {
                    for (_, _, safe) in &mut imports {
                        *safe = false;
                    }
                }
            }
        }
        if cursor.goto_first_child() { continue; }
        loop {
            if cursor.goto_next_sibling() { break; }
            if !cursor.goto_parent() {
                return serde_json::json!(imports.into_iter().map(|(alias, position, safe)| {
                    serde_json::json!({"alias": alias, "line": position.row + offset + 1,
                        "column": position.column, "safe": safe})
                }).collect::<Vec<_>>());
            }
        }
    }
    serde_json::json!(imports.into_iter().map(|(alias, position, safe)| {
        serde_json::json!({"alias": alias, "line": position.row + offset + 1,
            "column": position.column, "safe": safe})
    }).collect::<Vec<_>>())
}
