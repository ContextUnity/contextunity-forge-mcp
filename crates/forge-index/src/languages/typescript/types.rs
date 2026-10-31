use super::*;

pub(super) fn element_type_from_annotation<'a>(
    annotation: Syntax<'_>,
    arrow: Syntax<'_>,
    file: &FileContext,
    source: &'a str,
) -> Option<&'a str> {
    let element = match annotation.kind() {
        "array_type" => annotation.named_child(0)?,
        "generic_type" => {
            let name = annotation.child_by_field_name("name")?;
            let name_str = text(name, source);
            if name.kind() != "type_identifier"
                || !matches!(
                    name_str,
                    "Array" | "NodeListOf" | "HTMLCollectionOf" | "Set"
                )
            {
                return None;
            }
            let arguments = annotation.child_by_field_name("type_arguments")?;
            if arguments.named_child_count() != 1 || array_type_shadowed(arrow, file) {
                return None;
            }
            arguments.named_child(0)?
        }
        _ => return None,
    };
    if !matches!(element.kind(), "type_identifier" | "identifier") {
        return None;
    }
    Some(text(element, source))
}

pub(super) fn array_callback_element<'a>(
    arrow: Syntax<'_>,
    source: &'a str,
    file: &FileContext,
) -> Option<&'a str> {
    let call = arrow
        .parent()
        .filter(|parent| parent.kind() == "arguments")?
        .parent()?;
    let function = call
        .child_by_field_name("function")
        .filter(|function| function.kind() == "member_expression")?;
    let method = function.child_by_field_name("property")?;
    if !matches!(
        text(method, source),
        "forEach" | "map" | "filter" | "find" | "some" | "every" | "flatMap"
    ) {
        return None;
    }
    let receiver = function.child_by_field_name("object")?;
    if receiver.kind() == "identifier" {
        let receiver_name = text(receiver, source);
        let mut parent = call.parent();
        let enclosing = loop {
            let node = parent?;
            if matches!(
                node.kind(),
                "function_declaration"
                    | "method_definition"
                    | "function_expression"
                    | "arrow_function"
            ) {
                break node;
            }
            parent = node.parent();
        };
        if let Some(parameters) = enclosing.child_by_field_name("parameters") {
            let mut cursor = parameters.walk();
            let mut matches = parameters.named_children(&mut cursor).filter(|parameter| {
                parameter
                    .child_by_field_name("name")
                    .or_else(|| parameter.child_by_field_name("pattern"))
                    .is_some_and(|name| {
                        name.kind() == "identifier" && text(name, source) == receiver_name
                    })
            });
            if let Some(parameter) = matches.next() {
                if matches.next().is_none() {
                    let annotation = parameter.child_by_field_name("type")?.named_child(0)?;
                    if !file
                        .rebound_function_parameters
                        .get(&enclosing.id())
                        .is_some_and(|names| names.contains(receiver_name))
                    {
                        return element_type_from_annotation(annotation, arrow, file, source);
                    }
                }
            }
        }
        let mut current_scope = call.parent();
        while let Some(node) = current_scope {
            if matches!(node.kind(), "statement_block" | "program" | "source_file") {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.start_byte() >= call.start_byte() {
                        break;
                    }
                    if matches!(child.kind(), "lexical_declaration" | "variable_declaration") {
                        let mut decl_cursor = child.walk();
                        for declarator in child.named_children(&mut decl_cursor) {
                            if declarator.kind() == "variable_declarator" {
                                if let Some(var_name) = declarator.child_by_field_name("name") {
                                    if var_name.kind() == "identifier"
                                        && text(var_name, source) == receiver_name
                                    {
                                        if let Some(ty_node) =
                                            declarator.child_by_field_name("type")
                                        {
                                            let annotation =
                                                ty_node.named_child(0).unwrap_or(ty_node);
                                            if let Some(el) = element_type_from_annotation(
                                                annotation, arrow, file, source,
                                            ) {
                                                return Some(el);
                                            }
                                        }
                                        if let Some(init) = declarator.child_by_field_name("value")
                                        {
                                            if init.kind() == "call_expression" {
                                                if let Some(callee) = init
                                                    .child_by_field_name("function")
                                                    .filter(|c| c.kind() == "member_expression")
                                                {
                                                    if let Some(method) =
                                                        callee.child_by_field_name("property")
                                                    {
                                                        if matches!(
                                                            text(method, source),
                                                            "querySelectorAll"
                                                                | "getElementsByTagName"
                                                                | "getElementsByClassName"
                                                        ) {
                                                            if let Some(obj) =
                                                                callee.child_by_field_name("object")
                                                            {
                                                                if dom_receiver_from_ast(
                                                                    obj, source, file, 0,
                                                                )
                                                                .is_some()
                                                                {
                                                                    return Some("Element");
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if matches!(
                node.kind(),
                "function_declaration"
                    | "method_definition"
                    | "function_expression"
                    | "arrow_function"
            ) {
                break;
            }
            current_scope = node.parent();
        }
        None
    } else if receiver.kind() == "member_expression"
        && receiver
            .child_by_field_name("object")
            .is_some_and(|obj| obj.kind() == "this")
    {
        let property = receiver.child_by_field_name("property")?;
        let field_name = text(property, source);
        let (_, class) = lexical_this_method(arrow)?;
        let body = class.child_by_field_name("body")?;
        let mut cursor = body.walk();
        for child in body.named_children(&mut cursor) {
            if matches!(child.kind(), "field_definition" | "public_field_definition") {
                if let Some(prop) = child
                    .child_by_field_name("property")
                    .or_else(|| child.child_by_field_name("name"))
                {
                    if text(prop, source) == field_name {
                        let ty_node = child.child_by_field_name("type")?;
                        let annotation = ty_node.named_child(0).unwrap_or(ty_node);
                        return element_type_from_annotation(annotation, arrow, file, source);
                    }
                }
            }
        }
        None
    } else if receiver.kind() == "call_expression" {
        let callee = receiver
            .child_by_field_name("function")
            .filter(|callee| callee.kind() == "member_expression")?;
        let method = callee.child_by_field_name("property")?;
        let method_name = text(method, source);
        if matches!(
            method_name,
            "querySelectorAll" | "getElementsByTagName" | "getElementsByClassName"
        ) {
            let object = callee.child_by_field_name("object")?;
            if dom_receiver_from_ast(object, source, file, 0).is_some() {
                return Some("Element");
            }
        }
        None
    } else {
        None
    }
}
