use crate::core::models::Facts;
use crate::core::semantic::{SourcePosition, TemplateContextFact};
use crate::engine::ast::{field, text};
use crate::engine::languages::manifests::syntax::{SyntaxOperation, SyntaxRule};
use crate::engine::languages::LanguageContext;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tree_sitter::Node;

#[derive(Default)]
struct PreparedSyntaxContext {
    language_context: Arc<LanguageContext>,
    render_candidates: HashMap<String, Vec<usize>>,
    loader_candidates: HashMap<String, Vec<usize>>,
    named_route_candidates: HashMap<String, Vec<usize>>,
    receiver_registration_candidates: HashMap<usize, HashMap<String, usize>>,
    receiver_registration_conflicts: HashMap<usize, HashSet<String>>,
    receiver_fact_node_cursor: usize,
    receiver_fact_reference_cursor: usize,
    receiver_class_ids: HashSet<String>,
    receiver_imported_class_counts: HashMap<String, usize>,
}

thread_local! {
    static PREPARED_SYNTAX_CONTEXT: RefCell<Option<PreparedSyntaxContext>> = const { RefCell::new(None) };
}

pub(super) fn prepare_file(root: Node<'_>, source: &str, language_context: &Arc<LanguageContext>) {
    let rules = &language_context.syntax_rules;
    let imports = module_imports(root, source);
    let candidates = |indices: &[usize]| {
        super::super::manifests::semantic::imported_rule_candidates(rules, indices, &imports)
    };
    let compiled = language_context.prepared_projections("python");
    let (render_candidates, loader_candidates, named_route_candidates) = compiled
        .map(|prepared| {
            (
                candidates(&prepared.render_rules),
                candidates(&prepared.loader_rules),
                candidates(&prepared.named_route_rules),
            )
        })
        .unwrap_or_default();
    let mut receiver_registration_candidates: HashMap<usize, HashMap<String, usize>> =
        HashMap::new();
    let mut receiver_registration_conflicts: HashMap<usize, HashSet<String>> = HashMap::new();
    if !imports.is_empty() {
        let mut raw_registrations = HashMap::new();
        let mut module_proof = None;
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
                let raw = raw_registrations
                    .entry((member.as_str(), *path_argument, name_keyword.as_str()))
                    .or_insert_with(|| {
                        collect_receiver_registrations(
                            root,
                            source,
                            member,
                            *path_argument,
                            name_keyword,
                        )
                    });
                let (candidates, conflicts) = prepare_receiver_registrations(
                    root,
                    source,
                    module,
                    constructors,
                    *path_argument,
                    *handler_argument,
                    name_keyword,
                    raw,
                    &imports,
                    &mut module_proof,
                );
                for candidate in candidates {
                    receiver_registration_candidates
                        .entry(candidate)
                        .or_default()
                        .insert(rule.key(), *handler_argument);
                }
                for id in conflicts {
                    receiver_registration_conflicts
                        .entry(id)
                        .or_default()
                        .insert(rule.key());
                }
            }
        }
    }
    PREPARED_SYNTAX_CONTEXT.with(|context| {
        *context.borrow_mut() = Some(PreparedSyntaxContext {
            language_context: Arc::clone(language_context),
            render_candidates,
            loader_candidates,
            named_route_candidates,
            receiver_registration_candidates,
            receiver_registration_conflicts,
            receiver_fact_node_cursor: 0,
            receiver_fact_reference_cursor: 0,
            receiver_class_ids: HashSet::new(),
            receiver_imported_class_counts: HashMap::new(),
        });
    });
}

pub(super) fn with_syntax_rules<R>(visit: impl FnOnce(&[SyntaxRule]) -> R) -> R {
    PREPARED_SYNTAX_CONTEXT.with(|context| {
        let context = context.borrow();
        visit(context.as_ref().map_or(&[], |context| {
            context.language_context.syntax_rules.as_slice()
        }))
    })
}

pub(super) fn for_each_loader_rule(alias: &str, mut visit: impl FnMut(&SyntaxRule)) {
    PREPARED_SYNTAX_CONTEXT.with(|context| {
        let context = context.borrow();
        if let Some(context) = context.as_ref() {
            if let Some(indices) = context.loader_candidates.get(alias) {
                for (_, rule) in super::super::manifests::semantic::indexed_rules(
                    &context.language_context.syntax_rules,
                    indices.iter().copied(),
                ) {
                    visit(rule);
                }
            }
        }
    });
}

pub(super) fn for_each_decorated_registration_rule(mut visit: impl FnMut(&SyntaxRule)) {
    PREPARED_SYNTAX_CONTEXT.with(|context| {
        let context = context.borrow();
        if let Some(context) = context.as_ref() {
            let indices = context
                .language_context
                .prepared_projections("python")
                .map_or(&[][..], |prepared| {
                    prepared.decorated_registration_rules.as_slice()
                });
            for (_, rule) in super::super::manifests::semantic::indexed_rules(
                &context.language_context.syntax_rules,
                indices.iter().copied(),
            ) {
                visit(rule);
            }
        }
    });
}

pub(super) fn clear_file_context() {
    PREPARED_SYNTAX_CONTEXT.with(|context| *context.borrow_mut() = None);
}

fn module_imports<'a>(
    root: Node<'_>,
    source: &'a str,
) -> HashMap<(&'a str, &'a str), Vec<(&'a str, usize)>> {
    let mut imports: HashMap<(&'a str, &'a str), Vec<(&'a str, usize)>> = HashMap::new();
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
            let entries = imports.entry((module, imported)).or_default();
            if let Some(existing) = entries
                .iter_mut()
                .find(|(existing_alias, _)| *existing_alias == alias)
            {
                existing.1 = existing.1.min(statement.end_byte());
            } else {
                entries.push((alias, statement.end_byte()));
            }
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
    imports: &HashMap<(&str, &str), Vec<(&str, usize)>>,
    alias: &str,
    before: usize,
    module: &str,
    constructors: &[String],
) -> bool {
    constructors.iter().any(|name| {
        alias == name
            && imports.get(&(module, alias)).is_some_and(|entries| {
                entries
                    .iter()
                    .any(|(imported_alias, end)| *imported_alias == alias && *end < before)
            })
    })
}

#[derive(Default)]
struct ReceiverRegistrations<'tree> {
    calls: Vec<Node<'tree>>,
    conflicts: HashSet<usize>,
}

struct ReceiverModuleProof {
    binding_counts: super::template_loaders::BindingWrites,
    router_initializers: HashMap<String, (usize, usize, String)>,
}

fn collect_receiver_registrations<'tree>(
    root: Node<'tree>,
    source: &str,
    member: &str,
    path_argument: usize,
    name_keyword: &str,
) -> ReceiverRegistrations<'tree> {
    if !source.contains(member) {
        return ReceiverRegistrations::default();
    }
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

    ReceiverRegistrations {
        calls: registrations,
        conflicts,
    }
}

#[allow(clippy::too_many_arguments)]
fn prepare_receiver_registrations(
    root: Node<'_>,
    source: &str,
    module: &str,
    constructors: &[String],
    path_argument: usize,
    handler_argument: usize,
    name_keyword: &str,
    registrations: &ReceiverRegistrations<'_>,
    imports: &HashMap<(&str, &str), Vec<(&str, usize)>>,
    module_proof: &mut Option<ReceiverModuleProof>,
) -> (HashSet<usize>, HashSet<usize>) {
    if registrations.calls.is_empty()
        || !constructors.iter().any(|constructor| {
            imports
                .get(&(module, constructor.as_str()))
                .is_some_and(|entries| {
                    entries
                        .iter()
                        .any(|(alias, _)| *alias == constructor.as_str())
                })
        })
    {
        return (HashSet::new(), registrations.conflicts.clone());
    }
    let ReceiverModuleProof {
        binding_counts,
        router_initializers,
    } = module_proof.get_or_insert_with(|| ReceiverModuleProof {
        binding_counts: super::template_loaders::binding_counts(root, source),
        router_initializers: module_router_initializers(root, source),
    });
    let mut candidates = HashSet::new();
    for &registration in &registrations.calls {
        let router = text(
            registration
                .child_by_field_name("function")
                .and_then(|callee| callee.child_by_field_name("object"))
                .expect("collected router registration has an identifier object"),
            source,
        );
        if binding_counts.count(router) != 1 {
            continue;
        }
        let Some((assignment_end, assignment_start, constructor)) = router_initializers.get(router)
        else {
            continue;
        };
        if *assignment_end >= registration.start_byte()
            || binding_counts.count(constructor) != 1
            || !router_constructor_imported(
                imports,
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
    (candidates, registrations.conflicts.clone())
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
            .is_some_and(|prepared| prepared.named_route_candidates.contains_key(alias))
    })
}

#[cfg(test)]
fn is_prepared_receiver_registration_candidate(call_id: usize) -> bool {
    PREPARED_SYNTAX_CONTEXT.with(|context| {
        context.borrow().as_ref().is_some_and(|prepared| {
            prepared
                .receiver_registration_candidates
                .contains_key(&call_id)
        })
    })
}

#[cfg(test)]
fn is_prepared_receiver_registration_conflict(call_id: usize) -> bool {
    PREPARED_SYNTAX_CONTEXT.with(|context| {
        context.borrow().as_ref().is_some_and(|prepared| {
            prepared
                .receiver_registration_conflicts
                .contains_key(&call_id)
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
    PREPARED_SYNTAX_CONTEXT
        .with(|context| extract_prepared(node, source, context.borrow().as_ref()?))
}

fn extract_prepared(
    node: Node<'_>,
    source: &str,
    prepared: &PreparedSyntaxContext,
) -> Option<TemplateContextFact> {
    if node.kind() != "call" {
        return None;
    }
    let callee = node.child_by_field_name("function")?;
    if callee.kind() != "identifier" {
        return None;
    }
    let alias = text(callee, source);
    let arguments = node.child_by_field_name("arguments")?;
    let candidates = prepared.render_candidates.get(alias)?;
    let root = std::cell::OnceCell::new();
    crate::engine::languages::manifests::semantic::project_context_key(
        &prepared.language_context.syntax_rules,
        candidates,
        arguments.named_child_count(),
        alias,
        SourcePosition {
            line: node.start_position().row + 1,
            column: node.start_position().column,
        },
        |index| {
            arguments.named_child(index).is_some_and(|context| {
                context.kind() == "dictionary" && context.named_child_count() <= 32
            })
        },
        |index| {
            arguments
                .named_child(index)
                .and_then(|target| super::template_loaders::literal(target, source))
                .map(str::to_owned)
        },
        |module, export| {
            root.get_or_init(|| lexical_import_owner(node, source, alias))
                .is_some_and(|root| {
                    super::template_loaders::imported(
                        root,
                        source,
                        alias,
                        module,
                        export,
                        node.start_byte(),
                    )
                })
        },
        |index| {
            let context = arguments.named_child(index)?;
            context
                .named_children(&mut context.walk())
                .map(|pair| {
                    if pair.kind() != "pair" {
                        return None;
                    }
                    super::template_loaders::literal(pair.child_by_field_name("key")?, source)
                        .map(str::to_owned)
                })
                .collect()
        },
    )
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
    PREPARED_SYNTAX_CONTEXT.with(|context| {
        let context = context.borrow();
        let Some(context) = context.as_ref() else {
            return;
        };
        let Some(callee) = node.child_by_field_name("function") else {
            return;
        };
        let indices = if callee.kind() == "identifier" {
            let Some(indices) = context.named_route_candidates.get(text(callee, source)) else {
                return;
            };
            indices
        } else if callee.kind() == "attribute" {
            let Some(member) = callee.child_by_field_name("attribute") else {
                return;
            };
            let Some(indices) = context
                .language_context
                .prepared_projections("python")
                .and_then(|prepared| prepared.named_route_members.get(text(member, source)))
            else {
                return;
            };
            indices
        } else {
            return;
        };
        for (_, rule) in super::super::manifests::semantic::indexed_rules(
            &context.language_context.syntax_rules,
            indices.iter().copied(),
        ) {
            super::super::manifests::semantic::execute_named_route_rule(
                rule,
                owner,
                SourcePosition {
                    line: node.start_position().row + offset + 1,
                    column: node.start_position().column,
                },
                |module, export, argument| named_route_key(node, source, module, export, argument),
                facts,
            );
        }
    });
}

fn named_route_key(
    node: Node<'_>,
    source: &str,
    module: &str,
    export: &str,
    argument: usize,
) -> Option<String> {
    if node.kind() != "call" {
        return None;
    }
    let (direct_alias, module_expression) = reverse_call_candidate(node, source, export)?;
    let arguments = node.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let first = arguments
        .named_children(&mut cursor)
        .filter(|argument| argument.kind() != "comment")
        .nth(argument)?;
    if matches!(
        first.kind(),
        "keyword_argument" | "list_splat" | "dictionary_splat"
    ) {
        return None;
    }
    let name = super::template_loaders::literal(first, source)?;
    let root = direct_alias
        .and_then(|alias| lexical_import_owner(node, source, alias))
        .or_else(|| {
            let expression = module_expression?;
            let binding = expression.split('.').next()?;
            lexical_import_owner(node, source, binding)
        });
    let root = root?;
    let imported = direct_alias.is_some_and(|alias| {
        super::template_loaders::imported(root, source, alias, module, export, node.start_byte())
    });
    let module_imported = module_expression.is_some_and(|expression| {
        let binding = expression.split('.').next().unwrap_or("");
        imported_module(root, source, binding, expression, node.start_byte(), module)
    });
    if !imported && !module_imported {
        return None;
    }
    Some(name.to_owned())
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
    let Some(indices) = context
        .prepared_projections("python")
        .and_then(|prepared| prepared.class_attributes.get(attribute))
    else {
        return;
    };
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
    let position = node.start_position();
    for (_, effective) in super::super::manifests::semantic::indexed_rules(
        &context.semantic_rules,
        indices.iter().copied(),
    ) {
        super::super::manifests::semantic::execute_class_attribute_rule(
            effective,
            "python",
            attribute,
            owner,
            SourcePosition {
                line: position.row + offset + 1,
                column: position.column,
            },
            text(value, source),
            || super::template_loaders::literal(value, source).map(str::to_owned),
            || static_symbol(value, source),
            facts,
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
    if node.kind() != "call" {
        return false;
    }
    let handler_argument = PREPARED_SYNTAX_CONTEXT.with(|context| {
        context.borrow().as_ref().and_then(|context| {
            if context
                .receiver_registration_conflicts
                .get(&node.id())
                .is_some_and(|guards| guards.contains(guard))
            {
                return None;
            }
            context
                .receiver_registration_candidates
                .get(&node.id())
                .and_then(|guards| guards.get(guard))
                .copied()
        })
    });
    let Some(handler_argument) = handler_argument else {
        return false;
    };
    if !super::module_scope(node) {
        return false;
    }
    let Some(handler) =
        positional_argument(node, handler_argument).filter(|value| value.kind() == "identifier")
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
    prepare_file(root, source, &crate::engine::languages::dependency_registry::DependencyRegistry::bundled_framework_rules().language_context_for_path("test.py"));
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
    prepare_file(root, source, &crate::engine::languages::dependency_registry::DependencyRegistry::bundled_framework_rules().language_context_for_path("test.py"));
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
    prepare_file(root, source, &crate::engine::languages::dependency_registry::DependencyRegistry::bundled_framework_rules().language_context_for_path("test.py"));
    let binding_counts = super::template_loaders::binding_counts(root, source);
    for name in ["DefaultRouter", "router", "First", "Nested", "missing"] {
        assert_eq!(
            binding_counts.count(name),
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
