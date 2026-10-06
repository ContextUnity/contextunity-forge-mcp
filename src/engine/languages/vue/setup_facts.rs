use super::*;

pub(super) fn collect_typed_iterables(
    root: Syntax<'_>,
    source: &str,
    output: &mut BTreeMap<String, TypeExpr>,
    duplicates: &mut HashSet<String>,
) {
    for statement in root.named_children(&mut root.walk()) {
        if statement.kind() != "lexical_declaration"
            || !statement
                .child_by_field_name("kind")
                .is_some_and(|kind| text(kind, source) == "const")
        {
            continue;
        }
        for declaration in statement.named_children(&mut statement.walk()) {
            if declaration.kind() != "variable_declarator" {
                continue;
            }
            let (Some(name), Some(annotation)) = (
                declaration
                    .child_by_field_name("name")
                    .filter(|node| node.kind() == "identifier"),
                declaration.child_by_field_name("type"),
            ) else {
                continue;
            };
            let Some(ty) = annotation.named_child(0) else {
                continue;
            };
            let element = if ty.kind() == "array_type" {
                ty.named_child(0)
            } else if ty.kind() == "generic_type"
                && ty
                    .child_by_field_name("name")
                    .is_some_and(|base| text(base, source) == "Array")
            {
                ty.child_by_field_name("type_arguments").and_then(|args| {
                    (args.named_child_count() == 1)
                        .then(|| args.named_child(0))
                        .flatten()
                })
            } else {
                None
            };
            let Some(element) =
                element.filter(|node| matches!(node.kind(), "type_identifier" | "identifier"))
            else {
                continue;
            };
            let binding = text(name, source).to_owned();
            if output
                .insert(
                    binding.clone(),
                    TypeExpr::Named {
                        name: text(element, source).to_owned(),
                    },
                )
                .is_some()
            {
                duplicates.insert(binding);
            }
        }
    }
}

pub(super) fn collect_typed_slots(
    root: Syntax<'_>,
    source: &str,
    output: &mut Vec<VueSlotFact>,
    calls: &mut usize,
) -> bool {
    let is_slot_call = |node: Syntax<'_>| {
        node.kind() == "call_expression"
            && node.child_by_field_name("function").is_some_and(|callee| {
                callee.kind() == "identifier" && text(callee, source) == "defineSlots"
            })
    };
    for statement in root.named_children(&mut root.walk()) {
        if matches!(
            statement.kind(),
            "lexical_declaration" | "variable_declaration"
        ) {
            for declaration in statement.named_children(&mut statement.walk()) {
                if declaration.kind() == "variable_declarator"
                    && declaration
                        .child_by_field_name("value")
                        .is_some_and(is_slot_call)
                {
                    return true;
                }
            }
            continue;
        }
        if statement.kind() != "expression_statement" {
            continue;
        }
        let Some(call) = statement
            .named_child(0)
            .filter(|node| node.kind() == "call_expression")
        else {
            continue;
        };
        if !is_slot_call(call) {
            continue;
        }
        *calls += 1;
        if *calls > 1 {
            return true;
        }
        let Some(args) = call.child_by_field_name("type_arguments") else {
            return true;
        };
        if args.named_child_count() != 1 {
            return true;
        }
        let Some(slots) = args
            .named_child(0)
            .filter(|node| node.kind() == "object_type")
        else {
            return true;
        };
        for signature in slots.named_children(&mut slots.walk()) {
            if signature.kind() != "method_signature" {
                return true;
            }
            let Some(name) = signature
                .child_by_field_name("name")
                .filter(|node| node.kind() == "property_identifier")
            else {
                return true;
            };
            let Some(params) = signature.child_by_field_name("parameters") else {
                return true;
            };
            if params.named_child_count() != 1 {
                return true;
            }
            let Some(param) = params
                .named_child(0)
                .filter(|node| node.kind() == "required_parameter")
            else {
                return true;
            };
            let Some(props) = param
                .child_by_field_name("type")
                .and_then(|annotation| annotation.named_child(0))
                .filter(|node| node.kind() == "object_type")
            else {
                return true;
            };
            for property in props.named_children(&mut props.walk()) {
                if property.kind() != "property_signature" {
                    return true;
                }
                let Some(binding) = property
                    .child_by_field_name("name")
                    .filter(|node| node.kind() == "property_identifier")
                else {
                    return true;
                };
                let Some(ty) = property
                    .child_by_field_name("type")
                    .and_then(|annotation| annotation.named_child(0))
                    .filter(|node| matches!(node.kind(), "type_identifier" | "identifier"))
                else {
                    return true;
                };
                if output.len() >= 128 {
                    return true;
                }
                let at = source_position(source, binding.start_byte());
                output.push(VueSlotFact {
                    slot: text(name, source).to_owned(),
                    binding: text(binding, source).to_owned(),
                    position: SourcePosition {
                        line: at.line,
                        column: at.column,
                    },
                    type_expr: TypeExpr::Named {
                        name: text(ty, source).to_owned(),
                    },
                });
            }
        }
    }
    false
}

pub(super) fn collect_registered_components(
    root: Syntax<'_>,
    source: &str,
    output: &mut BTreeMap<String, (String, bool)>,
    duplicates: &mut HashSet<String>,
) {
    for statement in root.named_children(&mut root.walk()) {
        if statement.kind() != "export_statement"
            || !text(statement, source)
                .trim_start()
                .starts_with("export default")
        {
            continue;
        }
        let Some(value) = statement.child_by_field_name("value") else {
            continue;
        };
        let (options, wrapped) = if value.kind() == "object" {
            (value, false)
        } else if value.kind() == "call_expression"
            && value
                .child_by_field_name("function")
                .is_some_and(|callee| text(callee, source) == "defineComponent")
        {
            let Some(arguments) = value.child_by_field_name("arguments") else {
                continue;
            };
            if arguments.named_child_count() != 1 {
                continue;
            }
            let Some(options) = arguments
                .named_child(0)
                .filter(|node| node.kind() == "object")
            else {
                continue;
            };
            (options, true)
        } else {
            continue;
        };
        let mut cursor = options.walk();
        let mut properties = options.named_children(&mut cursor).filter(|node| {
            node.kind() == "pair"
                && node
                    .child_by_field_name("key")
                    .is_some_and(|key| text(key, source) == "components")
        });
        let Some(property) = properties.next() else {
            continue;
        };
        if properties.next().is_some() {
            continue;
        }
        let Some(registrations) = property
            .child_by_field_name("value")
            .filter(|node| node.kind() == "object")
        else {
            continue;
        };
        for entry in registrations.named_children(&mut registrations.walk()) {
            if entry.kind() != "pair" {
                continue;
            }
            let (Some(key), Some(value)) = (
                entry.child_by_field_name("key"),
                entry.child_by_field_name("value"),
            ) else {
                continue;
            };
            if value.kind() != "identifier" {
                continue;
            }
            let name = match key.kind() {
                "property_identifier" => text(key, source),
                "string" => {
                    let raw = text(key, source);
                    if raw.len() < 2 || raw.contains('\\') {
                        continue;
                    }
                    &raw[1..raw.len() - 1]
                }
                _ => continue,
            };
            if name.is_empty() || (output.len() >= 128 && !output.contains_key(name)) {
                continue;
            }
            if output
                .insert(name.to_owned(), (text(value, source).to_owned(), wrapped))
                .is_some()
            {
                duplicates.insert(name.to_owned());
            }
        }
    }
}

pub(super) fn collect_typed_props(
    root: Syntax<'_>,
    source: &str,
    output: &mut BTreeMap<String, BTreeMap<String, Position>>,
    duplicates: &mut HashSet<String>,
) {
    for statement in root.named_children(&mut root.walk()) {
        if statement.kind() != "lexical_declaration"
            || !statement
                .child_by_field_name("kind")
                .is_some_and(|kind| text(kind, source) == "const")
        {
            continue;
        }
        for declaration in statement.named_children(&mut statement.walk()) {
            if declaration.kind() != "variable_declarator" {
                continue;
            }
            let (Some(name), Some(value)) = (
                declaration.child_by_field_name("name"),
                declaration.child_by_field_name("value"),
            ) else {
                continue;
            };
            if name.kind() != "identifier"
                || value.kind() != "call_expression"
                || !value
                    .child_by_field_name("function")
                    .is_some_and(|callee| text(callee, source) == "defineProps")
            {
                continue;
            }
            let Some(type_arguments) = value.child_by_field_name("type_arguments") else {
                continue;
            };
            let Some(object_type) = type_arguments
                .named_child(0)
                .filter(|node| node.kind() == "object_type")
            else {
                continue;
            };
            if type_arguments.named_child_count() != 1 {
                continue;
            }
            let mut properties = BTreeMap::new();
            collect_typed_prop_members(object_type, source, "", &mut properties, 0);
            let binding = text(name, source).to_owned();
            if output.insert(binding.clone(), properties).is_some() {
                duplicates.insert(binding);
            }
        }
    }
}

fn collect_typed_prop_members(
    object_type: Syntax<'_>,
    source: &str,
    prefix: &str,
    output: &mut BTreeMap<String, Position>,
    depth: usize,
) {
    if depth >= 8 || output.len() >= 128 {
        return;
    }
    let mut unique = BTreeMap::new();
    let mut duplicates = HashSet::new();
    for property in object_type.named_children(&mut object_type.walk()) {
        if property.kind() != "property_signature" {
            continue;
        }
        let Some(key) = property
            .child_by_field_name("name")
            .filter(|key| key.kind() == "property_identifier")
        else {
            continue;
        };
        let member = text(key, source).to_owned();
        if unique.insert(member.clone(), (property, key)).is_some() {
            duplicates.insert(member);
        }
    }
    for member in duplicates {
        unique.remove(&member);
    }
    for (member, (property, key)) in unique {
        if output.len() >= 128 {
            break;
        }
        let path = if prefix.is_empty() {
            member
        } else {
            format!("{prefix}.{member}")
        };
        output.insert(path.clone(), source_position(source, key.start_byte()));
        if let Some(nested) = property
            .child_by_field_name("type")
            .and_then(|annotation| annotation.named_child(0))
            .filter(|node| node.kind() == "object_type")
        {
            collect_typed_prop_members(nested, source, &path, output, depth + 1);
        }
    }
}

pub(super) fn collect_setup_callables(
    root: Syntax<'_>,
    source: &str,
    output: &mut BTreeMap<String, (ReceiverHint, Position)>,
    duplicates: &mut HashSet<String>,
) {
    for statement in root.named_children(&mut root.walk()) {
        if statement.kind() != "lexical_declaration"
            || !statement
                .child_by_field_name("kind")
                .is_some_and(|kind| text(kind, source) == "const")
        {
            continue;
        }
        for declaration in statement.named_children(&mut statement.walk()) {
            if declaration.kind() != "variable_declarator" {
                continue;
            }
            let (Some(name), Some(value)) = (
                declaration.child_by_field_name("name"),
                declaration.child_by_field_name("value"),
            ) else {
                continue;
            };
            if value.kind() != "call_expression" {
                continue;
            }
            let Some(callee) = value.child_by_field_name("function") else {
                continue;
            };
            let producer = text(callee, source);
            let callable = match (name.kind(), producer) {
                ("identifier", "defineEmits") => {
                    Some((text(name, source), ReceiverHint::VueSetupMacroCallable))
                }
                _ => None,
            };
            if let Some((binding, hint)) = callable {
                let position = source_position(source, declaration.end_byte());
                if output
                    .insert(binding.to_owned(), (hint, position))
                    .is_some()
                {
                    duplicates.insert(binding.to_owned());
                }
            }
        }
    }
}

pub(super) fn collect_shadowed_producers(
    root: Syntax<'_>,
    source: &str,
    output: &mut HashSet<String>,
) {
    for statement in root.named_children(&mut root.walk()) {
        match statement.kind() {
            "function_declaration" | "class_declaration" => {
                if let Some(name) = statement.child_by_field_name("name") {
                    let name = text(name, source);
                    if matches!(name, "defineEmits" | "defineProps" | "defineSlots") {
                        output.insert(name.to_owned());
                    }
                }
            }
            "lexical_declaration" | "variable_declaration" => {
                for declaration in statement.named_children(&mut statement.walk()) {
                    if let Some(name) = declaration.child_by_field_name("name") {
                        let name = text(name, source);
                        if matches!(name, "defineEmits" | "defineProps" | "defineSlots") {
                            output.insert(name.to_owned());
                        }
                    }
                }
            }
            "import_statement" => {
                let mut pending = vec![statement];
                while let Some(node) = pending.pop() {
                    if node.kind() == "import_specifier" {
                        if let Some(local) = node
                            .child_by_field_name("alias")
                            .or_else(|| node.child_by_field_name("name"))
                        {
                            let name = text(local, source);
                            if matches!(name, "defineEmits" | "defineProps" | "defineSlots") {
                                output.insert(name.to_owned());
                            }
                        }
                        continue;
                    }
                    if node.kind() == "identifier" {
                        let name = text(node, source);
                        if matches!(name, "defineEmits" | "defineProps" | "defineSlots") {
                            output.insert(name.to_owned());
                        }
                        continue;
                    }
                    if matches!(
                        node.kind(),
                        "import_statement" | "import_clause" | "named_imports" | "namespace_import"
                    ) {
                        pending.extend(node.named_children(&mut node.walk()));
                    }
                }
            }
            _ => {}
        }
    }
}

pub(super) fn collect_setup_reassignments(
    root: Syntax<'_>,
    source: &str,
    output: &mut HashSet<String>,
) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if matches!(
            node.kind(),
            "assignment_expression" | "augmented_assignment_expression"
        ) {
            if let Some(left) = node.child_by_field_name("left") {
                if left.kind() == "identifier" {
                    output.insert(text(left, source).to_owned());
                }
            }
        } else if node.kind() == "update_expression" {
            if let Some(argument) = node.child_by_field_name("argument") {
                if argument.kind() == "identifier" {
                    output.insert(text(argument, source).to_owned());
                }
            }
        }
        stack.extend(node.named_children(&mut node.walk()));
    }
}
