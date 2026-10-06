use crate::core::semantic::{SourcePosition, TemplateContextFact};
use crate::engine::ast::text;
use std::collections::HashSet;
use tree_sitter::Node;

fn lexical_import_owner<'a>(node: Node<'a>, source: &str, alias: &str) -> Option<Node<'a>> {
    let mut parent = node.parent();
    while let Some(scope) = parent {
        match scope.kind() {
            "function_definition" => {
                if crate::engine::ast::scope_bindings(scope, source)
                    .all
                    .iter()
                    .any(|bound| bound == alias)
                {
                    return None;
                }
                let body = scope.child_by_field_name("body")?;
                if super::template_loaders::binding_count(body, source, alias) != 0 {
                    return None;
                }
            }
            "class_definition" | "lambda" => return None,
            "module" => return Some(scope),
            _ => {}
        }
        parent = scope.parent();
    }
    None
}

pub(super) fn extract(
    node: Node<'_>,
    source: &str,
    has_django_shortcuts_import: bool,
) -> Option<TemplateContextFact> {
    if !has_django_shortcuts_import || node.kind() != "call" {
        return None;
    }
    let callee = node.child_by_field_name("function")?;
    if callee.kind() != "identifier" {
        return None;
    }
    let alias = text(callee, source);
    let arguments = node.child_by_field_name("arguments")?;
    if arguments.named_child_count() != 3 {
        return None;
    }
    let target = arguments.named_child(1)?;
    let context = arguments.named_child(2)?;
    if context.kind() != "dictionary" {
        return None;
    }
    let root = lexical_import_owner(node, source, alias)?;
    if !super::template_loaders::imported(
        root,
        source,
        alias,
        "django.shortcuts",
        "render",
        node.start_byte(),
    ) {
        return None;
    }
    let target = super::template_loaders::literal(target, source)?;
    if target.is_empty() || target.len() > 256 {
        return None;
    }
    let mut keys = Vec::new();
    let mut unique = HashSet::new();
    if context.named_child_count() > 32 {
        return None;
    }
    let mut cursor = context.walk();
    for pair in context.named_children(&mut cursor) {
        if pair.kind() != "pair" {
            return None;
        }
        let key = super::template_loaders::literal(pair.child_by_field_name("key")?, source)?;
        if key.is_empty()
            || key.len() > 128
            || !key.chars().enumerate().all(|(index, ch)| {
                ch == '_' || if index == 0 { ch.is_alphabetic() } else { ch.is_alphanumeric() }
            })
            || !unique.insert(key)
        {
            return None;
        }
        keys.push(key.to_owned());
    }
    if keys.is_empty() {
        return None;
    }
    let start = node.start_position();
    Some(TemplateContextFact {
        import_alias: alias.to_owned(),
        target: target.to_owned(),
        keys,
        position: SourcePosition {
            line: start.row + 1,
            column: start.column,
        },
    })
}
