//! Finite Python projections from existing syntax events and completed lexical proof.
use super::*;
use crate::core::semantic::{NamedBindingFact, SourcePosition, StaticString, SymbolOperand};
use crate::engine::ast::routes::RouteContext;
use manifests::projections::{Api, NamedBindingForm, Slot};
use manifests::semantic::PreparedRules;
use manifests::syntax::SyntaxOperation;
use std::cell::RefCell;

#[derive(Clone)]
struct Constructor {
    module: String,
    export: String,
    before: usize,
}

#[derive(Default)]
struct DirectImports {
    statements: HashMap<String, HashSet<usize>>,
    modules_by_head: HashMap<String, HashMap<String, HashSet<String>>>,
    import_mask: u64,
}

pub(super) fn imported_module_candidate(head: &str, module: &str) -> bool {
    DIRECT_IMPORTS.with(|imports| {
        imports
            .borrow()
            .modules_by_head
            .get(head)
            .is_some_and(|modules| modules.contains_key(module))
    })
}

fn imported_export_candidate(head: &str, module: &str, export: &str) -> bool {
    DIRECT_IMPORTS.with(|imports| {
        imports
            .borrow()
            .modules_by_head
            .get(head)
            .and_then(|modules| modules.get(module))
            .is_some_and(|exports| exports.contains(export))
    })
}

thread_local! {
    static CONSTRUCTORS: RefCell<HashMap<usize, HashMap<String, Constructor>>> = RefCell::new(HashMap::new());
    static DIRECT_IMPORTS: RefCell<DirectImports> = RefCell::new(DirectImports::default());
}

pub(super) fn clear() {
    CONSTRUCTORS.with(|constructors| constructors.borrow_mut().clear());
    DIRECT_IMPORTS.with(|imports| *imports.borrow_mut() = DirectImports::default());
}

fn direct_scope(node: Syntax<'_>) -> Option<usize> {
    let statement = node
        .parent()
        .filter(|parent| parent.kind() == "expression_statement")
        .unwrap_or(node);
    let body = statement.parent()?;
    if body.kind() == "module" {
        return Some(body.id());
    }
    let scope = body
        .parent()
        .filter(|scope| scope.kind() == "function_definition")?;
    (scope.child_by_field_name("body") == Some(body)).then(|| scope.id())
}

pub(super) fn observe_import(ctx: &SyntaxContext<'_, '_>) {
    if direct_scope(ctx.node).is_some() {
        DIRECT_IMPORTS.with(|imports| {
            let mut imports = imports.borrow_mut();
            imports
                .statements
                .entry(ctx.owner.to_owned())
                .or_default()
                .insert(position(ctx.node, ctx.offset).line);
            let mut cursor = ctx.node.walk();
            for name in ctx.node.children_by_field_name("name", &mut cursor) {
                let imported =
                    field(name, ctx.source, "name").unwrap_or_else(|| text(name, ctx.source));
                let head = field(name, ctx.source, "alias").unwrap_or_else(|| {
                    if ctx.node.kind() == "import_statement" {
                        imported.split('.').next().unwrap_or(imported)
                    } else {
                        imported
                    }
                });
                let module = if ctx.node.kind() == "import_statement" {
                    imported
                } else {
                    field(ctx.node, ctx.source, "module_name").unwrap_or("")
                };
                imports.import_mask |= crate::engine::linker::interner::package_mask(module);
                // Every accepted import proof must come from one of these events.
                // Membership only rejects unrelated head/module/export tuples; scope,
                // order, uniqueness and provider identity remain the full proof.
                imports
                    .modules_by_head
                    .entry(head.to_owned())
                    .or_default()
                    .entry(module.to_owned())
                    .or_default()
                    .insert(imported.to_owned());
            }
        });
    }
}

fn static_name(node: Syntax<'_>, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" => Some(text(node, source).to_owned()),
        "attribute" => {
            let object = node.child_by_field_name("object")?;
            let member = node.child_by_field_name("attribute")?;
            (object.kind() == "identifier" && member.kind() == "identifier")
                .then(|| format!("{}.{}", text(object, source), text(member, source)))
        }
        _ => None,
    }
}

fn position(node: Syntax<'_>, offset: usize) -> SourcePosition {
    SourcePosition {
        line: node.start_position().row + offset + 1,
        column: node.start_position().column,
    }
}

fn literal(node: Option<Syntax<'_>>, source: &str) -> StaticString {
    node.and_then(|node| super::template_loaders::literal(node, source))
        .filter(|value| {
            !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
        })
        .map_or(StaticString::Unknown, |value| StaticString::Literal {
            value: value.to_owned(),
        })
}

fn operand(node: Option<Syntax<'_>>, source: &str) -> SymbolOperand {
    node.and_then(|node| static_name(node, source))
        .map_or(SymbolOperand::Unknown, |expression| SymbolOperand::Name {
            expression,
        })
}

fn keyword<'tree>(
    call: Syntax<'tree>,
    source: &str,
    name: &str,
) -> Result<Option<Syntax<'tree>>, ()> {
    let Some(arguments) = call.child_by_field_name("arguments") else {
        return Ok(None);
    };
    let mut cursor = arguments.walk();
    let mut found = None;
    for argument in arguments.named_children(&mut cursor) {
        if argument.kind() == "dictionary_splat" {
            return Err(());
        }
        if argument.kind() == "keyword_argument" && field(argument, source, "name") == Some(name) {
            if found.is_some() {
                return Err(());
            }
            found = argument.child_by_field_name("value");
            if found.is_none() {
                return Err(());
            }
        }
    }
    Ok(found)
}

fn slot<'tree>(
    call: Syntax<'tree>,
    source: &str,
    slot: &Slot,
) -> Result<Option<Syntax<'tree>>, ()> {
    match slot {
        Slot::Argument { index } => {
            let Some(arguments) = call.child_by_field_name("arguments") else {
                return Ok(None);
            };
            let mut cursor = arguments.walk();
            let mut values = Vec::new();
            for argument in arguments.named_children(&mut cursor) {
                if matches!(argument.kind(), "list_splat" | "dictionary_splat") {
                    return Err(());
                }
                if !matches!(argument.kind(), "keyword_argument" | "comment") {
                    values.push(argument);
                }
            }
            Ok(values.get(*index).copied())
        }
        Slot::Keyword { name } => keyword(call, source, name),
        Slot::KeywordMember { name, member } => {
            let Some(dictionary) = keyword(call, source, name)? else {
                return Ok(None);
            };
            if dictionary.kind() != "dictionary" {
                return Err(());
            }
            let mut cursor = dictionary.walk();
            let mut found = None;
            for pair in dictionary.named_children(&mut cursor) {
                if pair.kind() == "comment" {
                    continue;
                }
                if pair.kind() != "pair" {
                    return Err(());
                }
                let StaticString::Literal { value } =
                    literal(pair.child_by_field_name("key"), source)
                else {
                    return Err(());
                };
                if value == *member {
                    if found.is_some() {
                        return Err(());
                    }
                    found = pair.child_by_field_name("value");
                    if found.is_none() {
                        return Err(());
                    }
                }
            }
            Ok(found)
        }
    }
}

struct ImportProofEntry<'rules> {
    call: usize,
    module: &'rules str,
    export: &'rules str,
    proven: bool,
}

#[derive(Default)]
struct ImportProofMemo<'rules> {
    first: Option<ImportProofEntry<'rules>>,
    rest: Vec<ImportProofEntry<'rules>>,
}

impl<'rules> ImportProofMemo<'rules> {
    fn get(&self, call: usize, module: &str, export: &str) -> Option<bool> {
        self.first
            .iter()
            .chain(&self.rest)
            .find(|entry| entry.call == call && entry.module == module && entry.export == export)
            .map(|entry| entry.proven)
    }

    fn insert(&mut self, call: usize, module: &'rules str, export: &'rules str, proven: bool) {
        let entry = ImportProofEntry {
            call,
            module,
            export,
            proven,
        };
        if self.first.is_none() {
            self.first = Some(entry);
        } else {
            self.rest.push(entry);
        }
    }
}

fn api_candidate(call: Syntax<'_>, source: &str, api: &Api) -> bool {
    let Some(function) = call.child_by_field_name("function") else {
        return false;
    };
    match api {
        Api::Imported { module, export } => {
            let (head, imported_expression) = match function.kind() {
                "identifier" => (function, export.as_str()),
                "attribute" if field(function, source, "attribute") == Some(export.as_str()) => {
                    let Some(object) = function
                        .child_by_field_name("object")
                        .filter(|node| node.kind() == "identifier")
                    else {
                        return false;
                    };
                    (object, module.as_str())
                }
                _ => return false,
            };
            imported_export_candidate(text(head, source), module, imported_expression)
        }
        Api::ConstructedMember { member, .. } => {
            function.kind() == "attribute"
                && field(function, source, "attribute") == Some(member.as_str())
        }
        Api::Global { .. } | Api::GlobalMember { .. } => false,
    }
}

struct Proof<'a, 'rules> {
    source: &'a str,
    offset: usize,
    facts: &'a Facts,
    reference_frontier: usize,
    symbols: &'a HashMap<usize, String>,
    lexical: &'a RouteContext,
    imports: RefCell<ImportProofMemo<'rules>>,
}

impl<'rules> Proof<'_, 'rules> {
    fn imported(&self, call: Syntax<'_>, module: &'rules str, export: &'rules str) -> bool {
        let required_mask = crate::engine::linker::interner::package_mask(module);
        if required_mask != 0
            && !DIRECT_IMPORTS
                .with(|imports| imports.borrow().import_mask & required_mask == required_mask)
        {
            return false;
        }
        let Some(function) = call.child_by_field_name("function") else {
            return false;
        };
        if function.kind() == "attribute"
            && field(function, self.source, "attribute") != Some(export)
        {
            return false;
        }
        let Some(expression) = static_name(function, self.source) else {
            return false;
        };
        let head = expression.split('.').next().unwrap_or_default();
        let imported_expression = if expression == head {
            export
        } else {
            if expression
                .strip_prefix(head)
                .and_then(|tail| tail.strip_prefix('.'))
                != Some(export)
            {
                return false;
            }
            module
        };
        if !imported_export_candidate(head, module, imported_expression) {
            return false;
        }
        let before = position(call, self.offset).line;
        if let Some(proven) = self.imports.borrow().get(call.id(), module, export) {
            return proven;
        }
        let proven = (|| {
            let mut candidates = self
                .facts
                .references
                .iter()
                .take(self.reference_frontier)
                .filter(|reference| {
                    reference.kind == "imports"
                        && reference.module.as_deref() == Some(module)
                        && reference.expression == imported_expression
                        && reference.alias.as_deref().unwrap_or(&reference.expression) == head
                        && reference.line <= before
                        && !matches!(
                            reference.receiver_hint,
                            Some(crate::core::models::ReceiverHint::TypeOnlyImport)
                        )
                })
                .peekable();
            if candidates.peek().is_none() {
                return false;
            }
            let Some(scope) = self.lexical.lexical_scope(call, head) else {
                return false;
            };
            let owner = scope.id();
            if self.lexical.lexical_write_count(owner, head) != 1 {
                return false;
            }
            let source_owner = self.symbols.get(&owner).map(String::as_str).or_else(|| {
                (scope.kind() == "module")
                    .then(|| {
                        self.facts
                            .nodes
                            .iter()
                            .find(|node| node.kind == "module")
                            .map(|node| node.id.as_str())
                    })
                    .flatten()
            });
            let Some(source_owner) = source_owner else {
                return false;
            };
            let mut matches = candidates.filter(|reference| {
                reference.source == source_owner
                    && DIRECT_IMPORTS.with(|imports| {
                        imports
                            .borrow()
                            .statements
                            .get(source_owner)
                            .is_some_and(|lines| lines.contains(&reference.line))
                    })
            });
            matches.next().is_some() && matches.next().is_none()
        })();
        self.imports
            .borrow_mut()
            .insert(call.id(), module, export, proven);
        proven
    }

    fn api(&self, call: Syntax<'_>, api: &'rules Api) -> bool {
        match api {
            Api::Imported { module, export } => self.imported(call, module, export),
            Api::ConstructedMember {
                module,
                constructors,
                member,
            } => {
                let Some(function) = call
                    .child_by_field_name("function")
                    .filter(|node| node.kind() == "attribute")
                else {
                    return false;
                };
                if field(function, self.source, "attribute") != Some(member.as_str()) {
                    return false;
                }
                let Some(receiver) = function
                    .child_by_field_name("object")
                    .filter(|node| node.kind() == "identifier")
                else {
                    return false;
                };
                let name = text(receiver, self.source);
                let Some(owner) = self.lexical.lexical_owner(call, name) else {
                    return false;
                };
                self.lexical.lexical_write_count(owner, name) == 1
                    && CONSTRUCTORS.with(|values| {
                        values
                            .borrow()
                            .get(&owner)
                            .and_then(|by_name| by_name.get(name))
                            .is_some_and(|value| {
                                value.module == *module
                                    && constructors.contains(&value.export)
                                    && value.before < call.start_byte()
                            })
                    })
            }
            Api::Global { .. } | Api::GlobalMember { .. } => false,
        }
    }
}

fn constructor_apis<'rules: 'indices, 'indices>(
    prepared: &'indices PreparedRules,
    rules: &'rules LanguageContext,
) -> impl Iterator<Item = &'rules Api> + 'indices {
    prepared
        .semantic_constructors
        .iter()
        .flat_map(move |&index| {
            manifests::semantic::semantic_rule_apis(&rules.semantic_rules[index].rule)
                .into_iter()
                .flatten()
        })
        .chain(
            prepared
                .syntax_constructors
                .iter()
                .filter_map(move |&index| {
                    manifests::semantic::syntax_rule_api(&rules.syntax_rules[index])
                        .and_then(Result::ok)
                }),
        )
}

fn record_constructor<'rules>(
    ctx: &SyntaxContext<'_, '_>,
    kind: &str,
    rules: &'rules LanguageContext,
    prepared: &PreparedRules,
    proof: &Proof<'_, 'rules>,
) {
    if prepared.semantic_constructors.is_empty() && prepared.syntax_constructors.is_empty() {
        return;
    }
    if kind != "assignment" {
        return;
    }
    let Some(left) = ctx
        .node
        .child_by_field_name("left")
        .filter(|node| node.kind() == "identifier")
    else {
        return;
    };
    let Some(call) = ctx
        .node
        .child_by_field_name("right")
        .filter(|node| node.kind() == "call")
    else {
        return;
    };
    let name = text(left, ctx.source);
    let mut assignment_owner = None;
    for api in constructor_apis(prepared, rules) {
        let Api::ConstructedMember {
            module,
            constructors,
            ..
        } = api
        else {
            continue;
        };
        for export in constructors {
            if proof.imported(call, module, export) {
                let owner = if let Some(owner) = assignment_owner {
                    owner
                } else {
                    let Some(owner) = proof.lexical.lexical_owner(ctx.node, name) else {
                        return;
                    };
                    if proof.lexical.lexical_write_count(owner, name) != 1
                        || direct_scope(ctx.node) != Some(owner)
                    {
                        return;
                    }
                    assignment_owner = Some(owner);
                    owner
                };
                CONSTRUCTORS.with(|values| {
                    values.borrow_mut().entry(owner).or_default().insert(
                        name.to_owned(),
                        Constructor {
                            module: module.clone(),
                            export: export.clone(),
                            before: ctx.node.end_byte(),
                        },
                    );
                });
            }
        }
    }
}

fn push<T>(fact_count: usize, fact: T, insert: impl FnOnce(T)) -> bool {
    let exhausted = fact_count >= 128;
    // Entry 129 makes the canonical decoder reject the entire overfull proof.
    if fact_count <= 128 {
        insert(fact);
    }
    exhausted
}

fn registry_assignments(
    ctx: &SyntaxContext<'_, '_>,
    kind: &str,
    facts: &Facts,
    rules: &LanguageContext,
    lexical: &RouteContext,
    prepared: &PreparedRules,
) -> Vec<NamedBindingFact> {
    if prepared.registry_definitions.is_empty() {
        return Vec::new();
    }
    if !matches!(
        kind,
        "assignment" | "augmented_assignment" | "delete_statement"
    ) {
        return Vec::new();
    }
    let Some(left) = ctx.node.child_by_field_name("left").or_else(|| {
        (kind == "delete_statement")
            .then(|| ctx.node.named_child(0))
            .flatten()
    }) else {
        return Vec::new();
    };
    // Only unpack the current statement's deletion target list; do not revisit
    // a body or traverse receiver/key expressions as possible registrations.
    let mut pending = vec![left];
    let mut named = Vec::new();
    while let Some(left) = pending.pop() {
        if matches!(
            left.kind(),
            "expression_list" | "tuple" | "list" | "parenthesized_expression"
        ) {
            let mut cursor = left.walk();
            pending.extend(left.named_children(&mut cursor));
        } else {
            named.extend(registry_assignment(
                ctx, kind, left, facts, rules, lexical, prepared,
            ));
        }
    }
    named
}

fn registry_assignment(
    ctx: &SyntaxContext<'_, '_>,
    kind: &str,
    left: Syntax<'_>,
    facts: &Facts,
    rules: &LanguageContext,
    lexical: &RouteContext,
    prepared: &PreparedRules,
) -> Vec<NamedBindingFact> {
    let (registry, key) = if left.kind() == "subscript" {
        (
            left.child_by_field_name("value"),
            literal(left.child_by_field_name("subscript"), ctx.source),
        )
    } else {
        (Some(left), StaticString::Unknown)
    };
    let Some(registry) = registry.filter(|node| node.kind() == "attribute") else {
        return Vec::new();
    };
    if !prepared.registry_definitions.iter().any(|&index| {
        matches!(&rules.syntax_rules[index].operation,
            SyntaxOperation::NamedBinding { binding: NamedBindingForm::AssignmentDefinition { member, .. }, .. }
            if field(registry, ctx.source, "attribute") == Some(member.as_str()))
    }) { return Vec::new(); }
    let Some(receiver) = registry
        .child_by_field_name("object")
        .filter(|node| node.kind() == "identifier")
    else {
        return Vec::new();
    };
    let name = text(receiver, ctx.source);
    // Python's completed write summary counts the body, not parameter
    // declarations. A typed parameter is unique only with zero body writes.
    let parameter = facts
        .nodes
        .iter()
        .find(|node| node.id == ctx.owner)
        .is_some_and(|node| node.has_parameter(name));
    let unique_receiver = lexical
        .lexical_owner(ctx.node, name)
        .is_some_and(|owner| lexical.lexical_write_count(owner, name) == usize::from(!parameter));
    let mut named = Vec::new();
    for &index in &prepared.registry_definitions {
        let rule = &rules.syntax_rules[index];
        if let Some(fact) = manifests::semantic::execute_registry_rule(
            rule,
            position(ctx.node, ctx.offset),
            field(registry, ctx.source, "attribute"),
            key.clone(),
            || {
                if unique_receiver {
                    operand(Some(receiver), ctx.source)
                } else {
                    SymbolOperand::Unknown
                }
            },
            || {
                if kind == "assignment"
                    && left.kind() == "subscript"
                    && direct_scope(ctx.node).is_some()
                {
                    operand(ctx.node.child_by_field_name("right"), ctx.source)
                } else {
                    SymbolOperand::Unknown
                }
            },
        ) {
            named.push(fact);
        }
    }
    named
}

/// Necessary syntax/import candidates only; completed lexical proof stays in extract.
pub(super) fn candidate(ctx: &SyntaxContext<'_, '_>, rules: &LanguageContext) -> bool {
    let Some(prepared) = rules.prepared_projections("python") else {
        return false;
    };
    let (node, source) = (ctx.node, ctx.source);
    let semantic_candidate = |call, indices: &[usize]| {
        indices.iter().any(|&index| {
            manifests::semantic::semantic_rule_apis(&rules.semantic_rules[index].rule)[0]
                .is_some_and(|api| api_candidate(call, source, api))
        })
    };
    match node.kind() {
        "function_definition" => {
            !prepared.function_semantic.is_empty()
                && relations::decorators(node)
                    .into_iter()
                    .filter_map(|decorator| decorator.named_child(0))
                    .filter(|call| call.kind() == "call")
                    .any(|call| semantic_candidate(call, &prepared.function_semantic))
        }
        "call" => {
            semantic_candidate(node, &prepared.call_semantic)
                || prepared.named_references.iter().any(|&index| {
                    manifests::semantic::syntax_rule_api(&rules.syntax_rules[index])
                        .and_then(Result::ok)
                        .is_some_and(|api| api_candidate(node, source, api))
                })
        }
        "assignment" | "augmented_assignment" | "delete_statement" => {
            let left = node
                .child_by_field_name("left")
                .or_else(|| node.named_child(0));
            // Destructuring/delete lists retain the existing bounded current-target expansion.
            if !prepared.registry_definitions.is_empty()
                && left.is_some_and(|left| {
                    matches!(
                        left.kind(),
                        "subscript"
                            | "attribute"
                            | "expression_list"
                            | "tuple"
                            | "list"
                            | "parenthesized_expression"
                    )
                })
            {
                return true;
            }
            if node.kind() != "assignment"
                || !node
                    .child_by_field_name("left")
                    .is_some_and(|left| left.kind() == "identifier")
            {
                return false;
            }
            let Some(call) = node
                .child_by_field_name("right")
                .filter(|call| call.kind() == "call")
            else {
                return false;
            };
            let Some(function) = call.child_by_field_name("function") else {
                return false;
            };
            let head = match function.kind() {
                "identifier" => Some(function),
                "attribute" => function
                    .child_by_field_name("object")
                    .filter(|object| object.kind() == "identifier"),
                _ => None,
            };
            head.is_some_and(|head| constructor_apis(prepared, rules).any(|api| {
                matches!(api, Api::ConstructedMember { module, .. } if imported_module_candidate(text(head, source), module))
            }))
        }
        _ => false,
    }
}

pub(super) fn extract_with_frontier(
    ctx: &SyntaxContext<'_, '_>,
    facts: &mut Facts,
    rules: &LanguageContext,
    lexical: &RouteContext,
    symbols: &HashMap<usize, String>,
    reference_frontier: usize,
    projections: &mut HashMap<String, crate::core::semantic::ValueFlowFacts>,
) {
    let Some(prepared) = rules.prepared_projections("python") else {
        return;
    };
    let kind = ctx.node.kind();
    if !matches!(
        kind,
        "assignment" | "augmented_assignment" | "delete_statement" | "function_definition" | "call"
    ) {
        return;
    }
    let proof = Proof {
        source: ctx.source,
        offset: ctx.offset,
        facts,
        reference_frontier,
        symbols,
        lexical,
        imports: RefCell::new(ImportProofMemo::default()),
    };
    record_constructor(ctx, kind, rules, prepared, &proof);
    let mut attachments = Vec::new();
    let mut endpoints = Vec::new();
    let mut named = registry_assignments(ctx, kind, facts, rules, lexical, prepared);
    let semantic = match kind {
        "function_definition" => prepared.function_semantic.as_slice(),
        "call" => prepared.call_semantic.as_slice(),
        _ => &[],
    };
    let decorated_calls = if kind == "function_definition" && !semantic.is_empty() {
        relations::decorators(ctx.node)
            .into_iter()
            .filter_map(|decorator| {
                decorator
                    .named_child(0)
                    .filter(|node| node.kind() == "call")
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    for &index in semantic {
        let effective = &rules.semantic_rules[index];
        manifests::semantic::execute_semantic_rule(
            effective,
            ctx,
            "python",
            kind == "call",
            &decorated_calls,
            |call, api| proof.api(call, api),
            |call, selected| slot(call, ctx.source, selected),
            |call, name| keyword(call, ctx.source, name),
            |node| literal(node, ctx.source),
            |node| operand(node, ctx.source),
            || {
                symbols
                    .get(&ctx.node.id())
                    .map_or(SymbolOperand::Unknown, |id| SymbolOperand::Node {
                        id: id.clone(),
                    })
            },
            |call, attachment| {
                let Some(keyword_node) = call.parent().filter(|node| {
                    node.kind() == "keyword_argument"
                        && field(*node, ctx.source, "name") == Some(attachment.keyword.as_str())
                }) else {
                    return false;
                };
                let Some(parent) = keyword_node
                    .parent()
                    .and_then(|arguments| arguments.parent())
                    .filter(|node| node.kind() == "call")
                else {
                    return false;
                };
                keyword(parent, ctx.source, &attachment.keyword)
                    .ok()
                    .flatten()
                    == Some(call)
                    && proof.api(parent, &attachment.callee)
            },
            &mut attachments,
            &mut endpoints,
        );
    }
    if kind == "call" {
        for &index in &prepared.named_references {
            let rule = &rules.syntax_rules[index];
            if let Some(fact) = manifests::semantic::execute_reference_rule(
                rule,
                position(ctx.node, ctx.offset),
                facts,
                |api| proof.api(ctx.node, api),
                |key| {
                    slot(ctx.node, ctx.source, key)
                        .map(|node| node.map(|node| literal(Some(node), ctx.source)))
                },
            ) {
                named.push(fact);
            }
        }
    }
    if attachments.is_empty() && endpoints.is_empty() && named.is_empty() {
        return;
    }
    let Some(owner) = facts.nodes.iter().find(|node| node.id == ctx.owner) else {
        return;
    };
    let flow = projections.entry(owner.id.clone()).or_default();
    let framework = flow.framework.get_or_insert_with(Default::default);
    for fact in attachments {
        let overflow = push(framework.symbol_attachments().count(), fact, |fact| {
            framework.push_symbol_attachment(fact)
        });
        framework.fact_limit_exceeded |= overflow;
    }
    for fact in endpoints {
        let overflow = push(framework.endpoint_requests().count(), fact, |fact| {
            framework.push_endpoint_request(fact)
        });
        framework.fact_limit_exceeded |= overflow;
    }
    for fact in named {
        let overflow = push(framework.named_bindings().count(), fact, |fact| {
            framework.push_named_binding(fact)
        });
        framework.fact_limit_exceeded |= overflow;
    }
}
