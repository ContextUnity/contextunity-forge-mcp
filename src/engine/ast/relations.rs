use super::{text, Syntax};
use crate::core::models::{Edge, Facts, Node, Reference};
use serde_json::json;

pub(crate) fn reference(facts: &mut Facts, owner: &str, expression: &str, kind: &str, line: usize) {
    if expression.is_empty() {
        return;
    }
    facts.references.push(Reference {
        source: owner.into(),
        expression: expression.into(),
        kind: kind.into(),
        line,
        alias: None,
        module: None,
        dynamic: !expression
            .chars()
            .all(|c| c.is_alphanumeric() || "_.:".contains(c)),
        receiver_hint: None,
    });
}

pub(crate) fn type_name<'a>(node: Syntax<'_>, source: &'a str) -> &'a str {
    if matches!(
        node.kind(),
        "generic_type" | "generic_function" | "subscript"
    ) {
        node.child_by_field_name("name")
            .or_else(|| node.child_by_field_name("value"))
            .or_else(|| node.child_by_field_name("function"))
            .map(|n| text(n, source))
            .unwrap_or_else(|| text(node, source))
    } else {
        text(node, source)
    }
}

pub(crate) fn type_references(
    facts: &mut Facts,
    owner: &str,
    type_node: Syntax<'_>,
    source: &str,
    line: usize,
) {
    let mut stack = vec![type_node];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "scoped_type_identifier" | "attribute" => {
                let name = text(n, source);
                reference(facts, owner, name, "references", line);
            }
            "type_identifier" | "identifier" => {
                let name = text(n, source);
                if !matches!(
                    name,
                    "self" | "cls" | "this" | "true" | "false" | "None" | "nil" | "null"
                ) {
                    reference(facts, owner, name, "references", line);
                }
            }
            _ => {
                let mut cursor = n.walk();
                for child in n.named_children(&mut cursor) {
                    stack.push(child);
                }
            }
        }
    }
}

pub(crate) fn decorator_references(
    ctx: &crate::engine::languages::SyntaxContext<'_, '_>,
    facts: &mut Facts,
) {
    let (node, source, id, offset) = (ctx.node, ctx.source, ctx.owner, ctx.offset);
    for decorator in decorators(node) {
        if let Some(expr) = decorator.named_child(0) {
            let name = expr.child_by_field_name("function").unwrap_or(expr);
            reference(
                facts,
                id,
                text(name, source),
                "decorates",
                decorator.start_position().row + offset + 1,
            );
        }
    }
}

pub(crate) fn decorators(node: Syntax<'_>) -> Vec<Syntax<'_>> {
    let container = node
        .parent()
        .filter(|p| matches!(p.kind(), "decorated_definition" | "export_statement"))
        .unwrap_or(node);
    let mut c = container.walk();
    let mut result: Vec<_> = container
        .named_children(&mut c)
        .filter(|n| n.kind() == "decorator")
        .collect();
    if container != node {
        let mut c = node.walk();
        result.extend(
            node.named_children(&mut c)
                .filter(|n| n.kind() == "decorator"),
        );
    }
    if node.kind() == "method_definition" {
        let mut previous = node.prev_named_sibling();
        while let Some(decorator) = previous.filter(|n| n.kind() == "decorator") {
            result.push(decorator);
            previous = decorator.prev_named_sibling();
        }
    }
    result
}

struct FieldOwner<'a> {
    id: &'a str,
    qualname: &'a str,
    path: &'a str,
    is_test: bool,
    language: &'a str,
}

fn add_field(
    facts: &mut Facts,
    owner: &FieldOwner<'_>,
    name: &str,
    line: usize,
    end_line: usize,
) -> String {
    let qualname = format!("{}.{name}", owner.qualname);
    if let Some(n) = facts
        .nodes
        .iter()
        .find(|n| (n.kind == "field" || matches!(n.kind.as_str(), "method" | "function")) && n.qualname == qualname)
    {
        return n.id.clone();
    }
    let id = format!("field:{}:{line}:{qualname}", owner.path);
    facts.nodes.push(Node {
        id: id.clone(),
        kind: "field".into(),
        name: name.into(),
        qualname,
        path: owner.path.into(),
        line,
        end_line,
        is_test: owner.is_test,
        language: owner.language.into(),
        generated: false,
        details: json!({}),
    });
    facts.edges.push(Edge {
        src: owner.id.into(),
        dst: id.clone(),
        kind: "contains".into(),
        path: owner.path.into(),
        line,
        evidence: "field declaration".into(),
        confidence: "exact".into(),
    });
    id
}

pub(crate) fn mutation(
    ctx: &crate::engine::languages::SyntaxContext<'_, '_>,
    facts: &mut Facts,
    field_kinds: &[&str],
    assignment_kinds: &[&str],
    member_kinds: &[&str],
) {
    let node = ctx.node;
    if !field_kinds.contains(&node.kind()) && !assignment_kinds.contains(&node.kind()) {
        return;
    }
    let (source, owner, offset) = (ctx.source, ctx.owner, ctx.offset);
    let line = node.start_position().row + offset + 1;
    let (parent_id, parent_qualname, parent_path, parent_is_test, parent_language, parent_kind) = {
        let Some(p) = facts.nodes.iter().find(|n| n.id == owner) else {
            return;
        };
        (
            p.id.clone(),
            p.qualname.clone(),
            p.path.clone(),
            p.is_test,
            p.language.clone(),
            p.kind.clone(),
        )
    };
    let owner_meta = FieldOwner {
        id: &parent_id,
        qualname: &parent_qualname,
        path: &parent_path,
        is_test: parent_is_test,
        language: &parent_language,
    };

    if field_kinds.contains(&node.kind()) {
        if let Some(name) = node.child_by_field_name("name") {
            add_field(
                facts,
                &owner_meta,
                text(name, source),
                line,
                node.end_position().row + offset + 1,
            );
        }
    }
    if !assignment_kinds.contains(&node.kind()) {
        return;
    }
    let Some(left) = node
        .child_by_field_name("left")
        .or_else(|| node.child_by_field_name("argument"))
    else {
        return;
    };
    if matches!(parent_kind.as_str(), "class" | "struct") && left.kind() == "identifier" {
        let field_name = text(left, source);
        let id = add_field(
            facts,
            &owner_meta,
            field_name,
            line,
            node.end_position().row + offset + 1,
        );
        facts.edges.push(Edge {
            src: owner.into(),
            dst: id,
            kind: "mutates".into(),
            path: parent_path,
            line,
            evidence: field_name.into(),
            confidence: "exact".into(),
        });
    } else if member_kinds.contains(&left.kind()) {
        let expr = text(left, source);
        if expr.starts_with("self.") || expr.starts_with("this.") || expr.starts_with("cls.") {
            reference(facts, owner, expr, "mutates", line);
        }
    }
}

pub(crate) fn member_access(
    ctx: &crate::engine::languages::SyntaxContext<'_, '_>,
    facts: &mut Facts,
    member_kinds: &[&str],
) {
    let node = ctx.node;
    if !member_kinds.contains(&node.kind()) {
        return;
    }
    if let Some(parent) = node.parent() {
        if matches!(
            parent.kind(),
            "assignment"
                | "augmented_assignment"
                | "assignment_expr"
                | "compound_assignment_expr"
                | "assignment_expression"
                | "augmented_assignment_expression"
                | "update_expression"
        ) && (parent.child_by_field_name("left") == Some(node)
            || parent.child_by_field_name("argument") == Some(node))
        {
            return;
        }
    }
    let expr = text(node, ctx.source);
    if expr.starts_with("self.") || expr.starts_with("this.") || expr.starts_with("cls.") {
        let line = node.start_position().row + ctx.offset + 1;
        reference(facts, ctx.owner, expr, "references", line);
    }
}

pub(crate) fn implicit_fields(facts: &mut Facts) {
    let fields_to_add = {
        let node_map: hashbrown::HashMap<&str, (&str, &str)> = facts
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), (n.qualname.as_str(), n.kind.as_str())))
            .collect();
        let class_map: hashbrown::HashMap<&str, usize> = facts
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| matches!(n.kind.as_str(), "class" | "interface" | "struct"))
            .map(|(idx, n)| (n.qualname.as_str(), idx))
            .collect();
        let class_methods: hashbrown::HashSet<(&str, &str)> = facts
            .nodes
            .iter()
            .filter(|n| matches!(n.kind.as_str(), "method" | "function"))
            .filter_map(|n| {
                let (q, _) = n.qualname.rsplit_once('.')?;
                Some((q, n.name.as_str()))
            })
            .collect();

        let mut list = Vec::new();
        for r in &facts.references {
            if r.kind != "mutates" {
                continue;
            }
            let Some(name) = r
                .expression
                .strip_prefix("self.")
                .or_else(|| r.expression.strip_prefix("this."))
            else {
                continue;
            };
            if name.contains('.') {
                continue;
            }
            if let Some(&(source_qualname, _)) = node_map.get(r.source.as_str()) {
                if let Some((q, _)) = source_qualname.rsplit_once('.') {
                    if class_methods.contains(&(q, name)) {
                        continue;
                    }
                    if let Some(&class_idx) = class_map.get(q) {
                        list.push((class_idx, name.to_string(), r.line));
                    }
                }
            }
        }
        list
    };

    for (class_idx, name, line) in fields_to_add {
        let (id, qualname, path, is_test, language) = {
            let class = &facts.nodes[class_idx];
            (
                class.id.clone(),
                class.qualname.clone(),
                class.path.clone(),
                class.is_test,
                class.language.clone(),
            )
        };
        let owner_meta = FieldOwner {
            id: &id,
            qualname: &qualname,
            path: &path,
            is_test,
            language: &language,
        };
        add_field(facts, &owner_meta, &name, line, line);
    }
}
