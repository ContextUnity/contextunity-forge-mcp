use super::*;

pub(super) fn dom_global_name(name: &str) -> Option<&'static str> {
    match name {
        "document" => Some("document"),
        "window" => Some("window"),
        "Event" => Some("Event"),
        "CustomEvent" => Some("CustomEvent"),
        "MouseEvent" => Some("MouseEvent"),
        "KeyboardEvent" => Some("KeyboardEvent"),
        "EventTarget" => Some("EventTarget"),
        "Element" => Some("Element"),
        "HTMLElement" => Some("HTMLElement"),
        "HTMLTableElement" => Some("HTMLTableElement"),
        "HTMLDivElement" => Some("HTMLDivElement"),
        "HTMLInputElement" => Some("HTMLInputElement"),
        "HTMLButtonElement" => Some("HTMLButtonElement"),
        "HTMLAnchorElement" => Some("HTMLAnchorElement"),
        "Document" => Some("Document"),
        "Window" => Some("Window"),
        _ => None,
    }
}

pub(super) fn browser_global_type(name: &str) -> bool {
    if typed_dom_receiver(name).is_some() {
        return true;
    }
    matches!(
        name,
        "Response"
            | "Request"
            | "Headers"
            | "URL"
            | "URLSearchParams"
            | "FormData"
            | "Blob"
            | "File"
    )
}

pub(super) fn remember_dom_global(
    name: &str,
    names: &mut [Option<&'static str>; 16],
    count: &mut usize,
) {
    let Some(global) = dom_global_name(name.trim()) else {
        return;
    };
    if !names[..*count].contains(&Some(global)) {
        names[*count] = Some(global);
        *count += 1;
    }
}

pub(super) fn collect_dom_pattern_globals(
    pattern: Syntax<'_>,
    source: &str,
    names: &mut [Option<&'static str>; 16],
    count: &mut usize,
) {
    match pattern.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => {
            remember_dom_global(text(pattern, source), names, count);
        }
        "pair_pattern" => {
            if let Some(value) = pattern.child_by_field_name("value") {
                collect_dom_pattern_globals(value, source, names, count);
            }
        }
        "assignment_pattern" => {
            if let Some(left) = pattern.child_by_field_name("left") {
                collect_dom_pattern_globals(left, source, names, count);
            }
        }
        "formal_parameters" | "object_pattern" | "array_pattern" | "rest_pattern"
        | "required_parameter" | "optional_parameter" | "formal_parameter" => {
            if let Some(candidate) = pattern
                .child_by_field_name("name")
                .or_else(|| pattern.child_by_field_name("pattern"))
            {
                collect_dom_pattern_globals(candidate, source, names, count);
                return;
            }
            let mut cursor = pattern.walk();
            for child in pattern.named_children(&mut cursor) {
                collect_dom_pattern_globals(child, source, names, count);
            }
        }
        _ => {}
    }
}

pub(super) fn collect_dom_import_globals(
    node: Syntax<'_>,
    source: &str,
    names: &mut [Option<&'static str>; 16],
    count: &mut usize,
) {
    match node.kind() {
        "import_clause" | "namespace_import" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "identifier" {
                    remember_dom_global(text(child, source), names, count);
                }
            }
        }
        "import_specifier" => {
            if let Some(local) = node
                .child_by_field_name("alias")
                .or_else(|| node.child_by_field_name("name"))
            {
                remember_dom_global(text(local, source), names, count);
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_dom_import_globals(child, source, names, count);
    }
}

pub(super) fn nearest_binding_scope(node: Syntax<'_>, var_scoped: bool) -> Option<Syntax<'_>> {
    let mut current = Some(node);
    while let Some(scope) = current {
        if var_scoped {
            if matches!(
                scope.kind(),
                "program"
                    | "source_file"
                    | "function_declaration"
                    | "generator_function_declaration"
                    | "function_expression"
                    | "generator_function"
                    | "generator_function_expression"
                    | "arrow_function"
                    | "method_definition"
            ) {
                return Some(scope);
            }
        } else if matches!(
            scope.kind(),
            "statement_block"
                | "catch_clause"
                | "for_statement"
                | "for_in_statement"
                | "program"
                | "source_file"
        ) {
            return Some(scope);
        }
        current = scope.parent();
    }
    None
}

pub(super) fn collect_array_shadow_fact(node: Syntax<'_>, source: &str, file: &mut FileContext) {
    let (array_binding, array_var_scoped) = match node.kind() {
        "variable_declarator" => (
            node.child_by_field_name("name")
                .is_some_and(|name| name.kind() == "identifier" && text(name, source) == "Array"),
            node.parent()
                .is_some_and(|parent| parent.kind() == "variable_declaration"),
        ),
        "class_declaration"
        | "interface_declaration"
        | "type_alias_declaration"
        | "enum_declaration"
        | "function_declaration" => (field(node, source, "name") == Some("Array"), false),
        "type_parameter" => (field(node, source, "name") == Some("Array"), false),
        "required_parameter" | "optional_parameter" => {
            (field(node, source, "name") == Some("Array"), true)
        }
        "import_specifier" => (
            node.child_by_field_name("alias")
                .or_else(|| node.child_by_field_name("name"))
                .is_some_and(|name| text(name, source) == "Array"),
            false,
        ),
        "import_clause" => (
            node.named_child(0)
                .is_some_and(|name| name.kind() == "identifier" && text(name, source) == "Array"),
            false,
        ),
        "namespace_import" => (
            node.named_child(0)
                .is_some_and(|name| name.kind() == "identifier" && text(name, source) == "Array"),
            false,
        ),
        _ => (false, false),
    };
    if array_binding {
        let scope = if node.kind() == "type_parameter" {
            let mut current = node.parent();
            let mut owner = None;
            while let Some(candidate) = current {
                if matches!(
                    candidate.kind(),
                    "class_declaration"
                        | "class"
                        | "function_declaration"
                        | "function_expression"
                        | "arrow_function"
                        | "method_definition"
                ) {
                    owner = Some(candidate);
                    break;
                }
                current = candidate.parent();
            }
            owner
        } else {
            nearest_binding_scope(node, array_var_scoped)
        };
        if let Some(scope) = scope {
            std::sync::Arc::make_mut(&mut file.shadowed_array_type_scopes).insert(scope.id());
        }
    }
}

pub(super) fn collect_rebound_parameter_fact(
    node: Syntax<'_>,
    source: &str,
    file: &mut FileContext,
) {
    let rebound = match node.kind() {
        "assignment_expression" | "augmented_assignment_expression" => {
            node.child_by_field_name("left")
        }
        "variable_declarator" => node.child_by_field_name("name"),
        "update_expression" => node.named_child(0),
        _ => None,
    };
    let Some(name) = rebound.filter(|name| name.kind() == "identifier") else {
        return;
    };
    let name = text(name, source);
    let mut current = node.parent();
    while let Some(scope) = current {
        if matches!(
            scope.kind(),
            "function_declaration" | "method_definition" | "function_expression" | "arrow_function"
        ) {
            std::sync::Arc::make_mut(&mut file.rebound_function_parameters)
                .entry(scope.id())
                .or_default()
                .insert(name.to_owned());
        }
        current = scope.parent();
    }
}

pub(super) fn collect_dom_global_shadow_fact(
    node: Syntax<'_>,
    source: &str,
    file: &mut FileContext,
) {
    let mut shadowed_globals = [None; 16];
    let mut shadowed_count = 0;
    let mut var_scoped = false;
    match node.kind() {
        "variable_declarator" => {
            var_scoped = node
                .parent()
                .is_some_and(|parent| parent.kind() == "variable_declaration");
            if let Some(name) = node.child_by_field_name("name") {
                collect_dom_pattern_globals(
                    name,
                    source,
                    &mut shadowed_globals,
                    &mut shadowed_count,
                );
            }
        }
        "formal_parameters" | "required_parameter" | "optional_parameter" | "formal_parameter" => {
            var_scoped = true;
            collect_dom_pattern_globals(node, source, &mut shadowed_globals, &mut shadowed_count);
        }
        "arrow_function" => {
            var_scoped = true;
            if let Some(parameter) = node.child_by_field_name("parameter") {
                collect_dom_pattern_globals(
                    parameter,
                    source,
                    &mut shadowed_globals,
                    &mut shadowed_count,
                );
            }
        }
        "function_declaration"
        | "class_declaration"
        | "enum_declaration"
        | "interface_declaration"
        | "type_alias_declaration" => {
            if let Some(name) = field(node, source, "name") {
                remember_dom_global(name, &mut shadowed_globals, &mut shadowed_count);
            }
        }
        "function_expression" => {
            var_scoped = true;
            if let Some(name) = field(node, source, "name") {
                remember_dom_global(name, &mut shadowed_globals, &mut shadowed_count);
            }
        }
        "import_statement" => {
            collect_dom_import_globals(node, source, &mut shadowed_globals, &mut shadowed_count)
        }
        "catch_clause" => {
            if let Some(parameter) = node.child_by_field_name("parameter") {
                collect_dom_pattern_globals(
                    parameter,
                    source,
                    &mut shadowed_globals,
                    &mut shadowed_count,
                );
            }
        }
        "assignment_expression" | "augmented_assignment_expression" => {
            var_scoped = true;
            if let Some(left) = node
                .child_by_field_name("left")
                .filter(|left| left.kind() == "identifier")
            {
                remember_dom_global(
                    text(left, source),
                    &mut shadowed_globals,
                    &mut shadowed_count,
                );
            }
        }
        _ => {}
    }
    if shadowed_count > 0 {
        if let Some(scope) = nearest_binding_scope(node, var_scoped) {
            let by_scope = std::sync::Arc::make_mut(&mut file.shadowed_dom_global_scopes);
            let globals = by_scope.entry(scope.id()).or_default();
            for global in shadowed_globals[..shadowed_count].iter().flatten() {
                globals.insert((*global).to_owned());
            }
        }
    }
}

pub(super) fn collect_scope_facts(
    root: Syntax<'_>,
    source: &str,
    file: &mut FileContext,
    typescript: bool,
) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let (binding, var_scoped) = match node.kind() {
            "variable_declarator" => (
                node.child_by_field_name("name")
                    .is_some_and(|name| pattern_binds_require(name, source)),
                node.parent()
                    .is_some_and(|parent| parent.kind() == "variable_declaration"),
            ),
            "formal_parameters" | "required_parameter" | "optional_parameter"
            | "formal_parameter" => (pattern_binds_require(node, source), true),
            "arrow_function" => (
                node.child_by_field_name("parameter")
                    .is_some_and(|parameter| pattern_binds_require(parameter, source)),
                true,
            ),
            "function_expression" | "generator_function" | "generator_function_expression" => (
                field(node, source, "name").is_some_and(|name| name.trim() == "require"),
                true,
            ),
            "function_declaration" | "generator_function_declaration" | "class_declaration" => (
                field(node, source, "name").is_some_and(|name| name.trim() == "require"),
                false,
            ),
            "import_statement" => (import_binds_require(node, source), false),
            "catch_clause" => (
                node.child_by_field_name("parameter")
                    .is_some_and(|pattern| pattern_binds_require(pattern, source)),
                false,
            ),
            _ => (false, false),
        };
        if binding {
            if let Some(scope) = nearest_binding_scope(node, var_scoped) {
                std::sync::Arc::make_mut(&mut file.shadowed_require_scopes).insert(scope.id());
            }
        }
        if !typescript {
            let mut cursor = node.walk();
            stack.extend(node.named_children(&mut cursor));
            continue;
        }
        if node.kind() == "variable_declarator" {
            scope_facts::index_dom_listener_declarator(node, source, file);
        }
        if matches!(
            node.kind(),
            "function_declaration"
                | "generator_function_declaration"
                | "function_expression"
                | "generator_function"
                | "generator_function_expression"
                | "arrow_function"
                | "method_definition"
        ) {
            if let Some(parameters) = node
                .child_by_field_name("parameters")
                .or_else(|| node.child_by_field_name("parameter"))
            {
                let mut names = Vec::new();
                collect_pattern_binding_names(parameters, source, &mut names);
                if !names.is_empty() {
                    std::sync::Arc::make_mut(&mut file.dom_listener_parameter_bindings)
                        .entry(node.id())
                        .or_default()
                        .extend(names);
                }
            }
        }
        if node.kind() == "catch_clause" {
            if let Some(parameter) = node.child_by_field_name("parameter") {
                let mut names = Vec::new();
                collect_pattern_binding_names(parameter, source, &mut names);
                if !names.is_empty() {
                    std::sync::Arc::make_mut(&mut file.dom_listener_parameter_bindings)
                        .entry(node.id())
                        .or_default()
                        .extend(names);
                }
            }
        }
        let written = match node.kind() {
            "assignment_expression" | "augmented_assignment_expression" => {
                node.child_by_field_name("left")
            }
            "update_expression" => node.named_child(0),
            _ => None,
        };
        if let Some(name) = written.filter(|name| name.kind() == "identifier") {
            let name = text(name, source).to_owned();
            let local_writes = std::sync::Arc::make_mut(&mut file.dom_listener_local_writes);
            let mut parent = node.parent();
            while let Some(scope) = parent {
                if matches!(scope.kind(), "statement_block" | "program" | "source_file") {
                    local_writes
                        .entry(scope.id())
                        .or_default()
                        .entry(name.clone())
                        .or_default()
                        .push(node.start_byte());
                }
                if matches!(
                    scope.kind(),
                    "function_declaration"
                        | "function_expression"
                        | "arrow_function"
                        | "method_definition"
                ) {
                    break;
                }
                parent = scope.parent();
            }
        }
        collect_dom_global_shadow_fact(node, source, file);
        if matches!(node.kind(), "field_definition" | "public_field_definition") {
            if let Some(class) = node
                .parent()
                .filter(|body| body.kind() == "class_body")
                .and_then(|body| body.parent())
                .filter(|class| matches!(class.kind(), "class_declaration" | "class"))
            {
                if let Some(name) = node
                    .child_by_field_name("name")
                    .or_else(|| node.child_by_field_name("property"))
                    .filter(|name| matches!(name.kind(), "property_identifier" | "identifier"))
                {
                    let static_field = (0..node.child_count()).any(|index| {
                        node.child(index)
                            .is_some_and(|child| child.kind() == "static")
                    });
                    let ty = (!static_field)
                        .then(|| {
                            node.child_by_field_name("type")
                                .and_then(|annotation| annotation.named_child(0))
                                .filter(|ty| ty.kind() == "type_identifier")
                                .map(|ty| text(ty, source).to_owned())
                        })
                        .flatten();
                    let fields = std::sync::Arc::make_mut(&mut file.dom_listener_class_fields)
                        .entry(class.id())
                        .or_default();
                    let name = text(name, source).to_owned();
                    match fields.entry(name) {
                        std::collections::hash_map::Entry::Occupied(mut entry) => {
                            entry.insert(None);
                        }
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            entry.insert(ty);
                        }
                    }
                }
            }
        }
        if matches!(
            node.kind(),
            "function_declaration" | "method_definition" | "function_expression" | "arrow_function"
        ) {
            let mut receivers = HashMap::new();
            let mut duplicates = HashSet::new();
            if let Some(parameters) = node.child_by_field_name("parameters") {
                for parameter in parameters.named_children(&mut parameters.walk()) {
                    let Some(name) = parameter
                        .child_by_field_name("name")
                        .or_else(|| parameter.child_by_field_name("pattern"))
                        .filter(|name| name.kind() == "identifier")
                    else {
                        continue;
                    };
                    let Some(annotation) = parameter
                        .child_by_field_name("type")
                        .and_then(|ty| ty.named_child(0))
                        .filter(|ty| ty.kind() == "type_identifier")
                    else {
                        continue;
                    };
                    let ty = text(annotation, source);
                    if !matches!(
                        typed_dom_receiver(ty),
                        Some("Element" | "Document" | "Window" | "EventTarget")
                    ) {
                        continue;
                    }
                    let name = text(name, source).to_owned();
                    if receivers.insert(name.clone(), ty.to_owned()).is_some() {
                        duplicates.insert(name);
                    }
                }
            }
            if node.kind() == "arrow_function" {
                let parameter = node
                    .child_by_field_name("parameter")
                    .or_else(|| node.child_by_field_name("parameters")?.named_child(0))
                    .and_then(|p| {
                        if p.kind() == "identifier" {
                            Some(p)
                        } else {
                            p.child_by_field_name("name")
                                .or_else(|| p.child_by_field_name("pattern"))
                                .filter(|n| n.kind() == "identifier")
                        }
                    });
                if let (Some(parameter), Some(element)) =
                    (parameter, array_callback_element(node, source, file))
                {
                    if matches!(
                        typed_dom_receiver(element),
                        Some("Element" | "Document" | "Window" | "EventTarget")
                    ) {
                        receivers.insert(text(parameter, source).to_owned(), element.to_owned());
                    }
                }
            }
            for name in duplicates {
                receivers.remove(&name);
            }
            if !receivers.is_empty() {
                std::sync::Arc::make_mut(&mut file.dom_listener_receivers)
                    .insert(node.id(), receivers);
            }
        }
        collect_array_shadow_fact(node, source, file);
        collect_rebound_parameter_fact(node, source, file);
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
}

pub(super) fn import_binds_name(node: Syntax<'_>, source: &str, name: &str) -> bool {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        match current.kind() {
            "import_clause" | "namespace_import" => {
                let mut cursor = current.walk();
                if current
                    .named_children(&mut cursor)
                    .any(|child| child.kind() == "identifier" && text(child, source).trim() == name)
                {
                    return true;
                }
            }
            "import_specifier" => {
                let local =
                    field(current, source, "alias").or_else(|| field(current, source, "name"));
                if local.is_some_and(|local| local.trim() == name) {
                    return true;
                }
            }
            _ => {}
        }
        let mut cursor = current.walk();
        stack.extend(current.named_children(&mut cursor));
    }
    false
}

pub(super) fn import_binds_require(node: Syntax<'_>, source: &str) -> bool {
    import_binds_name(node, source, "require")
}

pub(super) fn has_type_import_modifier(node: Syntax<'_>) -> bool {
    (0..node.child_count()).any(|index| {
        node.child(index)
            .is_some_and(|child| child.kind() == "type")
    })
}

pub(super) fn array_type_shadowed(node: Syntax<'_>, file: &FileContext) -> bool {
    let mut current = Some(node);
    while let Some(scope) = current {
        if file.shadowed_array_type_scopes.contains(&scope.id()) {
            return true;
        }
        current = scope.parent();
    }
    false
}

pub(super) fn dom_global_shadowed(node: Syntax<'_>, file: &FileContext, name: &str) -> bool {
    let mut current = Some(node);
    while let Some(scope) = current {
        if file
            .shadowed_dom_global_scopes
            .get(&scope.id())
            .is_some_and(|names| names.contains(name))
        {
            return true;
        }
        current = scope.parent();
    }
    false
}

pub(super) fn dom_global_shadowed_in_scopes(
    scopes: &[usize],
    file: &FileContext,
    name: &str,
) -> bool {
    scopes.iter().any(|scope| {
        file.shadowed_dom_global_scopes
            .get(scope)
            .is_some_and(|names| names.contains(name))
    })
}

pub(super) fn dom_indexed_const<'a>(
    file: &'a FileContext,
    name: &str,
    at: usize,
    scopes: &[usize],
) -> Option<Result<&'a DomListenerLocalDeclaration, usize>> {
    for scope in scopes {
        let declaration = file
            .dom_listener_local_declarations
            .get(scope)
            .and_then(|bindings| bindings.get(name));
        let rebound = file
            .dom_listener_local_writes
            .get(scope)
            .and_then(|names| names.get(name))
            .is_some_and(|writes| writes.iter().any(|position| *position < at));
        if declaration.is_some() || rebound {
            let valid = declaration.filter(|declaration| {
                declaration.declaration_count == 1
                    && declaration.is_const
                    && declaration.statement_end <= at
                    && declaration.initializer.is_some()
                    && !rebound
            });
            return Some(valid.ok_or(*scope));
        }
        if file
            .dom_listener_parameter_bindings
            .get(scope)
            .is_some_and(|names| names.contains(name))
        {
            return Some(Err(*scope));
        }
    }
    None
}

pub(super) fn dom_receiver_from_indexed_expression(
    expression: &DomListenerReceiverExpression,
    file: &FileContext,
    scopes: &[usize],
    depth: usize,
) -> Option<&'static str> {
    if depth > 4 {
        return None;
    }
    match expression {
        DomListenerReceiverExpression::Identifier { name, position } => {
            if matches!(name.as_str(), "document" | "window")
                && !dom_global_shadowed_in_scopes(scopes, file, name)
            {
                return Some(if name == "document" {
                    "Document"
                } else {
                    "Window"
                });
            }
            match dom_indexed_const(file, name, *position, scopes)? {
                Ok(declaration) => dom_receiver_from_indexed_expression(
                    declaration.initializer.as_ref()?,
                    file,
                    &declaration.scope_chain,
                    depth + 1,
                ),
                Err(scope) => {
                    let rebound = file
                        .rebound_function_parameters
                        .get(&scope)
                        .is_some_and(|names| names.contains(name));
                    if rebound {
                        return None;
                    }
                    let ty = file.dom_listener_receivers.get(&scope)?.get(name)?;
                    typed_dom_receiver(ty)
                        .filter(|_| !dom_global_shadowed_in_scopes(scopes, file, ty))
                }
            }
        }
        DomListenerReceiverExpression::ThisField {
            name,
            class_scope,
            static_method,
        } => {
            if *static_method {
                return None;
            }
            let ty = file
                .dom_listener_class_fields
                .get(class_scope)?
                .get(name)?
                .as_deref()?;
            typed_dom_receiver(ty).filter(|_| !dom_global_shadowed_in_scopes(scopes, file, ty))
        }
        DomListenerReceiverExpression::MemberCall {
            receiver,
            method,
            type_arguments,
            has_arguments,
        } => {
            if !has_arguments {
                return None;
            }
            if !type_arguments.is_empty()
                && (type_arguments.len() != 1
                    || typed_dom_receiver(&type_arguments[0]) != Some("Element")
                    || dom_global_shadowed_in_scopes(scopes, file, &type_arguments[0]))
            {
                return None;
            }
            let owner = dom_receiver_from_indexed_expression(receiver, file, scopes, depth + 1)?;
            let produces_element = match owner {
                "Document" => {
                    matches!(
                        method.as_str(),
                        "getElementById" | "createElement" | "querySelector"
                    )
                }
                "Element" => matches!(method.as_str(), "querySelector" | "closest"),
                _ => false,
            };
            produces_element.then_some("Element")
        }
        DomListenerReceiverExpression::Parenthesized(inner) => {
            dom_receiver_from_indexed_expression(inner, file, scopes, depth + 1)
        }
        DomListenerReceiverExpression::Unsupported => None,
    }
}

pub(super) fn dom_const_initializer<'tree>(
    node: Syntax<'tree>,
    file: &FileContext,
    name: &str,
) -> Option<Result<Syntax<'tree>, Syntax<'tree>>> {
    let at = node.start_byte();
    let mut current = node.parent();
    while let Some(scope) = current {
        if matches!(scope.kind(), "statement_block" | "program" | "source_file") {
            let declaration = file
                .dom_listener_local_declarations
                .get(&scope.id())
                .and_then(|bindings| bindings.get(name));
            let rebound = file
                .dom_listener_local_writes
                .get(&scope.id())
                .and_then(|names| names.get(name))
                .is_some_and(|writes| writes.iter().any(|position| *position < at));
            if declaration.is_some() || rebound {
                return Some(
                    if declaration.is_some_and(|declaration| {
                        declaration.declaration_count == 1
                            && declaration.is_const
                            && declaration.statement_end <= at
                            && declaration.initializer.is_some()
                            && !rebound
                    }) {
                        Ok(scope)
                    } else {
                        Err(scope)
                    },
                );
            }
        }
        if file
            .dom_listener_parameter_bindings
            .get(&scope.id())
            .is_some_and(|names| names.contains(name))
        {
            return Some(Err(scope));
        }
        current = scope.parent();
    }
    None
}

pub(super) fn dom_receiver_from_ast(
    receiver: Syntax<'_>,
    source: &str,
    file: &FileContext,
    depth: usize,
) -> Option<&'static str> {
    if depth > 4 {
        return None;
    }
    match receiver.kind() {
        "identifier" => {
            let name = text(receiver, source);
            if matches!(name, "document" | "window") {
                return (!dom_global_shadowed(receiver, file, name)).then_some(
                    if name == "document" {
                        "Document"
                    } else {
                        "Window"
                    },
                );
            }
            match dom_const_initializer(receiver, file, name)? {
                Ok(scope) => {
                    let declaration = file
                        .dom_listener_local_declarations
                        .get(&scope.id())?
                        .get(name)?;
                    dom_receiver_from_indexed_expression(
                        declaration.initializer.as_ref()?,
                        file,
                        &declaration.scope_chain,
                        depth + 1,
                    )
                }
                Err(scope)
                    if matches!(
                        scope.kind(),
                        "function_declaration"
                            | "function_expression"
                            | "method_definition"
                            | "arrow_function"
                    ) =>
                {
                    if file
                        .rebound_function_parameters
                        .get(&scope.id())
                        .is_some_and(|names| names.contains(name))
                    {
                        return None;
                    }
                    let ty = file.dom_listener_receivers.get(&scope.id())?.get(name)?;
                    typed_dom_receiver(ty).filter(|_| !dom_global_shadowed(receiver, file, ty))
                }
                Err(_) => None,
            }
        }
        "member_expression" => {
            let object = receiver.child_by_field_name("object")?;
            let property = receiver
                .child_by_field_name("property")
                .filter(|property| property.kind() == "property_identifier")?;
            if object.kind() == "this" {
                let (method, class) = lexical_this_method(receiver)?;
                if (0..method.child_count()).any(|index| {
                    method
                        .child(index)
                        .is_some_and(|child| child.kind() == "static")
                }) {
                    return None;
                }
                let ty = file
                    .dom_listener_class_fields
                    .get(&class.id())?
                    .get(text(property, source))?
                    .as_deref()?;
                return typed_dom_receiver(ty).filter(|_| !dom_global_shadowed(receiver, file, ty));
            }
            None
        }
        "call_expression" => {
            let callee = receiver
                .child_by_field_name("function")
                .filter(|callee| callee.kind() == "member_expression")?;
            let object = callee.child_by_field_name("object")?;
            let method = callee
                .child_by_field_name("property")
                .filter(|property| property.kind() == "property_identifier")?;
            let arguments = receiver.child_by_field_name("arguments")?;
            if arguments.named_child_count() == 0 {
                return None;
            }
            if let Some(type_arguments) = receiver.child_by_field_name("type_arguments") {
                let ty = type_arguments
                    .named_child(0)
                    .filter(|_| type_arguments.named_child_count() == 1)?;
                let name = text(ty, source);
                if ty.kind() != "type_identifier"
                    || typed_dom_receiver(name) != Some("Element")
                    || dom_global_shadowed(receiver, file, name)
                {
                    return None;
                }
            }
            let owner = dom_receiver_from_ast(object, source, file, depth + 1)?;
            let method = text(method, source);
            let produces_element = match owner {
                "Document" => {
                    matches!(method, "getElementById" | "createElement" | "querySelector")
                }
                "Element" => matches!(method, "querySelector" | "closest"),
                _ => false,
            };
            produces_element.then_some("Element")
        }
        "parenthesized_expression" => receiver
            .named_child(0)
            .and_then(|inner| dom_receiver_from_ast(inner, source, file, depth + 1)),
        _ => None,
    }
}

pub(super) fn dom_event_callback_parameter<'a>(
    callback: Syntax<'_>,
    source: &'a str,
    file: &FileContext,
) -> Option<&'a str> {
    if let Some(args) = callback
        .parent()
        .filter(|parent| parent.kind() == "arguments")
    {
        if args.named_child(1) != Some(callback) || args.named_child_count() > 3 {
            return None;
        }
        let event = args
            .named_child(0)
            .filter(|event| event.kind() == "string")?;
        let literal = text(event, source);
        if literal.len() < 2
            || literal.contains('\\')
            || !matches!(literal.as_bytes()[0], b'\'' | b'"')
            || literal.as_bytes()[0] != *literal.as_bytes().last()?
        {
            return None;
        }
        let call = args
            .parent()
            .filter(|parent| parent.kind() == "call_expression")?;
        let callee = call
            .child_by_field_name("function")
            .filter(|callee| callee.kind() == "member_expression")?;
        let member = callee.child_by_field_name("property")?;
        if member.kind() != "property_identifier" || text(member, source) != "addEventListener" {
            return None;
        }
        let receiver = callee.child_by_field_name("object")?;
        let verified_receiver = matches!(
            dom_receiver_from_ast(receiver, source, file, 0),
            Some("Element" | "Document" | "Window" | "EventTarget")
        );
        if !verified_receiver
            || dom_global_shadowed(callback, file, "Event")
            || dom_global_shadowed(callback, file, "EventTarget")
        {
            return None;
        }
    } else {
        let assignment = callback
            .parent()
            .filter(|p| p.kind() == "assignment_expression")?;
        if assignment.child_by_field_name("right") != Some(callback) {
            return None;
        }
        let left = assignment
            .child_by_field_name("left")
            .filter(|left| left.kind() == "member_expression")?;
        let property = left.child_by_field_name("property")?;
        let prop_name = text(property, source);
        if !prop_name.starts_with("on") || prop_name.len() <= 2 {
            return None;
        }
        let receiver = left.child_by_field_name("object")?;
        let verified_receiver = matches!(
            dom_receiver_from_ast(receiver, source, file, 0),
            Some("Element" | "Document" | "Window" | "EventTarget")
        );
        if !verified_receiver || dom_global_shadowed(callback, file, "Event") {
            return None;
        }
    }
    first_callback_parameter(callback).map(|parameter| text(parameter, source))
}

pub(super) fn configured_callback_parameter<'a>(
    callback: Syntax<'_>,
    source: &'a str,
    callees: &[String],
) -> Option<&'a str> {
    let call = callback
        .parent()
        .filter(|p| p.kind() == "arguments")?
        .parent()?;
    if call.kind() != "call_expression" {
        return None;
    }
    let callee = call.child_by_field_name("function")?;
    if callee.kind() != "identifier"
        || !callees
            .iter()
            .any(|configured| text(callee, source) == configured)
    {
        return None;
    }
    first_callback_parameter(callback).map(|parameter| text(parameter, source))
}

pub(in crate::engine::languages) fn first_callback_parameter<'tree>(
    callback: Syntax<'tree>,
) -> Option<Syntax<'tree>> {
    let parameter = callback
        .child_by_field_name("parameter")
        .or_else(|| callback.child_by_field_name("parameters")?.named_child(0))?;
    let parameter = if parameter.kind() == "identifier" {
        parameter
    } else {
        parameter
            .child_by_field_name("name")
            .or_else(|| parameter.child_by_field_name("pattern"))?
    };
    (parameter.kind() == "identifier").then_some(parameter)
}
