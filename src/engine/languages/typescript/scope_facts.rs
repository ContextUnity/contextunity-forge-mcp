use super::*;

#[derive(Default)]
pub(super) struct ScopeFactNeeds {
    pub dom_scope_facts: bool,
    pub array_callback_scope_facts: bool,
    pub dom_array_receiver_facts: bool,
    pub commonjs_scope_facts: bool,
}

const ARRAY_CALLBACK_METHODS: &[&str] = &[
    "forEach", "map", "filter", "find", "some", "every", "flatMap",
];
const DOM_ARRAY_RECEIVER_METHODS: &[&str] = &[
    "querySelectorAll",
    "getElementsByTagName",
    "getElementsByClassName",
];

#[derive(Default)]
struct MemberCandidates {
    dom_event_property: bool,
    add_event_listener: bool,
    array_callback: bool,
    dom_array_receiver: bool,
}

fn is_array_callback_property(property: &[u8]) -> bool {
    ARRAY_CALLBACK_METHODS.iter().any(|method| {
        property.starts_with(method.as_bytes())
            && property.get(method.len()).is_none_or(|byte| {
                !byte.is_ascii_alphanumeric() && !matches!(byte, b'_' | b'$') && *byte < 0x80
            })
    })
}

fn is_dom_event_property(property: &[u8]) -> bool {
    property.starts_with(b"on") && property.len() > 2
}

fn is_dom_array_receiver_property(property: &[u8]) -> bool {
    DOM_ARRAY_RECEIVER_METHODS
        .iter()
        .any(|method| property == method.as_bytes())
}

fn is_scope_trivia(character: char) -> bool {
    character.is_whitespace() || matches!(character, '\u{200b}' | '\u{2060}' | '\u{feff}')
}

fn is_line_terminator(character: char) -> bool {
    matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn member_candidates(source: &str) -> MemberCandidates {
    let bytes = source.as_bytes();
    let mut candidates = MemberCandidates::default();
    let mut dot = 0;
    while dot < bytes.len() {
        if bytes[dot] != b'.' {
            dot += 1;
            continue;
        }
        let mut property = dot + 1;
        loop {
            while let Some(character) = source.get(property..).and_then(|rest| rest.chars().next())
            {
                if !is_scope_trivia(character) {
                    break;
                }
                property += character.len_utf8();
            }
            if bytes.get(property..property + 2) == Some(b"//") {
                property += 2;
                while let Some(character) =
                    source.get(property..).and_then(|rest| rest.chars().next())
                {
                    if is_line_terminator(character) {
                        break;
                    }
                    property += character.len_utf8();
                }
                continue;
            }
            if bytes.get(property..property + 2) == Some(b"/*") {
                property += 2;
                while property + 1 < bytes.len() && bytes.get(property..property + 2) != Some(b"*/")
                {
                    property += 1;
                }
                property = (property + 2).min(bytes.len());
                continue;
            }
            break;
        }
        let start = property;
        while let Some(character) = source.get(property..).and_then(|rest| rest.chars().next()) {
            let identifier_continuation = if character.is_ascii() {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '$')
            } else {
                !is_scope_trivia(character)
            };
            if !identifier_continuation {
                break;
            }
            property += character.len_utf8();
        }
        let name = &bytes[start..property];
        candidates.dom_event_property |=
            is_dom_event_property(name) || (name == b"on" && bytes.get(property) == Some(&b'\\'));
        candidates.add_event_listener |= name == b"addEventListener";
        candidates.array_callback |= is_array_callback_property(name);
        candidates.dom_array_receiver |= is_dom_array_receiver_property(name);
        dot += 1;
    }
    candidates
}

pub(super) fn classify_scope_fact_needs(
    root: Syntax<'_>,
    source: &str,
    javascript: bool,
) -> ScopeFactNeeds {
    let mut candidates = member_candidates(source);
    candidates.add_event_listener |= source.contains("addEventListener");
    candidates.dom_array_receiver |= candidates.array_callback
        && DOM_ARRAY_RECEIVER_METHODS
            .iter()
            .any(|method| source.contains(method));
    let commonjs_candidate = source.contains("require");
    if !commonjs_candidate
        && !candidates.dom_event_property
        && !candidates.add_event_listener
        && !candidates.array_callback
        && !candidates.dom_array_receiver
    {
        return ScopeFactNeeds::default();
    }

    let mut needs = ScopeFactNeeds::default();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if commonjs_candidate && node.kind() == "identifier" && text(node, source) == "require" {
            needs.commonjs_scope_facts = true;
        }
        if node.kind() == "member_expression" {
            if let Some(property) = node
                .child_by_field_name("property")
                .filter(|property| property.kind() == "property_identifier")
            {
                let name = text(property, source).as_bytes();
                needs.dom_scope_facts |= (candidates.dom_event_property
                    && is_dom_event_property(name))
                    || (candidates.add_event_listener && name == b"addEventListener");
                needs.array_callback_scope_facts |=
                    candidates.array_callback && is_array_callback_property(name);
                needs.dom_array_receiver_facts |=
                    candidates.dom_array_receiver && is_dom_array_receiver_property(name);
            }
        }

        // TypeScript's full collector subsumes all partial facts. JavaScript also
        // needs the partial collector for DOM callbacks, so require DOM evidence
        // (or its absence from the candidate set) before stopping on CommonJS.
        let dispatch_complete = if javascript {
            needs.dom_scope_facts
                || (needs.commonjs_scope_facts
                    && !candidates.dom_event_property
                    && !candidates.add_event_listener)
        } else {
            needs.dom_scope_facts
                || needs.commonjs_scope_facts
                || (needs.array_callback_scope_facts && needs.dom_array_receiver_facts)
        };
        if dispatch_complete {
            break;
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    needs
}

fn dom_scope_chain(node: Syntax<'_>) -> Vec<usize> {
    let mut scopes = Vec::new();
    let mut current = Some(node);
    while let Some(scope) = current {
        scopes.push(scope.id());
        current = scope.parent();
    }
    scopes
}

fn dom_listener_receiver_expression(
    node: Syntax<'_>,
    source: &str,
    depth: usize,
) -> DomListenerReceiverExpression {
    if depth > 4 {
        return DomListenerReceiverExpression::Unsupported;
    }
    match node.kind() {
        "identifier" => DomListenerReceiverExpression::Identifier {
            name: text(node, source).to_owned(),
            position: node.start_byte(),
        },
        "member_expression" => {
            let Some(object) = node.child_by_field_name("object") else {
                return DomListenerReceiverExpression::Unsupported;
            };
            let Some(name) = node
                .child_by_field_name("property")
                .filter(|property| property.kind() == "property_identifier")
            else {
                return DomListenerReceiverExpression::Unsupported;
            };
            if object.kind() != "this" {
                return DomListenerReceiverExpression::Unsupported;
            }
            let Some((method, class)) = lexical_this_method(node) else {
                return DomListenerReceiverExpression::Unsupported;
            };
            let static_method = (0..method.child_count()).any(|index| {
                method
                    .child(index)
                    .is_some_and(|child| child.kind() == "static")
            });
            DomListenerReceiverExpression::ThisField {
                name: text(name, source).to_owned(),
                class_scope: class.id(),
                static_method,
            }
        }
        "call_expression" => {
            let Some(callee) = node
                .child_by_field_name("function")
                .filter(|callee| callee.kind() == "member_expression")
            else {
                return DomListenerReceiverExpression::Unsupported;
            };
            let Some(receiver) = callee.child_by_field_name("object") else {
                return DomListenerReceiverExpression::Unsupported;
            };
            let Some(method) = callee
                .child_by_field_name("property")
                .filter(|property| property.kind() == "property_identifier")
            else {
                return DomListenerReceiverExpression::Unsupported;
            };
            let has_arguments = node
                .child_by_field_name("arguments")
                .is_some_and(|arguments| arguments.named_child_count() > 0);
            let type_arguments = node
                .child_by_field_name("type_arguments")
                .map(|arguments| {
                    arguments
                        .named_children(&mut arguments.walk())
                        .map(|argument| text(argument, source).to_owned())
                        .collect()
                })
                .unwrap_or_default();
            DomListenerReceiverExpression::MemberCall {
                receiver: Box::new(dom_listener_receiver_expression(
                    receiver,
                    source,
                    depth + 1,
                )),
                method: text(method, source).to_owned(),
                type_arguments,
                has_arguments,
            }
        }
        "parenthesized_expression" => node
            .named_child(0)
            .map(|inner| {
                DomListenerReceiverExpression::Parenthesized(Box::new(
                    dom_listener_receiver_expression(inner, source, depth + 1),
                ))
            })
            .unwrap_or_default(),
        _ => DomListenerReceiverExpression::Unsupported,
    }
}

pub(super) fn index_dom_listener_declarator(
    node: Syntax<'_>,
    source: &str,
    file: &mut FileContext,
) {
    let Some(binding) = node.child_by_field_name("name") else {
        return;
    };
    let Some(statement) = node.parent().filter(|statement| {
        matches!(
            statement.kind(),
            "lexical_declaration" | "variable_declaration"
        )
    }) else {
        return;
    };
    let Some(scope) = statement
        .parent()
        .filter(|scope| matches!(scope.kind(), "statement_block" | "program" | "source_file"))
    else {
        return;
    };
    let mut names = Vec::new();
    collect_pattern_binding_names(binding, source, &mut names);
    names.sort_unstable();
    names.dedup();
    if names.is_empty() {
        return;
    }
    let is_const = statement.kind() == "lexical_declaration"
        && statement
            .child_by_field_name("kind")
            .is_some_and(|keyword| text(keyword, source) == "const");
    let initializer = if is_const {
        node.child_by_field_name("value")
            .map(|value| dom_listener_receiver_expression(value, source, 0))
    } else {
        None
    };
    let scope_chain = std::sync::Arc::new(dom_scope_chain(scope));
    let declarations = std::sync::Arc::make_mut(&mut file.dom_listener_local_declarations);
    let block = declarations.entry(scope.id()).or_default();
    for name in names {
        let declaration = block.entry(name).or_default();
        declaration.declaration_count += 1;
        if declaration.declaration_count == 1 {
            declaration.is_const = is_const;
            declaration.statement_end = statement.end_byte();
            declaration.initializer = initializer.clone();
            declaration.scope_chain = std::sync::Arc::clone(&scope_chain);
        } else {
            declaration.initializer = None;
        }
    }
}

pub(super) fn collect_array_callback_scope_facts(
    root: Syntax<'_>,
    source: &str,
    file: &mut FileContext,
) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "variable_declarator" {
            index_dom_listener_declarator(node, source, file);
        }
        collect_dom_global_shadow_fact(node, source, file);
        collect_array_shadow_fact(node, source, file);
        collect_rebound_parameter_fact(node, source, file);
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
}
