use super::*;

pub(super) fn method_owner<'tree>(node: Syntax<'tree>) -> Option<Syntax<'tree>> {
    let mut current = node.parent()?;
    while matches!(
        current.kind(),
        "class_body" | "interface_body" | "object_type"
    ) {
        if let Some(owner) = current.parent() {
            if matches!(
                owner.kind(),
                "class_declaration" | "class" | "interface_declaration"
            ) {
                return Some(owner);
            }
        }
        current = current.parent()?;
    }
    None
}

pub(super) fn lexical_this_method<'tree>(
    node: Syntax<'tree>,
) -> Option<(Syntax<'tree>, Syntax<'tree>)> {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "arrow_function" => {}
            "method_definition" => return method_owner(parent).map(|class| (parent, class)),
            "function_declaration"
            | "generator_function_declaration"
            | "function_expression"
            | "generator_function"
            | "generator_function_expression"
            | "class_declaration"
            | "class"
            | "method_signature" => return None,
            _ => {}
        }
        current = parent.parent();
    }
    None
}

pub(super) fn prototype_target<'a>(
    left: Syntax<'_>,
    source: &'a str,
) -> Option<(&'a str, &'a str)> {
    if left.kind() != "member_expression" {
        return None;
    }
    let member = left
        .child_by_field_name("property")
        .filter(|property| property.kind() == "property_identifier")?;
    let prototype = left
        .child_by_field_name("object")
        .filter(|object| object.kind() == "member_expression")?;
    let property = prototype
        .child_by_field_name("property")
        .filter(|property| property.kind() == "property_identifier")?;
    let owner = prototype
        .child_by_field_name("object")
        .filter(|object| object.kind() == "identifier")?;
    let owner = text(owner, source);
    let member = text(member, source);
    (text(property, source) == "prototype" && owner.len() + member.len() <= 128)
        .then_some((owner, member))
}

pub(super) fn direct_prototype_owner<'a>(left: Syntax<'_>, source: &'a str) -> Option<&'a str> {
    if left.kind() != "member_expression" {
        return None;
    }
    let property = left
        .child_by_field_name("property")
        .filter(|property| property.kind() == "property_identifier")?;
    let owner = left
        .child_by_field_name("object")
        .filter(|object| object.kind() == "identifier")?;
    (text(property, source) == "prototype").then(|| text(owner, source))
}

pub(super) fn prototype_assignment<'a>(
    node: Syntax<'_>,
    source: &'a str,
) -> Option<(&'a str, &'a str)> {
    if !matches!(node.kind(), "function_expression" | "arrow_function") {
        return None;
    }
    let assignment = node
        .parent()
        .filter(|parent| parent.kind() == "assignment_expression")?;
    if assignment.child_by_field_name("right")?.id() != node.id()
        || assignment
            .child_by_field_name("operator")
            .is_some_and(|operator| text(operator, source) != "=")
        || !assignment.parent().is_some_and(|parent| {
            parent.kind() == "expression_statement"
                && parent.parent().is_some_and(|root| root.kind() == "program")
        })
    {
        return None;
    }
    prototype_target(assignment.child_by_field_name("left")?, source)
}

pub(super) fn prototype_identifier_assignment<'a>(
    node: Syntax<'_>,
    source: &'a str,
) -> Option<(&'a str, &'a str, &'a str)> {
    if node.kind() != "assignment_expression"
        || !node.parent().is_some_and(|parent| {
            parent.kind() == "expression_statement"
                && parent.parent().is_some_and(|root| root.kind() == "program")
        })
        || node
            .child_by_field_name("operator")
            .is_some_and(|operator| text(operator, source) != "=")
    {
        return None;
    }
    let (owner, member) = prototype_target(node.child_by_field_name("left")?, source)?;
    let implementation = node
        .child_by_field_name("right")
        .filter(|right| right.kind() == "identifier")?;
    Some((owner, member, text(implementation, source)))
}

pub(super) fn lexical_prototype_owner<'a>(node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "arrow_function" => {}
            "function_expression" => {
                return prototype_assignment(parent, source).map(|(owner, _)| owner);
            }
            "function_declaration"
            | "generator_function_declaration"
            | "generator_function"
            | "generator_function_expression"
            | "method_definition"
            | "class_declaration"
            | "class" => return None,
            _ => {}
        }
        current = parent.parent();
    }
    None
}
