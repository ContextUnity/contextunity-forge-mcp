use crate::core::models::{Facts, Reference};
use crate::core::semantic::{SourcePosition, TemplateContextFact};
use crate::engine::ast::{field, text};
use crate::engine::languages::manifests::syntax::{SyntaxOperation, SyntaxRule};
use crate::engine::languages::LanguageProfile;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

#[derive(Default)]
struct PreparedSyntaxContext {
    rules: Vec<SyntaxRule>,
    reverse_aliases: HashSet<String>,
    receiver_registration_candidates: HashMap<(usize, String), usize>,
    receiver_registration_conflicts: HashSet<(usize, String)>,
    receiver_fact_node_cursor: usize,
    receiver_fact_reference_cursor: usize,
    receiver_class_ids: HashSet<String>,
    receiver_imported_class_counts: HashMap<String, usize>,
}

thread_local! {
    static PREPARED_SYNTAX_CONTEXT: RefCell<Option<PreparedSyntaxContext>> = const { RefCell::new(None) };
}

pub(super) fn prepare_file(root: Node<'_>, source: &str, rules: &[SyntaxRule]) {
    let reverse_aliases = rules
        .iter()
        .flat_map(|rule| {
            let SyntaxOperation::NamedRouteCall { module, export, .. } = &rule.operation else {
                return HashSet::new();
            };
            imported_reverse_aliases(root, source, module, export)
        })
        .collect();
    let mut receiver_registration_candidates = HashMap::new();
    let mut receiver_registration_conflicts = HashSet::new();
    for rule in rules {
        if let SyntaxOperation::ReceiverRegistration {
            module,
            constructors,
            member,
            path_argument,
            handler_argument,
            name_keyword,
        } = &rule.operation
        {
            let (candidates, conflicts) = prepare_receiver_registrations(
                root,
                source,
                module,
                constructors,
                member,
                *path_argument,
                *handler_argument,
                name_keyword,
            );
            for candidate in candidates {
                receiver_registration_candidates.insert((candidate, rule.key()), *handler_argument);
            }
            receiver_registration_conflicts
                .extend(conflicts.into_iter().map(|id| (id, rule.key())));
        }
    }
    PREPARED_SYNTAX_CONTEXT.with(|context| {
        *context.borrow_mut() = Some(PreparedSyntaxContext {
            rules: rules.to_vec(),
            reverse_aliases,
            receiver_registration_candidates,
            receiver_registration_conflicts,
            receiver_fact_node_cursor: 0,
            receiver_fact_reference_cursor: 0,
            receiver_class_ids: HashSet::new(),
            receiver_imported_class_counts: HashMap::new(),
        });
    });
}

pub(super) fn syntax_rules() -> Vec<SyntaxRule> {
    PREPARED_SYNTAX_CONTEXT.with(|context| {
        context
            .borrow()
            .as_ref()
            .map(|context| context.rules.clone())
            .unwrap_or_default()
    })
}

pub(super) fn clear_file_context() {
    PREPARED_SYNTAX_CONTEXT.with(|context| *context.borrow_mut() = None);
}

fn imported_reverse_aliases(
    root: Node<'_>,
    source: &str,
    module: &str,
    export: &str,
) -> HashSet<String> {
    let mut aliases = HashSet::new();
    let mut cursor = root.walk();
    for statement in root.named_children(&mut cursor).filter(|statement| {
        statement.kind() == "import_from_statement"
            && field(*statement, source, "module_name") == Some(module)
    }) {
        let mut imports = statement.walk();
        for name in statement.children_by_field_name("name", &mut imports) {
            let imported = field(name, source, "name").unwrap_or_else(|| text(name, source));
            if imported == export {
                aliases.insert(field(name, source, "alias").unwrap_or(imported).to_owned());
            }
        }
    }
    aliases
}

#[derive(Default)]
struct ModuleBindingCounts {
    names: HashMap<String, usize>,
    wildcard_imports: usize,
}

impl ModuleBindingCounts {
    fn count(&self, name: &str) -> usize {
        self.names.get(name).copied().unwrap_or_default() + self.wildcard_imports
    }
}

fn module_binding_counts(root: Node<'_>, source: &str) -> ModuleBindingCounts {
    fn record(node: Node<'_>, source: &str, counts: &mut ModuleBindingCounts) {
        *counts
            .names
            .entry(text(node, source).to_owned())
            .or_default() += 1;
    }

    fn targets(node: Node<'_>, source: &str, counts: &mut ModuleBindingCounts) {
        match node.kind() {
            "identifier" => record(node, source, counts),
            "attribute" | "subscript" => {}
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    targets(child, source, counts);
                }
            }
        }
    }

    fn captures(node: Node<'_>, source: &str, counts: &mut ModuleBindingCounts) {
        match node.kind() {
            "identifier" => record(node, source, counts),
            "dotted_name" if node.named_child_count() == 1 => record(node, source, counts),
            "dict_pattern" => {
                let mut values = node.walk();
                for value in node.children_by_field_name("value", &mut values) {
                    captures(value, source, counts);
                }
                let mut rest = node.walk();
                for child in node.named_children(&mut rest) {
                    if child.kind() == "splat_pattern" {
                        captures(child, source, counts);
                    }
                }
            }
            "class_pattern" | "keyword_pattern" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor).skip(1) {
                    captures(child, source, counts);
                }
            }
            "case_pattern" | "list_pattern" | "tuple_pattern" | "union_pattern" | "as_pattern"
            | "splat_pattern" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    captures(child, source, counts);
                }
            }
            _ => {}
        }
    }

    fn visit(node: Node<'_>, source: &str, counts: &mut ModuleBindingCounts) {
        match node.kind() {
            "case_pattern" => captures(node, source, counts),
            "type_alias_statement" => {
                if let Some(name) = super::Python.symbol_name(node, source) {
                    *counts.names.entry(name.to_owned()).or_default() += 1;
                }
            }
            "assignment" | "annotated_assignment" | "augmented_assignment" | "for_statement" => {
                if let Some(left) = node.child_by_field_name("left") {
                    targets(left, source, counts);
                }
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    visit(child, source, counts);
                }
            }
            "named_expression" | "as_pattern" => {
                if let Some(target) = node
                    .child_by_field_name("name")
                    .or_else(|| node.child_by_field_name("alias"))
                {
                    targets(target, source, counts);
                }
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    visit(child, source, counts);
                }
            }
            "import_from_statement" | "import_statement" => {
                let mut imports = node.walk();
                for child in node.children_by_field_name("name", &mut imports) {
                    let imported =
                        field(child, source, "name").unwrap_or_else(|| text(child, source));
                    let bound = field(child, source, "alias").unwrap_or(imported);
                    *counts.names.entry(bound.to_owned()).or_default() += 1;
                }
                let mut wildcard = node.walk();
                if node
                    .named_children(&mut wildcard)
                    .any(|child| child.kind() == "wildcard_import")
                {
                    counts.wildcard_imports += 1;
                }
            }
            "function_definition" | "class_definition" => {
                if let Some(name) = field(node, source, "name") {
                    *counts.names.entry(name.to_owned()).or_default() += 1;
                }
            }
            "delete_statement" => {
                let mut cursor = node.walk();
                for target in node.named_children(&mut cursor) {
                    targets(target, source, counts);
                }
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    visit(child, source, counts);
                }
            }
        }
    }

    let mut counts = ModuleBindingCounts::default();
    visit(root, source, &mut counts);
    counts
}

fn module_imports(root: Node<'_>, source: &str) -> HashMap<(String, String, String), usize> {
    let mut imports: HashMap<(String, String, String), usize> = HashMap::new();
    let mut cursor = root.walk();
    for statement in root
        .named_children(&mut cursor)
        .filter(|statement| statement.kind() == "import_from_statement")
    {
        let Some(module) = field(statement, source, "module_name") else {
            continue;
        };
        let mut import_cursor = statement.walk();
        for name in statement.children_by_field_name("name", &mut import_cursor) {
            let imported = field(name, source, "name").unwrap_or_else(|| text(name, source));
            let alias = field(name, source, "alias").unwrap_or(imported);
            let key = (alias.to_owned(), module.to_owned(), imported.to_owned());
            imports
                .entry(key)
                .and_modify(|end| *end = (*end).min(statement.end_byte()))
                .or_insert(statement.end_byte());
        }
    }
    imports
}

fn module_router_initializers(
    root: Node<'_>,
    source: &str,
) -> HashMap<String, (usize, usize, String)> {
    let mut initializers = HashMap::new();
    let mut cursor = root.walk();
    for statement in root.named_children(&mut cursor) {
        let assignment = if statement.kind() == "expression_statement" {
            statement.named_child(0).unwrap_or(statement)
        } else {
            statement
        };
        if assignment.kind() != "assignment" {
            continue;
        }
        let Some(router) = assignment
            .child_by_field_name("left")
            .filter(|left| left.kind() == "identifier")
        else {
            continue;
        };
        let Some(constructor) = assignment
            .child_by_field_name("right")
            .filter(|right| right.kind() == "call")
            .and_then(|call| call.child_by_field_name("function"))
            .filter(|callee| callee.kind() == "identifier")
        else {
            continue;
        };
        initializers.insert(
            text(router, source).to_owned(),
            (
                assignment.end_byte(),
                assignment.start_byte(),
                text(constructor, source).to_owned(),
            ),
        );
    }
    initializers
}

fn router_constructor_imported(
    imports: &HashMap<(String, String, String), usize>,
    alias: &str,
    before: usize,
    module: &str,
    constructors: &[String],
) -> bool {
    constructors.iter().any(|name| {
        alias == name
            && imports
                .get(&(alias.to_owned(), module.to_owned(), name.to_string()))
                .is_some_and(|end| *end < before)
    })
}

fn binding_count(counts: &ModuleBindingCounts, name: &str) -> usize {
    counts.count(name)
}

#[allow(clippy::too_many_arguments)]
fn prepare_receiver_registrations(
    root: Node<'_>,
    source: &str,
    module: &str,
    constructors: &[String],
    member: &str,
    path_argument: usize,
    handler_argument: usize,
    name_keyword: &str,
) -> (HashSet<usize>, HashSet<usize>) {
    let mut by_prefix: std::collections::HashMap<(String, String), Vec<usize>> =
        std::collections::HashMap::new();
    let mut by_basename: std::collections::HashMap<(String, String), Vec<usize>> =
        std::collections::HashMap::new();
    let mut registrations = Vec::new();
    collect_module_router_registrations(
        root,
        source,
        &mut by_prefix,
        &mut by_basename,
        &mut registrations,
        (member, path_argument, name_keyword),
    );

    let mut conflicts = HashSet::new();
    for registrations in by_prefix
        .values()
        .chain(by_basename.values())
        .filter(|registrations| registrations.len() > 1)
    {
        conflicts.extend(registrations.iter().copied());
    }

    let binding_counts = module_binding_counts(root, source);
    let imports = module_imports(root, source);
    let router_initializers = module_router_initializers(root, source);
    let mut candidates = HashSet::new();
    for registration in registrations {
        let router = text(
            registration
                .child_by_field_name("function")
                .and_then(|callee| callee.child_by_field_name("object"))
                .expect("collected router registration has an identifier object"),
            source,
        );
        if binding_count(&binding_counts, router) != 1 {
            continue;
        }
        let Some((assignment_end, assignment_start, constructor)) = router_initializers.get(router)
        else {
            continue;
        };
        if *assignment_end >= registration.start_byte()
            || binding_count(&binding_counts, constructor) != 1
            || !router_constructor_imported(
                &imports,
                constructor,
                *assignment_start,
                module,
                constructors,
            )
        {
            continue;
        }
        if positional_argument(registration, path_argument)
            .and_then(|value| super::template_loaders::literal(value, source))
            .is_none()
            || !positional_argument(registration, handler_argument)
                .is_some_and(|handler| handler.kind() == "identifier")
            || router_basename(registration, source, name_keyword).is_none()
        {
            continue;
        }
        candidates.insert(registration.id());
    }
    (candidates, conflicts)
}

fn collect_module_router_registrations<'tree>(
    node: Node<'tree>,
    source: &str,
    by_prefix: &mut std::collections::HashMap<(String, String), Vec<usize>>,
    by_basename: &mut std::collections::HashMap<(String, String), Vec<usize>>,
    registrations: &mut Vec<Node<'tree>>,
    registration: (&str, usize, &str),
) {
    let (member, path_argument, name_keyword) = registration;
    if matches!(
        node.kind(),
        "function_definition" | "class_definition" | "lambda"
    ) {
        return;
    }
    if node.kind() == "call"
        && node.child_by_field_name("function").is_some_and(|callee| {
            callee.kind() == "attribute"
                && callee
                    .child_by_field_name("attribute")
                    .is_some_and(|attribute| text(attribute, source) == member)
                && callee
                    .child_by_field_name("object")
                    .is_some_and(|object| object.kind() == "identifier")
        })
    {
        registrations.push(node);
        let callee = node
            .child_by_field_name("function")
            .expect("checked callee");
        let router = callee
            .child_by_field_name("object")
            .map(|object| text(object, source).to_owned())
            .expect("checked router object");
        if let Some(prefix) = positional_argument(node, path_argument)
            .and_then(|value| super::template_loaders::literal(value, source))
        {
            by_prefix
                .entry((router.clone(), prefix.to_owned()))
                .or_default()
                .push(node.id());
        }
        if let Some(Some(basename)) = router_basename(node, source, name_keyword) {
            by_basename
                .entry((router, basename))
                .or_default()
                .push(node.id());
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_module_router_registrations(
            child,
            source,
            by_prefix,
            by_basename,
            registrations,
            registration,
        );
    }
}

fn has_prepared_reverse_alias(alias: &str) -> bool {
    PREPARED_SYNTAX_CONTEXT.with(|context| {
        context
            .borrow()
            .as_ref()
            .is_some_and(|prepared| prepared.reverse_aliases.contains(alias))
    })
}

#[cfg(test)]
fn is_prepared_receiver_registration_candidate(call_id: usize) -> bool {
    PREPARED_SYNTAX_CONTEXT.with(|context| {
        context.borrow().as_ref().is_some_and(|prepared| {
            prepared
                .receiver_registration_candidates
                .keys()
                .any(|(id, _)| *id == call_id)
        })
    })
}

#[cfg(test)]
fn is_prepared_receiver_registration_conflict(call_id: usize) -> bool {
    PREPARED_SYNTAX_CONTEXT.with(|context| {
        context.borrow().as_ref().is_some_and(|prepared| {
            prepared
                .receiver_registration_conflicts
                .iter()
                .any(|(id, _)| *id == call_id)
        })
    })
}

fn prepared_receiver_handler_is_class(
    symbol_id: Option<&str>,
    handler_name: &str,
    facts: &Facts,
) -> (bool, bool) {
    PREPARED_SYNTAX_CONTEXT.with(|context| {
        let mut context = context.borrow_mut();
        let Some(prepared) = context.as_mut() else {
            return (false, false);
        };
        for candidate in facts.nodes.iter().skip(prepared.receiver_fact_node_cursor) {
            if candidate.kind == "class" {
                prepared.receiver_class_ids.insert(candidate.id.clone());
            }
        }
        prepared.receiver_fact_node_cursor = facts.nodes.len();
        for reference in facts
            .references
            .iter()
            .skip(prepared.receiver_fact_reference_cursor)
        {
            if reference.kind == "imports"
                && reference.alias.as_deref().unwrap_or(&reference.expression)
                    == reference.expression
                && reference
                    .module
                    .as_deref()
                    .is_some_and(|module| !module.is_empty())
            {
                *prepared
                    .receiver_imported_class_counts
                    .entry(reference.expression.clone())
                    .or_default() += 1;
            }
        }
        prepared.receiver_fact_reference_cursor = facts.references.len();
        (
            symbol_id.is_some_and(|symbol_id| prepared.receiver_class_ids.contains(symbol_id)),
            prepared
                .receiver_imported_class_counts
                .get(handler_name)
                .copied()
                == Some(1),
        )
    })
}

pub(super) fn lexical_import_owner<'a>(
    node: Node<'a>,
    source: &str,
    alias: &str,
) -> Option<Node<'a>> {
    super::lexical_binding_owner(node, source, alias).filter(|scope| scope.kind() == "module")
}

pub(super) fn extract(node: Node<'_>, source: &str) -> Option<TemplateContextFact> {
    if node.kind() != "call" {
        return None;
    }
    let callee = node.child_by_field_name("function")?;
    if callee.kind() != "identifier" {
        return None;
    }
    let alias = text(callee, source);
    let arguments = node.child_by_field_name("arguments")?;
    let root = lexical_import_owner(node, source, alias)?;
    let rules = syntax_rules();
    let (rule, target_index, context_index) = rules.iter().find_map(|rule| {
        let SyntaxOperation::RenderContext {
            module,
            export,
            target_argument,
            context_argument,
            argument_count,
            ..
        } = &rule.operation
        else {
            return None;
        };
        (arguments.named_child_count() == *argument_count
            && super::template_loaders::imported(
                root,
                source,
                alias,
                module,
                export,
                node.start_byte(),
            ))
        .then_some((rule, *target_argument, *context_argument))
    })?;
    let target = arguments.named_child(target_index)?;
    let context = arguments.named_child(context_index)?;
    if context.kind() != "dictionary" {
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
                ch == '_'
                    || if index == 0 {
                        ch.is_alphabetic()
                    } else {
                        ch.is_alphanumeric()
                    }
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
        origin: Some(crate::core::semantic::ExtractionRuleOrigin {
            pack: rule.pack.clone(),
            rule: rule.id.clone(),
        }),
        import_alias: alias.to_owned(),
        target: target.to_owned(),
        target_path: None,
        keys,
        position: SourcePosition {
            line: start.row + 1,
            column: start.column,
        },
    })
}

fn route_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'))
}

fn imported_module(
    root: Node<'_>,
    source: &str,
    binding: &str,
    module_expression: &str,
    before: usize,
    expected_module: &str,
) -> bool {
    let implicit_dotted_imports = root
        .named_children(&mut root.walk())
        .filter(|statement| statement.kind() == "import_statement")
        .map(|statement| {
            let mut imports = statement.walk();
            statement
                .children_by_field_name("name", &mut imports)
                .filter(|name| {
                    let spelling = text(*name, source);
                    let imported = field(*name, source, "name")
                        .or_else(|| spelling.split_once(" as ").map(|(imported, _)| imported))
                        .unwrap_or(spelling);
                    let alias = field(*name, source, "alias")
                        .or_else(|| spelling.split_once(" as ").map(|(_, alias)| alias.trim()));
                    alias.is_none()
                        && imported.contains('.')
                        && imported.split('.').next() == Some(binding)
                })
                .count()
        })
        .sum::<usize>();
    if super::template_loaders::binding_count(root, source, binding) + implicit_dotted_imports != 1
    {
        return false;
    }
    let mut cursor = root.walk();
    let matches = root.named_children(&mut cursor).any(|statement| {
        if statement.end_byte() >= before {
            return false;
        }
        match statement.kind() {
            "import_statement" => {
                let mut imports = statement.walk();
                let matches = statement
                    .children_by_field_name("name", &mut imports)
                    .any(|name| {
                        let spelling = text(name, source);
                        let imported = field(name, source, "name")
                            .or_else(|| spelling.split_once(" as ").map(|(imported, _)| imported))
                            .unwrap_or(spelling);
                        let alias = field(name, source, "alias")
                            .or_else(|| spelling.split_once(" as ").map(|(_, alias)| alias.trim()));
                        let bound =
                            alias.unwrap_or_else(|| imported.split('.').next().unwrap_or(""));
                        let canonical_expression = if let Some(alias) = alias {
                            imported == expected_module && module_expression == alias
                        } else {
                            imported == expected_module && module_expression == expected_module
                        };
                        bound == binding && canonical_expression
                    });
                matches
            }
            "import_from_statement"
                if field(statement, source, "module_name")
                    == expected_module.rsplit_once('.').map(|(module, _)| module) =>
            {
                let mut imports = statement.walk();
                let matches = statement
                    .children_by_field_name("name", &mut imports)
                    .any(|name| {
                        let imported =
                            field(name, source, "name").unwrap_or_else(|| text(name, source));
                        let alias = field(name, source, "alias").unwrap_or(imported);
                        Some(imported) == expected_module.rsplit_once('.').map(|(_, export)| export)
                            && alias == binding
                            && module_expression == alias
                    });
                matches
            }
            _ => false,
        }
    });
    matches
}

pub(super) fn extract_reverse_route(
    node: Node<'_>,
    source: &str,
    owner: &str,
    offset: usize,
    facts: &mut Facts,
) {
    for rule in syntax_rules() {
        if let SyntaxOperation::NamedRouteCall {
            module,
            export,
            argument,
            expression_prefix,
        } = &rule.operation
        {
            extract_named_route(
                node,
                source,
                owner,
                offset,
                facts,
                module,
                export,
                *argument,
                expression_prefix,
                &rule,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn extract_named_route(
    node: Node<'_>,
    source: &str,
    owner: &str,
    offset: usize,
    facts: &mut Facts,
    module: &str,
    export: &str,
    argument: usize,
    expression_prefix: &str,
    rule: &SyntaxRule,
) {
    if node.kind() != "call" {
        return;
    }
    let Some((direct_alias, module_expression)) = reverse_call_candidate(node, source, export)
    else {
        return;
    };
    let root = direct_alias
        .and_then(|alias| lexical_import_owner(node, source, alias))
        .or_else(|| {
            let expression = module_expression?;
            let binding = expression.split('.').next()?;
            lexical_import_owner(node, source, binding)
        });
    let Some(root) = root else {
        return;
    };
    let imported = direct_alias.is_some_and(|alias| {
        super::template_loaders::imported(root, source, alias, module, export, node.start_byte())
    });
    let module_imported = module_expression.is_some_and(|expression| {
        let binding = expression.split('.').next().unwrap_or("");
        imported_module(root, source, binding, expression, node.start_byte(), module)
    });
    if !imported && !module_imported {
        return;
    }
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return;
    };
    let mut cursor = arguments.walk();
    let Some(first) = arguments
        .named_children(&mut cursor)
        .filter(|argument| argument.kind() != "comment")
        .nth(argument)
    else {
        return;
    };
    if matches!(
        first.kind(),
        "keyword_argument" | "list_splat" | "dictionary_splat"
    ) {
        return;
    }
    let Some(name) =
        super::template_loaders::literal(first, source).filter(|name| route_name(name))
    else {
        return;
    };
    let position = node.start_position();
    facts.references.push(Reference {
        source: owner.to_owned(),
        dynamic: false,
        expression: format!("{expression_prefix}{name}"),
        kind: "references_route".into(),
        line: position.row + offset + 1,
        column: position.column,
        alias: None,
        module: None,
        receiver_hint: Some(crate::core::models::ReceiverHint::NamedRoute {
            origin: crate::core::semantic::ExtractionRuleOrigin {
                pack: rule.pack.clone(),
                rule: rule.id.clone(),
            },
            key: name.to_owned(),
        }),
    });
}

fn reverse_call_candidate<'source>(
    node: Node<'_>,
    source: &'source str,
    export: &str,
) -> Option<(Option<&'source str>, Option<&'source str>)> {
    let callee = node.child_by_field_name("function")?;
    match callee.kind() {
        "identifier" => {
            let name = text(callee, source);
            (name == export || has_prepared_reverse_alias(name)).then_some((Some(name), None))
        }
        "attribute" => {
            if !callee
                .child_by_field_name("attribute")
                .is_some_and(|attribute| text(attribute, source) == export)
            {
                return None;
            }
            let object = callee.child_by_field_name("object")?;
            Some((None, Some(text(object, source))))
        }
        _ => None,
    }
}

fn static_symbol(node: Node<'_>, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" => Some(text(node, source).to_owned()),
        "attribute" => {
            let object = static_symbol(node.child_by_field_name("object")?, source)?;
            let member = node.child_by_field_name("attribute")?;
            (member.kind() == "identifier").then(|| format!("{object}.{}", text(member, source)))
        }
        _ => None,
    }
}

pub(super) fn extract_class_attribute(
    node: Node<'_>,
    source: &str,
    owner: &str,
    offset: usize,
    facts: &mut Facts,
    context: &super::super::LanguageContext,
) {
    if !matches!(node.kind(), "assignment" | "annotated_assignment") {
        return;
    }
    let statement = if node
        .parent()
        .is_some_and(|parent| parent.kind() == "expression_statement")
    {
        node.parent().expect("checked parent")
    } else {
        node
    };
    let is_class_body = statement
        .parent()
        .filter(|block| block.kind() == "block")
        .and_then(|block| block.parent())
        .is_some_and(|class| class.kind() == "class_definition");
    if !is_class_body {
        return;
    }
    let (Some(left), Some(value)) = (
        node.child_by_field_name("left"),
        node.child_by_field_name("right"),
    ) else {
        return;
    };
    if left.kind() != "identifier" {
        return;
    }
    let attribute = text(left, source);
    let position = node.start_position();
    for effective in context.semantic_rules.iter().filter(|rule| rule.active) {
        let (expression, identity_path, role) = match &effective.rule.operation {
            super::super::manifests::SemanticOperation::ClassAttributeToFile {
                language,
                attribute: expected,
                target: _,
                relation,
            } if language == "python" && expected == attribute => {
                let literal = super::template_loaders::literal(value, source)
                    .filter(|value| super::super::manifests::semantic::normal_relative_path(value));
                (
                    text(value, source).to_owned(),
                    literal.unwrap_or("").to_owned(),
                    super::super::semantic::SemanticFactRole::ClassAttributeToFile {
                        attribute: attribute.to_owned(),
                        relation: *relation,
                    },
                )
            }
            super::super::manifests::SemanticOperation::ClassAttributeToSymbol {
                language,
                attribute: expected,
                target: _,
                relation,
            } if language == "python" && expected == attribute => {
                let expression = static_symbol(value, source).unwrap_or_default();
                (
                    text(value, source).to_owned(),
                    expression,
                    super::super::semantic::SemanticFactRole::ClassAttributeToSymbol {
                        attribute: attribute.to_owned(),
                        relation: *relation,
                    },
                )
            }
            _ => continue,
        };
        super::super::semantic::push(
            facts,
            effective.fact(
                owner,
                position.row + offset + 1,
                position.column,
                identity_path,
                expression,
                role,
            ),
        );
    }
}

fn positional_argument<'tree>(node: Node<'tree>, index: usize) -> Option<Node<'tree>> {
    let arguments = node.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let result = arguments
        .named_children(&mut cursor)
        .filter(|argument| !matches!(argument.kind(), "comment" | "keyword_argument"))
        .nth(index);
    result
}

fn router_basename(node: Node<'_>, source: &str, name_keyword: &str) -> Option<Option<String>> {
    let arguments = node.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let keyword = arguments.named_children(&mut cursor).find(|argument| {
        argument.kind() == "keyword_argument"
            && argument
                .child_by_field_name("name")
                .is_some_and(|name| text(name, source) == name_keyword)
    });
    match keyword {
        None => Some(None),
        Some(keyword) => {
            super::template_loaders::literal(keyword.child_by_field_name("value")?, source)
                .map(|name| Some(name.to_owned()))
        }
    }
}

pub(super) fn is_receiver_registration(
    node: Node<'_>,
    source: &str,
    symbols: &std::collections::HashMap<usize, String>,
    facts: &Facts,
    guard: &str,
) -> bool {
    if node.kind() != "call"
        || !super::module_scope(node)
        || PREPARED_SYNTAX_CONTEXT.with(|context| {
            context.borrow().as_ref().is_none_or(|context| {
                context
                    .receiver_registration_conflicts
                    .contains(&(node.id(), guard.to_owned()))
            })
        })
    {
        return false;
    }
    let handler_argument = PREPARED_SYNTAX_CONTEXT.with(|context| {
        context.borrow().as_ref().and_then(|context| {
            context
                .receiver_registration_candidates
                .get(&(node.id(), guard.to_owned()))
                .copied()
        })
    });
    let Some(handler) = handler_argument
        .and_then(|index| positional_argument(node, index))
        .filter(|value| value.kind() == "identifier")
    else {
        return false;
    };
    let handler_name = text(handler, source);
    let (local_class, imported_class) = prepared_receiver_handler_is_class(
        symbols.get(&handler.id()).map(String::as_str),
        handler_name,
        facts,
    );
    if !local_class && !imported_class {
        return false;
    }
    true
}

#[cfg(test)]
#[test]
fn reverse_call_candidate_rejects_unrelated_identifier_calls() {
    let source =
        "len(items)\nprint(items)\nreverse('home')\nurls.reverse('home')\nlogger.info(items)\n";
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&super::language()).unwrap();
    let tree = parser.parse(source, None).unwrap();
    let root = tree.root_node();
    prepare_file(root, source, &crate::engine::languages::dependency_registry::DependencyRegistry::bundled_framework_rules().language_context_for_path("test.py").syntax_rules);
    let mut cursor = root.walk();
    let calls = root
        .named_children(&mut cursor)
        .filter_map(|statement| statement.named_child(0))
        .collect::<Vec<_>>();

    assert_eq!(calls.len(), 5);
    assert_eq!(reverse_call_candidate(calls[0], source, "reverse"), None);
    assert_eq!(reverse_call_candidate(calls[1], source, "reverse"), None);
    let mut facts = crate::core::models::Facts::default();
    extract_reverse_route(calls[0], source, "consumer", 0, &mut facts);
    extract_reverse_route(calls[1], source, "consumer", 0, &mut facts);
    assert!(facts.references.is_empty());
    assert_eq!(
        reverse_call_candidate(calls[2], source, "reverse"),
        Some((Some("reverse"), None))
    );
    assert_eq!(
        reverse_call_candidate(calls[3], source, "reverse"),
        Some((None, Some("urls")))
    );
    assert_eq!(reverse_call_candidate(calls[4], source, "reverse"), None);
    clear_file_context();
}

#[cfg(test)]
#[test]
fn imported_reverse_alias_remains_a_candidate() {
    let source = "from django.urls import reverse as route_reverse\nroute_reverse('home')\n";
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&super::language()).unwrap();
    let tree = parser.parse(source, None).unwrap();
    let root = tree.root_node();
    prepare_file(root, source, &crate::engine::languages::dependency_registry::DependencyRegistry::bundled_framework_rules().language_context_for_path("test.py").syntax_rules);
    let call = root.named_child(1).unwrap().named_child(0).unwrap();
    let mut facts = crate::core::models::Facts::default();

    extract_reverse_route(call, source, "consumer", 0, &mut facts);

    assert_eq!(facts.references.len(), 1);
    assert_eq!(facts.references[0].expression, "django.route_name.home");
    clear_file_context();
}

#[cfg(test)]
#[test]
fn receiver_registration_conflicts_are_prepared_once_per_module_scope() {
    let source = "from rest_framework.routers import DefaultRouter\nrouter = DefaultRouter()\nrouter.register('duplicate', First, basename='first')\nrouter.register('duplicate', Second, basename='second')\nrouter.register('unique', Third, basename='third')\ndef nested():\n    router.register('duplicate', Nested, basename='nested')\n";
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&super::language()).unwrap();
    let tree = parser.parse(source, None).unwrap();
    let root = tree.root_node();
    prepare_file(root, source, &crate::engine::languages::dependency_registry::DependencyRegistry::bundled_framework_rules().language_context_for_path("test.py").syntax_rules);
    let binding_counts = module_binding_counts(root, source);
    for name in ["DefaultRouter", "router", "First", "Nested", "missing"] {
        assert_eq!(
            binding_count(&binding_counts, name),
            super::template_loaders::binding_count(root, source, name),
            "binding count for {name}"
        );
    }

    let mut ids_by_line = std::collections::HashMap::new();
    let mut cursor = root.walk();
    for node in root.named_children(&mut cursor) {
        collect_registration_lines(node, source, &mut ids_by_line);
    }
    assert!(is_prepared_receiver_registration_candidate(ids_by_line[&3]));
    assert!(is_prepared_receiver_registration_candidate(ids_by_line[&4]));
    assert!(is_prepared_receiver_registration_candidate(ids_by_line[&5]));
    assert!(!is_prepared_receiver_registration_candidate(
        ids_by_line[&7]
    ));
    assert!(is_prepared_receiver_registration_conflict(ids_by_line[&3]));
    assert!(is_prepared_receiver_registration_conflict(ids_by_line[&4]));
    assert!(!is_prepared_receiver_registration_conflict(ids_by_line[&5]));
    assert!(!is_prepared_receiver_registration_conflict(ids_by_line[&7]));
    clear_file_context();
}

#[cfg(test)]
fn collect_registration_lines(
    node: Node<'_>,
    source: &str,
    output: &mut std::collections::HashMap<usize, usize>,
) {
    if node.kind() == "call"
        && node
            .child_by_field_name("function")
            .is_some_and(|callee| text(callee, source) == "router.register")
    {
        output.insert(node.start_position().row + 1, node.id());
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_registration_lines(child, source, output);
    }
}
