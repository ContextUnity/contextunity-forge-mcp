use super::*;

pub(super) struct SetupFactOutputs<'a> {
    pub setup_callables: &'a mut BTreeMap<String, (ReceiverHint, Position)>,
    pub duplicate_callables: &'a mut HashSet<String>,
    pub typed_props: &'a mut BTreeMap<String, BTreeMap<String, Position>>,
    pub duplicate_typed_props: &'a mut HashSet<String>,
    pub typed_iterables: &'a mut BTreeMap<String, TypeExpr>,
    pub duplicate_typed_iterables: &'a mut HashSet<String>,
    pub typed_slots: &'a mut Vec<VueSlotFact>,
    pub typed_slots_invalid: &'a mut bool,
    pub typed_slot_calls: &'a mut usize,
    pub shadowed_producers: &'a mut HashSet<String>,
    pub setup_reassignments: &'a mut HashSet<String>,
    pub registered_components: &'a mut BTreeMap<String, (String, bool)>,
    pub duplicate_components: &'a mut HashSet<String>,
    pub component_reassignments: &'a mut HashSet<String>,
}

/// Collects Vue setup and component facts during one depth-first syntax walk.
pub(super) fn collect_setup_facts(
    root: Syntax<'_>,
    source: &str,
    setup_script: bool,
    typescript: bool,
    output: &mut SetupFactOutputs<'_>,
) {
    let mut slots_stopped = *output.typed_slots_invalid;
    let mut contexts = Vec::new();
    let statements: Vec<_> = root.named_children(&mut root.walk()).collect();
    for statement in statements.into_iter().rev() {
        ast::visitor::walk(statement, &mut |node, entering| {
            if !entering {
                contexts.pop();
                return;
            }
            let direct = node.parent() == Some(root);
            let (depth, statement_kind, const_declaration, in_import, active) = if direct {
                let kind = node.kind();
                (
                    0usize,
                    kind,
                    kind == "lexical_declaration"
                        && node
                            .child_by_field_name("kind")
                            .is_some_and(|keyword| text(keyword, source) == "const"),
                    kind == "import_statement",
                    true,
                )
            } else if let Some((depth, kind, constant, import, active)) = contexts.last().copied() {
                (depth + 1, kind, constant, import, active)
            } else {
                (0, "", false, false, false)
            };
            contexts.push((
                depth,
                statement_kind,
                const_declaration,
                in_import,
                active && !(in_import && node.kind() == "import_specifier"),
            ));
            if !active {
                return;
            }
            if depth == 0 {
                if !setup_script && node.kind() == "export_statement" {
                    collect_registered_component(
                        node,
                        source,
                        output.registered_components,
                        output.duplicate_components,
                    );
                }
                if setup_script
                    && matches!(node.kind(), "function_declaration" | "class_declaration")
                {
                    if let Some(name) = node.child_by_field_name("name") {
                        insert_macro_name(text(name, source), output.shadowed_producers);
                    }
                }
            }

            if depth == 1 {
                if matches!(
                    statement_kind,
                    "lexical_declaration" | "variable_declaration"
                ) {
                    if let Some(name) = node.child_by_field_name("name") {
                        if setup_script {
                            insert_macro_name(text(name, source), output.shadowed_producers);
                        }
                    }
                    if setup_script
                        && typescript
                        && !slots_stopped
                        && node.kind() == "variable_declarator"
                        && node
                            .child_by_field_name("value")
                            .is_some_and(|value| is_define_slots(value, source))
                    {
                        *output.typed_slots_invalid = true;
                        slots_stopped = true;
                    }
                }

                if setup_script
                    && statement_kind == "lexical_declaration"
                    && const_declaration
                    && node.kind() == "variable_declarator"
                {
                    collect_typed_prop_declaration(
                        node,
                        source,
                        output.typed_props,
                        output.duplicate_typed_props,
                    );
                    collect_setup_callable_declaration(
                        node,
                        source,
                        output.setup_callables,
                        output.duplicate_callables,
                    );
                    if typescript {
                        collect_typed_iterable_declaration(
                            node,
                            source,
                            output.typed_iterables,
                            output.duplicate_typed_iterables,
                        );
                    }
                }
            }

            if setup_script && in_import {
                if node.kind() == "import_specifier" {
                    if let Some(local) = node
                        .child_by_field_name("alias")
                        .or_else(|| node.child_by_field_name("name"))
                    {
                        insert_macro_name(text(local, source), output.shadowed_producers);
                    }
                } else if node.kind() == "identifier" {
                    insert_macro_name(text(node, source), output.shadowed_producers);
                }
            }

            if matches!(
                node.kind(),
                "assignment_expression" | "augmented_assignment_expression"
            ) {
                if let Some(left) = node.child_by_field_name("left") {
                    if left.kind() == "identifier" {
                        let target = if setup_script {
                            &mut *output.setup_reassignments
                        } else {
                            &mut *output.component_reassignments
                        };
                        target.insert(text(left, source).to_owned());
                    }
                }
            } else if node.kind() == "update_expression" {
                if let Some(argument) = node.child_by_field_name("argument") {
                    if argument.kind() == "identifier" {
                        let target = if setup_script {
                            &mut *output.setup_reassignments
                        } else {
                            &mut *output.component_reassignments
                        };
                        target.insert(text(argument, source).to_owned());
                    }
                }
            }

            if setup_script
                && typescript
                && depth == 0
                && !slots_stopped
                && node.kind() == "expression_statement"
            {
                if let Some(call) = node
                    .named_child(0)
                    .filter(|node| is_define_slots(*node, source))
                {
                    *output.typed_slot_calls += 1;
                    if *output.typed_slot_calls > 1
                        || collect_typed_slot_call(call, source, output.typed_slots)
                    {
                        *output.typed_slots_invalid = true;
                        slots_stopped = true;
                    }
                }
            }
        });
    }
}

fn insert_macro_name(name: &str, output: &mut HashSet<String>) {
    if matches!(name, "defineEmits" | "defineProps" | "defineSlots") {
        output.insert(name.to_owned());
    }
}

fn is_define_slots(node: Syntax<'_>, source: &str) -> bool {
    node.kind() == "call_expression"
        && node.child_by_field_name("function").is_some_and(|callee| {
            callee.kind() == "identifier" && text(callee, source) == "defineSlots"
        })
}

fn collect_typed_iterable_declaration(
    declaration: Syntax<'_>,
    source: &str,
    output: &mut BTreeMap<String, TypeExpr>,
    duplicates: &mut HashSet<String>,
) {
    let (Some(name), Some(annotation)) = (
        declaration
            .child_by_field_name("name")
            .filter(|node| node.kind() == "identifier"),
        declaration.child_by_field_name("type"),
    ) else {
        return;
    };
    let Some(ty) = annotation.named_child(0) else {
        return;
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
        return;
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

fn collect_typed_prop_declaration(
    declaration: Syntax<'_>,
    source: &str,
    output: &mut BTreeMap<String, BTreeMap<String, Position>>,
    duplicates: &mut HashSet<String>,
) {
    let (Some(name), Some(value)) = (
        declaration.child_by_field_name("name"),
        declaration.child_by_field_name("value"),
    ) else {
        return;
    };
    if name.kind() != "identifier"
        || value.kind() != "call_expression"
        || !value
            .child_by_field_name("function")
            .is_some_and(|callee| text(callee, source) == "defineProps")
    {
        return;
    }
    let Some(type_arguments) = value.child_by_field_name("type_arguments") else {
        return;
    };
    let Some(object_type) = type_arguments
        .named_child(0)
        .filter(|node| node.kind() == "object_type")
    else {
        return;
    };
    if type_arguments.named_child_count() != 1 {
        return;
    }
    let mut properties = BTreeMap::new();
    collect_typed_prop_members(object_type, source, "", &mut properties, 0);
    let binding = text(name, source).to_owned();
    if output.insert(binding.clone(), properties).is_some() {
        duplicates.insert(binding);
    }
}

fn collect_setup_callable_declaration(
    declaration: Syntax<'_>,
    source: &str,
    output: &mut BTreeMap<String, (ReceiverHint, Position)>,
    duplicates: &mut HashSet<String>,
) {
    let (Some(name), Some(value)) = (
        declaration.child_by_field_name("name"),
        declaration.child_by_field_name("value"),
    ) else {
        return;
    };
    if value.kind() != "call_expression"
        || !value
            .child_by_field_name("function")
            .is_some_and(|callee| text(callee, source) == "defineEmits")
        || name.kind() != "identifier"
    {
        return;
    }
    let binding = text(name, source);
    let position = source_position(source, declaration.end_byte());
    if output
        .insert(
            binding.to_owned(),
            (ReceiverHint::VueSetupMacroCallable, position),
        )
        .is_some()
    {
        duplicates.insert(binding.to_owned());
    }
}

fn collect_typed_slot_call(call: Syntax<'_>, source: &str, output: &mut Vec<VueSlotFact>) -> bool {
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
    false
}

fn collect_registered_component(
    statement: Syntax<'_>,
    source: &str,
    output: &mut BTreeMap<String, (String, bool)>,
    duplicates: &mut HashSet<String>,
) {
    if !text(statement, source)
        .trim_start()
        .starts_with("export default")
    {
        return;
    }
    let Some(value) = statement.child_by_field_name("value") else {
        return;
    };
    let (options, wrapped) = if value.kind() == "object" {
        (value, false)
    } else if value.kind() == "call_expression"
        && value
            .child_by_field_name("function")
            .is_some_and(|callee| text(callee, source) == "defineComponent")
    {
        let Some(arguments) = value.child_by_field_name("arguments") else {
            return;
        };
        if arguments.named_child_count() != 1 {
            return;
        }
        let Some(options) = arguments
            .named_child(0)
            .filter(|node| node.kind() == "object")
        else {
            return;
        };
        (options, true)
    } else {
        return;
    };
    let mut cursor = options.walk();
    let mut properties = options.named_children(&mut cursor).filter(|node| {
        node.kind() == "pair"
            && node
                .child_by_field_name("key")
                .is_some_and(|key| text(key, source) == "components")
    });
    let Some(property) = properties.next() else {
        return;
    };
    if properties.next().is_some() {
        return;
    }
    let Some(registrations) = property
        .child_by_field_name("value")
        .filter(|node| node.kind() == "object")
    else {
        return;
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
